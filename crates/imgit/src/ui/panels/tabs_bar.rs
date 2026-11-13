//! The tabs bar, the New Tab page and the tab naming entries.

use std::path::Path;

use dear_imgui_rs::{
    ChildFlags, DrawFlags, InputTextFlags, ItemHoveredFlags, Key, KeyChord, KeyMods, MouseButton,
    StyleColor, StyleVar, WindowFlags,
};
use imgit_git::Author;

use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::app::tabs::TabGroup;
use crate::theme::{
    Color, FONT_SIZE_ROW, FONT_SIZE_SMALL, GRAPH_COLOR_COUNT, Ink, TABS_BAR_HEIGHT,
};
use crate::ui::draw::DrawList;
use crate::ui::menu::{MenuItems, menu_item, menu_submenu, popup_menu, separated_menu};
use crate::ui::panels::menus::tab_menu;
use crate::ui::panels::tab_popovers::{
    draw_notifications_popover, draw_profile_popover, draw_tabs_list_popover,
};
use crate::ui::widgets::contains_ci;
use crate::ui::{Cx, Icon, Vec2, raw, vec2};
use crate::window::CursorShape;

// .tabs-bar .tab: padding 0 25px 0 20px, icon, 6px, Open Sans Semibold 14px. Selected and hovered tabs get
// toolbar__bg1 with 4px top corners; the others end in a half-transparent 1px divider (80% of the height).
const TAB_PADDING_LEFT: f32 = 20.0;
const TAB_PADDING_RIGHT: f32 = 25.0;
const TAB_ICON_GAP: f32 = 6.0;
const TAB_FONT: f32 = 14.0;
const TAB_ICON_EM: f32 = 12.0;

/// The tabs bar's state between frames (the C++ function-local statics).
#[derive(Default)]
pub struct State {
    drag: TabDrag,
    /// The New Tab page's search box.
    filter: String,
}

/// A repository tab held and moved 4px is dragged along the bar under the system's drag-and-drop cursor, over
/// the other tabs, and swaps places with a neighbour once its leading edge passes that neighbour's centre.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct TabDrag {
    /// The tab held.
    index: Option<usize>,
    /// Moved far enough to count as a drag.
    active: bool,
    press_x: f32,
    /// The press, from the tab's left edge.
    grab: f32,
    width: f32,
    /// The repository tabs' extent, from the last frame.
    strip_min: f32,
    strip_max: f32,
}

impl TabDrag {
    /// Where the dragged tab's left edge is shown for the mouse at `mouse_x`: inside the strip.
    fn shown_x(&self, mouse_x: f32, strip_min: f32, strip_max: f32) -> f32 {
        (mouse_x - self.grab).clamp(strip_min, strip_min.max(strip_max - self.width))
    }
}

/// The slot tab `index` (shown with its left edge at `shown`, `width` wide) swaps into: a neighbour once its
/// leading edge passes that neighbour's centre.
fn drag_target(index: usize, shown: f32, width: f32, slots: &[f32], widths: &[f32]) -> usize {
    let centre_of = |n: usize| slots[n] + widths[n] / 2.0;
    if index > 0 && shown < centre_of(index - 1) {
        index - 1
    } else if index + 1 < slots.len() && shown + width > centre_of(index + 1) {
        index + 1
    } else {
        index
    }
}

/// What a frame of the bar asks for once its child window is closed.
#[derive(Default)]
struct BarEvents {
    /// The repository tab closed (its x, or a middle click).
    close_tab: Option<usize>,
    /// The repository tab right-clicked.
    menu_tab: Option<usize>,
    /// The tab group label right-clicked.
    menu_group: Option<String>,
}

/// What a tab drew and what happened to it.
#[derive(Default)]
struct TabOutcome {
    clicked: bool,
    /// The mouse is down on it.
    held: bool,
    width: f32,
    /// Closed by its x or a middle click.
    close: bool,
    /// Right-clicked.
    menu: bool,
}

/// One tab of the bar. `plain`: the "New Tab" entry, no icon and the regular 14px font. `offset`: the tab is
/// drawn this far from its slot (a dragged tab).
struct TabItem<'a> {
    id: &'a str,
    icon: Icon,
    label: &'a str,
    active: bool,
    closable: bool,
    index: usize,
    tip: Option<&'a str>,
    divider: bool,
    plain: bool,
    offset: f32,
}

fn tab_item(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    x: &mut f32,
    y: f32,
    height: f32,
    item: &TabItem<'_>,
) -> TabOutcome {
    let ui = cx.ui;
    let p = cx.palette;
    let font = if item.plain {
        cx.fonts.regular
    } else {
        cx.fonts.semibold
    };
    let icon_w = if item.plain {
        0.0
    } else {
        cx.icon_width(item.icon, cx.px(TAB_ICON_EM))
    };
    let gap = if item.plain { 0.0 } else { cx.px(TAB_ICON_GAP) };
    let text_w = cx.text_size(font, TAB_FONT, item.label).x.min(cx.px(200.0));
    let tab_width = cx.px(TAB_PADDING_LEFT) + icon_w + gap + text_w + cx.px(TAB_PADDING_RIGHT);
    let slot_x = *x;
    let mut outcome = TabOutcome {
        width: tab_width,
        ..TabOutcome::default()
    };

    cx.set_cursor(vec2(*x, y));
    let _id = ui.push_id(item.id);
    let _index = ui.push_id(item.index);
    if item.closable {
        ui.set_next_item_allow_overlap();
    }
    ui.invisible_button("##tab", vec2(tab_width, height));
    // The close button overlapping the tab takes its hover; the tab still counts as hovered for its plate and
    // the close button itself, else the button would vanish under the mouse every other frame.
    let hovered = ui.is_item_hovered_with_flags(ItemHoveredFlags::ALLOW_WHEN_OVERLAPPED_BY_ITEM);
    let hovered_self = ui.is_item_hovered();
    outcome.held = ui.is_item_active();
    outcome.clicked = ui.is_item_clicked_with_button(MouseButton::Left);
    // The rest draws where the tab is shown; the layout keeps its slot.
    *x += item.offset;
    if item.closable && ui.is_item_clicked_with_button(MouseButton::Middle) {
        outcome.close = true;
    }
    if item.closable && !item.plain && hovered && ui.is_mouse_released(MouseButton::Right) {
        outcome.menu = true;
    }
    // transition: background-color .25s ease
    let bg_t = cx.smooth(
        &format!("##tab_bg{}{}", item.index, item.id),
        item.active || hovered,
    );
    if bg_t > 0.0 {
        draw.add_rect_filled_corners(
            vec2(*x, y),
            vec2(*x + tab_width, y + height),
            p.toolbar_bg1.with_alpha(bg_t),
            cx.px(4.0),
            DrawFlags::ROUND_TOP,
        );
    }
    let mid = y + height / 2.0;
    let icon_x = *x + cx.px(TAB_PADDING_LEFT);
    if !item.plain {
        let color = if item.active {
            Ink(0.35).of(p)
        } else {
            p.text_secondary
        };
        cx.draw_icon(
            draw,
            item.icon,
            vec2(icon_x + icon_w / 2.0, mid),
            cx.px(TAB_ICON_EM),
            color,
        );
    }
    let line_h = cx.text_size(font, TAB_FONT, "A").y;
    cx.draw_text_ellipsis(
        draw,
        font,
        TAB_FONT,
        vec2(icon_x + icon_w + gap, mid - line_h / 2.0),
        text_w + 1.0,
        if item.active {
            p.text_selected
        } else {
            p.text_secondary
        },
        item.label,
    );
    if item.closable && (item.active || hovered) {
        let close_x = *x + tab_width - cx.px(13.0);
        let side = cx.px(16.0);
        cx.set_cursor(vec2(close_x - side / 2.0, mid - side / 2.0));
        ui.invisible_button("##close", vec2(side, side));
        let close_hovered = ui.is_item_hovered();
        cx.draw_icon(
            draw,
            Icon::Close,
            vec2(close_x, mid),
            cx.px(14.0),
            if close_hovered {
                p.text_selected
            } else {
                p.text_secondary
            },
        );
        if close_hovered {
            cx.tooltip("Close tab");
        }
        if ui.is_item_clicked_with_button(MouseButton::Left) {
            outcome.close = true;
        }
    }
    if hovered_self && let Some(tip) = item.tip {
        // As in the C++: the tooltip of the last item (the close button, when the tab shows one).
        cx.tooltip(tip);
    }
    *x = slot_x + tab_width;
    if item.divider && !item.active {
        draw.add_line(
            vec2(*x - 0.5, y + height * 0.1),
            vec2(*x - 0.5, y + height * 0.9),
            p.section_border.with_alpha(p.section_border.a * 0.5),
            1.0,
        );
    }
    outcome
}

/// A bare icon button of the tabs bar (folder, +, tab list, bell, cog). `active`: its popover is open (a 20%
/// #4D88FF plate, rad2).
struct BarIcon<'a> {
    id: &'a str,
    icon: Icon,
    center: Vec2,
    em: f32,
    color: Color,
    tip: &'a str,
    active: bool,
}

impl BarIcon<'_> {
    fn show(self, cx: &mut Cx<'_>) -> bool {
        let ui = cx.ui;
        let p = cx.palette;
        let side = cx.px(28.0);
        let min = self.center - vec2(side / 2.0, side / 2.0);
        cx.set_cursor(min);
        let pressed = ui.invisible_button(self.id, vec2(side, side));
        let hovered = ui.is_item_hovered();
        let draw = cx.window_draw_list();
        if self.active || hovered {
            // hover-row: 10% #4D88FF; the open popover's button: 20%.
            draw.add_rect_filled(
                min,
                min + vec2(side, side),
                if self.active {
                    p.selected_row
                } else {
                    p.hover_row
                },
                cx.px(2.0),
            );
        }
        cx.draw_icon(
            draw,
            self.icon,
            self.center,
            cx.px(self.em),
            if hovered { p.text_selected } else { self.color },
        );
        if hovered && !self.active {
            cx.tooltip(self.tip);
        }
        pressed
    }
}

/// The index of the repository tab opened on `workdir`, `None` when it is gone.
fn tab_index_of(app: &Application, workdir: &Path) -> Option<usize> {
    app.tabs.iter().position(|tab| tab.root_workdir == workdir)
}

fn save_tab_groups(app: &Application) {
    if app.settings.remember_tabs {
        app.save_state();
    }
}

/// A tab group's label in front of its tabs: a rounded plate in the group's colour holding its name (and its
/// tab count while collapsed). A click collapses or expands the group, a right-click opens its menu.
fn group_label(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    x: &mut f32,
    bar: Bar,
    app: &mut Application,
    (name, tab_count): (&str, usize),
    events: &mut BarEvents,
) {
    let Some(group) = app.find_tab_group(name) else {
        return;
    };
    let (color, collapsed) = (group.color, group.collapsed);
    let ui = cx.ui;
    let p = cx.palette;
    let font = cx.fonts.semibold;
    let label = if collapsed {
        format!("{name}  {tab_count}")
    } else {
        name.to_owned()
    };
    let text_w = cx
        .text_size(font, FONT_SIZE_SMALL, &label)
        .x
        .min(cx.px(160.0));
    let w = text_w + cx.px(16.0);
    let h = cx.px(20.0);
    let min = vec2(
        *x + cx.px(6.0),
        (bar.origin.y + (bar.height - h) / 2.0).round(),
    );
    let max = min + vec2(w, h);
    cx.set_cursor(min);
    let clicked = {
        let _group = ui.push_id("##tab_group");
        let _name = ui.push_id(name);
        ui.invisible_button("##label", vec2(w, h))
    };
    let hovered = ui.is_item_hovered();
    let plate = p.graph_color(color as i32);
    draw.add_rect_filled(
        min,
        max,
        plate.with_alpha(if hovered { 1.0 } else { 0.85 }),
        cx.px(4.0),
    );
    let line_h = cx.text_size(font, FONT_SIZE_SMALL, "A").y;
    cx.draw_text_ellipsis(
        draw,
        font,
        FONT_SIZE_SMALL,
        vec2(min.x + cx.px(8.0), min.y + (h - line_h) / 2.0),
        text_w + 1.0,
        p.text_inverse,
        &label,
    );
    if hovered {
        cx.tooltip(if collapsed {
            "Expand group"
        } else {
            "Collapse group"
        });
    }
    if clicked {
        if let Some(group) = app.find_tab_group_mut(name) {
            group.collapsed = !group.collapsed;
        }
        save_tab_groups(app);
    }
    if hovered && ui.is_mouse_released(MouseButton::Right) {
        events.menu_group = Some(name.to_owned());
    }
    *x = max.x + cx.px(4.0);
}

/// The context menu of a tab group's label.
fn tab_group_menu(app: &Application, name: &str) -> MenuItems {
    let Some(group) = app.find_tab_group(name) else {
        return MenuItems::new();
    };
    let collapsed = group.collapsed;
    let rename = name.to_owned();
    let toggle = name.to_owned();
    let recolor = name.to_owned();
    let ungroup = name.to_owned();
    let close = name.to_owned();
    separated_menu(vec![
        vec![
            menu_item("Rename group", move |_, app| {
                let from = rename.clone();
                open_text_prompt(
                    app,
                    format!("Rename tab group '{rename}' to:"),
                    rename,
                    false,
                    move |_, app, to| app.rename_tab_group(&from, to),
                );
            }),
            menu_item(
                if collapsed {
                    "Expand group"
                } else {
                    "Collapse group"
                },
                move |_, app| {
                    if let Some(group) = app.find_tab_group_mut(&toggle) {
                        group.collapsed = !group.collapsed;
                        save_tab_groups(app);
                    }
                },
            ),
            menu_item("Next color", move |_, app| {
                if let Some(group) = app.find_tab_group_mut(&recolor) {
                    group.color = (group.color + 1) % GRAPH_COLOR_COUNT;
                    save_tab_groups(app);
                }
            }),
        ],
        vec![
            menu_item("Ungroup", move |_, app| app.ungroup_tabs(&ungroup)),
            menu_item("Close group", move |_, app| app.close_tab_group(&close)),
        ],
    ])
}

/// Asks for a name in the prompt bar, starting from `initial`; `allow_empty` lets an empty answer through.
/// `on_text` runs with the entered text once it is confirmed.
pub fn open_text_prompt(
    app: &mut Application,
    message: impl Into<String>,
    initial: impl Into<String>,
    allow_empty: bool,
    on_text: impl FnOnce(&mut Cx<'_>, &mut Application, &str) + 'static,
) {
    app.dialog.open(DialogKind::TextPrompt);
    app.dialog.message = message.into();
    app.dialog.text_a = initial.into();
    app.dialog.flag = allow_empty;
    app.dialog.on_text = Some(Box::new(on_text));
}

/// The last component of a working directory, trailing separators ignored.
fn folder_name(workdir: &str) -> &str {
    let trimmed = workdir.trim_end_matches(['/', '\\']);
    trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed)
}

/// The repository tab's naming entries: rename tab, its group, Alias repository.
#[must_use]
pub fn tab_naming_items(_cx: &mut Cx<'_>, app: &mut Application, index: usize) -> MenuItems {
    naming_items(app, index)
}

fn naming_items(app: &Application, index: usize) -> MenuItems {
    let Some(tab) = app.tabs.get(index) else {
        return MenuItems::new();
    };
    let workdir = tab.root_workdir.clone();
    let tab_name = tab.name.clone();
    let tab_group = tab.group.clone();
    let mut items = MenuItems::new();
    let current = tab_name
        .clone()
        .unwrap_or_else(|| app.repo_display_name(&workdir));
    items.push(menu_item("Rename tab", {
        let workdir = workdir.clone();
        move |_, app| {
            open_text_prompt(app, "Rename tab to:", current, true, move |_, app, name| {
                // The repository's own name, or nothing, gives the tab back its default.
                let name = if name == app.repo_display_name(&workdir) {
                    ""
                } else {
                    name
                };
                if let Some(index) = tab_index_of(app, &workdir) {
                    app.rename_tab(index, name);
                }
            });
        }
    }));
    if tab_name.is_some() {
        let workdir = workdir.clone();
        items.push(menu_item("Reset tab name", move |_, app| {
            if let Some(index) = tab_index_of(app, &workdir) {
                app.rename_tab(index, "");
            }
        }));
    }
    let set_group = |workdir: &Path, group: Option<String>| {
        let workdir = workdir.to_path_buf();
        move |_: &mut Cx<'_>, app: &mut Application| {
            if let Some(index) = tab_index_of(app, &workdir) {
                app.set_tab_group(index, group.as_deref());
            }
        }
    };
    let mut groups = MenuItems::new();
    groups.push(menu_item("New group...", {
        let workdir = workdir.clone();
        move |_, app| {
            open_text_prompt(
                app,
                "Name the new tab group:",
                "",
                false,
                move |_, app, name| {
                    if let Some(index) = tab_index_of(app, &workdir) {
                        app.set_tab_group(index, Some(name));
                    }
                },
            );
        }
    }));
    for group in &app.tab_groups {
        if tab_group.as_deref() != Some(group.name.as_str()) {
            groups.push(menu_item(
                group.name.clone(),
                set_group(&workdir, Some(group.name.clone())),
            ));
        }
    }
    items.push(menu_submenu("Add tab to group", groups));
    if tab_group.is_some() {
        items.push(menu_item(
            "Remove tab from group",
            set_group(&workdir, None),
        ));
    }
    let alias = app.repo_display_name(&workdir);
    items.push(menu_item("Alias repository", move |_, app| {
        let path = workdir.to_string_lossy().into_owned();
        let folder = folder_name(&path).to_owned();
        let initial = if alias == folder {
            String::new()
        } else {
            alias
        };
        open_text_prompt(
            app,
            format!("Alias repository '{folder}' as (empty: no alias):"),
            initial,
            true,
            move |_, app, name| app.set_repo_alias(&workdir, name),
        );
    }));
    items
}

/// Where the bar is, for the helpers that lay out inside it.
#[derive(Clone, Copy)]
struct Bar {
    origin: Vec2,
    width: f32,
    height: f32,
    mid: f32,
}

/// The tabs bar: repository tabs, groups, the "+" button, the folder, the notifications and profile buttons.
pub fn draw_tabs_bar(cx: &mut Cx<'_>, app: &mut Application) {
    app.with_panel_state(
        |panels| &mut panels.tabs_bar,
        |app, state| tabs_bar(cx, app, state),
    );
}

fn tabs_bar(cx: &mut Cx<'_>, app: &mut Application, state: &mut State) {
    let ui = cx.ui;
    let height = cx.px(TABS_BAR_HEIGHT);
    let origin = cx.cursor();
    let width = cx.avail().x;
    let bar = Bar {
        origin,
        width,
        height,
        mid: origin.y + height / 2.0,
    };
    let mut events = BarEvents::default();
    ui.child_window("##tabs_bar")
        .size([width, height])
        .child_flags(ChildFlags::NONE)
        .flags(WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE)
        .build(ui, || bar_contents(cx, app, state, bar, &mut events));

    if let Some(index) = events.close_tab {
        app.close_tab(index);
    }
    if let Some(index) = events.menu_tab {
        let items = tab_menu(cx, app, index);
        popup_menu(cx, items, None);
    } else if let Some(name) = &events.menu_group {
        let items = tab_group_menu(app, name);
        popup_menu(cx, items, None);
    }
}

fn bar_contents(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut State,
    bar: Bar,
    events: &mut BarEvents,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let Bar {
        origin,
        width,
        height,
        mid,
    } = bar;
    let draw = cx.window_draw_list();
    draw.add_rect_filled(origin, origin + vec2(width, height), p.toolbar_bg0, 0.0);

    // Left: the window's own controls where the bar is the title bar (macOS), open-repo folder, repository
    // tabs, +
    let left = origin.x + cx.px(cx.shell.title_bar_inset());
    let folder = BarIcon {
        id: "##open_repo",
        icon: Icon::Folder,
        center: vec2(left + cx.px(21.0), mid),
        em: 14.0,
        color: p.text_secondary,
        tip: "Repository Management",
        active: app.repo_management_open,
    };
    if folder.show(cx) {
        app.repo_management_open = !app.repo_management_open;
    }
    let mut x = left + cx.px(41.0);
    let tab_count = app.tabs.len();
    let mouse = cx.mouse_pos();
    if state.drag.index.is_some_and(|index| index >= tab_count) {
        state.drag = TabDrag::default();
    }
    // The dragged tab draws over the others.
    draw.channels_split(2);
    draw.channels_set_current(0);
    let strip_min = x;
    let mut slots: Vec<f32> = Vec::with_capacity(tab_count); // each tab's left edge
    let mut widths: Vec<f32> = Vec::with_capacity(tab_count);
    let mut held_tab: Option<(usize, f32)> = None;
    let mut previous_group: Option<String> = None;
    for i in 0..tab_count {
        let dragging = state.drag.active && state.drag.index == Some(i);
        // A group's label goes in front of each run of its tabs; a collapsed group hides them, but for the
        // active (or dragged) one.
        let group_name = app.tabs[i].group.clone();
        let known = group_name
            .as_deref()
            .is_some_and(|name| app.find_tab_group(name).is_some());
        if known && group_name != previous_group {
            let run = app.tabs[i..]
                .iter()
                .take_while(|tab| tab.group == group_name)
                .count();
            if let Some(name) = group_name.as_deref() {
                group_label(cx, draw, &mut x, bar, app, (name, run), events);
            }
        }
        let group: Option<TabGroup> = group_name
            .as_deref()
            .and_then(|name| app.find_tab_group(name))
            .cloned();
        previous_group = group_name;
        if group
            .as_ref()
            .is_some_and(|group| group.collapsed && app.active_tab != Some(i) && !dragging)
        {
            slots.push(x);
            widths.push(0.0);
            continue;
        }
        let mut offset = 0.0;
        if dragging {
            offset = state
                .drag
                .shown_x(mouse.x, state.drag.strip_min, state.drag.strip_max)
                - x;
            draw.channels_set_current(1);
        }
        slots.push(x);
        let label = app.tab_label(i);
        let tip = app.tabs[i].workdir.to_string_lossy().into_owned();
        let outcome = tab_item(
            cx,
            draw,
            &mut x,
            origin.y,
            height,
            &TabItem {
                id: "repo",
                icon: Icon::Branch,
                label: &label,
                active: app.active_tab == Some(i),
                closable: true,
                index: i,
                tip: Some(&tip),
                divider: app.active_tab != Some(i + 1),
                plain: false,
                offset,
            },
        );
        if outcome.clicked {
            app.active_tab = Some(i);
        }
        if outcome.close {
            events.close_tab = Some(i);
        }
        if outcome.menu {
            events.menu_tab = Some(i);
        }
        widths.push(outcome.width);
        if let Some(group) = &group {
            // The group's colour along the tab's bottom edge.
            let tab_left = slots[i] + offset;
            draw.add_rect_filled(
                vec2(tab_left + cx.px(2.0), origin.y + height - cx.px(2.0)),
                vec2(tab_left + outcome.width - cx.px(2.0), origin.y + height),
                p.graph_color(group.color as i32),
                0.0,
            );
        }
        if outcome.held {
            held_tab = Some((i, slots[i]));
        }
        if dragging {
            draw.channels_set_current(0);
        }
    }
    draw.channels_merge();
    // The drag: picked up on the press, moving after 4px, dropped with the mouse.
    if let Some((held, held_x)) = held_tab
        && state.drag.index.is_none()
    {
        state.drag = TabDrag {
            index: Some(held),
            active: false,
            press_x: mouse.x,
            grab: mouse.x - held_x,
            width: widths[held],
            ..TabDrag::default()
        };
    }
    if state.drag.index.is_some() {
        state.drag.strip_min = strip_min;
        state.drag.strip_max = x;
        if !ui.is_mouse_down(MouseButton::Left) {
            state.drag = TabDrag::default();
        } else {
            if !state.drag.active && (mouse.x - state.drag.press_x).abs() >= cx.px(4.0) {
                state.drag.active = true;
            }
            if state.drag.active {
                cx.set_drag_cursor(CursorShape::Move);
            }
        }
    }
    if state.drag.active
        && let Some(index) = state.drag.index
    {
        let shown = state.drag.shown_x(mouse.x, strip_min, x);
        let to = drag_target(index, shown, state.drag.width, &slots, &widths);
        if to != index {
            app.move_tab(index, to);
            state.drag.index = Some(to);
        }
    }
    if app.new_tab_open || tab_count == 0 {
        let outcome = tab_item(
            cx,
            draw,
            &mut x,
            origin.y,
            height,
            &TabItem {
                id: "new_tab",
                icon: Icon::Plus,
                label: "New Tab",
                active: app.active_tab.is_none(),
                closable: true,
                index: 0,
                tip: None,
                divider: true,
                plain: true,
                offset: 0.0,
            },
        );
        if outcome.clicked {
            app.active_tab = None;
        }
        // The New Tab page is all there is without repositories, so it stays open then.
        if outcome.close && tab_count > 0 {
            app.new_tab_open = false;
            if app.active_tab.is_none() {
                app.active_tab = Some(tab_count - 1);
            }
        }
    }
    let new_tab = BarIcon {
        id: "##new_tab",
        icon: Icon::Plus,
        center: vec2(x + cx.px(21.5), mid),
        em: 14.0,
        color: p.text_secondary,
        tip: "New Tab (Ctrl+T)",
        active: false,
    };
    if new_tab.show(cx) {
        app.new_tab_open = true;
        app.active_tab = None;
    }

    // Right: tabs list, notifications, preferences, profile.
    let bright = Ink(0.9).of(p);
    let mut right = origin.x + width;
    {
        let current = app.profiles.current().clone();
        let font = cx.fonts.regular;
        let text_w = cx.text_size(font, FONT_SIZE_SMALL, &current.name).x;
        let caret_w = cx.icon_width(Icon::CaretDown, cx.px(12.0));
        let avatar = cx.px(20.0);
        let w = avatar + cx.px(6.0) + text_w + cx.px(5.0) + caret_w;
        let x0 = right - cx.px(14.0) - w;
        cx.set_cursor(vec2(x0, origin.y + cx.px(4.0)));
        ui.invisible_button("##profile", vec2(w, height - cx.px(8.0)));
        let hovered = ui.is_item_hovered();
        let clicked = ui.is_item_clicked_with_button(MouseButton::Left);
        let open = ui.is_popup_open("##profile_menu");
        if open || hovered {
            draw.add_rect_filled(
                vec2(x0 - cx.px(3.0), origin.y + cx.px(3.0)),
                vec2(x0 + w + cx.px(3.0), origin.y + cx.px(31.0)),
                p.accent.with_alpha(if open { 0.2 } else { 0.1 }),
                cx.px(2.0),
            );
        }
        let color = if hovered { p.text_selected } else { bright };
        let author = Author {
            name: current.user_name.clone(),
            email: current.user_email.clone(),
        };
        cx.draw_avatar(
            draw,
            vec2(x0 + avatar / 2.0, mid),
            avatar,
            &author,
            Some(cx.px(3.0)),
        );
        let line_h = cx.text_size(font, FONT_SIZE_SMALL, "A").y;
        cx.draw_text(
            draw,
            font,
            FONT_SIZE_SMALL,
            vec2(x0 + avatar + cx.px(6.0), mid - line_h / 2.0),
            color,
            &current.name,
        );
        cx.draw_icon(
            draw,
            Icon::CaretDown,
            vec2(x0 + w - caret_w / 2.0, mid),
            cx.px(12.0),
            color,
        );
        if clicked {
            cx.open_popover("##profile_menu");
        }
        draw_profile_popover(cx, app, vec2(x0 + w, origin.y + cx.px(31.0)));
        right = x0;
    }
    let prefs = BarIcon {
        id: "##prefs",
        icon: Icon::Gear,
        center: vec2(right - cx.px(24.5), mid),
        em: 16.0,
        color: bright,
        tip: "Preferences",
        active: false,
    };
    if prefs.show(cx) {
        app.preferences_open = true;
    }
    let bell = BarIcon {
        id: "##notifications",
        icon: Icon::Bell,
        center: vec2(right - cx.px(57.0), mid),
        em: 16.0,
        color: bright,
        tip: "Notifications",
        active: ui.is_popup_open("##notifications"),
    };
    if bell.show(cx) || app.notifications_requested {
        app.notifications_requested = false;
        cx.open_popover("##notifications");
    }
    let unread = app.toasts.unread();
    if unread > 0 {
        // The unread count: a 12px #4D88FF disc at the bell's top right, 8px semibold white.
        let badge = vec2(
            right - cx.px(57.0) - cx.px(14.0) + cx.px(20.3),
            origin.y + cx.px(11.0),
        );
        draw.add_circle_filled(badge, cx.px(6.0), p.accent, 0);
        let count = unread.min(99).to_string();
        let size = cx.text_size(cx.fonts.semibold, 8.0, &count);
        cx.draw_text(
            draw,
            cx.fonts.semibold,
            8.0,
            badge - size / 2.0,
            Color::WHITE,
            &count,
        );
    }
    draw_notifications_popover(
        cx,
        app,
        vec2(right - cx.px(57.0) + cx.px(14.0), origin.y + cx.px(31.0)),
    );
    let chord =
        ui.is_key_chord_pressed(KeyChord::new(Key::A).with_mods(KeyMods::CTRL | KeyMods::SHIFT));
    let tabs_list = BarIcon {
        id: "##tabs_list",
        icon: Icon::ChevronDown,
        center: vec2(right - cx.px(90.5), mid),
        em: 16.0,
        color: bright,
        tip: "Tabs list (Ctrl + Shift + A)",
        active: ui.is_popup_open("##tabs_list"),
    };
    if tabs_list.show(cx) || chord || app.tabs_list_requested {
        app.tabs_list_requested = false;
        cx.open_popover("##tabs_list");
    }
    draw_tabs_list_popover(
        cx,
        app,
        vec2(right - cx.px(90.5) + cx.px(14.0), origin.y + cx.px(31.0)),
    );

    // Where the bar is the title bar (macOS), its empty part, from the + to the right-hand icons, acts as one
    // (the platform takes the presses there); in window coordinates, which are the viewport's.
    let (viewport_pos, _) = raw::viewport(ui);
    let empty_min = x + cx.px(33.0);
    let empty_max = right - cx.px(103.0);
    cx.shell.set_title_bar_drag_region(
        [empty_min - viewport_pos.x, origin.y - viewport_pos.y],
        [
            empty_max - viewport_pos.x,
            origin.y + height - viewport_pos.y,
        ],
    );
}

/// The New Tab page: a centred container (max 2000px, 40px 100px padding) holding Repositories (Open / Clone /
/// Create, the search box, Recent).
pub fn draw_new_tab_view(cx: &mut Cx<'_>, app: &mut Application, size: Vec2) {
    app.with_panel_state(
        |panels| &mut panels.tabs_bar,
        |app, state| new_tab_view(cx, app, state, size),
    );
}

/// The page's text with its CSS box top at `box_top`: Chromium's baseline for `font_size`.
fn text_at(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    font: crate::fonts::Font,
    font_size: f32,
    pos: (f32, f32),
    color: Color,
    text: &str,
) {
    let (x, box_top) = pos;
    let ascent = font_size * 2189.0 / 2048.0 * cx.scale;
    cx.draw_text(
        draw,
        font,
        font_size,
        vec2(x, (box_top + ascent.round()).round() - ascent.ceil()),
        color,
        text,
    );
}

/// .btn-plain.fs-4: panel-bg0, a 1px 8% border, radius 4, 10px padding, a 16px icon and 16px white text.
/// Returns whether it was clicked and its width.
fn plain_button(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    id: &str,
    icon: &str,
    label: &str,
    pos: Vec2,
) -> (bool, f32) {
    let ui = cx.ui;
    let p = cx.palette;
    let icon_w = cx.fa_icon_width(icon, cx.px(16.0));
    let text_w = cx.text_size(cx.fonts.regular, 16.0, label).x;
    let w = cx.px(22.0) + icon_w + cx.px(10.0) + text_w;
    let min = pos;
    let max = pos + vec2(w, cx.px(44.8));
    cx.set_cursor(min);
    let clicked = ui.invisible_button(id, max - min);
    let hovered = ui.is_item_hovered();
    draw.add_rect_filled(
        min,
        max,
        if hovered { p.panel_bg1 } else { p.panel_bg0 },
        cx.px(4.0),
    );
    draw.add_rect(min, max, p.section_border, cx.px(4.0), 1.0);
    let mid = pos.y + cx.px(22.4);
    cx.draw_fa_icon(
        draw,
        icon,
        vec2(pos.x + cx.px(11.0) + icon_w / 2.0, mid),
        cx.px(16.0),
        p.text_normal,
    );
    text_at(
        cx,
        draw,
        cx.fonts.regular,
        16.0,
        (pos.x + cx.px(21.0) + icon_w, pos.y + cx.px(11.0)),
        p.text_selected,
        label,
    );
    (clicked, w)
}

fn new_tab_view(cx: &mut Cx<'_>, app: &mut Application, state: &mut State, size: Vec2) {
    let ui = cx.ui;
    let mut open_request: Option<String> = None;
    ui.child_window("##new_tab")
        .size(size.into())
        .build(ui, || {
            let p = cx.palette;
            let origin = cx.cursor();
            let draw = cx.window_draw_list();
            let container = cx.px(2000.0).min(size.x);
            let content_x = origin.x + (size.x - container) / 2.0 + cx.px(100.0);
            let content_w = container - cx.px(200.0);
            let left_w = content_w;
            let top = origin.y + cx.px(40.0);

            // Repositories
            text_at(
                cx,
                draw,
                cx.fonts.regular,
                20.0,
                (content_x, top - cx.px(3.0)),
                p.text_normal,
                "Repositories",
            );
            {
                let mut x = content_x;
                let y = top + cx.px(42.0);
                let (open, ow) =
                    plain_button(cx, draw, "##nt_open", "fas:folder-open", "Open", vec2(x, y));
                x += ow + cx.px(10.0);
                let (clone, cw) = plain_button(
                    cx,
                    draw,
                    "##nt_clone",
                    "fas:cloud-download",
                    "Clone",
                    vec2(x, y),
                );
                x += cw + cx.px(10.0);
                let (create, _) = plain_button(
                    cx,
                    draw,
                    "##nt_create",
                    "fas:plus-square",
                    "Create",
                    vec2(x, y),
                );
                if open {
                    app.browse_for_repository(cx);
                }
                if clone {
                    app.dialog.open(DialogKind::CloneRepo);
                }
                if create {
                    app.dialog.open(DialogKind::InitRepo);
                }
            }
            {
                // .input-bg with a 20px left padding for the magnifier, focused on open.
                let min = vec2(content_x, top + cx.px(106.8));
                let max = vec2(content_x + left_w, min.y + cx.px(29.1));
                draw.add_rect_filled(min, max, p.input_bg, cx.px(3.0));
                cx.set_cursor(vec2(min.x + cx.px(20.0), min.y));
                let text_h = cx.font_px(cx.fonts.regular, FONT_SIZE_ROW);
                let _padding = ui.push_style_var(StyleVar::FramePadding([
                    cx.px(1.0),
                    (max.y - min.y - text_h) / 2.0,
                ]));
                let _border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
                let _bg = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
                let _bg_hovered =
                    ui.push_style_color(StyleColor::FrameBgHovered, Color::TRANSPARENT);
                let _bg_active = ui.push_style_color(StyleColor::FrameBgActive, Color::TRANSPARENT);
                let font = cx.push_font(cx.fonts.regular, FONT_SIZE_ROW);
                if ui.is_window_appearing()
                    || ui.is_key_chord_pressed(
                        KeyChord::new(Key::F).with_mods(KeyMods::CTRL | KeyMods::ALT),
                    )
                {
                    ui.set_keyboard_focus_here();
                }
                let hint = cx.shortcut_label("Search repositories (Ctrl + Alt + F)");
                cx.input_text(
                    "##repo_search",
                    &mut state.filter,
                    Some(&hint),
                    InputTextFlags::empty(),
                    max.x - min.x - cx.px(25.0),
                );
                let focused = ui.is_item_active();
                drop(font);
                draw.add_rect(
                    min,
                    max,
                    if focused { p.accent } else { p.section_border },
                    cx.px(3.0),
                    1.0,
                );
                cx.draw_fa_icon(
                    draw,
                    "far:search",
                    vec2(min.x + cx.px(13.0), min.y + cx.px(14.5)),
                    cx.px(12.0),
                    p.text_secondary,
                );
            }
            text_at(
                cx,
                draw,
                cx.fonts.regular,
                14.0,
                (content_x, top + cx.px(153.98)),
                p.text_normal,
                "Recent",
            );
            let mut y = top + cx.px(176.4);
            for (i, recent) in app.recent_repos.iter().enumerate() {
                let path = recent.to_string_lossy();
                let name = app.repo_display_name(recent);
                if !state.filter.is_empty()
                    && !contains_ci(&path, &state.filter)
                    && !contains_ci(&name, &state.filter)
                {
                    continue;
                }
                let shown = display_path(&path);
                // a.bold.btn-link (2px padding, 1px border) then the path in text-disabled, 10px on.
                let name_w = cx.text_size(cx.fonts.semibold, 14.0, &name).x + cx.px(6.0);
                cx.set_cursor(vec2(content_x, y));
                let clicked = {
                    let _id = ui.push_id(i);
                    ui.invisible_button("##recent", vec2(name_w, cx.px(22.0)))
                };
                let hovered = ui.is_item_hovered();
                text_at(
                    cx,
                    draw,
                    cx.fonts.semibold,
                    14.0,
                    (content_x + cx.px(3.0), y + cx.px(1.0)),
                    p.link,
                    &name,
                );
                if hovered {
                    let underline = (y + cx.px(17.0)).round();
                    draw.add_rect_filled(
                        vec2(content_x + cx.px(3.0), underline),
                        vec2(content_x + name_w - cx.px(3.0), underline + 1.0),
                        p.link,
                        0.0,
                    );
                }
                // The path is cut at the column's edge, without an ellipsis.
                draw.with_clip_rect(
                    vec2(content_x, y),
                    vec2(content_x + left_w, y + cx.px(22.0)),
                    true,
                    || {
                        text_at(
                            cx,
                            draw,
                            cx.fonts.regular,
                            14.0,
                            (content_x + name_w + cx.px(10.0), y),
                            p.text_disabled,
                            &shown,
                        );
                    },
                );
                if clicked {
                    open_request = Some(path.into_owned());
                    break;
                }
                y += cx.px(22.0);
            }
        });
    if let Some(path) = open_request {
        app.open_repository(&path);
    }
}

/// A recent repository's path as the page lists it: backslashes, no trailing one.
fn display_path(path: &str) -> String {
    path.replace('/', "\\").trim_end_matches('\\').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::icons::Icons;

    #[test]
    fn a_dragged_tab_swaps_past_its_neighbours_centre() {
        // Three tabs 100 wide.
        let slots = [0.0, 100.0, 200.0];
        let widths = [100.0; 3];
        assert_eq!(drag_target(1, 100.0, 100.0, &slots, &widths), 1);
        // Its left edge is left of the previous tab's centre (50).
        assert_eq!(drag_target(1, 49.0, 100.0, &slots, &widths), 0);
        assert_eq!(drag_target(1, 50.0, 100.0, &slots, &widths), 1);
        // Its right edge is right of the next tab's centre (250).
        assert_eq!(drag_target(1, 151.0, 100.0, &slots, &widths), 2);
        assert_eq!(drag_target(1, 150.0, 100.0, &slots, &widths), 1);
        // The ends have no neighbour beyond them.
        assert_eq!(drag_target(0, -30.0, 100.0, &slots, &widths), 0);
        assert_eq!(drag_target(2, 400.0, 100.0, &slots, &widths), 2);
    }

    #[test]
    fn the_dragged_tab_stays_inside_the_strip() {
        let drag = TabDrag {
            grab: 10.0,
            width: 100.0,
            ..TabDrag::default()
        };
        assert_eq!(drag.shown_x(5.0, 40.0, 340.0), 40.0);
        assert_eq!(drag.shown_x(150.0, 40.0, 340.0), 140.0);
        assert_eq!(drag.shown_x(900.0, 40.0, 340.0), 240.0);
        // A strip narrower than the tab: it stays at the start.
        assert_eq!(drag.shown_x(900.0, 40.0, 100.0), 40.0);
    }

    #[test]
    fn folder_names_ignore_trailing_separators() {
        assert_eq!(folder_name("C:\\work\\repo"), "repo");
        assert_eq!(folder_name("C:\\work\\repo\\\\"), "repo");
        assert_eq!(folder_name("/home/me/repo/"), "repo");
        assert_eq!(folder_name("repo"), "repo");
        assert_eq!(folder_name(""), "");
    }

    #[test]
    fn recent_paths_are_listed_with_backslashes() {
        assert_eq!(display_path("C:/work/repo/"), "C:\\work\\repo");
        assert_eq!(display_path("D:\\x"), "D:\\x");
    }

    #[test]
    fn the_icons_of_the_page_exist() {
        let icons = Icons::new();
        for name in [
            "fas:folder-open",
            "fas:cloud-download",
            "fas:plus-square",
            "far:search",
        ] {
            assert!(icons.find(name).is_some(), "{name}");
        }
    }

    #[test]
    fn the_naming_entries_follow_the_tab() {
        let scratch = tempfile::tempdir().unwrap();
        let repo = scratch.path().join("repo");
        drop(imgit_git::Repository::init(&repo).unwrap());
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        assert!(naming_items(&app, 0).is_empty());
        app.open_repository(&repo.to_string_lossy());
        let labels = |app: &Application| -> Vec<String> {
            naming_items(app, 0)
                .iter()
                .map(|item| item.label.clone())
                .collect()
        };
        assert_eq!(
            labels(&app),
            ["Rename tab", "Add tab to group", "Alias repository"]
        );
        // Renamed and grouped: the entries to undo that appear, and the group is not offered again.
        app.rename_tab(0, "Mine");
        app.set_tab_group(0, Some("Team"));
        app.set_tab_group(0, Some("Team"));
        assert_eq!(
            labels(&app),
            [
                "Rename tab",
                "Reset tab name",
                "Add tab to group",
                "Remove tab from group",
                "Alias repository"
            ]
        );
        let items = naming_items(&app, 0);
        let groups = &items[2].submenu;
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "New group...");
    }
}
