//! The merge tool, in place of the whole tab under the toolbar: a 38px header (the file, its conflict count, Open
//! in external merge tool, Save, close), A (ours) and B (theirs) side by side, and the Output under them with the
//! conflict navigator. Each conflict is highlighted in A / B with a checkbox on its first line that picks the
//! whole side; a click on a line picks or drops that line alone. The Output shows the merge: agreed lines, the
//! picked lines tagged A / B, and the base's lines (magenta) where nothing was picked yet; a double-click edits it
//! as text. Save writes the Output and marks the file resolved. A | B and the Output are split by drag handles,
//! the split kept in the settings. Positions are CSS px; code is Consolas 12px on 16px lines (by default: the
//! Editor preferences) with an 82px margin (glyphs, line numbers, decorations).

use std::ops::Range;
use std::path::Path;

use dear_imgui_rs::{MouseButton, StyleColor, StyleVar, WindowFlags, sys};
use imgit_app::RepoState;
use imgit_app::merge_tool::{MergeTool, Origin, OutputLine, Side};
use imgit_app::preferences::{pref, preferences};
use imgit_git::Error;
use imgit_syntax::TextCache;

use crate::app::application::Application;
use crate::fonts::Font;
use crate::theme::Color;
use crate::ui::widgets::resize::ResizeAxis;
use crate::ui::{Cx, DrawList, Vec2, vec2};

const HEADER_H: f32 = 38.2;
const PANE_HEADER_H: f32 = 35.0;
const MARGIN_W: f32 = 82.0;
const NUMBERS_RIGHT: f32 = 62.0;
/// The margin decoration: a 4px bar, then the side's tint.
const DECORATION_X: f32 = 18.0;
/// The in-output / add / remove glyph, 14.4px.
const GLYPH_X: f32 = 67.0;
/// The narrowest A / B and the lowest panes the handles leave.
const MIN_PANE_W: f32 = 200.0;
const MIN_PANE_H: f32 = 110.0;

/// The sides' colours: A sky blue, B amber, Output violet.
const A_COLOR: Color = Color::rgb(0x2B_A6_E0);
const B_COLOR: Color = Color::rgb(0xF2_B1_34);
const OUTPUT_COLOR: Color = Color::rgb(0xA8_5C_F0);
const LINE_NUMBER: Color = Color::from_rgba8(133, 133, 133, 255);

fn side_color(side: Side) -> Color {
    match side {
        Side::A => A_COLOR,
        Side::B => B_COLOR,
    }
}

/// The merge view's state between frames: the highlighting of each pane's text, kept until the text changes.
#[derive(Default)]
pub struct State {
    /// A and B.
    sides: [TextCache; 2],
    output: TextCache,
}

/// The y of text in a line box at `box_top`, like Chromium (`mono`: Consolas' ascent).
fn css_y(cx: &Cx<'_>, size: f32, box_top: f32, mono: bool) -> f32 {
    let ascent = size * if mono { 1884.0 } else { 2189.0 } / 2048.0 * cx.scale;
    (box_top + ascent.round()).round() - ascent.ceil()
}

/// `text` in a `line_h` line box at `line_top` (device px), centred like Chromium.
#[allow(clippy::too_many_arguments)]
fn line_text(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    font: Font,
    size: f32,
    x: f32,
    line_top: f32,
    line_h: f32,
    color: Color,
    text: &str,
) {
    let content = (size * 2189.0 / 2048.0).round() + (size * 600.0 / 2048.0).round();
    cx.draw_text(
        draw,
        font,
        size,
        vec2(
            x.round(),
            css_y(
                cx,
                size,
                line_top + ((line_h - content) / 2.0).floor() * cx.scale,
                false,
            ),
        ),
        color,
        text,
    );
}

/// ImGui advances a tab by `IM_TABSIZE` spaces; the Tab Size preference replaces that in the editor font at this
/// size. Set again each frame: ImGui rebuilds a baked size it dropped.
fn set_tab_size(baked: *mut sys::ImFontBaked, tab_size: u32) {
    // SAFETY: `baked` is a baked font size of the live ImGui context (from `ImFont_GetFontBaked` or
    // `igGetFontBaked`); the glyphs and the advance table it owns are only touched through it, on the thread
    // that builds the frame, with no references kept.
    unsafe {
        let space = sys::ImFontBaked_FindGlyphNoFallback(baked, ' ' as sys::ImWchar);
        if space.is_null() {
            return;
        }
        let advance = (*space).AdvanceX * tab_size as f32;
        let tab = sys::ImFontBaked_FindGlyphNoFallback(baked, '\t' as sys::ImWchar);
        if tab.is_null() {
            return;
        }
        (*tab).AdvanceX = advance;
        let table = &mut (*baked).IndexAdvanceX;
        if table.Size > '\t' as i32 && !table.Data.is_null() {
            *table.Data.add('\t' as usize) = advance;
        }
    }
}

/// Preferences > Editor, read at the start of each `draw_merge_view`: the face and size, Monaco's line height for
/// the size (16px at the default 12px), the glyph box's offset in the line, and whether numbers show.
#[derive(Debug, Clone, Copy)]
struct CodeStyle {
    font: Font,
    size: f32,
    /// CSS px, as the offset.
    line_h: f32,
    text_dy: f32,
    line_numbers: bool,
    tab_size: u32,
}

impl CodeStyle {
    fn read(cx: &Cx<'_>) -> Self {
        let prefs = preferences();
        let font = cx.fonts.editor;
        let size = prefs.number(pref::EDITOR_FONT_SIZE, 6..=40) as f32;
        let line_h = (size * 1.35).round();
        let style = Self {
            font,
            size,
            line_h,
            // Chromium centres the glyph box in the line; its baseline lands a pixel lower (measured).
            text_dy: ((line_h - size * font.em_scale) / 2.0).round() + 1.0,
            line_numbers: prefs.flag(pref::EDITOR_LINE_NUMBERS),
            tab_size: prefs.number(pref::EDITOR_TAB_SIZE, 1..=16),
        };
        // SAFETY: the font belongs to the live ImGui context; ImGui returns the baked size for this glyph size.
        let baked = unsafe { sys::ImFont_GetFontBaked(font.raw(), cx.font_px(font, size), -1.0) };
        set_tab_size(baked, style.tab_size);
        style
    }

    /// One line's height, device px.
    fn line(&self, cx: &Cx<'_>) -> f32 {
        cx.px(self.line_h)
    }

    fn mono_text(
        &self,
        cx: &Cx<'_>,
        draw: DrawList<'_>,
        x: f32,
        line_top: f32,
        color: Color,
        text: &str,
    ) {
        cx.draw_text(
            draw,
            self.font,
            self.size,
            vec2(
                x,
                css_y(cx, self.size, line_top + cx.px(self.text_dy), true),
            ),
            color,
            text,
        );
    }
}

/// The design-system checkbox: 12px, rgba(0,0,0,.2) with a 1px 40% white border, radius 1; checked: the accent
/// with a white check.
fn checkbox(cx: &mut Cx<'_>, draw: DrawList<'_>, id: &str, min: Vec2, checked: bool) -> bool {
    let p = cx.palette;
    let max = min + vec2(cx.px(12.0), cx.px(12.0));
    cx.set_cursor(min);
    let clicked = cx.ui.invisible_button(id, (max - min).to_array());
    if checked {
        draw.add_rect_filled(min, max, p.accent, cx.px(1.0));
        cx.draw_fa_icon(draw, "fas:check", (min + max) / 2.0, cx.px(9.0), p.white);
    } else {
        draw.add_rect_filled(min, max, p.input_bg, cx.px(1.0));
        let hovered = cx.ui.is_item_hovered();
        draw.add_rect(
            min,
            max,
            p.ink(if hovered { 0.7 } else { 0.4 }),
            cx.px(1.0),
            1.0,
        );
    }
    clicked
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Primary,
    Success,
}

/// What a `.btn-xs` reports.
struct Pressed {
    clicked: bool,
    width: f32,
}

/// A .btn-xs (22px, 1px 5px padding, 12px text) with an optional icon 10px after the label, its right edge at
/// `right`.
#[allow(clippy::too_many_arguments)]
fn xs_button(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    id: &str,
    label: Option<&str>,
    icon: Option<&str>,
    right: f32,
    top: f32,
    tone: Tone,
) -> Pressed {
    let p = cx.palette;
    let color = match tone {
        Tone::Primary => p.accent,
        Tone::Success => p.green,
    };
    let regular = cx.fonts.regular;
    let label_w = label.map_or(0.0, |label| cx.text_size(regular, 12.0, label).x);
    let icon_w = icon.map_or(0.0, |icon| cx.fa_icon_width(icon, cx.px(12.0)));
    let gap = if label.is_some() && icon.is_some() {
        cx.px(10.0)
    } else {
        0.0
    };
    let w = label_w + gap + icon_w + cx.px(12.0);
    let min = vec2((right - w).round(), top);
    let max = vec2(min.x + w, top + cx.px(22.0));
    cx.set_cursor(min);
    let clicked = cx.ui.invisible_button(id, [w, max.y - min.y]);
    let hovered = cx.ui.is_item_hovered();
    draw.add_rect_filled(
        min,
        max,
        color.with_alpha(if hovered { 0.3 } else { 0.2 }),
        cx.px(2.0),
    );
    draw.add_rect(min, max, color, cx.px(2.0), 1.0);
    let text = if hovered {
        p.text_selected
    } else {
        p.text_normal
    };
    if let Some(label) = label {
        line_text(
            cx,
            draw,
            regular,
            12.0,
            min.x + cx.px(6.0),
            min.y + cx.px(2.0),
            cx.px(18.0),
            text,
            label,
        );
    }
    if let Some(icon) = icon {
        cx.draw_fa_icon(
            draw,
            icon,
            vec2(
                min.x + cx.px(6.0) + label_w + gap + icon_w / 2.0,
                min.y + cx.px(11.0),
            ),
            cx.px(12.0),
            text,
        );
    }
    Pressed { clicked, width: w }
}

/// The decorations of one code line: a tinted background from the text on, and in the margin a 4px bar of the
/// solid colour then the tint.
fn decorate(
    cx: &Cx<'_>,
    code: &CodeStyle,
    draw: DrawList<'_>,
    color: Color,
    left: f32,
    top: f32,
    right: f32,
) {
    let tint = color.with_alpha(0.25);
    draw.add_rect_filled(
        vec2(left + cx.px(DECORATION_X), top),
        vec2(right, top + code.line(cx)),
        tint,
        0.0,
    );
    draw.add_rect_filled(
        vec2(left + cx.px(DECORATION_X), top),
        vec2(left + cx.px(DECORATION_X + 4.0), top + code.line(cx)),
        color,
        0.0,
    );
}

fn line_number(
    cx: &Cx<'_>,
    code: &CodeStyle,
    draw: DrawList<'_>,
    left: f32,
    top: f32,
    number: usize,
) {
    if !code.line_numbers {
        return;
    }
    let text = number.to_string();
    code.mono_text(
        cx,
        draw,
        left + cx.px(NUMBERS_RIGHT) - cx.text_size(code.font, code.size, &text).x,
        top,
        LINE_NUMBER,
        &text,
    );
}

/// The glyph beside the line number: a green check for a line in the output; on hover a red minus to take it out
/// or a green plus to put it in.
fn glyph(
    cx: &mut Cx<'_>,
    code: &CodeStyle,
    draw: DrawList<'_>,
    left: f32,
    top: f32,
    picked: bool,
    hovered: bool,
) {
    let p = cx.palette;
    let (icon, color) = if hovered {
        (
            if picked {
                "fas:minus-circle"
            } else {
                "fas:plus-circle"
            },
            if picked { p.red } else { p.green },
        )
    } else if picked {
        ("fas:check-circle", p.green)
    } else {
        return;
    };
    cx.draw_fa_icon(
        draw,
        icon,
        vec2(left + cx.px(GLYPH_X + 7.2), top + cx.px(code.line_h / 2.0)),
        cx.px(14.0),
        color,
    );
}

/// Scrolls the current child so `line` sits a few lines below the top.
fn scroll_to_line(cx: &Cx<'_>, code: &CodeStyle, line: usize) {
    cx.ui
        .set_scroll_y(line.saturating_sub(3) as f32 * code.line(cx));
}

/// The lines `0..count` (of `line_h` px each, the first at `top`) that the clip rectangle of `draw` shows.
fn visible_lines(draw: DrawList<'_>, top: f32, line_h: f32, count: usize) -> Range<usize> {
    visible_range(
        draw.clip_rect().min.y,
        draw.clip_rect().max.y,
        top,
        line_h,
        count,
    )
}

fn visible_range(
    clip_min: f32,
    clip_max: f32,
    top: f32,
    line_h: f32,
    count: usize,
) -> Range<usize> {
    if line_h <= 0.0 {
        return 0..0;
    }
    let first = ((clip_min - top) / line_h).floor().max(0.0) as usize;
    let end = (((clip_max - top) / line_h).ceil().max(0.0) as usize).min(count);
    first.min(end)..end
}

/// A pick or take-back the user asked for in a pane; applied once the pane is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    /// A whole side of a conflict (`None`: of every conflict).
    Side(Option<usize>, Side),
    /// One line of a side.
    Line(usize, Side, usize),
}

fn apply(tool: &mut MergeTool, edits: &[Edit]) {
    for &edit in edits {
        match edit {
            Edit::Side(conflict, side) => tool.toggle_side(conflict, side),
            Edit::Line(conflict, side, line) => tool.toggle_line(conflict, side, line),
        }
    }
}

/// A (ours) or B (theirs): the side's file with its conflicts highlighted and pickable, its code coloured by its
/// syntax when `syntax_highlighting`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn draw_side(
    cx: &mut Cx<'_>,
    code: &CodeStyle,
    cache: &mut TextCache,
    tool: &MergeTool,
    side: Side,
    origin: Vec2,
    size: Vec2,
    syntax_highlighting: bool,
    edits: &mut Vec<Edit>,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let a = side == Side::A;
    let color = side_color(side);
    let edited = tool.edited.is_some();

    // Header: the take-all checkbox, the boxed letter, "Commit <sha6> on <branch>".
    let hmax = vec2(origin.x + size.x, origin.y + cx.px(PANE_HEADER_H));
    draw.add_rect_filled(origin, hmax, p.panel_bg0, 0.0);
    draw.add_rect_filled(vec2(origin.x, hmax.y - 1.0), hmax, p.section_border, 0.0);
    let all = tool.side_fully_picked(None, side);
    if checkbox(
        cx,
        draw,
        if a { "##take_all_a" } else { "##take_all_b" },
        origin + vec2(cx.px(12.0), cx.px(14.0)),
        all,
    ) && !edited
    {
        edits.push(Edit::Side(None, side));
    }
    let bmin = origin + vec2(cx.px(32.0), cx.px(5.0));
    let bmax = bmin + vec2(cx.px(24.0), cx.px(24.0));
    draw.add_rect_filled(bmin, bmax, p.panel_bg2, 0.0);
    draw.add_rect(
        bmin + vec2(1.0, 1.0),
        bmax - vec2(1.0, 1.0),
        color,
        0.0,
        cx.px(2.0),
    );
    let letter = if a { "A" } else { "B" };
    let letter_size = cx.text_size(cx.fonts.bold, 20.0, letter);
    cx.draw_text(
        draw,
        cx.fonts.bold,
        20.0,
        vec2(
            ((bmin.x + bmax.x - letter_size.x) / 2.0).round(),
            ((bmin.y + bmax.y - letter_size.y) / 2.0).round(),
        ),
        color,
        letter,
    );
    let mut x = origin.x + cx.px(68.0);
    let label_top = origin.y + cx.px(7.0);
    // One word of the label, `x` after it.
    let word = |cx: &Cx<'_>, x: f32, font: Font, text: &str, text_color: Color| {
        line_text(
            cx,
            draw,
            font,
            14.0,
            x,
            label_top,
            cx.px(19.0),
            text_color,
            text,
        );
        x + cx.text_size(font, 14.0, text).x + cx.px(4.0)
    };
    let (sha, branch) = if a {
        (&tool.ours_sha, &tool.ours_label)
    } else {
        (&tool.theirs_sha, &tool.theirs_label)
    };
    x = word(cx, x, cx.fonts.regular, "Commit", p.text_normal);
    x = word(
        cx,
        x,
        cx.fonts.regular,
        sha.get(..6).unwrap_or(sha),
        p.text_normal,
    );
    if !branch.is_empty() {
        x = word(cx, x, cx.fonts.regular, "on", p.text_normal);
        x += cx.px(3.0);
        word(cx, x, cx.fonts.bold, branch, color);
    }

    // The code, its syntax colours kept per side until its text changes.
    let lines: &[String] = if a { &tool.ours } else { &tool.theirs };
    let colours = if syntax_highlighting {
        cache.get(
            &tool.path,
            lines,
            tool.revision.map(imgit_git::Revision::get),
        )
    } else {
        None
    };
    cx.set_cursor(vec2(origin.x, hmax.y));
    let background = ui.push_style_color(StyleColor::ChildBg, p.app_bg0);
    ui.child_window(if a { "##merge_a" } else { "##merge_b" })
        .size([size.x, size.y - cx.px(PANE_HEADER_H)])
        .flags(WindowFlags::HORIZONTAL_SCROLLBAR)
        .build(ui, || {
            if let Some(request) = tool.scroll_request
                && let Some(chunk) = tool.conflict_chunk(request)
            {
                scroll_to_line(cx, code, chunk.side(side).start);
            }
            let code_draw = cx.window_draw_list();
            let top_left = cx.cursor();
            let right = top_left.x + cx.avail().x;
            let line_h = code.line(cx);
            // A final line break shows as an empty last line, like any editor.
            let shown = lines.len() + usize::from(tool.final_newline && !lines.is_empty());
            let mut widest = 0.0_f32;
            for i in visible_lines(code_draw, top_left.y, line_h, shown) {
                let top = top_left.y + i as f32 * line_h;
                if i >= lines.len() {
                    line_number(cx, code, code_draw, top_left.x, top, i + 1);
                    continue;
                }
                let conflict = tool.conflict_at(side, i);
                let _id = ui.push_id(i as i32);
                if let Some(conflict) = conflict {
                    decorate(cx, code, code_draw, color, top_left.x, top, right);
                    // The conflict's checkbox on its first line, in the glyph margin.
                    if tool
                        .conflict_chunk(conflict)
                        .is_some_and(|chunk| chunk.side(side).start == i)
                        && checkbox(
                            cx,
                            code_draw,
                            "##take_side",
                            vec2(top_left.x + cx.px(2.0), top + cx.px(2.0)),
                            tool.side_fully_picked(Some(conflict), side),
                        )
                        && !edited
                    {
                        edits.push(Edit::Side(Some(conflict), side));
                    }
                    // The rest of the line picks it alone.
                    cx.set_cursor(vec2(top_left.x + cx.px(16.0), top));
                    let clicked = ui.invisible_button(
                        "##pick_line",
                        [(right - top_left.x - cx.px(16.0)).max(1.0), line_h],
                    );
                    let hovered = ui.is_item_hovered() && !edited;
                    if clicked && !edited {
                        edits.push(Edit::Line(conflict, side, i));
                    }
                    glyph(
                        cx,
                        code,
                        code_draw,
                        top_left.x,
                        top,
                        tool.is_picked(conflict, side, i),
                        hovered,
                    );
                }
                drop(_id);
                line_number(cx, code, code_draw, top_left.x, top, i + 1);
                let text = &lines[i];
                cx.draw_code_text(
                    code_draw,
                    code.font,
                    code.size,
                    vec2(
                        top_left.x + cx.px(MARGIN_W),
                        css_y(cx, code.size, top + cx.px(code.text_dy), true),
                    ),
                    text,
                    colours.map_or(&[][..], |colours| colours.line(i)),
                    p.text_normal,
                );
                widest = widest.max(cx.text_size(code.font, code.size, text).x);
            }
            cx.set_cursor(top_left);
            ui.dummy([
                cx.px(MARGIN_W) + widest + cx.px(40.0),
                shown as f32 * line_h + size.y / 2.0,
            ]);
        });
    drop(background);
}

/// The text the Output turns into when it is edited by hand: its lines, placeholders left out.
fn editable_text(lines: &[OutputLine]) -> String {
    lines
        .iter()
        .filter(|line| !line.is_placeholder())
        .fold(String::new(), |mut text, line| {
            text += &line.text;
            text.push('\n');
            text
        })
}

/// The 1-based line numbers of the Output's rows: placeholders have none.
fn output_numbers(lines: &[OutputLine]) -> Vec<usize> {
    let mut number = 0;
    lines
        .iter()
        .map(|line| {
            if line.is_placeholder() {
                0
            } else {
                number += 1;
                number
            }
        })
        .collect()
}

/// The Output: the merge as it stands, its code coloured by its syntax when `syntax_highlighting`, or the text
/// being edited.
#[allow(clippy::too_many_lines)]
fn draw_output(
    cx: &mut Cx<'_>,
    code: &CodeStyle,
    cache: &mut TextCache,
    tool: &mut MergeTool,
    origin: Vec2,
    size: Vec2,
    syntax_highlighting: bool,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    draw.add_rect_filled(
        origin,
        vec2(origin.x + size.x, origin.y + 1.0),
        p.panel_border,
        0.0,
    );
    let hmin = vec2(origin.x, origin.y + 1.0);
    let hmax = vec2(origin.x + size.x, hmin.y + cx.px(PANE_HEADER_H));
    draw.add_rect_filled(hmin, hmax, p.panel_bg0, 0.0);
    draw.add_rect_filled(vec2(hmin.x, hmax.y - 1.0), hmax, p.section_border, 0.0);
    let title = "Output";
    line_text(
        cx,
        draw,
        cx.fonts.regular,
        18.0,
        hmin.x + cx.px(10.0),
        origin.y + cx.px(6.1),
        cx.px(25.7),
        p.text_normal,
        title,
    );
    let title_end = hmin.x + cx.px(10.0) + cx.text_size(cx.fonts.regular, 18.0, title).x;

    // Reset on the right once something was picked or typed.
    let reset_right = hmax.x - cx.px(10.0);
    if (tool.any_picked() || tool.edited.is_some())
        && xs_button(
            cx,
            draw,
            "##merge_reset",
            Some("Reset"),
            None,
            reset_right,
            hmin.y + cx.px(7.0),
            Tone::Primary,
        )
        .clicked
    {
        tool.reset();
    }
    // The navigator, centred between the title and the Reset area: "conflict N of M", up, down.
    let count = tool.conflict_count();
    if count > 0 {
        tool.current = tool.current.min(count - 1);
        let info = format!("conflict {} of {}", tool.current + 1, count);
        let info_w = cx.text_size(cx.fonts.semibold, 14.0, &info).x;
        let group = info_w + cx.px(10.0 + 24.0 + 10.0 + 24.0);
        let center = (title_end + hmax.x - cx.px(65.0)) / 2.0;
        let mut x = (center - group / 2.0).round();
        line_text(
            cx,
            draw,
            cx.fonts.semibold,
            14.0,
            x,
            hmin.y + cx.px(7.0),
            cx.px(22.0),
            A_COLOR,
            &info,
        );
        x += info_w + cx.px(10.0);
        if xs_button(
            cx,
            draw,
            "##conflict_up",
            None,
            Some("far:chevron-up"),
            x + cx.px(24.0),
            hmin.y + cx.px(7.0),
            Tone::Primary,
        )
        .clicked
        {
            tool.current = (tool.current + count - 1) % count;
            tool.scroll_request = Some(tool.current);
        }
        x += cx.px(24.0 + 10.0);
        if xs_button(
            cx,
            draw,
            "##conflict_down",
            None,
            Some("far:chevron-down"),
            x + cx.px(24.0),
            hmin.y + cx.px(7.0),
            Tone::Primary,
        )
        .clicked
        {
            tool.current = (tool.current + 1) % count;
            tool.scroll_request = Some(tool.current);
        }
    }

    let body = vec2(origin.x, hmax.y + 1.0);
    let body_size = vec2(size.x, (origin.y + size.y - body.y).max(1.0));
    if let Some(mut text) = tool.edited.take() {
        // Typed-in output: a plain text editor with the code font.
        cx.set_cursor(body);
        {
            let _frame = ui.push_style_color(StyleColor::FrameBg, p.app_bg0);
            let _padding = ui.push_style_var(StyleVar::FramePadding([cx.px(MARGIN_W), cx.px(2.0)]));
            let _font = cx.push_font(code.font, code.size);
            // SAFETY: the pushed font is current, so ImGui returns its baked size for this frame.
            set_tab_size(unsafe { sys::igGetFontBaked() }, code.tab_size);
            cx.input_multiline("##merge_output_text", &mut text, None, body_size, false);
        }
        tool.edited = Some(text);
        return;
    }

    let lines = tool.output();
    // Its syntax colours, kept until the merged text changes (a pick, a reset).
    let colours = if syntax_highlighting {
        let texts: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
        cache.get(&tool.path, &texts, None)
    } else {
        None
    };
    cx.set_cursor(body);
    let background = ui.push_style_color(StyleColor::ChildBg, p.app_bg0);
    let mut edits = Vec::new();
    let mut edit_text = false;
    ui.child_window("##merge_output")
        .size(body_size.into())
        .flags(WindowFlags::HORIZONTAL_SCROLLBAR)
        .build(ui, || {
            if let Some(request) = tool.scroll_request
                && let Some(row) = lines
                    .iter()
                    .position(|line| line.conflict() == Some(request))
            {
                scroll_to_line(cx, code, row);
            }
            let code_draw = cx.window_draw_list();
            let top_left = cx.cursor();
            let right = top_left.x + cx.avail().x;
            let line_h = code.line(cx);
            // Line numbers skip the placeholders.
            let numbers = output_numbers(&lines);
            let number = numbers.iter().copied().max().unwrap_or(0);
            let shown = lines.len() + usize::from(tool.final_newline && number > 0);
            let mut widest = 0.0_f32;
            for i in visible_lines(code_draw, top_left.y, line_h, shown) {
                let top = top_left.y + i as f32 * line_h;
                let Some(line) = lines.get(i) else {
                    line_number(cx, code, code_draw, top_left.x, top, number + 1);
                    continue;
                };
                let _id = ui.push_id(i as i32);
                match line.origin {
                    Origin::Unresolved { .. } => {
                        decorate(cx, code, code_draw, OUTPUT_COLOR, top_left.x, top, right);
                    }
                    Origin::Picked {
                        side,
                        conflict,
                        line: source,
                    } => {
                        decorate(
                            cx,
                            code,
                            code_draw,
                            side_color(side),
                            top_left.x,
                            top,
                            right,
                        );
                        line_text(
                            cx,
                            code_draw,
                            cx.fonts.regular,
                            12.0,
                            top_left.x + cx.px(4.0),
                            top,
                            line_h,
                            p.text_normal,
                            if side == Side::A { "A" } else { "B" },
                        );
                        // The glyph takes the line back out.
                        cx.set_cursor(vec2(top_left.x + cx.px(GLYPH_X), top));
                        let clicked = ui.invisible_button("##drop_line", [cx.px(15.0), line_h]);
                        glyph(
                            cx,
                            code,
                            code_draw,
                            top_left.x,
                            top,
                            true,
                            ui.is_item_hovered(),
                        );
                        if clicked {
                            edits.push(Edit::Line(conflict, side, source));
                        }
                    }
                    Origin::Merged => {}
                }
                drop(_id);
                if numbers[i] > 0 {
                    line_number(cx, code, code_draw, top_left.x, top, numbers[i]);
                }
                cx.draw_code_text(
                    code_draw,
                    code.font,
                    code.size,
                    vec2(
                        top_left.x + cx.px(MARGIN_W),
                        css_y(cx, code.size, top + cx.px(code.text_dy), true),
                    ),
                    &line.text,
                    colours.map_or(&[][..], |colours| colours.line(i)),
                    p.text_normal,
                );
                widest = widest.max(cx.text_size(code.font, code.size, &line.text).x);
            }
            // A double-click on the text switches to editing the output by hand.
            cx.set_cursor(vec2(top_left.x + cx.px(MARGIN_W), top_left.y));
            let content_h = shown as f32 * line_h + body_size.y / 2.0;
            ui.invisible_button(
                "##output_text",
                [(right - top_left.x - cx.px(MARGIN_W)).max(1.0), content_h],
            );
            if ui.is_item_hovered() && ui.is_mouse_double_clicked(MouseButton::Left) {
                edit_text = true;
            }
            cx.set_cursor(top_left);
            ui.dummy([cx.px(MARGIN_W) + widest + cx.px(40.0), content_h]);
        });
    drop(background);
    if edit_text {
        tool.edited = Some(editable_text(&lines));
    }
    apply(tool, &edits);
}

/// What closing the merge tool asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Save,
    External,
    Close,
}

/// The merge tool for `state.merge_tool`, over the whole tab, `size` device px.
pub fn draw_merge_view(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState, size: Vec2) {
    let Some(mut tool) = state.merge_tool.take() else {
        return;
    };
    let outcome = app.with_panel_state(
        |panels| &mut panels.merge_view,
        |app, ui_state| draw_view(cx, app, ui_state, &mut tool, size),
    );
    let Some(outcome) = outcome else {
        state.merge_tool = Some(tool);
        return;
    };
    let path = tool.path.clone();
    match outcome {
        Outcome::Save => {
            let text = tool.output_text();
            let file = state.repo.workdir().join(&path);
            // Staging the file clears its conflict.
            let result = std::fs::write(&file, text)
                .map_err(|source| Error::Io {
                    path: path.clone(),
                    source,
                })
                .and_then(|()| state.repo.stage(&[path.as_str()]));
            app.run(state, "Save resolution", result, None);
        }
        Outcome::External => {
            app.run_git_cli(
                state,
                "External merge tool",
                ["mergetool", "--no-prompt", "--", path.as_str()],
            );
        }
        Outcome::Close => {}
    }
}

#[allow(clippy::too_many_lines)]
fn draw_view(
    cx: &mut Cx<'_>,
    app: &mut Application,
    ui_state: &mut State,
    tool: &mut MergeTool,
    size: Vec2,
) -> Option<Outcome> {
    let ui = cx.ui;
    let p = cx.palette;
    let code = CodeStyle::read(cx);
    let origin = cx.cursor();
    let mut outcome = None;
    let syntax = app.settings.syntax_highlighting;
    let (a_fraction, top_fraction) = (
        app.settings.merge_a_fraction,
        app.settings.merge_top_fraction,
    );
    let body_top = origin.y + cx.px(HEADER_H);
    let body_h = size.y - cx.px(HEADER_H);
    // A | B over the Output, split where the user dragged the handles (halves by default).
    let min_w = cx.px(MIN_PANE_W).min(size.x / 2.0);
    let min_h = cx.px(MIN_PANE_H).min(body_h / 2.0);
    let panes_h = (body_h * top_fraction)
        .clamp(min_h, (body_h - min_h).max(min_h))
        .round();
    let half = (size.x * a_fraction)
        .clamp(min_w, (size.x - min_w).max(min_w))
        .round();
    ui.child_window("##merge_view")
        .size(size.into())
        .flags(WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE)
        .build(ui, || {
            let draw = cx.window_draw_list();
            draw.add_rect_filled(origin, origin + size, p.app_bg0, 0.0);
            outcome = draw_header(cx, tool, origin, size);
            let mut edits = Vec::new();
            let [cache_a, cache_b] = &mut ui_state.sides;
            draw_side(
                cx,
                &code,
                cache_a,
                tool,
                Side::A,
                vec2(origin.x, body_top),
                vec2(half - 1.0, panes_h),
                syntax,
                &mut edits,
            );
            draw.add_rect_filled(
                vec2(origin.x + half - 1.0, body_top),
                vec2(origin.x + half, body_top + panes_h),
                p.panel_border,
                0.0,
            );
            draw_side(
                cx,
                &code,
                cache_b,
                tool,
                Side::B,
                vec2(origin.x + half, body_top),
                vec2(size.x - half, panes_h),
                syntax,
                &mut edits,
            );
            apply(tool, &edits);
            draw_output(
                cx,
                &code,
                &mut ui_state.output,
                tool,
                vec2(origin.x, body_top + panes_h),
                vec2(size.x, body_h - panes_h),
                syntax,
            );
            tool.scroll_request = None;
        });

    // The resize handles over the panes' borders (windows of their own, above the panes' child windows): the
    // border between A and B, and the Output's top border. The split is kept as a share of the view.
    let mut a_w = half;
    let mut top_h = panes_h;
    let mut released = cx.overlay_resize_drag(
        "##merge_resize_ab",
        vec2(origin.x + half - cx.px(4.0), body_top),
        vec2(origin.x + half + cx.px(2.0), body_top + panes_h),
        ResizeAxis::Columns,
        1.0,
        &mut a_w,
        min_w,
        size.x - min_w,
    );
    released |= cx.overlay_resize_drag(
        "##merge_resize_output",
        vec2(origin.x, body_top + panes_h - cx.px(3.0)),
        vec2(origin.x + size.x, body_top + panes_h + cx.px(3.0)),
        ResizeAxis::Rows,
        1.0,
        &mut top_h,
        min_h,
        body_h - min_h,
    );
    // Only a drag moves the stored split (not the clamp of a narrow window).
    if a_w != half {
        app.settings.merge_a_fraction = (a_w / size.x).clamp(0.1, 0.9);
    }
    if top_h != panes_h {
        app.settings.merge_top_fraction = (top_h / body_h).clamp(0.1, 0.9);
    }
    if released {
        app.settings.save(&app.config_path("settings.txt"));
    }
    outcome
}

/// The header: panel-bg0, 5px 10px padding, a 1px section border under it. Right to left: close, Save, Open in
/// external merge tool, 10px apart.
fn draw_header(cx: &mut Cx<'_>, tool: &MergeTool, origin: Vec2, size: Vec2) -> Option<Outcome> {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let hmax = vec2(origin.x + size.x, origin.y + cx.px(HEADER_H));
    draw.add_rect_filled(origin, hmax, p.panel_bg0, 0.0);
    draw.add_rect_filled(vec2(origin.x, hmax.y - 1.0), hmax, p.section_border, 0.0);
    cx.draw_fa_icon(
        draw,
        "fas:exclamation-triangle",
        origin + vec2(cx.px(17.9), cx.px(18.3)),
        cx.px(14.0),
        p.orange,
    );
    let name = Path::new(&tool.path)
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let regular = cx.fonts.regular;
    line_text(
        cx,
        draw,
        regular,
        14.0,
        origin.x + cx.px(30.8),
        origin.y + cx.px(8.6),
        cx.px(20.0),
        p.text_normal,
        &name,
    );
    let count = tool.conflict_count();
    let conflicts = format!(
        "({count} {})",
        if count == 1 { "conflict" } else { "conflicts" }
    );
    line_text(
        cx,
        draw,
        regular,
        12.6,
        origin.x + cx.px(30.8) + cx.text_size(regular, 14.0, &name).x + cx.px(10.0),
        origin.y + cx.px(10.6),
        cx.px(18.0),
        p.text_secondary,
        &conflicts,
    );

    let top = origin.y + cx.px(9.1);
    let mut right = hmax.x - cx.px(10.0);
    let cmin = vec2(right - cx.px(18.5), origin.y + cx.px(7.6));
    cx.set_cursor(cmin);
    let close = ui.invisible_button("##merge_close", [cx.px(18.5), cx.px(22.0)]);
    cx.draw_fa_icon(
        draw,
        "fas:times",
        cmin + vec2(cx.px(9.25), cx.px(11.0)),
        cx.px(20.0),
        if ui.is_item_hovered() {
            p.text_selected
        } else {
            p.text_normal
        },
    );
    right = cmin.x - cx.px(10.0);
    let save = xs_button(
        cx,
        draw,
        "##merge_save",
        Some("Save"),
        Some("far:save"),
        right,
        top,
        Tone::Success,
    );
    right -= save.width + cx.px(10.0);
    let external = xs_button(
        cx,
        draw,
        "##merge_external",
        Some("Open in external merge tool"),
        Some("far:external-link-alt"),
        right,
        top,
        Tone::Primary,
    );
    if save.clicked {
        Some(Outcome::Save)
    } else if external.clicked {
        Some(Outcome::External)
    } else if close {
        Some(Outcome::Close)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use imgit_app::merge_tool::Origin;

    fn line(text: &str, origin: Origin) -> OutputLine {
        OutputLine {
            text: text.to_owned(),
            origin,
        }
    }

    #[test]
    fn visible_lines_follow_the_clip_rectangle() {
        // 10 lines of 16px from y = 100; a window showing y 120..170 (lines 1 to 4).
        assert_eq!(visible_range(120.0, 170.0, 100.0, 16.0, 10), 1..5);
        // Scrolled past the content, or above it: nothing, never an inverted range.
        assert_eq!(visible_range(400.0, 500.0, 100.0, 16.0, 10), 10..10);
        assert_eq!(visible_range(0.0, 50.0, 100.0, 16.0, 10), 0..0);
        // The whole content, capped at the count.
        assert_eq!(visible_range(0.0, 1000.0, 100.0, 16.0, 10), 0..10);
        assert_eq!(visible_range(0.0, 1000.0, 100.0, 0.0, 10), 0..0);
    }

    #[test]
    fn the_output_numbers_skip_placeholders() {
        let lines = [
            line("a", Origin::Merged),
            line(
                "",
                Origin::Unresolved {
                    conflict: 0,
                    placeholder: true,
                },
            ),
            line(
                "b",
                Origin::Unresolved {
                    conflict: 0,
                    placeholder: false,
                },
            ),
        ];
        assert_eq!(output_numbers(&lines), [1, 0, 2]);
        assert_eq!(editable_text(&lines), "a\nb\n");
        assert_eq!(editable_text(&[]), "");
    }

    #[test]
    fn edits_pick_and_drop() {
        let mut tool = MergeTool::default();
        tool.load("a\nb\nc\n", "a\nours\nc\n", "a\ntheirs\nc\n");
        assert_eq!(tool.conflict_count(), 1);
        apply(
            &mut tool,
            &[Edit::Line(0, Side::A, 1), Edit::Line(0, Side::B, 1)],
        );
        assert!(tool.is_picked(0, Side::A, 1) && tool.is_picked(0, Side::B, 1));
        apply(&mut tool, &[Edit::Line(0, Side::A, 1)]);
        assert!(!tool.is_picked(0, Side::A, 1));
        apply(&mut tool, &[Edit::Side(None, Side::A)]);
        assert!(tool.side_fully_picked(Some(0), Side::A));
    }

    #[test]
    fn side_colours() {
        assert_eq!(side_color(Side::A), A_COLOR);
        assert_eq!(side_color(Side::B), B_COLOR);
    }
}
