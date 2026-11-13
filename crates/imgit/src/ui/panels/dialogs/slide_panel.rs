//! The Add / Edit Submodule panel sliding out of the left panel over the graph (`.panel-slide`, 0.25s): the
//! section icon and the 18px title with a close cross in a 50px header, then the form, 20px in.

use dear_imgui_rs::{
    ChildFlags, FocusedFlags, InputTextFlags, Key, MouseButton, StyleColor, StyleVar, WindowFlags,
};
use imgit_app::RepoState;
use imgit_git::{Commit, Submodule};

use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::theme::Color;
use crate::ui::{Cx, Vec2, vec2};

/// What the panel remembers: the form keeps its last values for the way out, once the dialog is gone.
#[derive(Default)]
pub struct SlideState {
    editing: bool,
    url: String,
    path: String,
    original_url: String,
    /// The Edit panel takes the keyboard focus (for Esc) once the double-click that opened it is over: a click
    /// focuses the window under the mouse when the frame ends.
    focus_panel: bool,
    /// The commit a submodule has checked out, read from the submodule itself, by folder and sha.
    commit: Option<(String, Option<Commit>)>,
}

impl SlideState {
    /// The commit `sha` of the submodule in `folder` (cached by folder and sha).
    fn commit(&mut self, folder: &str, sha: &str) -> Option<&Commit> {
        let key = format!("{folder}\n{sha}");
        if self.commit.as_ref().map(|(cached, _)| cached) != Some(&key) {
            let commit = imgit_git::Repository::open(folder)
                .ok()
                .and_then(|repo| repo.load_commit(sha).ok());
            self.commit = Some((key, commit));
        }
        self.commit.as_ref().and_then(|(_, commit)| commit.as_ref())
    }
}

/// The folder name `git submodule add` picks for `url`: its last component without a trailing slash or `.git`.
pub fn name_from_url(url: &str) -> String {
    let mut name = url.trim_end_matches(['/', '\\']);
    name = name.strip_suffix(".git").unwrap_or(name);
    name.rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or(name)
        .to_owned()
}

/// What the status line says about `submodule` of the repository called `repo_name`.
fn status_sentence(submodule: &Submodule, repo_name: &str) -> String {
    let recorded = submodule.recorded.is_some();
    let initialized = submodule.checked_out.is_some();
    if !recorded {
        format!("This submodule is not committed to your repository ({repo_name}).")
    } else if !initialized {
        "This submodule is not initialized.".to_owned()
    } else if submodule.in_sync() {
        format!("This submodule is in-sync with your repository ({repo_name}).")
    } else {
        format!("This submodule is out of sync with your repository ({repo_name}).")
    }
}

/// A button with a Font Awesome icon before its label, the two centred.
fn icon_label_button(
    cx: &mut Cx<'_>,
    danger: bool,
    id: &str,
    icon: &str,
    label: &str,
    size: Vec2,
    enabled: bool,
) -> bool {
    let pressed = if danger {
        cx.danger_button(id, size, enabled)
    } else {
        cx.primary_button(id, size, enabled)
    };
    let draw = cx.window_draw_list();
    let rect = cx.item_rect();
    let icon_w = cx.fa_icon_width(icon, cx.px(12.0));
    let text = cx.text_size(cx.fonts.regular, 13.0, label);
    let x = ((rect.min.x + rect.max.x - icon_w - cx.px(5.0) - text.x) / 2.0).round();
    let mid = (rect.min.y + rect.max.y) / 2.0;
    let mut color = if enabled && cx.ui.is_item_hovered() {
        cx.palette.text_selected
    } else {
        cx.palette.text_normal
    };
    if !enabled {
        color = color.faded(0.5);
    }
    let icon_size = cx.px(12.0);
    cx.draw_fa_icon(draw, icon, vec2(x + icon_w / 2.0, mid), icon_size, color);
    cx.draw_text(
        draw,
        cx.fonts.regular,
        13.0,
        vec2(x + icon_w + cx.px(5.0), (mid - text.y / 2.0).round()),
        color,
        label,
    );
    pressed
}

/// What the panel's buttons asked for this frame.
#[derive(Default)]
struct Requests {
    submit: bool,
    close: bool,
    open_submodule: bool,
    delete_submodule: bool,
    /// Initialize / update it to the recorded commit: the title and the git arguments.
    sync: Option<(&'static str, Vec<String>)>,
}

/// The panel sliding out of the left panel at `origin`, over the graph (`max_width` wide) (nothing while
/// closed).
///
/// Add Submodule (400px): the Remote URL and Name/Path fields under their labels and the Add Submodule button.
/// Edit Submodule (600px): the Remote URL (editable) and the Path, the Edit this submodule button, whether the
/// submodule is checked out at the commit the repository records, the commit it has checked out, and the Open /
/// Delete this submodule buttons.
pub fn draw(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    st: &mut SlideState,
    origin: Vec2,
    height: f32,
    max_width: f32,
) {
    let ui = cx.ui;
    let editing_kind = matches!(
        app.dialog.kind,
        Some(DialogKind::AddSubmodule | DialogKind::EditSubmodule)
    );
    let for_this_tab = match &app.dialog.workdir {
        None => app.active_workdir() == Some(state.repo.workdir()),
        Some(workdir) => workdir == state.repo.workdir(),
    };
    let open = editing_kind && for_this_tab;
    let t = cx.smooth("##slide_panel", open);
    if t <= 0.0 {
        return;
    }
    if open {
        st.editing = app.dialog.kind == Some(DialogKind::EditSubmodule);
        st.url.clone_from(&app.dialog.text_a);
        st.path.clone_from(&app.dialog.text_b);
        if app.dialog.just_opened {
            st.original_url.clone_from(&app.dialog.text_a);
            st.focus_panel = st.editing;
        }
    } else {
        st.focus_panel = false;
    }
    let editing = st.editing;
    let submodule = editing
        .then(|| {
            state
                .history
                .submodules
                .iter()
                .rfind(|entry| entry.path == st.path)
                .cloned()
        })
        .flatten();
    let p = cx.palette;
    // Narrower over a narrow graph, rather than over the detail panel.
    let width = cx
        .px(if editing { 600.0 } else { 400.0 })
        .min(cx.px(400.0).max(max_width - cx.px(10.0)));
    let shadow = cx.px(10.0);
    let visible = (width * t).round();
    // The child holds the panel's shown part and its shadow over the graph.
    cx.set_cursor(origin);
    ui.child_window("##slide_panel")
        .size([visible + shadow, height])
        .child_flags(ChildFlags::NONE)
        .flags(WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE)
        .build(ui, || {
            if st.focus_panel && !ui.is_mouse_down(MouseButton::Left) {
                ui.set_window_focus(None);
                st.focus_panel = false;
            }
            let draw = cx.window_draw_list();
            let x = origin.x + visible - width; // slides in from under the left panel
            let min = vec2(x, origin.y);
            let max = vec2(x + width, origin.y + height);
            draw.add_rect_filled(min, max, p.panel_bg1, 0.0);
            // The shadow on its right edge (box-shadow 0 0 10px rgba(0,0,0,.5)), over the graph.
            let steps = shadow as i32;
            for i in 0..steps {
                let i = i as f32;
                let alpha = 0.15 * (1.0 - i / shadow);
                draw.add_rect_filled(
                    vec2(max.x + i, min.y),
                    vec2(max.x + i + 1.0, max.y),
                    Color::new(0.0, 0.0, 0.0, alpha),
                    0.0,
                );
            }
            draw.add_rect_filled(vec2(max.x - 1.0, min.y), max, p.panel_border, 0.0);

            let pad = cx.px(20.0);
            // Header
            let section_icon = cx.px(16.0);
            cx.draw_fa_icon(
                draw,
                "far:layer-group",
                vec2(x + pad + cx.px(8.0), origin.y + cx.px(25.0)),
                section_icon,
                p.text_secondary,
            );
            let text_h = cx.text_size(cx.fonts.regular, 18.0, "A").y;
            cx.draw_text(
                draw,
                cx.fonts.regular,
                18.0,
                vec2(x + pad + cx.px(27.0), origin.y + cx.px(25.0) - text_h / 2.0),
                p.text_selected,
                if editing {
                    "Edit Submodule"
                } else {
                    "Add Submodule"
                },
            );
            let mut requests = Requests::default();
            {
                let close_min = vec2(max.x - pad - cx.px(16.0), origin.y + cx.px(17.0));
                cx.set_cursor(close_min);
                if ui.invisible_button("##slide_close", vec2(cx.px(16.0), cx.px(16.0))) {
                    requests.close = true;
                }
                let color = if ui.is_item_hovered() {
                    p.text_selected
                } else {
                    p.text_normal
                };
                let cross = cx.px(14.0);
                cx.draw_fa_icon(
                    draw,
                    "fas:times",
                    close_min + vec2(cx.px(8.0), cx.px(8.0)),
                    cross,
                    color,
                );
            }

            // The path defaults to the repository's name, as `git submodule add` picks it.
            let name = name_from_url(&st.url);

            let field_w = width - 2.0 * pad;
            let label = |cx: &mut Cx<'_>, text: &str, y: f32| {
                cx.draw_text(
                    draw,
                    cx.fonts.regular,
                    14.0,
                    vec2(x + pad, origin.y + y),
                    p.text_normal,
                    text,
                );
            };
            let field = |cx: &mut Cx<'_>,
                         id: &str,
                         value: &mut String,
                         hint: &str,
                         y: f32,
                         focus: bool,
                         flags: InputTextFlags| {
                cx.set_cursor(vec2(x + pad, origin.y + y));
                let line = cx.text_size(cx.fonts.regular, 14.0, "A").y;
                let _padding = ui.push_style_var(StyleVar::FramePadding([
                    cx.px(8.0),
                    (cx.px(32.0) - line) / 2.0,
                ]));
                let _font = cx.push_font(cx.fonts.regular, 14.0);
                if focus {
                    ui.set_keyboard_focus_here();
                }
                cx.input_text(
                    id,
                    value,
                    Some(hint),
                    InputTextFlags::ENTER_RETURNS_TRUE | flags,
                    field_w,
                )
            };
            const URL_HINT: &str = "https://github.com/owner/repo.git";
            let mut submit = false;
            {
                let _bg = ui.push_style_color(StyleColor::FrameBg, p.input_bg);
                let _hovered = ui.push_style_color(StyleColor::FrameBgHovered, p.input_bg);
                let _active = ui.push_style_color(StyleColor::FrameBgActive, p.input_bg);
                let _hint = ui.push_style_color(StyleColor::TextDisabled, p.text_disabled);
                let focus = open && app.dialog.just_opened;
                if open {
                    app.dialog.just_opened = false;
                }
                label(cx, "Remote URL", 58.0);
                if editing {
                    submit |= field(
                        cx,
                        "##submodule_url",
                        &mut st.url,
                        URL_HINT,
                        82.0,
                        false,
                        InputTextFlags::empty(),
                    );
                    label(cx, "Path", 132.0);
                    // The path is the submodule's identity here: shown, not edited.
                    let _text = ui.push_style_color(StyleColor::Text, p.text_disabled);
                    field(
                        cx,
                        "##submodule_path",
                        &mut st.path,
                        "",
                        156.0,
                        false,
                        InputTextFlags::READ_ONLY,
                    );
                } else {
                    submit |= field(
                        cx,
                        "##submodule_url",
                        &mut st.url,
                        URL_HINT,
                        82.0,
                        focus,
                        InputTextFlags::empty(),
                    );
                    label(cx, "Name/Path", 132.0);
                    let hint = if name.is_empty() {
                        "path/in/this/repository"
                    } else {
                        &name
                    };
                    submit |= field(
                        cx,
                        "##submodule_path",
                        &mut st.path,
                        hint,
                        156.0,
                        false,
                        InputTextFlags::empty(),
                    );
                }
            }
            // A button as wide as its label in the font it is drawn with, 12px either side.
            let button_width = |cx: &Cx<'_>, text: &str, at_least: f32| {
                at_least.max(ui.calc_text_size(text)[0].ceil() + cx.px(24.0))
            };
            cx.set_cursor(vec2(x + pad, origin.y + cx.px(206.0)));
            let url_changed = !st.url.is_empty() && st.url != st.original_url;
            if editing {
                let size = vec2(
                    button_width(cx, "Edit this submodule", cx.px(132.0)),
                    cx.px(28.0),
                );
                submit |= cx.success_button("Edit this submodule", size, url_changed);
                submit = submit && url_changed;
            } else {
                let size = vec2(button_width(cx, "Add Submodule", cx.px(112.0)), cx.px(28.0));
                submit |= cx.success_button("Add Submodule", size, !st.url.is_empty());
                submit = submit && !st.url.is_empty();
            }
            requests.submit = submit;

            // Edit Submodule: its state and revision, and what can be done with it.
            if let (true, Some(submodule)) = (editing, &submodule) {
                draw_details(
                    cx,
                    state,
                    st,
                    submodule,
                    &mut requests,
                    vec2(x + pad, origin.y),
                    field_w,
                );
            }

            if open {
                finish(cx, app, state, st, submodule.as_ref(), &name, requests);
            }
        });
}

/// The Edit panel under the form: the status line, the sync button, the current revision and the Open / Delete
/// buttons. `at` is the panel's content origin (its left padding, the panel's top).
fn draw_details(
    cx: &mut Cx<'_>,
    state: &RepoState,
    st: &mut SlideState,
    submodule: &Submodule,
    requests: &mut Requests,
    at: Vec2,
    field_w: f32,
) {
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let divider = |y: f32| {
        draw.add_rect_filled(
            vec2(at.x, y),
            vec2(at.x + field_w, y + 1.0),
            p.panel_border,
            0.0,
        );
    };
    let mut y = at.y + cx.px(254.0);
    divider(y);
    y += cx.px(24.0);

    // The status line: an icon and a sentence centred under the divider. A submodule only in .gitmodules (added,
    // never committed or staged) has no commit to check out.
    let recorded = submodule.recorded.is_some();
    let initialized = submodule.checked_out.is_some();
    let in_sync = submodule.in_sync();
    let sentence = status_sentence(submodule, state.repo.name());
    let (status_icon, status_color) = if in_sync {
        ("fas:check", p.green)
    } else {
        ("fas:exclamation-triangle", p.yellow)
    };
    {
        let icon_size = cx.px(13.0);
        let icon_w = cx.fa_icon_width(status_icon, icon_size);
        let text = cx.text_size(cx.fonts.regular, 14.0, &sentence);
        let text_w = text.x.min(field_w - icon_w - cx.px(5.0));
        let sx = (at.x + (field_w - icon_w - cx.px(5.0) - text_w) / 2.0).round();
        cx.draw_fa_icon(
            draw,
            status_icon,
            vec2(sx + icon_w / 2.0, y),
            icon_size,
            status_color,
        );
        cx.draw_text_ellipsis(
            draw,
            cx.fonts.regular,
            14.0,
            vec2(sx + icon_w + cx.px(5.0), (y - text.y / 2.0).round()),
            text_w + 1.0,
            status_color,
            &sentence,
        );
    }
    y += cx.px(18.0);
    if recorded && !in_sync {
        // Checks out the commit the repository records (cloning the submodule first when needed).
        let action = if initialized {
            "Update this submodule"
        } else {
            "Initialize this submodule"
        };
        let button_w = cx.ui.calc_text_size(action)[0].ceil() + cx.px(24.0);
        cx.set_cursor(vec2(
            (at.x + (field_w - button_w) / 2.0).round(),
            y + cx.px(4.0),
        ));
        if cx.success_button(action, vec2(button_w, cx.px(28.0)), !state.busy()) {
            let path = st.path.clone();
            requests.sync = Some(if initialized {
                (
                    "Update submodule",
                    ["submodule", "update", "--"]
                        .into_iter()
                        .map(String::from)
                        .chain([path])
                        .collect(),
                )
            } else {
                (
                    "Initialize submodule",
                    ["submodule", "update", "--init", "--"]
                        .into_iter()
                        .map(String::from)
                        .chain([path])
                        .collect(),
                )
            });
        }
        y += cx.px(44.0);
    }

    // Current Revision: the commit checked out in the submodule.
    if let Some(checked_out) = &submodule.checked_out {
        let folder = state.repo.workdir().join(&st.path);
        let commit = st.commit(&folder.to_string_lossy(), checked_out).cloned();
        y += cx.px(6.0);
        let caption = cx.text_size(cx.fonts.regular, 14.0, "Current Revision");
        cx.draw_text(
            draw,
            cx.fonts.regular,
            14.0,
            vec2(at.x, y),
            p.text_normal,
            "Current Revision",
        );
        if let Some(commit) = &commit {
            let date = format!("({})", cx.format_date_time(commit.author_time));
            cx.draw_text(
                draw,
                cx.fonts.regular,
                14.0,
                vec2(at.x + caption.x + cx.px(10.0), y),
                p.text_secondary,
                &date,
            );
        }
        y += caption.y + cx.px(6.0);

        // The box: "Commit" and "Commit Message" in bold monospace, their values beside them.
        const MONO: f32 = 12.0;
        const MAX_LINES: usize = 12;
        let line_h = cx.px(19.0);
        let value_x = at.x + cx.px(125.0);
        let value_w = at.x + field_w - cx.px(10.0) - value_x;
        let message = commit.as_ref().map_or_else(String::new, |commit| {
            let mut message = commit.summary.clone();
            if !commit.body.is_empty() {
                message.push_str("\n\n");
                message.push_str(&commit.body);
            }
            message.trim_end_matches(['\n', ' ']).to_owned()
        });
        let mut lines: Vec<String> = cx
            .wrap_text(cx.fonts.mono, MONO, &message, value_w)
            .into_iter()
            .map(str::to_owned)
            .collect();
        if lines.len() > MAX_LINES {
            lines.truncate(MAX_LINES);
            if let Some(last) = lines.last_mut() {
                last.push_str("...");
            }
        }
        let rows = 1.0 + lines.len().max(1) as f32;
        let box_min = vec2(at.x, y);
        let box_max = vec2(at.x + field_w, y + cx.px(16.0) + line_h * rows);
        draw.add_rect_filled(box_min, box_max, p.panel_bg0, 0.0);
        let mono_h = cx.text_size(cx.fonts.mono, MONO, "A").y;
        let mut row_y = y + cx.px(8.0) + (line_h - mono_h) / 2.0;
        cx.draw_text(
            draw,
            cx.fonts.mono,
            MONO,
            vec2(box_min.x + cx.px(5.0), row_y),
            p.text_normal,
            "Commit",
        );
        cx.draw_text(
            draw,
            cx.fonts.mono,
            MONO,
            vec2(value_x, row_y),
            p.text_selected,
            checked_out,
        );
        row_y += line_h;
        cx.draw_text(
            draw,
            cx.fonts.mono,
            MONO,
            vec2(box_min.x + cx.px(5.0), row_y),
            p.text_normal,
            "Commit Message",
        );
        for line in &lines {
            cx.draw_text(
                draw,
                cx.fonts.mono,
                MONO,
                vec2(value_x, row_y),
                p.text_selected,
                line,
            );
            row_y += line_h;
        }
        y = box_max.y + cx.px(11.0);
        divider(y);
        y += cx.px(21.0);
    }

    // Open / Delete this submodule, side by side.
    let button_w = (field_w - cx.px(10.0)) / 2.0;
    cx.set_cursor(vec2(at.x, y));
    // Not while a command runs here: this repository goes behind the submodule.
    requests.open_submodule = icon_label_button(
        cx,
        false,
        "##open_submodule",
        "far:folder-open",
        "Open this submodule",
        vec2(button_w, cx.px(32.0)),
        initialized && !state.busy(),
    );
    cx.set_cursor(vec2(at.x + button_w + cx.px(10.0), y));
    requests.delete_submodule = icon_label_button(
        cx,
        true,
        "##delete_submodule",
        "far:trash-alt",
        "Delete this submodule",
        vec2(button_w, cx.px(32.0)),
        !state.busy(),
    );
}

/// Carries out what the panel's controls asked for.
fn finish(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    st: &mut SlideState,
    submodule: Option<&Submodule>,
    name: &str,
    requests: Requests,
) {
    let ui = cx.ui;
    let Requests {
        submit,
        mut close,
        open_submodule,
        delete_submodule,
        sync,
    } = requests;
    app.dialog.text_a.clone_from(&st.url);
    app.dialog.text_b.clone_from(&st.path);
    if ui.is_window_focused_with_flags(FocusedFlags::CHILD_WINDOWS)
        && ui.is_key_pressed(Key::Escape)
    {
        close = true;
    }
    // The submodule is gone (deleted, or a checkout without it): nothing left to edit.
    if st.editing && submodule.is_none() {
        close = true;
    }
    if submit && st.editing {
        app.run_git_cli(
            state,
            "Edit submodule",
            ["submodule", "set-url", "--", &st.path, &st.url],
        );
        close = true;
    } else if submit {
        let path = if st.path.is_empty() { name } else { &st.path };
        app.run_git_cli(
            state,
            "Add submodule",
            ["submodule", "add", "--", &st.url, path],
        );
        close = true;
    }
    if let Some((title, args)) = sync {
        app.run_git_cli(state, title, args);
    }
    if close {
        app.dialog.close();
    } else if delete_submodule {
        if let Some(submodule) = submodule {
            ask_to_delete(app, state, submodule, &st.path);
        }
    } else if open_submodule {
        app.dialog.close();
        // Last: the tab shows the submodule from the next frame on (this state stays alive behind it).
        app.open_submodule(state, &st.path);
    }
}

/// The git commands that remove the submodule at `target`.
///
/// `git rm` drops the folder and its .gitmodules entry once deinit has emptied it, and stages the whole
/// .gitmodules; it refuses while .gitmodules has unstaged changes, so those are staged first. A submodule only
/// in .gitmodules is known to neither: its section is dropped from the file, and its folder (if any) stays.
fn removal_commands(submodule: &Submodule, target: &str, has_gitmodules: bool) -> Vec<Vec<String>> {
    let recorded = submodule.recorded.is_some();
    let words = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    let mut runs = Vec::new();
    if recorded {
        runs.push(words(&["submodule", "deinit", "-f", "--", target]));
    } else {
        runs.push(words(&[
            "config",
            "-f",
            ".gitmodules",
            "--remove-section",
            &format!("submodule.{}", submodule.name),
        ]));
    }
    if has_gitmodules {
        runs.push(words(&["add", "--", ".gitmodules"]));
    }
    if recorded {
        runs.push(words(&["rm", "-f", "--", target]));
    }
    runs
}

/// Replaces the panel with the confirmation of deleting the submodule at `path`.
fn ask_to_delete(app: &mut Application, state: &RepoState, submodule: &Submodule, path: &str) {
    let workdir = state.repo.workdir().to_path_buf();
    let runs = removal_commands(submodule, path, workdir.join(".gitmodules").exists());
    let recorded = submodule.recorded.is_some();
    let d = &mut app.dialog;
    d.open(DialogKind::Confirm);
    d.workdir = Some(workdir.clone());
    d.title = "Delete submodule".to_owned();
    d.message = if recorded {
        format!(
            "Delete the submodule '{path}'? Its folder and its entry in .gitmodules are removed, and the removal \
             is staged with .gitmodules."
        )
    } else {
        format!(
            "Delete the submodule '{path}'? Its entry in .gitmodules is removed and staged; its folder is left \
             as it is."
        )
    };
    d.confirm_label = "Delete".to_owned();
    d.danger = true;
    d.on_confirm = Some(Box::new(move |_, app| {
        app.with_repo(&workdir, |app, state| {
            app.run_git_cli_sequence(state, "Delete submodule", runs);
        });
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_name_comes_from_the_url() {
        assert_eq!(name_from_url("https://github.com/owner/repo.git"), "repo");
        assert_eq!(name_from_url("https://github.com/owner/repo/"), "repo");
        assert_eq!(name_from_url("git@github.com:owner/lib.git"), "lib");
        assert_eq!(name_from_url("C:\\projects\\tool"), "tool");
        assert_eq!(name_from_url("repo"), "repo");
        assert_eq!(name_from_url(""), "");
    }

    fn submodule(recorded: bool, checked_out: Option<&str>) -> Submodule {
        Submodule {
            name: "lib".to_owned(),
            path: "vendor/lib".to_owned(),
            url: String::new(),
            recorded: recorded.then(|| "a".repeat(40)),
            checked_out: checked_out.map(|sha| sha.repeat(40)),
        }
    }

    #[test]
    fn the_status_line_tells_where_the_submodule_stands() {
        assert_eq!(
            status_sentence(&submodule(false, None), "app"),
            "This submodule is not committed to your repository (app)."
        );
        assert_eq!(
            status_sentence(&submodule(true, None), "app"),
            "This submodule is not initialized."
        );
        assert_eq!(
            status_sentence(&submodule(true, Some("a")), "app"),
            "This submodule is in-sync with your repository (app)."
        );
        assert_eq!(
            status_sentence(&submodule(true, Some("b")), "app"),
            "This submodule is out of sync with your repository (app)."
        );
    }

    #[test]
    fn removing_a_committed_submodule_deinits_and_removes_it() {
        let runs = removal_commands(&submodule(true, Some("a")), "vendor/lib", true);
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0], ["submodule", "deinit", "-f", "--", "vendor/lib"]);
        assert_eq!(runs[1], ["add", "--", ".gitmodules"]);
        assert_eq!(runs[2], ["rm", "-f", "--", "vendor/lib"]);
    }

    #[test]
    fn removing_an_uncommitted_submodule_only_edits_gitmodules() {
        let runs = removal_commands(&submodule(false, None), "vendor/lib", true);
        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0],
            [
                "config",
                "-f",
                ".gitmodules",
                "--remove-section",
                "submodule.lib"
            ]
        );
        assert_eq!(runs[1], ["add", "--", ".gitmodules"]);
        assert_eq!(
            removal_commands(&submodule(false, None), "x", false).len(),
            1
        );
    }
}
