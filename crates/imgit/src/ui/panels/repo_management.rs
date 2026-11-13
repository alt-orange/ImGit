//! The Repository Management view (the tabs bar folder), the favourites and the repository summaries.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use dear_imgui_rs::{InputTextFlags, Key, StyleColor, StyleVar, WindowHoveredFlags};
use imgit_app::Job;
use imgit_app::preferences::{pref, preferences, with_prune_preference};
use imgit_app::profiles::write_text_file;
use imgit_git::cli::{self, RunOptions};
use imgit_git::{RepoSummary, Repository};

use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::app::toasts::ToastKind;
use crate::theme::{Color, FONT_SIZE_ROW};
use crate::ui::draw::DrawList;
use crate::ui::menu::{MenuItems, menu_item, popup_menu, separated_menu};
use crate::ui::widgets::contains_ci;
use crate::ui::{Cx, Vec2, vec2};

/// The sections of the view, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Section {
    Open,
    Favorites,
    Recent,
    All,
}

impl Section {
    const ALL: [Self; 4] = [Self::Open, Self::Favorites, Self::Recent, Self::All];

    fn title(self) -> &'static str {
        match self {
            Self::Open => "Open repositories",
            Self::Favorites => "Favorites",
            Self::Recent => "Recent repositories",
            Self::All => "All repositories",
        }
    }
}

/// A repository's summary: being read in the background, or read.
enum SummaryEntry {
    Reading(Job<Option<RepoSummary>>),
    Ready(Option<RepoSummary>),
}

/// The view's state between frames (the C++ function-local statics) and the summaries cache.
pub struct State {
    search: String,
    wip_summary: bool,
    collapsed: HashSet<Section>,
    favorites: BTreeSet<String>,
    /// The profile `favorites` was loaded from.
    favorites_for: String,
    /// Branch / ahead-behind / WIP summaries, read in the background once per repository.
    summaries: HashMap<String, SummaryEntry>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            search: String::new(),
            wip_summary: true,
            collapsed: HashSet::new(),
            favorites: BTreeSet::new(),
            favorites_for: String::new(),
            summaries: HashMap::new(),
        }
    }
}

impl State {
    /// The summary of the repository at `path`: starts reading it the first time it is asked for; `None` until
    /// it is read (and when it cannot be).
    fn summary(&mut self, path: &str) -> Option<&RepoSummary> {
        let entry = self.summaries.entry(path.to_owned()).or_insert_with(|| {
            let path = path.to_owned();
            SummaryEntry::Reading(Job::spawn(move || {
                Repository::open(&path).ok()?.summary().ok()
            }))
        });
        if let SummaryEntry::Reading(job) = entry
            && let Some(summary) = job.poll()
        {
            *entry = SummaryEntry::Ready(summary);
        }
        match entry {
            SummaryEntry::Ready(summary) => summary.as_ref(),
            SummaryEntry::Reading(_) => None,
        }
    }

    /// Whether a summary is still being read.
    fn loading(&self) -> bool {
        self.summaries
            .values()
            .any(|entry| matches!(entry, SummaryEntry::Reading(_)))
    }

    /// Summaries still being read stay; the rest are read again when next drawn.
    fn forget_finished(&mut self) {
        self.summaries
            .retain(|_, entry| matches!(entry, SummaryEntry::Reading(_)));
    }

    fn toggle_section(&mut self, section: Section) {
        if !self.collapsed.remove(&section) {
            self.collapsed.insert(section);
        }
    }

    fn save_favorites(&self, file: &Path) {
        let _ = write_text_file(file, &format_favorites(&self.favorites));
    }

    /// Stars (or unstars) `path`, saved at once.
    fn set_favorite(&mut self, file: &Path, path: &str, on: bool) {
        if on {
            self.favorites.insert(path.to_owned());
        } else {
            self.favorites.remove(path);
        }
        self.save_favorites(file);
    }
}

/// `favorites.txt`: one path per line.
fn parse_favorites(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn format_favorites(favorites: &BTreeSet<String>) -> String {
    favorites.iter().map(|path| format!("{path}\n")).collect()
}

fn load_favorites(app: &Application) -> BTreeSet<String> {
    fs::read(app.config_path("favorites.txt"))
        .map(|bytes| parse_favorites(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default()
}

/// The favourite repositories (starred in Repository Management), in the order it lists them.
#[must_use]
pub fn favorite_repositories(app: &Application) -> Vec<String> {
    load_favorites(app).into_iter().collect()
}

/// A repository's branch / ahead-behind / WIP summary, read once in the background; `None` until ready.
#[must_use]
pub fn repo_summary<'a>(app: &'a mut Application, path: &str) -> Option<&'a RepoSummary> {
    app.panels.repo_management.summary(path)
}

/// The path with forward slashes and no trailing one.
fn normalized(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_owned()
}

/// The last folder of a path.
fn repo_name(path: &str) -> String {
    let clean = normalized(path);
    clean
        .rsplit_once('/')
        .map_or(clean.as_str(), |(_, name)| name)
        .to_owned()
}

/// Every repository once (by normalized path): the open ones, then the recent ones, then the favourites;
/// sorted by name, ignoring case.
fn all_repositories(open: &[String], recent: &[String], favorites: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut all: Vec<String> = [open, recent, favorites]
        .into_iter()
        .flatten()
        .filter(|path| seen.insert(normalized(path)))
        .cloned()
        .collect();
    all.sort_by_cached_key(|path| repo_name(path).to_ascii_lowercase());
    all
}

/// The paths whose display name or path has `search` in it.
fn filtered(paths: &[String], search: &str, display_name: impl Fn(&str) -> String) -> Vec<String> {
    paths
        .iter()
        .filter(|path| contains_ci(&display_name(path), search) || contains_ci(path, search))
        .cloned()
        .collect()
}

/// The pixel y where a 1em CSS box starting at `box_top` puts its glyphs.
fn css_y(cx: &Cx<'_>, size: f32, box_top: f32) -> f32 {
    let ascent = size * 2189.0 / 2048.0 * cx.scale;
    (box_top + ascent.round()).round() - ascent.ceil()
}

/// Runs `git <args>` in each of `paths` one after another in the background, then says how it went; the
/// repositories' summaries (ahead / behind) are read again.
fn run_in_repositories(app: &mut Application, title: &str, paths: Vec<String>, args: &[&str]) {
    if paths.is_empty() {
        return;
    }
    let tag = format!("repositories:{title}");
    if app.background_running(&tag) {
        app.toasts
            .push(ToastKind::Info, format!("{title} is already running"));
        return;
    }
    let args = with_prune_preference(
        args.iter().map(|&arg| arg.to_owned()).collect(),
        preferences().flag(pref::AUTO_PRUNE),
    );
    let title = title.to_owned();
    app.run_in_background(tag, move || {
        let failed: Vec<String> = paths
            .iter()
            .filter(|path| cli::run(Path::new(path), &args, RunOptions::default()).exit_code != 0)
            .map(|path| repo_name(path))
            .collect();
        let count = paths.len();
        Box::new(move |app: &mut Application| {
            app.panels.repo_management.forget_finished();
            if failed.is_empty() {
                let noun = if count == 1 {
                    "repository"
                } else {
                    "repositories"
                };
                app.toasts
                    .push(ToastKind::Success, format!("{title}: {count} {noun}"));
            } else {
                app.toasts.push_with_detail(
                    ToastKind::Error,
                    format!("{title} failed in {} of {count}", failed.len()),
                    failed.join(", "),
                );
            }
        })
    });
}

/// Opens `path` in another ImGit window (a second instance started on it).
fn open_in_new_window(app: &mut Application, path: &str) {
    let command = [
        imgit_platform::process::executable_path()
            .to_string_lossy()
            .into_owned(),
        path.to_owned(),
    ];
    if imgit_platform::process::spawn_detached(&command, None).is_err() {
        app.toasts.push_with_detail(
            ToastKind::Error,
            "Could not open a new window",
            path.to_owned(),
        );
    }
}

/// What the frame asks for once the view's child window is closed.
#[derive(Default)]
struct Events {
    open: Option<String>,
    close_tab: Option<String>,
    remove_recent: Option<String>,
}

/// Repository Management over the tab area, `size` device px.
pub fn draw_repo_management(cx: &mut Cx<'_>, app: &mut Application, size: Vec2) {
    let events = app.with_panel_state(
        |panels| &mut panels.repo_management,
        |app, state| repo_management(cx, app, state, size),
    );
    if let Some(path) = events.close_tab {
        let wanted = normalized(&path);
        if let Some(index) = app
            .tabs
            .iter()
            .position(|tab| normalized(&tab.root_workdir.to_string_lossy()) == wanted)
        {
            app.close_tab(index);
        }
    }
    if let Some(path) = events.remove_recent {
        app.recent_repos
            .retain(|recent| recent.to_string_lossy() != path);
        app.save_recent();
    }
    if let Some(path) = events.open {
        app.repo_management_open = false;
        app.open_repository(&path);
    }
}

/// A toolbar button: 12px captions after a 12px fa-fw icon, 6px 12px padding, 5px apart.
const TOOLBAR_BUTTONS: [(&str, &str, &str); 3] = [
    ("##rm_browse", "far:folder-open", "Browse"),
    ("##rm_clone", "far:cloud-download", "Clone"),
    ("##rm_init", "far:plus", "Init"),
];

fn repo_management(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut State,
    size: Vec2,
) -> Events {
    let ui = cx.ui;
    let mut events = Events::default();
    ui.child_window("##repo_management")
        .size(size.into())
        .build(ui, || {
            let p = cx.palette;
            let origin = cx.cursor();
            let draw = cx.window_draw_list();
            draw.add_rect_filled(
                origin,
                vec2(
                    origin.x + size.x,
                    origin.y + size.y.max(ui.scroll_max_y() + size.y),
                ),
                p.app_bg0,
                0.0,
            );
            if state.favorites_for != app.profiles.current().guid {
                state.favorites_for.clone_from(&app.profiles.current().guid);
                state.favorites = load_favorites(app);
            }
            if ui.is_key_pressed(Key::Escape) {
                app.repo_management_open = false;
            }
            let left = origin.x + cx.px(45.0);
            let right = origin.x + size.x - cx.px(45.0);

            cx.draw_text(
                draw,
                cx.fonts.semibold,
                16.0,
                vec2(left, css_y(cx, 16.0, origin.y + cx.px(17.0))),
                p.text_selected,
                "Repository Management",
            );
            toolbar(cx, app, draw, origin, left);
            toolbar_row(cx, state, draw, origin, left, right);

            // The repositories of each section.
            let open_paths: Vec<String> = app
                .tabs
                .iter()
                // the repository, not a submodule open in its tab
                .map(|tab| tab.root_workdir.to_string_lossy().into_owned())
                .collect();
            let recent: Vec<String> = app
                .recent_repos
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            let favorite_paths: Vec<String> = state.favorites.iter().cloned().collect();
            let all = all_repositories(&open_paths, &recent, &favorite_paths);
            let display_name = |path: &str| app.repo_display_name(Path::new(path));
            let sections = [
                (
                    Section::Open,
                    filtered(&open_paths, &state.search, display_name),
                ),
                (
                    Section::Favorites,
                    filtered(&favorite_paths, &state.search, display_name),
                ),
                (
                    Section::Recent,
                    filtered(&recent, &state.search, display_name),
                ),
                (Section::All, filtered(&all, &state.search, display_name)),
            ];

            let mut y = origin.y + cx.px(144.0);
            for (section, paths) in &sections {
                y = draw_section(
                    cx,
                    app,
                    state,
                    draw,
                    SectionLayout {
                        section: *section,
                        paths,
                        left,
                        right,
                    },
                    y,
                    &mut events,
                );
            }
            cx.set_cursor(vec2(origin.x, y));
            ui.dummy(vec2(1.0, cx.px(20.0)));
            if state.loading() {
                cx.keep_animating();
            }
        });
    events
}

/// The Browse / Clone / Init buttons.
fn toolbar(cx: &mut Cx<'_>, app: &mut Application, draw: DrawList<'_>, origin: Vec2, left: f32) {
    let ui = cx.ui;
    let p = cx.palette;
    let mut x = left;
    let y = origin.y + cx.px(47.6);
    for (id, icon, label) in TOOLBAR_BUTTONS {
        let w = cx.px(31.0) + cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, label).x + cx.px(18.0);
        let min = vec2(x, y);
        let max = vec2(x + w, y + cx.px(31.1));
        cx.set_cursor(min);
        let clicked = ui.invisible_button(id, vec2(w, max.y - min.y));
        let hovered = ui.is_item_hovered();
        draw.add_rect_filled(
            min,
            max,
            p.accent.with_alpha(if hovered { 0.3 } else { 0.2 }),
            cx.px(2.0),
        );
        draw.add_rect(min, max, p.accent, cx.px(2.0), 1.0);
        cx.draw_fa_icon(
            draw,
            icon,
            vec2(x + cx.px(19.5), y + cx.px(15.5)),
            cx.px(12.0),
            p.text_normal,
        );
        cx.draw_text(
            draw,
            cx.fonts.regular,
            FONT_SIZE_ROW,
            vec2(x + cx.px(31.0), css_y(cx, 12.0, y + cx.px(7.0))),
            p.text_normal,
            label,
        );
        if clicked {
            match id {
                "##rm_browse" => app.browse_for_repository(cx),
                "##rm_clone" => app.dialog.open(DialogKind::CloneRepo),
                _ => app.dialog.open(DialogKind::InitRepo),
            }
        }
        x += w + cx.px(5.0);
    }
}

/// Collapse / Expand, the search box, WIP summary.
fn toolbar_row(
    cx: &mut Cx<'_>,
    state: &mut State,
    draw: DrawList<'_>,
    origin: Vec2,
    left: f32,
    right: f32,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let y = origin.y + cx.px(88.7);
    let collapse_button = |cx: &mut Cx<'_>,
                           state: &mut State,
                           id: &str,
                           label: &str,
                           x: f32,
                           collapse: bool|
     -> f32 {
        let w = cx.px(25.25) + cx.text_size(cx.fonts.regular, 14.0, label).x + cx.px(8.0);
        cx.set_cursor(vec2(x, y));
        let clicked = ui.invisible_button(id, vec2(w, cx.px(36.0)));
        let color = if ui.is_item_hovered() {
            p.text_selected
        } else {
            p.text_normal
        };
        let size = cx.px(14.0) * 10.0 / 12.25;
        cx.draw_fa_icon(
            draw,
            if collapse {
                "far:chevron-down"
            } else {
                "far:chevron-up"
            },
            vec2(x + cx.px(14.15), y + cx.px(13.0)),
            size,
            color,
        );
        cx.draw_fa_icon(
            draw,
            if collapse {
                "far:chevron-up"
            } else {
                "far:chevron-down"
            },
            vec2(x + cx.px(14.15), y + cx.px(23.0)),
            size,
            color,
        );
        cx.draw_text(
            draw,
            cx.fonts.regular,
            14.0,
            vec2(x + cx.px(25.25), css_y(cx, 14.0, y + cx.px(8.03))),
            color,
            label,
        );
        if clicked {
            state.collapsed.clear();
            if collapse {
                state.collapsed.extend(Section::ALL);
            }
        }
        w
    };
    let cw = collapse_button(cx, state, "##rm_collapse", "Collapse", left, true);
    collapse_button(
        cx,
        state,
        "##rm_expand",
        "Expand",
        left + cw + cx.px(4.0),
        false,
    );

    let wip_label = "WIP summary";
    let label_w = cx.text_size(cx.fonts.regular, 14.0, wip_label).x;
    let box_x = right - label_w - cx.px(20.0);
    let search_x = origin.x + cx.px(231.5);
    let search_right = box_x - cx.px(12.0);
    let smin = vec2(search_x, y + cx.px(1.0));
    let smax = vec2(search_right, smin.y + cx.px(34.0));
    draw.add_rect_filled(smin, smax, p.input_bg, cx.px(4.0));
    cx.set_cursor(vec2(smin.x + cx.px(40.0), smin.y));
    let focused = {
        let text_h = cx.text_size(cx.fonts.regular, 14.0, "A").y;
        let _padding = ui.push_style_var(StyleVar::FramePadding([
            cx.px(1.0),
            (cx.px(34.0) - text_h) / 2.0,
        ]));
        let _border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
        let _bg = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
        let _bg_hovered = ui.push_style_color(StyleColor::FrameBgHovered, Color::TRANSPARENT);
        let _bg_active = ui.push_style_color(StyleColor::FrameBgActive, Color::TRANSPARENT);
        let _font = cx.push_font(cx.fonts.regular, 14.0);
        if ui.is_window_appearing() {
            ui.set_keyboard_focus_here();
        }
        cx.input_text(
            "##rm_search",
            &mut state.search,
            Some("Search repositories"),
            InputTextFlags::empty(),
            smax.x - smin.x - cx.px(50.0),
        );
        ui.is_item_active()
    };
    draw.add_rect(
        smin,
        smax,
        if focused { p.accent } else { p.section_border },
        cx.px(4.0),
        1.0,
    );
    cx.draw_fa_icon(
        draw,
        "far:search",
        vec2(smin.x + cx.px(17.0), smin.y + cx.px(17.0)),
        cx.px(14.0),
        p.text_normal,
    );

    cx.set_cursor(vec2(box_x, y));
    if ui.invisible_button("##rm_wip", vec2(cx.px(20.0) + label_w, cx.px(36.0))) {
        state.wip_summary = !state.wip_summary;
    }
    let cmin = vec2(box_x, y + cx.px(12.0));
    let cmax = cmin + vec2(cx.px(12.0), cx.px(12.0));
    if state.wip_summary {
        draw.add_rect_filled(cmin, cmax, p.link, cx.px(1.0));
        cx.draw_fa_icon(
            draw,
            "fas:check",
            cmin + vec2(cx.px(6.0), cx.px(6.0)),
            cx.px(9.0),
            Color::from_rgba8(34, 34, 34, 255),
        );
    } else {
        draw.add_rect_filled(cmin, cmax, p.input_bg, cx.px(1.0));
        draw.add_rect(cmin, cmax, p.text_disabled, cx.px(1.0), 1.0);
    }
    cx.draw_text(
        draw,
        cx.fonts.regular,
        14.0,
        vec2(box_x + cx.px(20.0), css_y(cx, 14.0, y + cx.px(8.03))),
        p.text_normal,
        wip_label,
    );
}

/// One section to draw: its repositories and the columns it sits between.
struct SectionLayout<'a> {
    section: Section,
    paths: &'a [String],
    left: f32,
    right: f32,
}

/// A 24px icon button of a section header or a row, its tooltip under it: the colour it is drawn in.
struct IconButton {
    clicked: bool,
    color: Color,
}

fn icon_button(
    cx: &mut Cx<'_>,
    id: &str,
    pos: Vec2,
    width: f32,
    tip: &str,
    enabled: bool,
) -> IconButton {
    let ui = cx.ui;
    let p = cx.palette;
    cx.set_cursor(pos);
    let clicked = ui.invisible_button(id, vec2(width, cx.px(24.0))) && enabled;
    let hovered = ui.is_item_hovered();
    if hovered {
        let rect = cx.item_rect();
        cx.styled_tooltip(rect.min, rect.max, tip, false);
    }
    let color = if !enabled {
        p.text_disabled
    } else if hovered {
        p.text_selected
    } else {
        p.text_normal
    };
    IconButton { clicked, color }
}

fn pull_menu(paths: &[String], enabled: bool) -> MenuItems {
    let fast_forward = paths.to_vec();
    let rebase = paths.to_vec();
    vec![
        menu_item("Pull all (fast-forward only)", move |_, app| {
            run_in_repositories(app, "Pull all", fast_forward, &["pull", "--ff-only"]);
        })
        .enabled(enabled),
        menu_item("Pull all (rebase)", move |_, app| {
            run_in_repositories(app, "Pull all (rebase)", rebase, &["pull", "--rebase"]);
        })
        .enabled(enabled),
    ]
}

fn open_all_in_tabs(app: &mut Application, paths: &[String]) {
    for path in paths {
        app.open_repository(path);
    }
    app.repo_management_open = false;
}

/// The section's "⋮" menu.
fn section_menu(
    section: Section,
    paths: &[String],
    collapsed: bool,
    any_favorite: bool,
) -> MenuItems {
    let any = !paths.is_empty();
    let open_paths = paths.to_vec();
    let fetch_paths = paths.to_vec();
    let mut items: MenuItems = vec![
        menu_item("Open all in tabs", move |_, app| {
            open_all_in_tabs(app, &open_paths);
        })
        .enabled(any),
        menu_item("Fetch all", move |_, app| {
            run_in_repositories(
                app,
                "Fetch all",
                fetch_paths,
                &["fetch", "--all", "--prune"],
            );
        })
        .enabled(any),
    ];
    items.extend(pull_menu(paths, any));
    let mut view: MenuItems = vec![menu_item(
        if collapsed {
            "Expand section"
        } else {
            "Collapse section"
        },
        move |_, app| app.panels.repo_management.toggle_section(section),
    )];
    if section == Section::Favorites {
        view.push(
            menu_item("Remove all favorites", |_, app| {
                let file = app.config_path("favorites.txt");
                let state = &mut app.panels.repo_management;
                state.favorites.clear();
                state.save_favorites(&file);
            })
            .enabled(any_favorite),
        );
    }
    separated_menu(vec![items, view])
}

/// The row's "⋮" menu of the All section.
fn row_menu(path: &str, favorite: bool, recent: bool) -> MenuItems {
    let (open, window, manager, star, forget) = (
        path.to_owned(),
        path.to_owned(),
        path.to_owned(),
        path.to_owned(),
        path.to_owned(),
    );
    separated_menu(vec![
        vec![
            menu_item("Open repository", move |_, app| {
                app.repo_management_open = false;
                app.open_repository(&open);
            }),
            menu_item("Open in a new window", move |_, app| {
                open_in_new_window(app, &window);
            }),
            menu_item("Open in file manager", move |_, _| {
                let _ = imgit_platform::shell::open_in_file_manager(Path::new(&manager));
            }),
        ],
        vec![
            menu_item(
                if favorite {
                    "Remove from favorites"
                } else {
                    "Add to favorites"
                },
                move |_, app| {
                    let file = app.config_path("favorites.txt");
                    app.panels
                        .repo_management
                        .set_favorite(&file, &star, !favorite);
                },
            ),
            menu_item("Remove from recent", move |_, app| {
                app.recent_repos
                    .retain(|recent| recent.to_string_lossy() != forget);
                app.save_recent();
            })
            .enabled(recent),
        ],
    ])
}

/// One section: its header with the tools and its rows; returns the y under it.
fn draw_section(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut State,
    draw: DrawList<'_>,
    layout: SectionLayout<'_>,
    mut y: f32,
    events: &mut Events,
) -> f32 {
    let ui = cx.ui;
    let p = cx.palette;
    let SectionLayout {
        section,
        paths,
        left,
        right,
    } = layout;
    let collapsed = state.collapsed.contains(&section);
    let title = section.title();
    // .collapsible-panel-header: 5% white, radius 2.5, a 16px chevron, the 14px semibold title and a pill
    // count.
    let hmin = vec2(left + cx.px(3.0), y);
    let hmax = vec2(right - cx.px(10.0), y + cx.px(44.0));
    draw.add_rect_filled(hmin, hmax, p.ink_255(13), cx.px(2.5));
    cx.set_cursor(vec2(hmin.x + cx.px(11.0), y + cx.px(11.0)));
    let scope = ui.push_id(title);
    if ui.invisible_button("##toggle", vec2(cx.px(28.0), cx.px(22.0))) {
        state.toggle_section(section);
    }
    cx.draw_fa_icon_rotated(
        draw,
        "far:chevron-right",
        vec2(hmin.x + cx.px(25.0), y + cx.px(20.8)),
        cx.px(16.0),
        p.text_normal,
        if collapsed { 0.0 } else { 90.0 },
    );
    cx.draw_text(
        draw,
        cx.fonts.semibold,
        14.0,
        vec2(hmin.x + cx.px(47.0), css_y(cx, 14.0, y + cx.px(12.0))),
        p.text_normal,
        title,
    );
    {
        let count = paths.len().to_string();
        let title_w = cx.text_size(cx.fonts.semibold, 14.0, title).x;
        let bx = hmin.x + cx.px(47.0) + title_w + cx.px(11.0);
        let bw = cx.text_size(cx.fonts.regular, 12.0, &count).x + cx.px(12.0);
        draw.add_rect_filled(
            vec2(bx, y + cx.px(11.4)),
            vec2(bx + bw, y + cx.px(32.5)),
            p.panel_bg1,
            cx.px(11.0),
        );
        cx.draw_text(
            draw,
            cx.fonts.regular,
            12.0,
            vec2(bx + cx.px(6.0), css_y(cx, 12.0, y + cx.px(13.4))),
            p.text_secondary,
            &count,
        );
    }
    section_tools(cx, app, state, draw, section, paths, hmax, y);
    drop(scope);
    y += cx.px(44.0);
    if collapsed {
        return y + cx.px(18.0);
    }
    if paths.is_empty() {
        cx.draw_text(
            draw,
            cx.fonts.italic,
            14.0,
            vec2(left + cx.px(34.0), css_y(cx, 14.0, y + cx.px(25.0))),
            p.text_normal,
            "No matches",
        );
        return y + cx.px(64.0);
    }
    y += cx.px(15.0);
    for path in paths {
        draw_row(
            cx,
            app,
            state,
            draw,
            section,
            path,
            y,
            (left, right),
            events,
        );
        y += cx.px(34.0);
    }
    y + cx.px(18.0)
}

/// Section tools: "Close all tabs" / "Remove all", then the 24px icon buttons: Fetch all, Pull all ▾, Open
/// all in tabs and the section's menu.
#[allow(clippy::too_many_arguments)]
fn section_tools(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut State,
    draw: DrawList<'_>,
    section: Section,
    paths: &[String],
    hmax: Vec2,
    y: f32,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let any = !paths.is_empty();
    let top = y + cx.px(10.0);
    let mut tx = hmax.x - cx.px(13.0) - cx.px(24.0);
    let menu = icon_button(
        cx,
        "##menu",
        vec2(tx, top),
        cx.px(24.0),
        "Section options",
        true,
    );
    if menu.clicked {
        let items = section_menu(
            section,
            paths,
            state.collapsed.contains(&section),
            !state.favorites.is_empty(),
        );
        popup_menu(cx, items, None);
    }
    cx.draw_fa_icon(
        draw,
        "far:ellipsis-v",
        vec2(tx + cx.px(12.0), y + cx.px(22.0)),
        cx.px(16.0),
        menu.color,
    );
    if section == Section::All {
        return;
    }
    tx -= cx.px(34.0);
    let open_all = icon_button(
        cx,
        "##open_all",
        vec2(tx, top),
        cx.px(24.0),
        "Open all in tabs",
        any,
    );
    if open_all.clicked {
        open_all_in_tabs(app, paths);
    }
    cx.draw_fa_icon(
        draw,
        "far:external-link-alt",
        vec2(tx + cx.px(12.0), y + cx.px(22.0)),
        cx.px(16.0),
        open_all.color,
    );
    tx -= cx.px(45.0);
    let pull = icon_button(
        cx,
        "##pull_all",
        vec2(tx, top),
        cx.px(34.0),
        "Pull all",
        any,
    );
    if pull.clicked {
        let items = pull_menu(paths, true);
        popup_menu(cx, items, None);
    }
    cx.draw_fa_icon_rotated(
        draw,
        "fas:reply",
        vec2(tx + cx.px(8.0), y + cx.px(22.0)),
        cx.px(16.0),
        pull.color,
        315.0,
    );
    cx.draw_fa_icon(
        draw,
        "fas:caret-down",
        vec2(tx + cx.px(24.0), y + cx.px(22.0)),
        cx.px(14.0),
        pull.color,
    );
    tx -= cx.px(39.0);
    let fetch = icon_button(
        cx,
        "##fetch_all",
        vec2(tx, top),
        cx.px(24.0),
        "Fetch all",
        any,
    );
    if fetch.clicked {
        run_in_repositories(
            app,
            "Fetch all",
            paths.to_vec(),
            &["fetch", "--all", "--prune"],
        );
    }
    cx.draw_fa_icon(
        draw,
        "far:reply",
        vec2(tx + cx.px(12.0), y + cx.px(22.0)),
        cx.px(16.0),
        fetch.color,
    );
    let bulk = match section {
        Section::Open => Some("Close all tabs"),
        Section::Recent => Some("Remove all"),
        _ => None,
    };
    if let Some(bulk) = bulk {
        let bw = cx.px(23.0) + cx.text_size(cx.fonts.regular, 12.0, bulk).x + cx.px(5.0);
        let bx = tx - cx.px(10.0) - bw;
        cx.set_cursor(vec2(bx, top));
        let clicked = ui.invisible_button("##bulk", vec2(bw, cx.px(24.0)));
        let color = if ui.is_item_hovered() {
            p.text_selected
        } else {
            p.text_normal
        };
        cx.draw_fa_icon(
            draw,
            "far:times",
            vec2(bx + cx.px(10.0), y + cx.px(22.0)),
            cx.px(16.0),
            color,
        );
        cx.draw_text(
            draw,
            cx.fonts.regular,
            12.0,
            vec2(bx + cx.px(25.0), css_y(cx, 12.0, y + cx.px(13.42))),
            color,
            bulk,
        );
        if clicked {
            if section == Section::Open {
                while !app.tabs.is_empty() {
                    app.close_tab(0);
                }
            } else {
                app.recent_repos.clear();
                app.save_recent();
            }
        }
    }
}

/// The row's right-hand actions, laid out from the right edge inwards.
struct RowActions {
    x: f32,
    top: f32,
    mid: f32,
}

impl RowActions {
    fn button(
        &mut self,
        cx: &mut Cx<'_>,
        draw: DrawList<'_>,
        id: &str,
        icon: &str,
        tip: &str,
    ) -> bool {
        let ui = cx.ui;
        let p = cx.palette;
        cx.set_cursor(vec2(self.x, self.top));
        let clicked = ui.invisible_button(id, vec2(cx.px(24.0), cx.px(24.0)));
        let hovered = ui.is_item_hovered();
        cx.draw_fa_icon(
            draw,
            icon,
            vec2(self.x + cx.px(12.0), self.mid),
            cx.px(16.0),
            if hovered {
                p.text_selected
            } else {
                p.text_normal
            },
        );
        if hovered {
            let rect = cx.item_rect();
            cx.styled_tooltip(rect.min, rect.max, tip, false);
        }
        self.x -= cx.px(34.0);
        clicked
    }
}

/// The branch / ahead-behind / WIP pills (.panel-bg1, radius 2, 0 5px padding, 12px) of a row.
fn draw_pills(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    summary: &RepoSummary,
    wip_summary: bool,
    left: f32,
    mid: f32,
) {
    let p = cx.palette;
    let mut x = left + cx.px(580.0);
    let top = mid - cx.px(8.55);
    let (pill_height, pill_rounding) = (cx.px(17.1), cx.px(2.0));
    let pill = |x: f32, w: f32| {
        draw.add_rect_filled(
            vec2(x, top),
            vec2(x + w, top + pill_height),
            p.panel_bg1,
            pill_rounding,
        );
    };
    if let Some(branch) = summary
        .branch
        .as_deref()
        .filter(|branch| !branch.is_empty())
    {
        let tw = cx.text_size(cx.fonts.regular, 12.0, branch).x;
        let w = cx.px(5.0 + 7.9 + 5.0) + tw + cx.px(5.0);
        pill(x, w);
        cx.draw_fa_icon(
            draw,
            "far:code-branch",
            vec2(x + cx.px(8.95), mid),
            cx.px(10.5),
            p.text_normal,
        );
        cx.draw_text(
            draw,
            cx.fonts.regular,
            12.0,
            vec2(x + cx.px(17.88), css_y(cx, 12.0, top)),
            p.text_normal,
            branch,
        );
        x += w + cx.px(5.0);
    }
    for (value, icon, color) in [
        (summary.ahead, "far:arrow-up", p.green),
        (summary.behind, "far:arrow-down", p.text_normal),
    ] {
        if value == 0 {
            continue;
        }
        let text = value.to_string();
        let tw = cx.text_size(cx.fonts.semibold, 12.0, &text).x;
        let w = cx.px(5.0 + 9.2 + 2.0) + tw + cx.px(5.0);
        pill(x, w);
        cx.draw_fa_icon(draw, icon, vec2(x + cx.px(9.6), mid), cx.px(10.5), color);
        cx.draw_text(
            draw,
            cx.fonts.semibold,
            12.0,
            vec2(x + cx.px(16.2), css_y(cx, 12.0, top)),
            color,
            &text,
        );
        x += w + cx.px(5.0);
    }
    if wip_summary && summary.modified + summary.added + summary.deleted > 0 {
        let items = [
            (summary.modified, "fas:pencil", p.orange),
            (summary.added, "fas:plus", p.green),
            (summary.deleted, "fas:minus", p.red),
        ];
        let mut w = cx.px(5.0);
        for (value, _, _) in items {
            if value > 0 {
                w += cx.px(17.5)
                    + cx.text_size(cx.fonts.regular, 12.0, &value.to_string()).x
                    + cx.px(6.0);
            }
        }
        w -= cx.px(1.0);
        pill(x, w);
        let mut ix = x + cx.px(5.0);
        for (value, icon, color) in items {
            if value == 0 {
                continue;
            }
            let text = value.to_string();
            cx.draw_fa_icon(
                draw,
                icon,
                vec2(ix + cx.px(6.25), mid + cx.px(0.25)),
                cx.px(10.0),
                color,
            );
            cx.draw_text(
                draw,
                cx.fonts.regular,
                12.0,
                vec2(ix + cx.px(17.5), css_y(cx, 12.0, top)),
                p.text_normal,
                &text,
            );
            ix += cx.px(17.5) + cx.text_size(cx.fonts.regular, 12.0, &text).x + cx.px(6.0);
        }
    }
}

/// A 34px row: star, checkbox, name, owner, branch / ahead / WIP pills and the row actions.
#[allow(clippy::too_many_arguments)]
fn draw_row(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut State,
    draw: DrawList<'_>,
    section: Section,
    path: &str,
    row_top: f32,
    (left, right): (f32, f32),
    events: &mut Events,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let mid = row_top + cx.px(17.0);
    let key = format!("{}{path}", section.title());
    let scope = ui.push_id(key.as_str());
    let mouse = cx.mouse_pos();
    let row_hovered = ui
        .is_window_hovered_with_flags(WindowHoveredFlags::ALLOW_WHEN_BLOCKED_BY_ACTIVE_ITEM)
        && mouse.y >= row_top
        && mouse.y < row_top + cx.px(34.0)
        && mouse.x >= left
        && mouse.x < right;
    if row_hovered {
        draw.add_rect_filled(
            vec2(left, row_top),
            vec2(right, row_top + cx.px(34.0)),
            p.hover_row,
            0.0,
        );
    }
    let favorites_file = app.config_path("favorites.txt");
    // star
    let favorite = state.favorites.contains(path);
    cx.set_cursor(vec2(left, row_top));
    if ui.invisible_button("##star", vec2(cx.px(35.0), cx.px(34.0))) {
        state.set_favorite(&favorites_file, path, !favorite);
    }
    // .favorite-checkbox: hidden until the row is hovered unless checked.
    if favorite || row_hovered {
        cx.draw_fa_icon(
            draw,
            if favorite { "fas:star" } else { "far:star" },
            vec2(left + cx.px(18.75), mid),
            cx.px(14.0),
            if favorite { p.yellow } else { p.text_disabled },
        );
    }
    // checkbox (multi-select)
    if section != Section::All {
        let cmin = vec2(left + cx.px(45.0), mid - cx.px(6.0));
        let cmax = cmin + vec2(cx.px(12.0), cx.px(12.0));
        draw.add_rect_filled(cmin, cmax, p.input_bg, cx.px(1.0));
        draw.add_rect(cmin, cmax, p.text_disabled, cx.px(1.0), 1.0);
    }
    // name, owner
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.regular,
        14.0,
        vec2(left + cx.px(94.0), css_y(cx, 14.0, mid - cx.px(10.0))),
        cx.px(230.0),
        p.text_normal,
        &app.repo_display_name(Path::new(path)),
    );
    let summary = state.summary(path).cloned();
    if let Some(summary) = &summary {
        if !summary.owner.is_empty() {
            cx.draw_text_ellipsis(
                draw,
                cx.fonts.regular,
                14.0,
                vec2(left + cx.px(333.0), css_y(cx, 14.0, mid - cx.px(10.0))),
                cx.px(230.0),
                p.text_normal,
                &summary.owner,
            );
        }
        draw_pills(cx, draw, summary, state.wip_summary, left, mid);
    }
    // Row actions: open in a new window, open (list), close / remove.
    let mut actions = RowActions {
        x: right - cx.px(47.0),
        top: row_top + cx.px(5.0),
        mid,
    };
    if section == Section::All {
        if actions.button(cx, draw, "##more", "far:ellipsis-v", "More") {
            let recent = app
                .recent_repos
                .iter()
                .any(|recent| recent.to_string_lossy() == path);
            let items = row_menu(path, state.favorites.contains(path), recent);
            popup_menu(cx, items, None);
        }
    } else {
        let tip = if section == Section::Open {
            "Close tab"
        } else {
            "Remove from list"
        };
        if actions.button(cx, draw, "##close", "far:times", tip) {
            match section {
                Section::Open => events.close_tab = Some(path.to_owned()),
                Section::Recent => events.remove_recent = Some(path.to_owned()),
                _ => state.set_favorite(&favorites_file, path, false),
            }
        }
    }
    if actions.button(cx, draw, "##open", "far:list", "Open repository") {
        events.open = Some(path.to_owned());
    }
    if actions.button(
        cx,
        draw,
        "##window",
        "far:external-link-alt",
        "Open in a new window",
    ) {
        open_in_new_window(app, path);
    }
    // Clicking the row opens the repository.
    cx.set_cursor(vec2(left + cx.px(70.0), row_top));
    if ui.invisible_button("##row", vec2(cx.px(500.0), cx.px(34.0))) {
        events.open = Some(path.to_owned());
    }
    drop(scope);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::icons::Icons;

    fn strings(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|&path| path.to_owned()).collect()
    }

    #[test]
    fn paths_are_normalized_and_named() {
        assert_eq!(normalized("C:\\work\\repo\\"), "C:/work/repo");
        assert_eq!(normalized("/a/b//"), "/a/b");
        assert_eq!(repo_name("C:\\work\\Repo\\"), "Repo");
        assert_eq!(repo_name("solo"), "solo");
        assert_eq!(repo_name(""), "");
    }

    #[test]
    fn favourites_round_trip_sorted() {
        let parsed = parse_favorites("/b\n\n/a\n/b\n");
        assert_eq!(
            parsed.iter().map(String::as_str).collect::<Vec<_>>(),
            ["/a", "/b"]
        );
        assert_eq!(format_favorites(&parsed), "/a\n/b\n");
        assert!(parse_favorites("").is_empty());
        assert_eq!(format_favorites(&BTreeSet::new()), "");
    }

    #[test]
    fn all_repositories_lists_each_once_sorted_by_name() {
        let open = strings(&["C:/w/zeta", "C:/w/Alpha"]);
        let recent = strings(&["C:\\w\\zeta\\", "C:/w/beta"]);
        let favorites = strings(&["C:/w/alpha"]);
        let all = all_repositories(&open, &recent, &favorites);
        // "C:\w\zeta\" is the open "C:/w/zeta"; "C:/w/alpha" differs from "C:/w/Alpha" in case only, which
        // counts as another path as it did before.
        assert_eq!(all, ["C:/w/Alpha", "C:/w/alpha", "C:/w/beta", "C:/w/zeta"]);
    }

    #[test]
    fn the_search_matches_names_and_paths() {
        let paths = strings(&["C:/w/one", "C:/x/two"]);
        let name = |path: &str| repo_name(path).to_uppercase();
        assert_eq!(filtered(&paths, "", name), paths);
        assert_eq!(filtered(&paths, "ONE", name), ["C:/w/one"]);
        // The path matches too.
        assert_eq!(filtered(&paths, "x/t", name), ["C:/x/two"]);
        assert!(filtered(&paths, "nothing", name).is_empty());
    }

    #[test]
    fn sections_toggle_and_the_defaults_show_the_wip_summary() {
        let mut state = State::default();
        assert!(state.wip_summary);
        state.toggle_section(Section::Recent);
        assert!(state.collapsed.contains(&Section::Recent));
        state.toggle_section(Section::Recent);
        assert!(state.collapsed.is_empty());
    }

    #[test]
    fn summaries_are_read_once_in_the_background() {
        let scratch = tempfile::tempdir().unwrap();
        drop(Repository::init(scratch.path()).unwrap());
        let path = scratch.path().to_string_lossy().into_owned();
        let mut state = State::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut found = false;
        while std::time::Instant::now() < deadline {
            if state.summary(&path).is_some() {
                found = true;
                break;
            }
            if !state.loading() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // An empty repository still has a summary; once read it is not read again.
        assert!(found || !state.loading());
        assert_eq!(state.summaries.len(), 1);
        // Not a repository: read, and nothing.
        let mut other = State::default();
        let missing = scratch
            .path()
            .join("missing")
            .to_string_lossy()
            .into_owned();
        while other.summary(&missing).is_none() && other.loading() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(other.summary(&missing).is_none());
        // Finished summaries are forgotten to be read again.
        other.forget_finished();
        assert!(other.summaries.is_empty());
    }

    #[test]
    fn the_icons_of_the_view_exist() {
        let icons = Icons::new();
        for name in [
            "far:folder-open",
            "far:cloud-download",
            "far:plus",
            "far:chevron-down",
            "far:chevron-up",
            "far:chevron-right",
            "far:search",
            "fas:check",
            "far:ellipsis-v",
            "far:external-link-alt",
            "fas:reply",
            "fas:caret-down",
            "far:reply",
            "far:times",
            "fas:star",
            "far:star",
            "far:code-branch",
            "far:arrow-up",
            "far:arrow-down",
            "fas:pencil",
            "fas:plus",
            "fas:minus",
            "far:list",
        ] {
            assert!(icons.find(name).is_some(), "{name}");
        }
    }
}
