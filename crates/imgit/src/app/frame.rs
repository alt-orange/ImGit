//! One frame of the application: the menu bar, the tabs bar, the active tab's layout (`draw_repo_tab`), the
//! status bar, the dialogs, the toasts and the open menus.

use dear_imgui_rs::{ChildFlags, Condition, WindowFlags};
use imgit_app::RepoState;
use imgit_app::preferences::{pref, preferences};
use winit::window::UserAttentionType;

use crate::app::application::{Application, VERSION};
use crate::app::dialog::DialogKind;
use crate::app::git_ops::STATUS_POLL_SECONDS;
use crate::app::menu_bar::{BAR_KEYS, BAR_LABELS, bar_menu};
use crate::app::toasts::{ToastKind, toast_corner};
use crate::app_menu::has_native_menu_bar;
use crate::theme::{
    Color, FONT_SIZE_ROW, FONT_SIZE_SMALL, INFO_BAR_HEIGHT, LEFT_PANEL_DEFAULT_WIDTH,
    MENU_BAR_HEIGHT, TABS_BAR_HEIGHT, TOOLBAR_HEIGHT,
};
use crate::ui::menu::{
    draw_menus, menu_keyboard_mode, menu_open, open_bar_menu, popup_bar_menu, set_menu_bar,
    strip_mnemonic,
};
use crate::ui::panels::{
    detail_panel, dialogs, diff_view, file_history, fuzzy_finder, graph_view, left_panel,
    merge_view, preferences as preferences_panel, rebase_panel, repo_management, tabs_bar,
    terminal_panel, toolbar,
};
use crate::ui::widgets::ResizeAxis;
use crate::ui::{Cx, Rect, Vec2, raw, vec2};

impl Application {
    /// Draws the whole window. Call once per frame between `NewFrame` and `Render`, with the `Cx` of the frame.
    pub fn draw_frame(&mut self, cx: &mut Cx<'_>) {
        let ui = cx.ui;
        self.toasts.set_time(cx.time());
        self.closed_submodules.clear();
        self.apply_profile_switch(cx);
        if let Some(problems) = cx.state.take_theme_reload() {
            self.report_theme_reload(&problems);
        }
        self.poll_async();
        self.poll_background();
        self.keep_submodules_updated();
        self.sync_hidden_refs();

        // Shortcuts and the tabs bar can close the active tab: the state is looked up again after each.
        self.handle_shortcuts(cx);
        self.draw_menu_bar(cx);

        let (work_pos, work_size) = raw::work_area(ui);
        ui.window("##root")
            .position(work_pos.into(), Condition::Always)
            .size(work_size.into(), Condition::Always)
            .flags(
                WindowFlags::NO_DECORATION
                    | WindowFlags::NO_MOVE
                    | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS
                    | WindowFlags::NO_NAV_FOCUS
                    | WindowFlags::NO_SCROLL_WITH_MOUSE,
            )
            .build(|| {
                // The chrome bands abut: place each one explicitly instead of letting ImGui add item spacing.
                let root = cx.cursor();
                let root_bottom = work_pos.y + work_size.y;
                tabs_bar::draw_tabs_bar(cx, self);
                let tabs_bottom = root.y + cx.px(TABS_BAR_HEIGHT);
                cx.set_cursor(vec2(root.x, tabs_bottom));
                let status_height = cx.px(INFO_BAR_HEIGHT);
                let body = vec2(work_size.x, root_bottom - tabs_bottom - status_height);
                if self.preferences_open {
                    // The Preferences view covers everything under the tabs bar, the info bar included.
                    preferences_panel::draw_preferences(
                        cx,
                        self,
                        vec2(work_size.x, root_bottom - tabs_bottom),
                    );
                    return;
                }
                if self.repo_management_open {
                    repo_management::draw_repo_management(cx, self, body);
                } else if self.active_workdir().is_none() {
                    tabs_bar::draw_new_tab_view(cx, self, body);
                } else {
                    self.with_active(|app, state| {
                        if !state.busy() {
                            state.poll_status(cx.time(), STATUS_POLL_SECONDS);
                        }
                        app.auto_fetch(cx.time(), state);
                        let mut toolbar_height = 0.0;
                        if app.show_toolbar {
                            toolbar::draw_toolbar(cx, app, state);
                            toolbar_height = cx.px(TOOLBAR_HEIGHT);
                            cx.set_cursor(vec2(root.x, tabs_bottom + toolbar_height));
                        }
                        app.draw_repo_tab(cx, state, vec2(body.x, body.y - toolbar_height));
                    });
                }
                cx.set_cursor(vec2(root.x, root_bottom - status_height));
                self.draw_status_bar(cx);
            });

        dialogs::draw_dialogs(cx, self);
        fuzzy_finder::draw_fuzzy_finder(cx, self);
        self.draw_toasts(cx);
        draw_menus(cx, self);
    }

    fn draw_toasts(&mut self, cx: &mut Cx<'_>) {
        let corner = toast_corner(&preferences().get(pref::NOTIFICATION_LOCATION));
        self.toasts.draw(cx, corner);
        // Desktop notifications: a toast that comes while ImGit is in the background makes the window ask for
        // attention (the taskbar button flashes, the Dock icon bounces).
        // Counted by pushes, not by the unread total: that one stays level when the history's cap drops an unread
        // entry as the new one comes in.
        let pushed = self.toasts.pushed();
        if pushed > self.toasts_pushed_seen
            && preferences().flag(pref::DESKTOP_NOTIFICATIONS)
            && !cx.shell.window.has_focus()
        {
            cx.shell
                .window
                .request_user_attention(Some(UserAttentionType::Informational));
        }
        self.toasts_pushed_seen = pushed;
    }

    /// The repository tab: the left panel, the graph (or diff, or rebase plan), the detail panel and the
    /// terminal, with their resize handles.
    pub fn draw_repo_tab(&mut self, cx: &mut Cx<'_>, state: &mut RepoState, size: Vec2) {
        // The diff view's whitespace toggle is one setting for every tab.
        if state.diff_whitespace != self.settings.diff_whitespace {
            state.diff_whitespace = self.settings.diff_whitespace;
            state.reload_diff();
        }
        // So is Smart Branch Visibility; the graph reloads when it changes.
        if state.smart_branch_visibility != self.settings.smart_branch_visibility {
            state.smart_branch_visibility = self.settings.smart_branch_visibility;
            state.refresh();
        }
        // The merge tool and the File History view take the whole tab.
        if state.merge_tool.is_some() {
            merge_view::draw_merge_view(cx, self, state, size);
            return;
        }
        if state.file_history.is_some() {
            file_history::draw_file_history(cx, self, state, size);
            return;
        }
        let handle = cx.px(7.0); // --handle-width
        let origin = cx.cursor();
        let window_width = size.x / cx.scale; // window.innerWidth, CSS px

        // Panels open and close at once (no transitions).
        let left_open =
            self.show_left_panel && (state.diff_target.is_none() || state.left_open_in_diff);
        // A pending interactive rebase covers the left panel and the graph.
        let rebasing = state.rebase.active && state.diff_target.is_none();
        // The file view covers the terminal too.
        let terminal_open = self.show_terminal_panel && state.diff_target.is_none();
        let collapsed_strip = cx.px(36.0); // LEFT_PANEL_COLLAPSED_WIDTH

        // The terminal sits under the graph only; the side panels run the full height.
        let min_terminal = cx.px(80.0);
        let max_terminal = min_terminal.max(size.y - cx.px(120.0));
        let mut terminal_height = 0.0;
        if terminal_open {
            terminal_height = if self.terminal.minimized {
                cx.px(30.0)
            } else {
                cx.px(self.settings.terminal_height)
                    .clamp(min_terminal, max_terminal)
            };
        }
        let upper_height = size.y - terminal_height;
        let full_height = size.y;

        // The limits: the left panel's resizable is 215..800 wide with its 7px handle margin (the panel shows
        // 7px less); the detail panel's 353..innerWidth - 687.
        let min_left = cx.px(LEFT_PANEL_DEFAULT_WIDTH);
        let max_left = cx.px(800.0 - 7.0);
        let min_detail = cx.px(353.0);
        let max_detail = min_detail.max(cx.px(window_width - 687.0));
        let mut left = if rebasing {
            0.0
        } else if left_open {
            cx.px(self.settings.left_panel_width)
                .clamp(min_left, max_left)
        } else {
            collapsed_strip
        };
        let mut detail = if self.show_detail_panel {
            cx.px(self.settings.detail_panel_width)
                .clamp(min_detail, max_detail)
        } else {
            0.0
        };
        // Nothing else keeps the panels apart; on a narrow window the detail panel gives way.
        detail = detail.min((size.x - left).max(0.0));
        let center_x = origin.x + left;
        let center_width = size.x - left - detail;

        // The panels abut; their borders are drawn by the panels, the resize handles overlay them.
        cx.set_cursor(origin);
        if !rebasing {
            left_panel::draw_left_panel(cx, self, state, vec2(left, full_height), left_open);
        }
        cx.set_cursor(vec2(center_x, origin.y));
        let center = vec2(center_width, upper_height);
        if state.diff_target.is_some() {
            diff_view::draw_diff_view(cx, self, state, center);
        } else if rebasing {
            rebase_panel::draw_rebase_panel(cx, self, state, center);
        } else {
            graph_view::draw_graph_view(cx, self, state, center);
        }
        if detail > 0.0 {
            cx.set_cursor(vec2(center_x + center_width, origin.y));
            detail_panel::draw_detail_panel(cx, self, state, vec2(detail, full_height));
        }
        if terminal_height > 0.0 {
            cx.set_cursor(vec2(center_x, origin.y + upper_height));
            terminal_panel::draw_terminal_panel(
                cx,
                self,
                Some(state),
                vec2(center_width, terminal_height),
            );
        }
        if !rebasing && state.diff_target.is_none() {
            dialogs::draw_slide_panel(
                cx,
                self,
                state,
                vec2(center_x, origin.y),
                full_height,
                center_width,
            );
        }

        // Sizes follow the mouse while a handle is held and are saved when it is let go.
        let mut resized = false;
        let (shown_left, shown_detail, shown_terminal) = (left, detail, terminal_height);
        if left_open && !rebasing {
            // .ref-panel-container's handle: 7px wide, 3px in from the resizable's right edge.
            let min = vec2(center_x - cx.px(3.0), origin.y);
            resized |= cx.overlay_resize_drag(
                "##left_resize",
                min,
                vec2(min.x + handle, origin.y + full_height),
                ResizeAxis::Columns,
                1.0,
                &mut left,
                min_left,
                max_left,
            );
            if left != shown_left {
                self.settings.left_panel_width = left / cx.scale;
            }
        }
        if detail > 0.0 {
            // .detail-panel's handle: 10px wide on its left edge.
            let min = vec2(center_x + center_width, origin.y);
            resized |= cx.overlay_resize_drag(
                "##detail_resize",
                min,
                vec2(min.x + cx.px(10.0), origin.y + full_height),
                ResizeAxis::Columns,
                -1.0,
                &mut detail,
                min_detail,
                max_detail,
            );
            if detail != shown_detail {
                self.settings.detail_panel_width = detail / cx.scale;
            }
        }
        if terminal_height > 0.0 && !self.terminal.minimized {
            let min = vec2(center_x, origin.y + upper_height - handle / 2.0);
            resized |= cx.overlay_resize_drag(
                "##terminal_resize",
                min,
                vec2(min.x + center_width, min.y + handle),
                ResizeAxis::Rows,
                -1.0,
                &mut terminal_height,
                min_terminal,
                max_terminal,
            );
            if terminal_height != shown_terminal {
                self.settings.terminal_height = terminal_height / cx.scale;
            }
        }
        if resized {
            self.settings.save(&self.config_path("settings.txt"));
        }
    }

    /// Chromium's views menu bar (Electron on Windows): Segoe UI 9pt on #1F1F1F, 26px tall, the pressed entry
    /// on #3E3E3E; under a light theme Windows' light bar (#F9F9F9, pressed #E6E6E6, #1A1A1A text, like the
    /// light menus). Each entry is its label (rounded up) plus 6px on both sides.
    fn draw_menu_bar(&mut self, cx: &mut Cx<'_>) {
        if has_native_menu_bar() {
            // TODO(macOS): the same menus in the system menu bar, built from `bar_menu()` as they open (see
            // `app_menu`); their clicks run here, on the UI thread inside the frame.
            return;
        }
        let ui = cx.ui;
        let height = cx.px(MENU_BAR_HEIGHT);
        let (_, viewport_size) = raw::viewport(ui);
        let layout = raw::viewport_side_bar_up(
            ui,
            "##menu_bar",
            height,
            WindowFlags::NO_DECORATION
                | WindowFlags::NO_SAVED_SETTINGS
                | WindowFlags::NO_NAV
                | WindowFlags::NO_SCROLL_WITH_MOUSE,
            |open| {
                if !open {
                    return None;
                }
                let draw = cx.window_draw_list();
                let origin: Vec2 = ui.window_pos().into();
                let right = origin.x + viewport_size.x;
                let light = cx.palette.light;
                let (bar_bg, plate, plate_bottom, label) = if light {
                    (
                        Color::rgb(0xF9F9F9),
                        Color::rgb(0xE6E6E6),
                        Color::rgb(0xE2E2E2),
                        Color::rgb(0x1A1A1A),
                    )
                } else {
                    (
                        Color::rgb(0x1F1F1F),
                        Color::from_rgba8(62, 62, 62, 255),
                        Color::from_rgba8(59, 59, 59, 255),
                        Color::rgb(0xE3E3E3),
                    )
                };
                draw.add_rect_filled(origin, vec2(right, origin.y + height), bar_bg, 0.0);
                // The bar's bottom rows are a shade darker, as Chromium paints them.
                let dark_shade = [30, 30, 30, 29];
                let light_shade = [247, 247, 246, 244];
                let shade = if light { light_shade } else { dark_shade };
                for (i, level) in shade.iter().enumerate() {
                    let y = origin.y + height - cx.px(5.0) + i as f32 * cx.px(1.0);
                    draw.add_rect_filled(
                        vec2(origin.x, y),
                        vec2(right, y + cx.px(1.0)),
                        Color::from_rgba8(*level, *level, *level, 255),
                        0.0,
                    );
                }

                let font = cx.fonts.system;
                let line = cx.font_px(font, FONT_SIZE_SMALL);
                let mut x = origin.x;
                let mut rects = Vec::with_capacity(BAR_LABELS.len());
                for entry in BAR_LABELS {
                    let text = strip_mnemonic(entry);
                    let width = cx.text_size(font, FONT_SIZE_SMALL, &text).x.ceil() + cx.px(12.0);
                    rects.push(Rect::new(
                        vec2(x, origin.y),
                        vec2(x + width, origin.y + height),
                    ));
                    x += width;
                }
                let open_index = open_bar_menu(cx);
                let underline = (ui.io().key_alt() && !menu_open(cx))
                    || (open_index.is_some() && menu_keyboard_mode(cx));
                for (i, rect) in rects.iter().enumerate() {
                    cx.set_cursor(rect.min);
                    let activated = {
                        let _id = ui.push_id(i as i32);
                        ui.invisible_button("##bar_entry", vec2(rect.width(), height));
                        ui.is_item_activated()
                    };
                    if activated && open_index != Some(i) {
                        let items = bar_menu(cx, i);
                        popup_bar_menu(cx, items, i, rect.min, rect.max, false);
                    }
                    if open_index == Some(i) {
                        // A 4px-rounded #3E3E3E plate, 1px narrower than the entry, fading a little at the
                        // bottom.
                        let plate_max = vec2(rect.max.x - cx.px(1.0), rect.max.y);
                        draw.add_rect_filled(rect.min, plate_max, plate, cx.px(4.0));
                        draw.add_rect_filled_multicolor(
                            vec2(rect.min.x + cx.px(4.0), rect.max.y - cx.px(5.0)),
                            vec2(plate_max.x - cx.px(4.0), rect.max.y),
                            plate,
                            plate,
                            plate_bottom,
                            plate_bottom,
                        );
                    }
                    let text = strip_mnemonic(BAR_LABELS[i]);
                    let text_pos = vec2(
                        rect.min.x + cx.px(6.0),
                        (rect.min.y + (height - line) / 2.0).floor() - cx.px(1.0),
                    );
                    cx.draw_text(draw, font, FONT_SIZE_SMALL, text_pos, label, &text);
                    if underline {
                        let first_len = text.chars().next().map_or(0, char::len_utf8);
                        let w = cx.text_size(font, FONT_SIZE_SMALL, &text[..first_len]).x;
                        let y = (text_pos.y + line * 0.86).round();
                        draw.add_rect_filled(
                            vec2(text_pos.x, y),
                            vec2(text_pos.x + w.round(), y + cx.px(1.0)),
                            label,
                            0.0,
                        );
                    }
                }
                Some(rects)
            },
        );

        let Some(rects) = layout else {
            return;
        };
        set_menu_bar(cx, bar_menu, rects.clone());

        // Alt + the underlined letter opens an entry from the keyboard.
        if !menu_open(cx) && !self.dialog.is_open() && ui.io().key_alt() && !ui.io().key_ctrl() {
            for (i, key) in BAR_KEYS.iter().enumerate() {
                if ui.is_key_pressed_with_repeat(*key, false) {
                    let items = bar_menu(cx, i);
                    popup_bar_menu(cx, items, i, rects[i].min, rects[i].max, true);
                }
            }
        }
    }

    /// The status bar: a 1px panel border over 25px of panel-bg0, 10px right padding. Right, inwards: the
    /// version button, the zoom select and the activity log / keyboard shortcuts / release notes icons.
    fn draw_status_bar(&mut self, cx: &mut Cx<'_>) {
        let ui = cx.ui;
        let p = cx.palette;
        let height = cx.px(INFO_BAR_HEIGHT);
        let origin = cx.cursor();
        let width = cx.avail().x;
        ui.child_window("##status_bar")
            .size([width, height])
            .child_flags(ChildFlags::NONE)
            .flags(WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE)
            .build(ui, || {
                let draw = cx.window_draw_list();
                draw.add_rect_filled(
                    origin,
                    vec2(origin.x + width, origin.y + height),
                    p.panel_bg0,
                    0.0,
                );
                draw.add_rect_filled(
                    origin,
                    vec2(origin.x + width, origin.y + 1.0),
                    p.section_border,
                    0.0,
                );
                let font = cx.fonts.regular;
                let text_at = |cx: &Cx<'_>,
                               font,
                               box_top: f32,
                               x: f32,
                               color: Color,
                               text: &str,
                               ascent_units: f32| {
                    let ascent = FONT_SIZE_ROW * ascent_units / 2048.0 * cx.scale;
                    cx.draw_text(
                        draw,
                        font,
                        FONT_SIZE_ROW,
                        vec2(
                            x,
                            (origin.y + box_top + ascent.round()).round() - ascent.ceil(),
                        ),
                        color,
                        text,
                    );
                };
                let clickable =
                    |cx: &mut Cx<'_>, id: &str, min: Vec2, max: Vec2, tip: Option<&str>| -> bool {
                        cx.set_cursor(min);
                        let pressed = ui.invisible_button(id, max - min);
                        if ui.is_item_hovered()
                            && let Some(tip) = tip
                        {
                            cx.styled_tooltip(min, max, tip, true);
                        }
                        pressed
                    };

                // Right, from the edge inwards.
                let mut x = origin.x + width - cx.px(10.0);
                {
                    // .version-btn: monospace 12px / 15px, 0 5px padding, 1px border, radius 2.
                    let mono = cx.fonts.mono;
                    let text_w = cx.text_size(mono, FONT_SIZE_ROW, VERSION).x;
                    let box_w = text_w + cx.px(12.0);
                    let bmin = vec2(x - box_w, origin.y + cx.px(5.0));
                    let bmax = vec2(x, bmin.y + cx.px(17.0));
                    let pressed =
                        clickable(cx, "##version", bmin, bmax, Some("About ImGit Desktop"));
                    let hovered = ui.is_item_hovered();
                    draw.add_rect_filled(
                        bmin,
                        bmax,
                        if hovered { p.hover_row } else { p.panel_bg0 },
                        cx.px(2.0),
                    );
                    draw.add_rect(bmin, bmax, p.section_border, cx.px(2.0), 1.0);
                    text_at(
                        cx,
                        mono,
                        cx.px(6.5),
                        bmin.x + cx.px(6.0),
                        p.text_normal,
                        VERSION,
                        1884.0,
                    );
                    if pressed {
                        self.dialog.open(DialogKind::About);
                    }
                    x = bmin.x - cx.px(10.0);
                }
                {
                    // .zoom-select: the search-plus icon (text-secondary) and a 41px select, 5px padding.
                    let zoom = format!("{}%", (self.settings.zoom * 100.0).round() as i32);
                    let bmin = vec2(x - cx.px(63.0), origin.y + cx.px(1.0));
                    let bmax = vec2(x, origin.y + height);
                    if clickable(cx, "##zoom", bmin, bmax, None) {
                        ui.open_popup("##zoom_levels");
                    }
                    let hovered = ui.is_item_hovered();
                    cx.draw_fa_icon(
                        draw,
                        "far:search-plus",
                        vec2(bmin.x + cx.px(11.0), origin.y + cx.px(13.0)),
                        cx.px(12.0),
                        p.text_secondary,
                    );
                    text_at(
                        cx,
                        font,
                        cx.px(5.0),
                        bmin.x + cx.px(22.3),
                        if hovered {
                            p.text_selected
                        } else {
                            p.text_normal
                        },
                        &zoom,
                        2189.0,
                    );
                    raw::set_next_window_pos(ui, bmin, vec2(0.0, 1.0));
                    if let Some(_popup) = ui.begin_popup("##zoom_levels") {
                        for level in [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200] {
                            let label = format!("{level}%");
                            let current = (self.settings.zoom * 100.0).round() as i32 == level;
                            if ui.selectable_config(&label).selected(current).build() {
                                self.set_zoom(cx, level as f32 / 100.0);
                            }
                        }
                    }
                    x = bmin.x - cx.px(10.0);
                }
                // .p1 .hover-text icons (5px padding, 10px apart), right to left.
                let mut icon_item = |cx: &mut Cx<'_>, id: &str, icon: &str, tip: &str| -> bool {
                    let w = cx.fa_icon_width(icon, cx.px(12.0));
                    let block = w + cx.px(10.0);
                    x -= cx.px(10.0) + block;
                    let bmin = vec2(x, origin.y);
                    let bmax = vec2(x + block, origin.y + height);
                    let pressed = clickable(cx, id, bmin, bmax, Some(tip));
                    let hovered = ui.is_item_hovered();
                    cx.draw_fa_icon(
                        draw,
                        icon,
                        vec2(x + cx.px(5.0) + w / 2.0, origin.y + cx.px(13.4)),
                        cx.px(12.0),
                        if hovered {
                            p.text_selected
                        } else {
                            p.text_normal
                        },
                    );
                    pressed
                };
                if icon_item(cx, "##release_notes", "far:gift", "What's new") {
                    self.toasts.push_with_detail(
                        ToastKind::Info,
                        "Release Notes",
                        format!("ImGit {VERSION}"),
                    );
                }
                if icon_item(cx, "##shortcuts", "far:keyboard", "Keyboard Shortcuts") {
                    self.dialog.open(DialogKind::KeyboardShortcuts);
                }
                if icon_item(cx, "##activity", "far:list", "Activity Log") {
                    self.show_terminal_panel = true;
                    self.terminal.focus_requested = true;
                }
            });
    }
}
