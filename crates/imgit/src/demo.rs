//! A stand-in for the real UI until the panels are ported: a gallery of every widget of the `ui` layer, in the
//! current theme. F5 (or the theme pills) cycles the built-in themes; `IMGIT_THEME` names the one to start
//! with, `IMGIT_GALLERY_AVATARS` a folder to download Gravatars into (without it the avatars are initials).

use std::env;
use std::path::PathBuf;

use dear_imgui_rs::{ChildFlags, Condition, InputTextFlags, Key, Ui, WindowFlags};
use imgit_git::{Author, FileStatus};
use imgit_syntax::{Language, tokenize_lines};

use crate::dev::DevCommand;
use crate::shell::{Frame, ShellContext};
use crate::theme::{FONT_SIZE_LARGE, FONT_SIZE_NORMAL, FONT_SIZE_SMALL, Ink, Themes};
use crate::ui::widgets::{ResizeAxis, SEGMENTED_HEIGHT, format_relative};
use crate::ui::{Cx, Icon, UiState, Vec2, vec2};
use crate::window::CursorShape;

/// A hairline in the ink at 12%: a constant that follows the theme.
const RULE: Ink = Ink(0.12);

const CODE: [&str; 4] = [
    "// Draws one frame of the gallery.",
    "fn draw(cx: &mut Cx<'_>, count: usize) -> bool {",
    "    let label = format!(\"{count} commits\");",
    "    cx.primary_button(&label, Vec2::ZERO, count > 0)",
    // "}" is left out: four lines show every token class the sample has.
];

pub struct DemoFrame {
    themes: Themes,
    state: UiState,
    clicks: u32,
    view: usize,
    search: String,
    message: String,
    amend: bool,
    sign: bool,
    panel_width: f32,
    scroll: f32,
    popup_clicks: u32,
    last_command: Option<String>,
}

impl DemoFrame {
    pub fn new(_shell: &mut ShellContext<'_>) -> Self {
        let mut themes = Themes::new();
        if let Some(theme) = env::var_os("IMGIT_THEME") {
            themes.set_theme(&theme.to_string_lossy());
        }
        let mut state = UiState::new();
        state
            .avatars
            .set_cache_directory(env::var_os("IMGIT_GALLERY_AVATARS").map(PathBuf::from));
        Self {
            themes,
            state,
            clicks: 0,
            view: 0,
            search: String::new(),
            message: String::new(),
            amend: false,
            sign: true,
            panel_width: 220.0,
            scroll: 120.0,
            popup_clicks: 0,
            last_command: None,
        }
    }

    /// The theme after the current one, round the built-in ones.
    fn next_theme(&self) -> String {
        let names = self.themes.theme_names();
        let current = names
            .iter()
            .position(|name| name == self.themes.current_theme())
            .unwrap_or(0);
        names[(current + 1) % names.len()].clone()
    }
}

/// A section heading with a hairline under it.
fn heading(cx: &Cx<'_>, title: &str) {
    cx.ui.dummy(vec2(0.0, cx.px(6.0)));
    cx.section_label(title);
    let pos = cx.cursor();
    cx.window_draw_list().add_line(
        pos,
        vec2(pos.x + cx.avail().x, pos.y),
        RULE.of(cx.palette),
        1.0,
    );
    cx.ui.dummy(vec2(0.0, cx.px(4.0)));
}

struct Values<'v> {
    theme: &'v str,
    theme_names: &'v [String],
    clicks: &'v mut u32,
    view: &'v mut usize,
    search: &'v mut String,
    message: &'v mut String,
    amend: &'v mut bool,
    sign: &'v mut bool,
    panel_width: &'v mut f32,
    scroll: &'v mut f32,
    popup_clicks: &'v mut u32,
    last_command: Option<&'v str>,
}

fn left_column(cx: &mut Cx<'_>, v: &mut Values<'_>) {
    let ui = cx.ui;
    let p = cx.palette;

    heading(cx, "Themes (F5 cycles)");
    for (index, name) in v.theme_names.iter().enumerate() {
        if index > 0 {
            ui.same_line();
        }
        let current = name == v.theme;
        let pill = cx.pill(
            &format!("##theme{index}"),
            name,
            if current { p.accent } else { p.panel_bg2 },
            p.selected_row,
            if current {
                p.text_inverse
            } else {
                p.text_normal
            },
            current,
            20.0,
            None,
        );
        if pill.clicked {
            cx.request_theme(name);
        }
    }

    heading(cx, "Buttons");
    if cx.primary_button("Commit", Vec2::ZERO, true) {
        *v.clicks += 1;
    }
    ui.same_line();
    if cx.default_button("Cancel", Vec2::ZERO, true) {
        *v.clicks += 1;
    }
    ui.same_line();
    if cx.danger_button("Delete", Vec2::ZERO, true) {
        *v.clicks += 1;
    }
    ui.same_line();
    if cx.success_button("Resolve", Vec2::ZERO, true) {
        *v.clicks += 1;
    }
    ui.same_line();
    if cx.text_button("Skip", Vec2::ZERO, true) {
        *v.clicks += 1;
    }
    ui.same_line();
    cx.primary_button("Disabled", vec2(cx.px(90.0), 0.0), false);
    ui.same_line();
    ui.align_text_to_frame_padding();
    if cx.link_text("a link") {
        *v.clicks += 1;
    }
    ui.same_line();
    cx.secondary_text(&format!("clicked {} times", v.clicks));

    heading(cx, "Pills and segmented");
    cx.pill(
        "##p1",
        "main",
        p.checked_out,
        p.selected_row,
        p.text_selected,
        true,
        22.0,
        None,
    );
    ui.same_line();
    cx.pill(
        "##p2",
        "origin/feature/rust-rewrite",
        p.panel_bg2,
        p.hover_row,
        p.text_normal,
        false,
        22.0,
        Some(cx.px(120.0)),
    );
    ui.same_line();
    cx.pill(
        "##p3",
        "v1.0",
        p.wip_status,
        p.selected_row,
        p.text_accent,
        false,
        18.0,
        None,
    );
    ui.same_line();
    cx.segmented("view", &["Path", "Tree"], v.view, SEGMENTED_HEIGHT);
    ui.same_line();
    let mut three = *v.view % 3;
    cx.segmented(
        "three",
        &["List", "Agents", "Graph"],
        &mut three,
        SEGMENTED_HEIGHT,
    );

    heading(cx, "Inputs");
    cx.input_text(
        "##search",
        v.search,
        Some("Search commits"),
        InputTextFlags::NONE,
        cx.px(260.0),
    );
    ui.same_line();
    cx.checkbox("Amend", v.amend);
    ui.same_line();
    cx.checkbox("Sign off", v.sign);
    cx.input_multiline(
        "##message",
        v.message,
        Some("Description"),
        vec2(cx.px(420.0), cx.px(54.0)),
        false,
    );

    heading(cx, "Text");
    let draw = cx.window_draw_list();
    let pos = cx.cursor();
    let font = cx.fonts.regular;
    cx.draw_text(
        draw,
        font,
        FONT_SIZE_NORMAL,
        pos,
        p.text_normal,
        "draw_text at a whole pixel",
    );
    cx.draw_text(
        draw,
        font,
        FONT_SIZE_NORMAL,
        pos + vec2(cx.px(220.0) + 0.5, 0.0),
        p.text_normal,
        "and half a pixel on",
    );
    let line = cx.px(20.0);
    let used = cx.draw_text_ellipsis(
        draw,
        cx.fonts.semibold,
        FONT_SIZE_NORMAL,
        pos + vec2(0.0, line),
        cx.px(230.0),
        p.text_selected,
        "An ellipsis ends a summary that is far too long to fit",
    );
    cx.draw_dashed_line(
        draw,
        pos + vec2(0.0, line * 2.0 + 2.0),
        pos + vec2(used, line * 2.0 + 2.0),
        p.accent,
        1.0,
        3.0,
        3.0,
    );
    let wrapped = cx.wrap_text(
        font,
        FONT_SIZE_SMALL,
        "wrap_text breaks a paragraph into lines between its words,\nand at its newlines.",
        cx.px(190.0),
    );
    for (row, text) in wrapped.iter().enumerate() {
        cx.draw_text(
            draw,
            font,
            FONT_SIZE_SMALL,
            pos + vec2(cx.px(250.0), line + row as f32 * cx.px(16.0)),
            p.text_secondary,
            text,
        );
    }
    ui.dummy(vec2(
        cx.px(440.0),
        line + wrapped.len() as f32 * cx.px(16.0),
    ));

    heading(cx, "Code");
    let pos = cx.cursor();
    let mono = cx.fonts.editor;
    let row = cx.px(17.0);
    let width = cx.px(440.0);
    draw.add_rect_filled(
        pos,
        pos + vec2(width, row * CODE.len() as f32 + cx.px(8.0)),
        p.terminal_bg,
        cx.px(3.0),
    );
    draw.add_rect_filled(
        pos + vec2(0.0, cx.px(4.0) + row * 2.0),
        pos + vec2(width, cx.px(4.0) + row * 3.0),
        p.added_line,
        0.0,
    );
    if let Some(rust) = Language::for_path("gallery.rs", None) {
        let spans = tokenize_lines(rust, &CODE);
        for (index, text) in CODE.iter().enumerate() {
            cx.draw_code_text(
                draw,
                mono,
                FONT_SIZE_SMALL,
                pos + vec2(cx.px(8.0), cx.px(4.0) + row * index as f32),
                text,
                spans.line(index),
                p.terminal_fg,
            );
        }
    }
    ui.dummy(vec2(width, row * CODE.len() as f32 + cx.px(8.0)));

    heading(cx, "Dates");
    let then = 1_792_946_580; // 2026-10-25
    cx.secondary_text(&format!(
        "{}   |   {}   |   {}   |   {}",
        cx.format_date_time(then),
        cx.format_date(then),
        cx.format_date_word(then),
        cx.format_date_long(then),
    ));
    cx.secondary_text(&format!(
        "{}   |   {}   |   {}",
        format_relative(then),
        cx.shortcut_label("Ctrl+Shift+T"),
        v.last_command.unwrap_or("no dev command"),
    ));

    heading(cx, "Window, child and popup");
    // The convention under test: `cx` used mutably inside nested window / child / popup scopes.
    ui.child_window("##nested")
        .size([cx.px(440.0), cx.px(44.0)])
        .child_flags(ChildFlags::BORDERS)
        .build(ui, || {
            cx.set_cursor(cx.cursor() + vec2(cx.px(8.0), cx.px(8.0)));
            if cx.icon_button(
                "##more",
                Icon::Bars,
                14.0,
                cx.palette.text_secondary,
                Some("Open the popup"),
                false,
            ) {
                ui.open_popup("##gallery-popup");
            }
            ui.same_line();
            ui.align_text_to_frame_padding();
            cx.secondary_text(&format!("popup button clicked {} times", v.popup_clicks));
            if let Some(_popup) = ui.begin_popup("##gallery-popup") {
                cx.section_label("In a popup");
                if cx.primary_button("Count", Vec2::ZERO, true) {
                    *v.popup_clicks += 1;
                }
                ui.same_line();
                if cx.text_button("Close", Vec2::ZERO, true) {
                    ui.close_current_popup();
                }
                let spin = cx.smooth("popup-open", true);
                cx.secondary_text(&format!("faded in to {spin:.2}"));
            }
        });
}

fn right_column(cx: &mut Cx<'_>, v: &mut Values<'_>) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();

    heading(cx, "Icons");
    let origin = cx.cursor();
    let cell = cx.px(30.0);
    let columns = 16;
    for (index, icon) in Icon::ALL.into_iter().enumerate() {
        let center = origin
            + vec2(
                (index % columns) as f32 * cell + cell / 2.0,
                (index / columns) as f32 * cell + cell / 2.0,
            );
        cx.draw_icon(draw, icon, center, cx.px(16.0), p.text_normal);
    }
    let rows = Icon::ALL.len().div_ceil(columns);
    ui.dummy(vec2(cell * columns as f32, cell * rows as f32));

    let origin = cx.cursor();
    for (index, icon) in [
        Icon::Branch,
        Icon::Tag,
        Icon::Gear,
        Icon::Lfs,
        Icon::Expand,
        Icon::Collapse,
        Icon::Headphones,
        Icon::Refresh,
        Icon::Eye,
    ]
    .into_iter()
    .enumerate()
    {
        cx.draw_icon_vector(
            draw,
            icon,
            origin + vec2(index as f32 * cell + cell / 2.0, cell / 2.0),
            cx.px(16.0),
            p.text_secondary,
        );
    }
    let x = origin.x + cell * 10.0;
    let y = origin.y + cell / 2.0;
    let size = cx.px(16.0);
    cx.draw_fa_icon_rotated(draw, "far:chevron-right", vec2(x, y), size, p.accent, 90.0);
    cx.draw_fa_icon_rotated(draw, "far:sync", vec2(x + cell, y), size, p.accent, 45.0);
    cx.draw_fa_icon_mirrored(
        draw,
        "far:undo-alt",
        vec2(x + cell * 2.0, y),
        size,
        p.accent,
    );
    cx.draw_fa_duotone(
        draw,
        "fad:check-circle",
        vec2(x + cell * 3.0, y),
        size,
        p.green,
        p.white,
    );
    if let Some(path) = crate::ui::icons::svg_path("code-branch") {
        cx.draw_svg_path(
            draw,
            "code-branch",
            path,
            448.0,
            512.0,
            vec2(x + cell * 4.0 - size / 2.0, y - size / 2.0),
            vec2(x + cell * 4.0 + size / 2.0, y + size / 2.0),
            p.link,
        );
    }
    ui.dummy(vec2(cell * columns as f32, cell));

    for (index, (icon, tip)) in [
        (Icon::Pull, "Pull (fast-forward if possible)"),
        (Icon::Push, "Push"),
        (Icon::Stash, "Stash"),
        (Icon::Undo, "Undo (Ctrl+Z)"),
        (Icon::Terminal, "Terminal"),
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            ui.same_line();
        }
        cx.icon_button(
            &format!("##tool{index}"),
            icon,
            16.0,
            p.text_normal,
            Some(tip),
            index == 2,
        );
    }
    ui.same_line();
    ui.align_text_to_frame_padding();
    cx.secondary_text("icon buttons with tooltips");

    heading(cx, "File status");
    let origin = cx.cursor();
    let badge = cx.px(16.0);
    let statuses = [
        FileStatus::Added,
        FileStatus::Modified,
        FileStatus::Deleted,
        FileStatus::Renamed,
        FileStatus::TypeChange,
        FileStatus::Untracked,
        FileStatus::Conflicted,
        FileStatus::Unknown,
    ];
    for (index, status) in statuses.into_iter().enumerate() {
        let x = origin.x + index as f32 * cx.px(44.0);
        cx.draw_status_badge(draw, vec2(x, origin.y), badge, status);
        cx.draw_status_icon(
            draw,
            vec2(x + badge + cx.px(12.0), origin.y + badge / 2.0),
            cx.px(12.0),
            status,
        );
    }
    ui.dummy(vec2(cx.px(360.0), badge));

    heading(cx, "Avatars");
    let origin = cx.cursor();
    let authors = [
        ("Ada Lovelace", "ada@example.com"),
        ("grace hopper", "grace@example.com"),
        ("Linus", "linus@example.com"),
        ("", "first.last@example.com"),
        ("", ""),
    ];
    for (index, (name, email)) in authors.into_iter().enumerate() {
        let author = Author {
            name: name.to_owned(),
            email: email.to_owned(),
        };
        let diameter = cx.px(if index % 2 == 0 { 22.0 } else { 32.0 });
        let center = origin + vec2(cx.px(20.0) + index as f32 * cx.px(44.0), cx.px(18.0));
        let rounding = (index == 3).then(|| cx.px(4.0));
        cx.draw_avatar(draw, center, diameter, &author, rounding);
    }
    for lane in 0..10 {
        draw.add_circle_filled(
            origin + vec2(cx.px(250.0) + lane as f32 * cx.px(18.0), cx.px(18.0)),
            cx.px(6.0),
            p.graph_color(lane),
            0,
        );
    }
    ui.dummy(vec2(cx.px(440.0), cx.px(36.0)));

    heading(cx, "Scrollbar and resize handle");
    let origin = cx.cursor();
    let width = cx.px(440.0);
    let bar_min = origin + vec2(0.0, cx.px(40.0));
    let bar_max = bar_min + vec2(width, cx.px(10.0));
    // Submitted before what it covers, drawn after it.
    let bar = cx.scrollbar_x("##scroll", bar_min, bar_max, v.scroll, 1000.0, 400.0);
    draw.add_rect_filled(origin, origin + vec2(width, cx.px(50.0)), p.panel_bg0, 0.0);
    draw.with_clip_rect(origin, origin + vec2(width, cx.px(40.0)), true, || {
        for tick in 0..=20 {
            let x = origin.x + (tick as f32 * 50.0 - *v.scroll) * width / 400.0;
            draw.add_line(
                vec2(x, origin.y),
                vec2(x, origin.y + cx.px(40.0)),
                p.section_border,
                1.0,
            );
            cx.draw_text(
                draw,
                cx.fonts.mono,
                FONT_SIZE_SMALL,
                vec2(x + cx.px(4.0), origin.y + cx.px(12.0)),
                p.text_secondary,
                &format!("{}", tick * 50),
            );
        }
    });
    cx.draw_scrollbar_x(draw, bar_min, bar_max, &bar);
    ui.dummy(vec2(width, cx.px(54.0)));

    let origin = cx.cursor();
    let height = cx.px(40.0);
    let panel = cx.px(*v.panel_width);
    draw.add_rect_filled(origin, origin + vec2(panel, height), p.panel_bg1, 0.0);
    draw.add_rect_filled(
        origin + vec2(panel, 0.0),
        origin + vec2(width, height),
        p.panel_bg2,
        0.0,
    );
    draw.add_line(
        origin + vec2(panel, 0.0),
        origin + vec2(panel, height),
        p.accent,
        1.0,
    );
    cx.draw_text(
        draw,
        cx.fonts.regular,
        FONT_SIZE_SMALL,
        origin + vec2(cx.px(8.0), cx.px(12.0)),
        p.text_normal,
        &format!("drag the edge: {:.0} px", *v.panel_width),
    );
    let mut device_width = panel;
    let handle_min = origin + vec2(panel - cx.px(3.0), 0.0);
    let handle_max = origin + vec2(panel + cx.px(3.0), height);
    cx.resize_drag(
        "##panel-edge",
        handle_min,
        handle_max,
        ResizeAxis::Columns,
        1.0,
        &mut device_width,
        cx.px(120.0),
        cx.px(400.0),
    );
    *v.panel_width = device_width / cx.scale;
    cx.tooltip_on_rect(
        origin + vec2(panel, 0.0),
        origin + vec2(width, height),
        "tooltip_on_rect, above",
        true,
    );
    ui.dummy(vec2(width, height));

    heading(cx, "Cursors and transitions");
    let origin = cx.cursor();
    for (index, (label, shape)) in [
        ("move", CursorShape::Move),
        ("no drop", CursorShape::NoDrop),
        ("grab", CursorShape::Grab),
    ]
    .into_iter()
    .enumerate()
    {
        let min = origin + vec2(index as f32 * cx.px(70.0), 0.0);
        let max = min + vec2(cx.px(62.0), cx.px(26.0));
        let hovered = ui.is_mouse_hovering_rect(min.into(), max.into());
        let glow = cx.smooth(&format!("cursor{index}"), hovered);
        draw.add_rect_filled(
            min,
            max,
            p.panel_bg2.mix(p.accent, 1.0 - glow * 0.6),
            cx.px(3.0),
        );
        cx.draw_text(
            draw,
            cx.fonts.regular,
            FONT_SIZE_SMALL,
            min + vec2(cx.px(8.0), cx.px(5.0)),
            p.text_selected,
            label,
        );
        if hovered {
            cx.set_drag_cursor(shape);
            cx.styled_tooltip_left(min, max, label);
        }
    }
    let level = cx.smooth_value("level", if *v.amend { 1.0 } else { 0.25 });
    let track = origin + vec2(cx.px(230.0), cx.px(9.0));
    draw.add_rect_filled(
        track,
        track + vec2(cx.px(200.0), cx.px(8.0)),
        p.input_bg,
        cx.px(4.0),
    );
    draw.add_rect_filled(
        track,
        track + vec2(cx.px(200.0) * level, cx.px(8.0)),
        p.accent,
        cx.px(4.0),
    );
    ui.dummy(vec2(width, cx.px(26.0)));
}

impl Frame for DemoFrame {
    fn draw(&mut self, ui: &Ui, shell: &mut ShellContext<'_>) {
        if ui.is_key_pressed(Key::F5) {
            let next = self.next_theme();
            self.state.request_theme(&next);
        }
        self.state.begin_frame(ui, &mut self.themes, shell);
        let mut cx = Cx::new(ui, shell, self.themes.palette(), &mut self.state);
        let mut values = Values {
            theme: self.themes.current_theme(),
            theme_names: self.themes.theme_names(),
            clicks: &mut self.clicks,
            view: &mut self.view,
            search: &mut self.search,
            message: &mut self.message,
            amend: &mut self.amend,
            sign: &mut self.sign,
            panel_width: &mut self.panel_width,
            scroll: &mut self.scroll,
            popup_clicks: &mut self.popup_clicks,
            last_command: self.last_command.as_deref(),
        };

        let viewport = ui.main_viewport();
        let flags = WindowFlags::NO_DECORATION
            | WindowFlags::NO_MOVE
            | WindowFlags::NO_SAVED_SETTINGS
            | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS;
        ui.window("##gallery")
            .position(viewport.work_pos(), Condition::Always)
            .size(viewport.work_size(), Condition::Always)
            .flags(flags)
            .build(|| {
                let margin = cx.px(24.0);
                let origin = cx.cursor() + vec2(margin, cx.px(14.0));
                cx.set_cursor(origin);
                {
                    let _font = cx.push_font(cx.fonts.semibold, FONT_SIZE_LARGE);
                    ui.text_colored(cx.palette.text_accent.into(), "ImGit Desktop");
                }
                ui.same_line();
                ui.align_text_to_frame_padding();
                cx.secondary_text(&format!(
                    "widget gallery  |  {}  |  scale {:.2}",
                    values.theme, cx.scale
                ));
                let top = cx.cursor().y;
                let column = Vec2::new(cx.px(680.0), cx.avail().y - cx.px(8.0));
                cx.set_cursor(vec2(origin.x, top));
                ui.child_window("##left")
                    .size(column.into())
                    .build(ui, || left_column(&mut cx, &mut values));
                cx.set_cursor(vec2(origin.x + column.x + margin, top));
                ui.child_window("##right")
                    .size(column.into())
                    .build(ui, || right_column(&mut cx, &mut values));
            });
    }

    fn dev_command(&mut self, command: DevCommand, _shell: &mut ShellContext<'_>) {
        self.last_command = Some(match command {
            DevCommand::SelectRow(row) => format!("dev command: select row {row}"),
            DevCommand::OpenFile { path, .. } => format!("dev command: open {path}"),
        });
    }

    fn busy(&self) -> bool {
        self.state.busy()
    }
}
