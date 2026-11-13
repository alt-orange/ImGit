//! The file view (diff, hunks, split, blame gutter) and the diff editor shared with the File History view.

use std::collections::HashMap;

use dear_imgui_rs::{ListClipper, MouseButton, MouseCursor, StyleVar, WindowFlags, sys};
use imgit_app::RepoState;
use imgit_app::repo_state::{DiffMode, DiffSource, DiffTarget, WIP_SHA};
use imgit_app::syntax::SyntaxDiff;
use imgit_git::repository::{LineSelection, short_sha};
use imgit_git::{Blame, BlameHunk, DiffLine, FileDiff, FileStatus, LineOrigin, Revision, Sha};
use imgit_syntax::{DiffCache, DiffSpans};

use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::app::toasts::ToastKind;
use crate::fonts::Font;
use crate::theme::{Color, FONT_SIZE_ROW, GRAPH_COLOR_COUNT, Ink};
use crate::ui::draw::DrawList;
use crate::ui::panels::file_editor::{navigate_from_editor, open_file_editor};
use crate::ui::widgets::file_name;
use crate::ui::{Cx, Vec2, vec2};

// The file view: a 36px header, a 53px toolbar row, then Monaco's diff editor with Consolas 12px on 16px lines
// (by default: the Editor preferences, `code_style()`).
pub(super) const HEADER_H: f32 = 36.0;
pub(super) const TOOLBAR_H: f32 = 53.0;
/// The file view's text, after the numbers.
const FILE_TEXT_X: f32 = 43.0;
/// The original editor's 33px line-number margin.
const OLD_NUMBERS_RIGHT: f32 = 33.0;
const NEW_NUMBERS_RIGHT: f32 = 66.0;
/// After the 10px +/- sign column.
const TEXT_X: f32 = 76.0;
/// `.diffViewport`: the original / modified overview rulers, 15px each.
const DIFF_RULER_W: f32 = 30.0;
const DECORATIONS_RULER_W: f32 = 14.0;
// The file view's blame gutter: the avatar column before the numbers, then (with the columns on) the commit's
// summary and date, then the 2px bar in the commit's colour before the text.
/// The avatar's centre.
const BLAME_AVATAR_X: f32 = 14.0;
const BLAME_AVATAR_DIAMETER: f32 = 14.0;
const BLAME_NUMBERS_RIGHT: f32 = 61.0;
const BLAME_TEXT_X: f32 = 78.0;
const BLAME_SUMMARY_X: f32 = 71.0;
const BLAME_SUMMARY_W: f32 = 210.0;
const BLAME_DATE_X: f32 = 289.0;
const BLAME_COLUMNS_TEXT_X: f32 = 378.0;

const LINE_NUMBER: Color = Color::from_rgba8(133, 133, 133, 255);
const ACTIVE_LINE_NUMBER: Color = Color::from_rgba8(198, 198, 198, 255);
const EDITOR_TEXT: Ink = Ink::alpha_255(255);

/// What the diff editors remember between frames: per editor (`DiffEditorOptions::id`) the syntax colours of
/// the diff it shows, and the row a shift-click in the line staging gutter extends from. Held in
/// [`UiState`](crate::ui::UiState), as [`draw_diff_editor`] has no panel state to put them in.
#[derive(Default)]
pub struct DiffEditors {
    editors: HashMap<String, EditorMemory>,
}

#[derive(Default)]
struct EditorMemory {
    /// The syntax colours of the diff, kept until it changes; none for plain text.
    syntax: DiffCache,
    /// The row the last line-staging click was on.
    anchor: Option<usize>,
    /// The diff `anchor` was clicked in.
    anchor_revision: Option<Revision>,
}

impl EditorMemory {
    /// Forgets the anchor when the editor shows another load of the diff than the one it was clicked in: the
    /// picked lines are cleared then (`draw_contents`), and a shift-click must not extend from a stale row.
    fn follow_revision(&mut self, revision: Option<Revision>) {
        if self.anchor_revision != revision {
            self.anchor_revision = revision;
            self.anchor = None;
        }
    }
}

impl DiffEditors {
    /// The memory of editor `id`, taken out while it draws (the colours borrow it, the frame's context too).
    fn take(&mut self, id: &str) -> EditorMemory {
        self.editors.remove(id).unwrap_or_default()
    }

    fn put(&mut self, id: &str, memory: EditorMemory) {
        self.editors.insert(id.to_owned(), memory);
    }
}

#[derive(Default)]
pub struct State {}

/// A line of the diff and where it is: its hunk and its index there (none for the empty line after the file's
/// final newline, which has no syntax colours).
#[derive(Debug, Clone, Copy)]
struct LineRef<'a> {
    line: &'a DiffLine,
    hunk: usize,
    index: Option<usize>,
}

impl LineRef<'_> {
    fn changed(&self) -> bool {
        self.line.origin != LineOrigin::Context
    }
}

/// One line of the inline / split / file layouts.
#[derive(Debug, Clone, Copy)]
struct Row<'a> {
    /// Inline: the line itself. Split: the old side.
    left: Option<LineRef<'a>>,
    /// Split: the new side.
    right: Option<LineRef<'a>>,
    hunk_header: bool,
    hunk: usize,
    /// Word wrap: which visual line of the row's text this is (0: the first).
    segment: usize,
}

impl Row<'_> {
    /// Whether either side is an addition or a deletion.
    fn changed(&self) -> bool {
        self.left.is_some_and(|line| line.changed())
            || self.right.is_some_and(|line| line.changed())
    }
}

/// The glyph box's top for text whose CSS box starts at `box_top` (Chromium rounds the baseline).
pub(super) fn css_text_y(cx: &Cx<'_>, size: f32, box_top: f32, mono: bool) -> f32 {
    let ascent = size * if mono { 1884.0 } else { 2189.0 } / 2048.0 * cx.scale;
    (box_top + ascent.round()).round() - ascent.ceil()
}

/// ImGui advances a tab by `IM_TABSIZE` spaces; the Tab Size preference replaces that in `font` at this size.
/// Set again each frame: ImGui rebuilds a baked size it dropped.
pub(super) fn set_tab_size(font: Font, font_px: f32, tab_size: u32) {
    // SAFETY: the font belongs to the live ImGui context (see `Font::raw`), which the callers' `Cx` proves is
    // current; the baked font and its glyph vector are ImGui's own, and no reference into them is kept across
    // the second glyph lookup (which may move the glyphs).
    unsafe {
        let baked = sys::ImFont_GetFontBaked(font.raw(), font_px, -1.0);
        if baked.is_null() {
            return;
        }
        let space = sys::ImFontBaked_FindGlyphNoFallback(baked, sys::ImWchar::from(b' '));
        if space.is_null() {
            return;
        }
        let advance = (*space).AdvanceX * tab_size as f32;
        let tab = sys::ImFontBaked_FindGlyphNoFallback(baked, sys::ImWchar::from(b'\t'));
        if tab.is_null() {
            return;
        }
        (*tab).AdvanceX = advance;
        let advances = &mut (*baked).IndexAdvanceX;
        if !advances.Data.is_null() && i32::from(b'\t') < advances.Size {
            *advances.Data.add(usize::from(b'\t')) = advance;
        }
    }
}

/// The window's scrollbar thickness (`ImGui::GetStyle().ScrollbarSize`) and item spacing.
fn style_scrollbar_size() -> f32 {
    // SAFETY: only called while a frame is open (the callers hold a `Cx`), so the style exists; one float read.
    unsafe { (*sys::igGetStyle()).ScrollbarSize }
}

pub(super) fn style_item_spacing_x() -> f32 {
    // SAFETY: as for `style_scrollbar_size`.
    unsafe { (*sys::igGetStyle()).ItemSpacing.x }
}

/// The right edge of the current window's visible content: its right edge less the vertical scrollbar,
/// whatever the horizontal scroll.
fn visible_right(cx: &Cx<'_>) -> f32 {
    // SAFETY: a window is current while its contents are drawn (the only caller runs inside the editor's child
    // window); one float of it is read.
    let scrollbar = unsafe {
        let window = sys::igGetCurrentWindow();
        if window.is_null() {
            0.0
        } else {
            (*window).ScrollbarSizes.x
        }
    };
    cx.ui.window_pos()[0] + cx.ui.window_size()[0] - scrollbar
}

/// Preferences -> Editor as the code is drawn: the face and size, Monaco's line height for the size (16px at
/// the default 12px), the glyph box's offset in the line, and whether the line numbers show.
#[derive(Debug, Clone, Copy)]
struct CodeStyle {
    font: Font,
    size: f32,
    /// CSS px, as the offset.
    line_h: f32,
    text_dy: f32,
    line_numbers: bool,
}

fn code_style(cx: &Cx<'_>) -> CodeStyle {
    use imgit_app::{pref, preferences};
    let prefs = preferences();
    let font = cx.fonts.editor;
    let size = prefs.number(pref::EDITOR_FONT_SIZE, 6..=40) as f32;
    let line_h = (size * 1.35).round();
    set_tab_size(
        font,
        cx.font_px(font, size),
        prefs.number(pref::EDITOR_TAB_SIZE, 1..=16),
    );
    CodeStyle {
        font,
        size,
        line_h,
        // Chromium centres the glyph box in the line; its baseline lands a pixel lower (measured).
        text_dy: ((line_h - size * font.em_scale) / 2.0).round() + 1.0,
        line_numbers: prefs.flag(pref::EDITOR_LINE_NUMBERS),
    }
}

/// Monaco text: Consolas 12px on the 16px line; Chromium puts its baseline 12px under the 14px glyph box that
/// starts 1px into the line (measured).
fn mono_text(
    cx: &Cx<'_>,
    code: &CodeStyle,
    draw: DrawList<'_>,
    x: f32,
    line_top: f32,
    color: Color,
    text: &str,
) {
    let y = css_text_y(cx, code.size, line_top + cx.px(code.text_dy), true);
    cx.draw_text(draw, code.font, code.size, vec2(x, y), color, text);
}

/// Monaco's line numbers: rgb(133,133,133), rgb(198,198,198) on the editor's cursor line.
fn mono_number(
    cx: &Cx<'_>,
    code: &CodeStyle,
    draw: DrawList<'_>,
    right: f32,
    line_top: f32,
    number: Option<u32>,
    active: bool,
) {
    let Some(number) = number.filter(|&number| number > 0 && code.line_numbers) else {
        return;
    };
    let text = number.to_string();
    let width = cx.text_size(code.font, code.size, &text).x;
    let color = if active {
        ACTIVE_LINE_NUMBER
    } else {
        LINE_NUMBER
    };
    mono_text(cx, code, draw, right - width, line_top, color, &text);
}

/// A toggle button (12px / 16px, 2.5px 10px padding, 23px): text or a 12px icon.
fn toggle_button(
    cx: &mut Cx<'_>,
    id: &str,
    label: Option<&str>,
    icon: Option<&str>,
    pos: Vec2,
    checked: bool,
    tip: Option<&str>,
) -> bool {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let w = toggle_width(cx, label, icon);
    let h = cx.px(23.0);
    cx.set_cursor(pos);
    let clicked = ui.invisible_button(id, [w, h]);
    let hovered = ui.is_item_hovered();
    let accent = p.accent.with_alpha(0.2);
    let max = pos + vec2(w, h);
    if checked {
        draw.add_rect_filled(pos, max, accent, 0.0);
    } else if hovered {
        draw.add_rect_filled(pos, max, p.hover_row, 0.0);
    }
    let border = if checked { accent } else { Ink(0.2).of(p) };
    draw.add_rect(pos, max, border, 0.0, 1.0);
    let color = if checked || hovered {
        p.text_selected
    } else {
        p.text_normal
    };
    if let Some(icon) = icon {
        cx.draw_fa_icon(
            draw,
            icon,
            vec2(pos.x + w / 2.0, pos.y + cx.px(11.0)),
            cx.px(12.0),
            color,
        );
    } else {
        cx.draw_text(
            draw,
            cx.fonts.regular,
            FONT_SIZE_ROW,
            vec2(
                pos.x + cx.px(11.0),
                css_text_y(cx, 12.0, pos.y + cx.px(2.5), false),
            ),
            color,
            label.unwrap_or_default(),
        );
    }
    if hovered && let Some(tip) = tip {
        cx.styled_tooltip(pos, max, tip, false);
    }
    clicked
}

fn toggle_width(cx: &Cx<'_>, label: Option<&str>, icon: Option<&str>) -> f32 {
    let content = match icon {
        Some(icon) => cx.fa_icon_width(icon, cx.px(12.0)),
        None => {
            cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, label.unwrap_or_default())
                .x
        }
    };
    content + cx.px(22.0)
}

/// The file view's toolbar toggles (text, or a 12px icon with a tooltip), 23px tall at `pos`. `true` when
/// clicked.
pub fn view_toggle_button(
    cx: &mut Cx<'_>,
    id: &str,
    label: Option<&str>,
    icon: Option<&str>,
    pos: Vec2,
    checked: bool,
    tip: Option<&str>,
) -> bool {
    toggle_button(cx, id, label, icon, pos, checked, tip)
}

/// The width of that toggle.
#[must_use]
pub fn view_toggle_width(cx: &Cx<'_>, label: Option<&str>, icon: Option<&str>) -> f32 {
    toggle_width(cx, label, icon)
}

/// The mark in the file view's header for what happened to the file.
fn status_icon(cx: &Cx<'_>, status: FileStatus) -> (&'static str, Color) {
    let p = cx.palette;
    match status {
        FileStatus::Added | FileStatus::Untracked => ("fas:plus", p.green),
        FileStatus::Deleted => ("fas:minus", p.red),
        _ => ("fas:pencil", p.orange),
    }
}

/// The empty line Monaco shows after the file's final newline: numbered after the diff's last lines.
fn final_line(diff: &FileDiff) -> Option<DiffLine> {
    let mut last = DiffLine::default();
    for line in diff.hunks.iter().flat_map(|hunk| &hunk.lines) {
        if let Some(number) = line.old_lineno {
            last.old_lineno = Some(number + 1);
        }
        if let Some(number) = line.new_lineno {
            last.new_lineno = Some(number + 1);
        }
    }
    (last.old_lineno.is_some() || last.new_lineno.is_some()).then_some(last)
}

/// Rows for the current mode. Split pairs the deletions and additions of each change block and pads the
/// shorter side; the file view keeps the new side only. `last` is [`final_line`], shown unless the hunk view is.
fn build_rows<'a>(
    diff: &'a FileDiff,
    mode: DiffMode,
    file_view: bool,
    last: Option<&'a DiffLine>,
) -> Vec<Row<'a>> {
    let split = mode == DiffMode::Split && !file_view;
    let row = |left: Option<LineRef<'a>>, right: Option<LineRef<'a>>, hunk: usize| Row {
        left,
        right,
        hunk_header: false,
        hunk,
        segment: 0,
    };
    let mut rows = Vec::new();
    for (h, hunk) in diff.hunks.iter().enumerate() {
        let line_ref = |index: usize| LineRef {
            line: &hunk.lines[index],
            hunk: h,
            index: Some(index),
        };
        if mode == DiffMode::Hunk && !file_view {
            rows.push(Row {
                hunk_header: true,
                ..row(None, None, h)
            });
        }
        if file_view || mode != DiffMode::Split {
            rows.extend(
                (0..hunk.lines.len())
                    .filter(|&i| !file_view || hunk.lines[i].origin != LineOrigin::Deleted)
                    .map(|i| row(Some(line_ref(i)), None, h)),
            );
            continue;
        }
        let mut i = 0;
        while i < hunk.lines.len() {
            if hunk.lines[i].origin == LineOrigin::Context {
                rows.push(row(Some(line_ref(i)), Some(line_ref(i)), h));
                i += 1;
                continue;
            }
            let (mut deleted, mut added) = (Vec::new(), Vec::new());
            while i < hunk.lines.len() && hunk.lines[i].origin != LineOrigin::Context {
                if hunk.lines[i].origin == LineOrigin::Deleted {
                    deleted.push(i);
                } else {
                    added.push(i);
                }
                i += 1;
            }
            for k in 0..deleted.len().max(added.len()) {
                rows.push(row(
                    deleted.get(k).map(|&i| line_ref(i)),
                    added.get(k).map(|&i| line_ref(i)),
                    h,
                ));
            }
        }
    }
    if (mode != DiffMode::Hunk || file_view)
        && let Some(last) = last
    {
        let last = LineRef {
            line: last,
            hunk: 0,
            index: None,
        };
        rows.push(row(Some(last), split.then_some(last), 0));
    }
    rows
}

/// How many `wrap_w` wide visual lines a line of `width` takes.
fn segment_count(width: f32, wrap_w: f32) -> usize {
    ((width / wrap_w - 0.001).ceil().max(1.0)) as usize
}

/// Word wrap: each row as many times as its longer side needs `wrap_w` wide visual lines (whole characters of
/// the monospace font), the copies numbered by `segment`. Hunk headers stay one line.
fn wrap_rows<'a>(cx: &Cx<'_>, code: &CodeStyle, rows: Vec<Row<'a>>, wrap_w: f32) -> Vec<Row<'a>> {
    let segments = |line: Option<LineRef<'_>>| match line {
        Some(line) if !line.line.text.is_empty() => segment_count(
            cx.text_size(code.font, code.size, &line.line.text).x,
            wrap_w,
        ),
        _ => 1,
    };
    let mut wrapped = Vec::with_capacity(rows.len());
    for row in rows {
        let count = if row.hunk_header {
            1
        } else {
            segments(row.left).max(segments(row.right))
        };
        wrapped.extend((0..count).map(|segment| Row { segment, ..row }));
    }
    wrapped
}

/// Change navigation: the first changed row above (`prev`) or below (`next`) the top row of the view, a row
/// starting a run of changed ones.
fn change_target(rows: &[Row<'_>], top_row: usize, prev: bool, next: bool) -> Option<usize> {
    let mut target = None;
    for (i, row) in rows.iter().enumerate() {
        let starts = row.changed() && (i == 0 || !rows[i - 1].changed());
        if !starts {
            continue;
        }
        if next && i > top_row {
            return Some(i);
        }
        if prev && i < top_row {
            target = Some(i);
        }
    }
    target
}

/// The whitespace of `text` drawn at `x` (where the text starts): a middle dot centred in each space's cell,
/// an arrow over each tab, in Monaco's editorWhitespace.foreground (rgba(227,228,226,.16)).
fn draw_whitespace(
    cx: &Cx<'_>,
    code: &CodeStyle,
    line_h: f32,
    draw: DrawList<'_>,
    x: f32,
    line_top: f32,
    text: &str,
) {
    let color = Color::from_rgba8(227, 228, 226, 72);
    let y = line_top + line_h / 2.0;
    for (i, c) in text.char_indices().filter(|&(_, c)| c == ' ' || c == '\t') {
        let x0 = x + cx.text_size(code.font, code.size, &text[..i]).x;
        let x1 = x + cx.text_size(code.font, code.size, &text[..=i]).x;
        if c == ' ' {
            draw.add_circle_filled(vec2((x0 + x1) / 2.0, y), cx.px(1.0), color, 8);
        } else {
            let head = cx.px(3.0);
            let tip = (x1 - cx.px(2.0)).min(x0 + cx.px(12.0));
            draw.add_line(vec2(x0 + cx.px(2.0), y), vec2(tip, y), color, 1.0);
            draw.add_line(vec2(tip - head, y - head), vec2(tip, y), color, 1.0);
            draw.add_line(vec2(tip - head, y + head), vec2(tip, y), color, 1.0);
        }
    }
}

/// Monaco's diagonal-fill: rgba(204,204,204,.2) stripes at -45 degrees, 8px apart (a 12.5% band).
fn diagonal_fill(cx: &Cx<'_>, draw: DrawList<'_>, min: Vec2, max: Vec2) {
    draw.with_clip_rect(min, max, true, || {
        let color = Color::from_rgba8(204, 204, 204, 51);
        let h = max.y - min.y;
        let mut x = min.x - h;
        while x < max.x {
            draw.add_line(vec2(x, max.y), vec2(x + h, min.y), color, cx.px(1.4));
            x += cx.px(8.0);
        }
    });
}

/// A graph colour index for a commit outside the loaded graph, stable per sha.
#[must_use]
pub fn sha_color_index(sha: &str) -> usize {
    let hash = sha.bytes().fold(0_u32, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(u32::from(byte))
    });
    hash as usize % GRAPH_COLOR_COUNT
}

/// The colour of a blame hunk's commit.
pub type BlameColor<'a> = Box<dyn Fn(&Sha) -> Color + 'a>;

/// What [`draw_diff_editor`] draws and how.
pub struct DiffEditorOptions<'a> {
    pub mode: DiffMode,
    pub file_view: bool,
    /// Scroll to the change above / below the top of the view.
    pub scroll_prev: bool,
    pub scroll_next: bool,
    /// File view: a gutter before the numbers for the blame of the shown revision: each hunk's first line
    /// carries its author's avatar, every line a bar in its commit's colour (`blame_color`, or one derived from
    /// the sha); `blame_columns` adds the commit's summary and date. `blame` may be `None` while it loads.
    pub blame_gutter: bool,
    pub blame: Option<&'a Blame>,
    pub blame_columns: bool,
    pub blame_color: Option<BlameColor<'a>>,
    /// Colour the code by its language (Preferences > Editor).
    pub syntax_highlighting: bool,
    /// Line staging (a working-directory diff, inline or hunk view): clicking a changed line's number gutter
    /// toggles it in `selected_lines` (shift: the range from the last one clicked); with `hunk_action` set
    /// ("Stage Hunk" / "Unstage Hunk"), each hunk header carries that button and a click sets `hunk_clicked`.
    pub selected_lines: Option<&'a mut Vec<LineSelection>>,
    pub hunk_action: Option<&'a str>,
    pub hunk_clicked: Option<&'a mut Option<usize>>,
    /// Long lines continue on the rows under them instead of scrolling sideways.
    pub word_wrap: bool,
    /// Dots for the spaces and arrows for the tabs of the changed lines.
    pub show_whitespace: bool,
    pub id: &'a str,
}

impl Default for DiffEditorOptions<'_> {
    fn default() -> Self {
        Self {
            mode: DiffMode::Inline,
            file_view: false,
            scroll_prev: false,
            scroll_next: false,
            blame_gutter: false,
            blame: None,
            blame_columns: false,
            blame_color: None,
            syntax_highlighting: true,
            selected_lines: None,
            hunk_action: None,
            hunk_clicked: None,
            word_wrap: false,
            show_whitespace: false,
            id: "##diff_lines",
        }
    }
}

/// What one frame of the editor's rows needs that does not change between rows.
struct Env<'a> {
    diff: &'a FileDiff,
    colours: Option<&'a DiffSpans>,
    code: CodeStyle,
    line_h: f32,
    word_wrap: bool,
    /// The text column's width when wrapping.
    wrap_w: f32,
    show_whitespace: bool,
    base_x: f32,
    content_right: f32,
    /// Where the text starts in a plain (file / inline) row, logical px.
    text_offset: f32,
    file_text_x: f32,
    window_hovered: bool,
}

impl Env<'_> {
    /// A row's line of code, coloured by its syntax (the add / delete backgrounds are drawn under it). Wrapped,
    /// the row shows its segment: the line shifted left by the segments before it, clipped to the column.
    fn code_text(
        &self,
        cx: &Cx<'_>,
        body: DrawList<'_>,
        x: f32,
        line_top: f32,
        segment: usize,
        line: &LineRef<'_>,
    ) {
        let shift = segment as f32 * self.wrap_w;
        if self.word_wrap {
            body.push_clip_rect(
                vec2(x, line_top),
                vec2(x + self.wrap_w, line_top + self.line_h),
                true,
            );
        }
        if self.show_whitespace && line.changed() {
            draw_whitespace(
                cx,
                &self.code,
                self.line_h,
                body,
                x - shift,
                line_top,
                &line.line.text,
            );
        }
        let spans = match (self.colours, line.index) {
            (Some(colours), Some(index)) => colours.line(line.hunk, index),
            _ => &[],
        };
        let y = css_text_y(
            cx,
            self.code.size,
            line_top + cx.px(self.code.text_dy),
            true,
        );
        cx.draw_code_text(
            body,
            self.code.font,
            self.code.size,
            vec2(x - shift, y),
            &line.line.text,
            spans,
            EDITOR_TEXT.of(cx.palette),
        );
        if self.word_wrap {
            body.pop_clip_rect();
        }
    }

    fn text_width(&self, cx: &Cx<'_>, line: &LineRef<'_>) -> f32 {
        cx.text_size(self.code.font, self.code.size, &line.line.text)
            .x
    }
}

/// Wrapped rows show their line numbers, signs and blame only on their first segment.
fn number_of(row: &Row<'_>, number: Option<u32>) -> Option<u32> {
    if row.segment == 0 { number } else { None }
}

/// Line staging: the lines picked, and per editor the row a shift-click extends from.
struct Staging<'a> {
    picked: &'a mut Vec<LineSelection>,
    anchor: &'a mut Option<usize>,
}

impl Staging<'_> {
    /// A click on the gutter of row `i` (a changed line of `rows`): toggles it, or with shift picks the range
    /// from the row clicked before.
    fn click(&mut self, rows: &[Row<'_>], i: usize, shift: bool) {
        let Some(key) = rows[i].left.and_then(|line| LineSelection::of(line.line)) else {
            return;
        };
        match *self.anchor {
            Some(anchor) if shift && anchor < rows.len() => {
                for other in &rows[anchor.min(i)..=anchor.max(i)] {
                    if let Some(other) = other.left.and_then(|line| LineSelection::of(line.line))
                        && !self.picked.contains(&other)
                    {
                        self.picked.push(other);
                    }
                }
            }
            _ if self.picked.contains(&key) => self.picked.retain(|picked| *picked != key),
            _ => self.picked.push(key),
        }
        *self.anchor = Some(i);
    }
}

/// The blame gutter's hunk under the mouse and the one clicked.
#[derive(Default)]
struct BlameHits<'a> {
    hovered: Option<(&'a BlameHunk, Vec2, Vec2)>,
    clicked: Option<&'a BlameHunk>,
}

/// What the rows of the child window left for the rest of the editor.
struct Painted<'a> {
    view_h: f32,
    scroll_y: f32,
    hits: BlameHits<'a>,
}

/// The tint of a Stage / Unstage button: `tone` at 20% (30% hovered), a 1px border, `label` in the text colour.
fn draw_tone_button(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    min: Vec2,
    max: Vec2,
    tone: Color,
    hovered: bool,
) {
    draw.add_rect_filled(
        min,
        max,
        tone.with_alpha(if hovered { 0.3 } else { 0.2 }),
        cx.px(2.0),
    );
    draw.add_rect(min, max, tone, cx.px(2.0), 1.0);
}

/// The header row of a hunk (hunk view), with its Stage / Unstage Hunk button at the visible right edge.
fn draw_hunk_header(
    cx: &Cx<'_>,
    env: &Env<'_>,
    body: DrawList<'_>,
    row: &Row<'_>,
    pos: Vec2,
    right: f32,
    button: Option<(&str, &mut Option<usize>)>,
) {
    let ui = cx.ui;
    let p = cx.palette;
    body.add_rect_filled(
        vec2(env.base_x, pos.y),
        vec2(right, pos.y + env.line_h),
        p.hunk_header,
        0.0,
    );
    mono_text(
        cx,
        &env.code,
        body,
        env.base_x + cx.px(TEXT_X),
        pos.y,
        p.text_secondary,
        &env.diff.hunks[row.hunk].header,
    );
    // Stage / Unstage Hunk at the visible right edge, whatever the horizontal scroll.
    let Some((label, clicked)) = button else {
        return;
    };
    let tone = if label.starts_with('U') {
        p.red
    } else {
        p.green
    };
    let bw = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, label).x + cx.px(12.0);
    let bmin = vec2(visible_right(cx) - bw - cx.px(6.0), pos.y);
    let bmax = vec2(bmin.x + bw, pos.y + env.line_h);
    cx.set_cursor(bmin);
    let _id = ui.push_id(row.hunk);
    if ui.invisible_button("##hunk_action", [bw, env.line_h]) {
        *clicked = Some(row.hunk);
    }
    let hovered = ui.is_item_hovered();
    draw_tone_button(cx, body, bmin, bmax, tone, hovered);
    cx.draw_text(
        body,
        cx.fonts.regular,
        FONT_SIZE_ROW,
        vec2(
            bmin.x + cx.px(6.0),
            css_text_y(cx, 12.0, pos.y + cx.px(1.0), false),
        ),
        p.text_normal,
        label,
    );
    cx.set_cursor(pos);
}

/// What the blame gutter of a file view row is made of: see [`DiffEditorOptions::blame_gutter`].
struct BlameGutter<'a, 'o> {
    blame: Option<&'a Blame>,
    columns: bool,
    color: Option<&'o (dyn Fn(&Sha) -> Color + 'a)>,
}

/// A row of the file view with the blame gutter: the avatar of the hunk's author on its first line, a bar in
/// its commit's colour, the numbers and the text. Returns the width of the text.
fn draw_blame_row<'a>(
    cx: &mut Cx<'_>,
    env: &Env<'_>,
    gutter: &BlameGutter<'a, '_>,
    body: DrawList<'_>,
    row: &Row<'_>,
    pos: Vec2,
    hits: &mut BlameHits<'a>,
) -> f32 {
    let ui = cx.ui;
    let p = cx.palette;
    let Some(line) = row.left else {
        return 0.0;
    };
    let lineno = line.line.new_lineno.unwrap_or(0);
    let hunk = gutter
        .blame
        .filter(|_| lineno > 0)
        .and_then(|blame| blame.hunk_at(lineno));
    let text_x = env.base_x + cx.px(env.text_offset);
    if let Some(hunk) = hunk {
        let color = match gutter.color {
            Some(color) => color(&hunk.sha),
            None => p.graph_color(sha_color_index(&hunk.sha) as i32),
        };
        let first = lineno == hunk.start_line && row.segment == 0;
        if first {
            cx.draw_avatar(
                body,
                vec2(env.base_x + cx.px(BLAME_AVATAR_X), pos.y + env.line_h / 2.0),
                cx.px(BLAME_AVATAR_DIAMETER),
                &hunk.author,
                None,
            );
        }
        if gutter.columns && first {
            let y = css_text_y(
                cx,
                12.0,
                pos.y + cx.px(((env.code.line_h - 16.0) / 2.0).round() + 2.0),
                true,
            );
            cx.draw_text_ellipsis(
                body,
                cx.fonts.mono,
                12.0,
                vec2(env.base_x + cx.px(BLAME_SUMMARY_X), y),
                cx.px(BLAME_SUMMARY_W),
                p.text_normal,
                &hunk.summary,
            );
            cx.draw_text(
                body,
                cx.fonts.mono,
                12.0,
                vec2(env.base_x + cx.px(BLAME_DATE_X), y),
                p.text_normal,
                &cx.format_date(hunk.time),
            );
        }
        body.add_rect_filled(
            vec2(text_x - cx.px(7.0), pos.y),
            vec2(text_x - cx.px(5.0), pos.y + env.line_h),
            color,
            0.0,
        );
        let hit_min = vec2(env.base_x, pos.y);
        let hit_max = vec2(text_x - cx.px(5.0), pos.y + env.line_h);
        if env.window_hovered && ui.is_mouse_hovering_rect(hit_min.into(), hit_max.into()) {
            hits.hovered = Some((hunk, hit_min, hit_max));
            if ui.is_mouse_clicked(MouseButton::Left) {
                hits.clicked = Some(hunk);
            }
        }
    }
    mono_number(
        cx,
        &env.code,
        body,
        env.base_x + cx.px(BLAME_NUMBERS_RIGHT),
        pos.y,
        number_of(row, line.line.new_lineno),
        false,
    );
    env.code_text(cx, body, text_x, pos.y, row.segment, &line);
    env.text_width(cx, &line)
}

/// A row of the plain file view: the new line's number and the text. Returns the width of the text.
fn draw_file_row(cx: &Cx<'_>, env: &Env<'_>, body: DrawList<'_>, row: &Row<'_>, pos: Vec2) -> f32 {
    let Some(line) = row.left else {
        return 0.0;
    };
    mono_number(
        cx,
        &env.code,
        body,
        env.base_x + cx.px(OLD_NUMBERS_RIGHT),
        pos.y,
        number_of(row, line.line.new_lineno),
        false,
    );
    env.code_text(
        cx,
        body,
        env.base_x + cx.px(env.file_text_x),
        pos.y,
        row.segment,
        &line,
    );
    env.text_width(cx, &line)
}

/// A row of the split view. Each side: 33px numbers, the sign column, the text; the missing side is hatched.
fn draw_split_row(
    cx: &Cx<'_>,
    env: &Env<'_>,
    body: DrawList<'_>,
    row: &Row<'_>,
    pos: Vec2,
    half: f32,
    right: f32,
) {
    let p = cx.palette;
    for side in 0..2 {
        let line = if side == 0 { row.left } else { row.right };
        let x0 = env.base_x + if side == 0 { 0.0 } else { half };
        let x1 = if side == 0 { env.base_x + half } else { right };
        let Some(line) = line else {
            diagonal_fill(
                cx,
                body,
                vec2(x0 + cx.px(43.0), pos.y),
                vec2(x1, pos.y + env.line_h),
            );
            continue;
        };
        if line.changed() {
            let added = line.line.origin == LineOrigin::Added;
            body.add_rect_filled(
                vec2(x0 + cx.px(43.0), pos.y),
                vec2(x1, pos.y + env.line_h),
                if added { p.added_line } else { p.deleted_line },
                0.0,
            );
            if row.segment == 0 {
                mono_text(
                    cx,
                    &env.code,
                    body,
                    x0 + cx.px(34.0),
                    pos.y,
                    p.text_normal,
                    if added { "+" } else { "-" },
                );
            }
        }
        let number = if side == 0 {
            line.line.old_lineno
        } else {
            line.line.new_lineno
        };
        mono_number(
            cx,
            &env.code,
            body,
            x0 + cx.px(OLD_NUMBERS_RIGHT),
            pos.y,
            number_of(row, number),
            false,
        );
        body.with_clip_rect(
            vec2(x0 + cx.px(43.0), pos.y),
            vec2(x1, pos.y + env.line_h),
            true,
            || env.code_text(cx, body, x0 + cx.px(43.0), pos.y, row.segment, &line),
        );
    }
}

/// A row of the inline / hunk view: the old and new numbers, the sign, the text. With line staging the number
/// gutter of a changed line picks it. Returns the width of the text.
#[allow(clippy::too_many_arguments)]
fn draw_inline_row(
    cx: &Cx<'_>,
    env: &Env<'_>,
    body: DrawList<'_>,
    rows: &[Row<'_>],
    i: usize,
    pos: Vec2,
    right: f32,
    new_cursor: u32,
    staging: Option<&mut Staging<'_>>,
) -> f32 {
    let ui = cx.ui;
    let p = cx.palette;
    let row = &rows[i];
    let Some(line) = row.left else {
        return 0.0;
    };
    if let Some(staging) = staging
        && line.changed()
    {
        // The number gutter picks the line for Stage / Unstage Lines; picked lines are tinted.
        let gmin = vec2(env.base_x, pos.y);
        let gmax = vec2(env.base_x + cx.px(TEXT_X), pos.y + env.line_h);
        let gutter_hovered =
            env.window_hovered && ui.is_mouse_hovering_rect(gmin.into(), gmax.into());
        if gutter_hovered {
            ui.set_mouse_cursor(Some(MouseCursor::Hand));
            if ui.is_mouse_clicked(MouseButton::Left) {
                staging.click(rows, i, ui.io().key_shift());
            }
        }
        let selected =
            LineSelection::of(line.line).is_some_and(|key| staging.picked.contains(&key));
        if selected || gutter_hovered {
            body.add_rect_filled(
                gmin,
                vec2(right, pos.y + env.line_h),
                p.accent.with_alpha(if selected { 0.4 } else { 0.15 }),
                0.0,
            );
        }
    }
    if line.changed() {
        let added = line.line.origin == LineOrigin::Added;
        body.add_rect_filled(
            vec2(env.base_x + cx.px(TEXT_X), pos.y),
            vec2(right, pos.y + env.line_h),
            if added { p.added_line } else { p.deleted_line },
            0.0,
        );
        // codicon diff-insert / diff-remove in the 10px sign column.
        if row.segment == 0 {
            let x = (env.base_x + cx.px(NEW_NUMBERS_RIGHT + 5.0)).floor();
            let y = (pos.y + env.line_h / 2.0).floor();
            body.add_rect_filled(
                vec2(x - cx.px(5.0), y),
                vec2(x + cx.px(5.0), y + 1.0),
                p.text_normal,
                0.0,
            );
            if added {
                body.add_rect_filled(
                    vec2(x, y - cx.px(5.0)),
                    vec2(x + 1.0, y + cx.px(5.0) + 1.0),
                    p.text_normal,
                    0.0,
                );
            }
        }
    }
    mono_number(
        cx,
        &env.code,
        body,
        env.base_x + cx.px(OLD_NUMBERS_RIGHT),
        pos.y,
        number_of(row, line.line.old_lineno),
        line.line.old_lineno == Some(1),
    );
    mono_number(
        cx,
        &env.code,
        body,
        env.base_x + cx.px(NEW_NUMBERS_RIGHT),
        pos.y,
        number_of(row, line.line.new_lineno),
        line.line.new_lineno == Some(new_cursor),
    );
    env.code_text(
        cx,
        body,
        env.base_x + cx.px(TEXT_X),
        pos.y,
        row.segment,
        &line,
    );
    env.text_width(cx, &line)
}

/// Draws the editor over `[min, max)`: `diff`'s lines, or `error` / "Loading..." (no diff) / "Binary file" /
/// "No Changes" alone. Returns the blame hunk whose gutter was clicked.
pub fn draw_diff_editor<'a>(
    cx: &mut Cx<'_>,
    diff: Option<&FileDiff>,
    error: &str,
    min: Vec2,
    max: Vec2,
    options: &mut DiffEditorOptions<'a>,
) -> Option<&'a BlameHunk> {
    let usable = if error.is_empty() {
        diff.filter(|diff| !diff.binary && !diff.hunks.is_empty())
    } else {
        None
    };
    let Some(diff) = usable else {
        let message = if !error.is_empty() {
            error
        } else {
            match diff {
                None => "Loading...",
                Some(diff) if diff.binary => "Binary file",
                Some(_) => "No Changes",
            }
        };
        let color = if error.is_empty() {
            cx.palette.text_normal
        } else {
            cx.palette.red
        };
        cx.draw_text(
            cx.window_draw_list(),
            cx.fonts.regular,
            14.0,
            vec2(min.x, css_text_y(cx, 14.0, min.y, false)),
            color,
            message,
        );
        return None;
    };
    let mut memory = cx.state.diff_editors.take(options.id);
    memory.follow_revision(diff.revision);
    let clicked = draw_lines(cx, diff, min, max, options, &mut memory);
    cx.state.diff_editors.put(options.id, memory);
    clicked
}

fn draw_lines<'a>(
    cx: &mut Cx<'_>,
    diff: &FileDiff,
    min: Vec2,
    max: Vec2,
    options: &mut DiffEditorOptions<'a>,
    memory: &mut EditorMemory,
) -> Option<&'a BlameHunk> {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let split = options.mode == DiffMode::Split && !options.file_view;
    let code = code_style(cx);
    let gutter = options.file_view && options.blame_gutter;
    // Without line numbers the plain file view's text moves into their margin.
    let file_text_x = if code.line_numbers { FILE_TEXT_X } else { 10.0 };
    let text_offset = if gutter {
        if options.blame_columns {
            BLAME_COLUMNS_TEXT_X
        } else {
            BLAME_TEXT_X
        }
    } else if options.file_view {
        file_text_x
    } else {
        TEXT_X
    };
    // Word wrap: the text column's width, in whole characters (less the vertical scrollbar).
    let mut wrap_w = 0.0;
    if options.word_wrap {
        let view_w = max.x - cx.px(DIFF_RULER_W) - min.x - style_scrollbar_size();
        let column_w = if split {
            view_w / 2.0 - cx.px(43.0)
        } else {
            view_w - cx.px(text_offset) - cx.px(10.0)
        };
        let char_w = cx.text_size(code.font, code.size, "M").x.max(1.0);
        wrap_w = (char_w * 8.0).max((column_w / char_w).floor() * char_w);
    }
    let last = final_line(diff);
    let mut rows = build_rows(diff, options.mode, options.file_view, last.as_ref());
    if options.word_wrap {
        rows = wrap_rows(cx, &code, rows, wrap_w);
    }
    // The syntax colours of the diff's lines, kept per editor until the diff changes; none for plain text.
    let colours = if options.syntax_highlighting {
        memory.syntax.get(&SyntaxDiff::new(diff))
    } else {
        None
    };
    let line_h = cx.px(code.line_h);
    let rulers_w = cx.px(DIFF_RULER_W);
    let content_right = max.x - rulers_w;
    let line_staging = options.selected_lines.is_some() && !split && !options.file_view;
    let mut staging = options
        .selected_lines
        .as_deref_mut()
        .filter(|_| line_staging)
        .map(|picked| Staging {
            picked,
            anchor: &mut memory.anchor,
        });
    let mut hunk_clicked = options.hunk_clicked.as_deref_mut();
    let hunk_action = options.hunk_action;
    let blame_gutter = BlameGutter {
        blame: options.blame,
        columns: options.blame_columns,
        color: options.blame_color.as_deref(),
    };
    let (word_wrap, show_whitespace) = (options.word_wrap, options.show_whitespace);
    let (scroll_prev, scroll_next) = (options.scroll_prev, options.scroll_next);
    let file_view = options.file_view;
    // The file view with a blame gutter: the avatar column, the numbers, the commit columns, the bar, the text.
    cx.set_cursor(min);
    let flags = if word_wrap {
        WindowFlags::empty()
    } else {
        WindowFlags::HORIZONTAL_SCROLLBAR
    };
    let painted = ui
        .child_window(options.id)
        .size([content_right - min.x, max.y - min.y])
        .flags(flags)
        .build(ui, || {
            let _spacing = ui.push_style_var(StyleVar::ItemSpacing([style_item_spacing_x(), 0.0]));
            let body = cx.window_draw_list();
            let view_h = ui.window_height();
            let scroll_y = ui.scroll_y();
            let env = Env {
                diff,
                colours,
                code,
                line_h,
                word_wrap,
                wrap_w,
                show_whitespace,
                base_x: cx.cursor().x,
                content_right,
                text_offset,
                file_text_x,
                window_hovered: ui.is_window_hovered(),
            };
            if (scroll_prev || scroll_next)
                && let Some(target) = change_target(
                    &rows,
                    (scroll_y / line_h) as usize,
                    scroll_prev,
                    scroll_next,
                )
            {
                ui.set_scroll_y(target as f32 * line_h);
            }
            let new_cursor = rows
                .iter()
                .flat_map(|row| [row.left, row.right])
                .flatten()
                .find(|line| line.line.origin == LineOrigin::Added)
                .and_then(|line| line.line.new_lineno)
                .unwrap_or(1);
            let half = (content_right - min.x) / 2.0;
            let mut max_text = 0.0_f32;
            let mut hits = BlameHits::default();
            let mut clipper = ListClipper::new(rows.len()).items_height(line_h).begin(ui);
            while clipper.step() {
                for i in clipper.display_range() {
                    let row = &rows[i];
                    let pos = cx.cursor();
                    let right =
                        content_right.max(env.base_x + max_text + cx.px(text_offset + 40.0));
                    if row.hunk_header {
                        let button = hunk_action.zip(hunk_clicked.as_deref_mut());
                        draw_hunk_header(cx, &env, body, row, pos, right, button);
                    } else if gutter {
                        let width =
                            draw_blame_row(cx, &env, &blame_gutter, body, row, pos, &mut hits);
                        max_text = max_text.max(width);
                    } else if file_view {
                        max_text = max_text.max(draw_file_row(cx, &env, body, row, pos));
                    } else if split {
                        draw_split_row(cx, &env, body, row, pos, half, right);
                    } else {
                        let width = draw_inline_row(
                            cx,
                            &env,
                            body,
                            &rows,
                            i,
                            pos,
                            right,
                            new_cursor,
                            staging.as_mut(),
                        );
                        max_text = max_text.max(width);
                    }
                    ui.dummy([
                        if word_wrap {
                            1.0
                        } else {
                            max_text + cx.px(text_offset + 40.0)
                        },
                        line_h,
                    ]);
                }
            }
            Painted {
                view_h,
                scroll_y,
                hits,
            }
        });
    let (view_h, scroll_y, hits) = match painted {
        Some(painted) => (painted.view_h, painted.scroll_y, painted.hits),
        None => (0.0, 0.0, BlameHits::default()),
    };
    let content_h = rows.len() as f32 * line_h;
    if let Some((hunk, hmin, hmax)) = hits.hovered {
        let tip = format!(
            "{}  {}\n{}, {}",
            short_sha(&hunk.sha),
            hunk.summary,
            hunk.author.name,
            cx.format_date_time(hunk.time)
        );
        cx.styled_tooltip(hmin, hmax, &tip, false);
    }

    // The modified editor's decorations ruler (14px, a 1px left border) and the diff overview: the visible
    // range in rgba(121,121,121,.4), deletions in the original (left) half, insertions in the modified half.
    let ruler_x = content_right - cx.px(DECORATIONS_RULER_W);
    draw.add_rect_filled(
        vec2(ruler_x, min.y),
        vec2(ruler_x + 1.0, max.y),
        Color::from_rgba8(127, 127, 127, 77),
        0.0,
    );
    let area_h = max.y - min.y;
    let scale = if content_h > area_h {
        area_h / content_h
    } else {
        1.0
    };
    let viewport_top = min.y + scroll_y * scale;
    let viewport_h = area_h.min(view_h * scale);
    draw.add_rect_filled(
        vec2(content_right, viewport_top),
        vec2(max.x, viewport_top + viewport_h),
        Color::from_rgba8(121, 121, 121, 102),
        0.0,
    );
    for (i, row) in rows.iter().enumerate() {
        let y0 = min.y + i as f32 * line_h * scale;
        let y1 = y0 + (line_h * scale).max(1.0);
        for line in [row.left, row.right].into_iter().flatten() {
            if !line.changed() {
                continue;
            }
            let added = line.line.origin == LineOrigin::Added;
            let x0 = content_right + if added { rulers_w / 2.0 } else { 0.0 };
            draw.add_rect_filled(
                vec2(x0, y0),
                vec2(x0 + rulers_w / 2.0, y1),
                if added { p.green } else { p.red }.with_alpha(0.5),
                0.0,
            );
        }
    }
    hits.clicked
}

/// Stages (or unstages) `lines` of `path`'s working-directory diff.
fn apply_lines(
    app: &mut Application,
    state: &mut RepoState,
    path: &str,
    lines: &[LineSelection],
    unstaging: bool,
) {
    if app.refuse_while_busy(state) {
        return;
    }
    let result = state.repo.apply_lines(path, lines, unstaging);
    app.run(
        state,
        if unstaging {
            "Unstage lines"
        } else {
            "Stage lines"
        },
        result,
        None,
    );
    state.diff_selected_lines.clear();
}

/// The header's close cross (zmdi-close, 20px) at `min`; `true` when clicked.
pub(super) fn close_cross(cx: &Cx<'_>, draw: DrawList<'_>, id: &str, min: Vec2) -> bool {
    let ui = cx.ui;
    let p = cx.palette;
    cx.set_cursor(min);
    let clicked = ui.invisible_button(id, [cx.px(20.0), cx.px(20.0)]);
    let color = if ui.is_item_hovered() {
        p.text_selected
    } else {
        p.text_normal
    };
    let a = cx.px(4.6);
    let b = cx.px(15.4);
    draw.add_line(min + vec2(a, a), min + vec2(b, b), color, cx.px(1.6));
    draw.add_line(min + vec2(b, a), min + vec2(a, b), color, cx.px(1.6));
    clicked
}

/// The header's UTF-8 button: a 10px text-disabled dropdown button, 5px padding, its right edge at `right`.
pub(super) fn encoding_button(
    cx: &Cx<'_>,
    app: &mut Application,
    draw: DrawList<'_>,
    origin_y: f32,
    right: f32,
    filled: bool,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let encoding = "UTF-8";
    let ew = cx.text_size(cx.fonts.regular, 10.0, encoding).x + cx.px(10.0);
    let emin = vec2(right - ew, origin_y + cx.px(4.5));
    cx.set_cursor(emin);
    if ui.invisible_button("##encoding", [ew, cx.px(26.0)]) {
        app.toasts
            .push_with_detail(ToastKind::Info, "Encoding", "Files are shown as UTF-8.");
    }
    let hovered = ui.is_item_hovered();
    if filled {
        draw.add_rect_filled(
            emin,
            vec2(emin.x + ew, emin.y + cx.px(26.0)),
            p.panel_bg0,
            cx.px(2.0),
        );
    }
    cx.draw_text(
        draw,
        cx.fonts.regular,
        10.0,
        vec2(
            emin.x + cx.px(5.0),
            css_text_y(cx, 10.0, origin_y + cx.px(10.5), false),
        ),
        if hovered {
            p.text_normal
        } else {
            p.text_disabled
        },
        encoding,
    );
}

/// A `.btn-xs` (26px here) in `tone`: Stage File / Unstage Lines..., 12px regular, its right edge at `right`.
/// Returns whether it was clicked and its left edge.
fn stage_button(
    cx: &Cx<'_>,
    id: &str,
    label: &str,
    tone: Color,
    origin_y: f32,
    right: f32,
) -> (bool, f32) {
    let ui = cx.ui;
    let draw = cx.window_draw_list();
    let w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, label).x + cx.px(12.0);
    let bmin = vec2(right - w, origin_y + cx.px(4.5));
    let bmax = vec2(bmin.x + w, bmin.y + cx.px(26.0));
    cx.set_cursor(bmin);
    let clicked = ui.invisible_button(id, [w, cx.px(26.0)]);
    draw_tone_button(cx, draw, bmin, bmax, tone, ui.is_item_hovered());
    cx.draw_text(
        draw,
        cx.fonts.regular,
        FONT_SIZE_ROW,
        vec2(
            bmin.x + cx.px(6.0),
            css_text_y(cx, 12.0, origin_y + cx.px(8.5), false),
        ),
        cx.palette.text_normal,
        label,
    );
    (clicked, bmin.x)
}

/// What the file view's toolbar buttons on the right do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Blame,
    History,
    Previous,
    Next,
    Hunk,
    Inline,
    Split,
    Whitespace,
    Wrap,
}

/// A button of the toolbar's right group.
struct ToolButton {
    id: &'static str,
    label: Option<&'static str>,
    icon: Option<&'static str>,
    tip: Option<&'static str>,
    /// Logical px before the button.
    gap: f32,
    tool: Tool,
}

/// Right: Blame, History | previous, next change | hunk, inline, split | whitespace, wrap.
const TOOL_BUTTONS: [ToolButton; 9] = [
    ToolButton {
        id: "##blame",
        label: Some("Blame"),
        icon: None,
        tip: None,
        gap: 0.0,
        tool: Tool::Blame,
    },
    ToolButton {
        id: "##history",
        label: Some("History"),
        icon: None,
        tip: None,
        gap: 0.0,
        tool: Tool::History,
    },
    ToolButton {
        id: "##prev",
        label: None,
        icon: Some("fas:arrow-up"),
        tip: Some("Previous change"),
        gap: 10.0,
        tool: Tool::Previous,
    },
    ToolButton {
        id: "##next",
        label: None,
        icon: Some("fas:arrow-down"),
        tip: Some("Next change"),
        gap: 0.0,
        tool: Tool::Next,
    },
    ToolButton {
        id: "##hunk",
        label: None,
        icon: Some("far:list-alt"),
        tip: Some("Hunk View"),
        gap: 10.0,
        tool: Tool::Hunk,
    },
    ToolButton {
        id: "##inline",
        label: None,
        icon: Some("far:list"),
        tip: Some("Inline View"),
        gap: 0.0,
        tool: Tool::Inline,
    },
    ToolButton {
        id: "##split",
        label: None,
        icon: Some("far:columns"),
        tip: Some("Split View"),
        gap: 0.0,
        tool: Tool::Split,
    },
    ToolButton {
        id: "##whitespace",
        label: None,
        icon: Some("far:paragraph"),
        tip: Some("Show whitespace"),
        gap: 10.0,
        tool: Tool::Whitespace,
    },
    ToolButton {
        id: "##wrap",
        label: None,
        icon: Some("fas:level-down"),
        tip: Some("Word wrap"),
        gap: 0.0,
        tool: Tool::Wrap,
    },
];

/// The file view over the centre of the repository tab, `size` device px.
pub fn draw_diff_view(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState, size: Vec2) {
    let ui = cx.ui;
    let origin = cx.cursor();
    ui.child_window("##diff_view")
        .size(size.into())
        .flags(WindowFlags::NO_SCROLLBAR)
        .build(ui, || draw_contents(cx, app, state, origin, size));
}

fn draw_contents(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    origin: Vec2,
    size: Vec2,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let Some(target) = state.diff_target.clone() else {
        return;
    };
    // A commit's diff or one compared against the working directory: nothing to stage.
    let commit_diff = matches!(
        target.source,
        DiffSource::Commit(_) | DiffSource::Compare(_)
    );
    draw.add_rect_filled(origin, origin + size, p.app_bg0, 0.0);
    // Line staging: the picked lines belong to the diff they were picked in.
    let diff_revision = state.diff.as_ref().and_then(|diff| diff.revision);
    if state.diff_selection_revision != diff_revision {
        state.diff_selection_revision = diff_revision;
        state.diff_selected_lines.clear();
    }
    let unstaging = target.source == DiffSource::Staged;

    // .file-view-header: status icon, the 14px white name; the encoding button and the close cross.
    let h = cx.px(HEADER_H);
    draw.add_rect_filled(
        origin,
        vec2(origin.x + size.x, origin.y + h),
        p.panel_bg0,
        0.0,
    );
    draw.add_rect_filled(
        vec2(origin.x, origin.y + h - 1.0),
        origin + vec2(size.x, h),
        p.section_border,
        0.0,
    );
    let (icon, tone) = status_icon(cx, target.status);
    cx.draw_fa_icon(
        draw,
        icon,
        vec2(origin.x + cx.px(16.25), origin.y + cx.px(17.5)),
        cx.px(10.0),
        tone,
    );
    let name = file_name(&target.path);
    let name_x = origin.x + cx.px(27.5);
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.regular,
        14.0,
        vec2(name_x, css_text_y(cx, 14.0, origin.y + cx.px(7.5), false)),
        size.x * 0.6,
        p.text_selected,
        name,
    );
    let name_w = cx.text_size(cx.fonts.regular, 14.0, name).x;
    if ui.is_mouse_hovering_rect([name_x, origin.y], [name_x + name_w, origin.y + h]) {
        cx.styled_tooltip(
            vec2(name_x, origin.y + cx.px(7.0)),
            vec2(name_x + name_w, origin.y + cx.px(28.0)),
            &target.path,
            false,
        );
    }
    // The close cross 10px from the right; it closes the editor too. Escape closes it from the application's
    // shortcuts, with their guards.
    let cross = vec2(origin.x + size.x - cx.px(30.0), origin.y + cx.px(7.5));
    if close_cross(cx, draw, "##close_diff", cross) {
        // Unsaved text in the file editor is asked about first.
        navigate_from_editor(app, state, |_app, state| {
            let _ = state.close_diff();
        });
        return;
    }
    let mut encoding_right = cross.x - cx.px(10.0);
    if !commit_diff {
        // .btn-xs (26px here): Stage File / Unstage File, 12px regular.
        let tone = if unstaging { p.red } else { p.green };
        let label = if unstaging {
            "Unstage File"
        } else {
            "Stage File"
        };
        let (clicked, left) =
            stage_button(cx, "##stage_file", label, tone, origin.y, encoding_right);
        if clicked {
            let path = target.path.clone();
            let status = target.status;
            // Unsaved text in the file editor is asked about first: the diff changes sides below it.
            navigate_from_editor(app, state, move |app, state| {
                if app.refuse_while_busy(state) {
                    return;
                }
                let (result, what, next) = if unstaging {
                    (
                        state.repo.unstage(&[path.as_str()]),
                        "Unstage",
                        DiffSource::Unstaged,
                    )
                } else {
                    (
                        state.repo.stage(&[path.as_str()]),
                        "Stage",
                        DiffSource::Staged,
                    )
                };
                app.run(state, what, result, None);
                let _ = state.open_diff(DiffTarget {
                    source: next,
                    path,
                    status,
                });
            });
            return;
        }
        encoding_right = left - cx.px(20.0);
        // Stage / Unstage Lines for the lines picked in the gutter.
        let count = state.diff_selected_lines.len();
        if count > 0 {
            let label = format!(
                "{} {} {}",
                if unstaging { "Unstage" } else { "Stage" },
                count,
                if count == 1 { "Line" } else { "Lines" }
            );
            let (clicked, left) = stage_button(
                cx,
                "##stage_lines",
                &label,
                tone,
                origin.y,
                left - cx.px(8.0),
            );
            if clicked {
                let lines = state.diff_selected_lines.clone();
                apply_lines(app, state, &target.path, &lines, unstaging);
                return;
            }
            encoding_right = left - cx.px(20.0);
        }
    }
    // UTF-8: a 10px text-disabled dropdown button, 5px padding.
    encoding_button(cx, app, draw, origin.y, encoding_right, true);

    // Toolbar row
    let bar_y = origin.y + cx.px(HEADER_H + 10.0);
    if state.file_edit.is_some() {
        super::file_editor::draw_file_editor(
            cx,
            app,
            state,
            vec2(origin.x + cx.px(10.0), bar_y + cx.px(1.5)),
            vec2(origin.x, origin.y + cx.px(HEADER_H + TOOLBAR_H)),
            vec2(origin.x + size.x - 1.0, origin.y + size.y),
        );
        return;
    }
    let (mut scroll_prev, mut scroll_next) = (false, false);
    {
        // .btn-xs.btn-primary with far:pencil: "Edit in Working Directory" for commits, "Edit This File" for
        // the working directory.
        let label = if commit_diff {
            "Edit in Working Directory"
        } else {
            "Edit This File"
        };
        let w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, label).x + cx.px(29.0);
        let bmin = vec2(origin.x + cx.px(10.0), bar_y + cx.px(1.5));
        let bmax = vec2(bmin.x + w, bmin.y + cx.px(20.0));
        cx.set_cursor(bmin);
        if ui.invisible_button("##edit_file", [w, cx.px(20.0)]) {
            open_file_editor(cx, app, state, &target.path);
        }
        let hovered = ui.is_item_hovered();
        draw.add_rect_filled(
            bmin,
            bmax,
            p.accent.with_alpha(if hovered { 0.3 } else { 0.2 }),
            cx.px(2.0),
        );
        draw.add_rect(bmin, bmax, p.accent, cx.px(2.0), 1.0);
        cx.draw_fa_icon(
            draw,
            "far:pencil",
            vec2(bmin.x + cx.px(12.0), bmin.y + cx.px(10.0)),
            cx.px(12.0),
            p.text_normal,
        );
        cx.draw_text(
            draw,
            cx.fonts.regular,
            FONT_SIZE_ROW,
            vec2(
                bmin.x + cx.px(23.02),
                css_text_y(cx, 12.0, bar_y + cx.px(2.5), false),
            ),
            p.text_normal,
            label,
        );
    }
    // Centred: [Unstaged / Staged] (working directory) then File View | Diff View.
    let side = if unstaging { "Staged" } else { "Unstaged" };
    let sw = if commit_diff {
        0.0
    } else {
        toggle_width(cx, Some(side), None) + cx.px(10.0)
    };
    let fw = toggle_width(cx, Some("File View"), None);
    let dw = toggle_width(cx, Some("Diff View"), None);
    let group = sw + fw + dw + if commit_diff { 0.0 } else { cx.px(10.0) };
    let mut x = (origin.x + (size.x - group) / 2.0).round();
    if !commit_diff {
        toggle_button(cx, "##side", Some(side), None, vec2(x, bar_y), true, None);
        x += sw;
    }
    if toggle_button(
        cx,
        "##file_view",
        Some("File View"),
        None,
        vec2(x, bar_y),
        state.file_view,
        None,
    ) {
        state.file_view = true;
        state.reload_diff();
    }
    if toggle_button(
        cx,
        "##diff_mode",
        Some("Diff View"),
        None,
        vec2(x + fw, bar_y),
        !state.file_view,
        None,
    ) {
        state.file_view = false;
        state.reload_diff();
    }
    let total: f32 = TOOL_BUTTONS
        .iter()
        .map(|button| cx.px(button.gap) + toggle_width(cx, button.label, button.icon))
        .sum();
    x = origin.x + size.x - cx.px(10.0) - total;
    for button in &TOOL_BUTTONS {
        x += cx.px(button.gap);
        let checked = match button.tool {
            Tool::Hunk => state.diff_mode == DiffMode::Hunk,
            Tool::Inline => state.diff_mode == DiffMode::Inline,
            Tool::Split => state.diff_mode == DiffMode::Split,
            Tool::Whitespace => app.settings.diff_whitespace,
            Tool::Wrap => app.settings.word_wrap,
            _ => false,
        };
        let clicked = toggle_button(
            cx,
            button.id,
            button.label,
            button.icon,
            vec2(x, bar_y),
            checked,
            button.tip,
        );
        x += toggle_width(cx, button.label, button.icon);
        if !clicked {
            continue;
        }
        match button.tool {
            Tool::Hunk | Tool::Inline | Tool::Split => {
                state.diff_mode = match button.tool {
                    Tool::Hunk => DiffMode::Hunk,
                    Tool::Inline => DiffMode::Inline,
                    _ => DiffMode::Split,
                };
                state.file_view = false;
                state.reload_diff();
            }
            Tool::Whitespace => {
                app.settings.diff_whitespace = !app.settings.diff_whitespace;
                state.diff_whitespace = app.settings.diff_whitespace;
                state.reload_diff();
                app.settings.save(&app.config_path("settings.txt"));
            }
            Tool::Wrap => {
                app.settings.word_wrap = !app.settings.word_wrap;
                app.settings.save(&app.config_path("settings.txt"));
            }
            Tool::Previous => scroll_prev = true,
            Tool::Next => scroll_next = true,
            Tool::Blame | Tool::History => {
                // Blame / History: the File History view of this file, at this commit (HEAD for the working
                // directory's files), the blame columns on for Blame.
                let sha: Option<Sha> = target
                    .source
                    .sha()
                    .filter(|sha| *sha != WIP_SHA)
                    .map(str::to_owned);
                state.open_file_history(
                    target.path.clone(),
                    sha.clone(),
                    sha,
                    button.tool == Tool::Blame,
                );
                return;
            }
        }
    }

    // The editor
    let editor_min = vec2(origin.x, origin.y + cx.px(HEADER_H + TOOLBAR_H));
    let editor_max = vec2(origin.x + size.x - 1.0, origin.y + size.y);
    let mut hunk_clicked = None;
    let mut options = DiffEditorOptions {
        mode: state.diff_mode,
        file_view: state.file_view,
        scroll_prev,
        scroll_next,
        syntax_highlighting: app.settings.syntax_highlighting,
        word_wrap: app.settings.word_wrap,
        show_whitespace: app.settings.diff_whitespace,
        ..DiffEditorOptions::default()
    };
    if !commit_diff {
        options.selected_lines = Some(&mut state.diff_selected_lines);
        options.hunk_action = Some(if unstaging {
            "Unstage Hunk"
        } else {
            "Stage Hunk"
        });
        options.hunk_clicked = Some(&mut hunk_clicked);
    }
    draw_diff_editor(
        cx,
        state.diff.as_ref(),
        state.diff_error.as_deref().unwrap_or_default(),
        editor_min,
        editor_max,
        &mut options,
    );
    drop(options);
    if let Some(hunk) = hunk_clicked
        && let Some(hunk) = state.diff.as_ref().and_then(|diff| diff.hunks.get(hunk))
    {
        let lines: Vec<LineSelection> = hunk.lines.iter().filter_map(LineSelection::of).collect();
        apply_lines(app, state, &target.path, &lines, unstaging);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use imgit_git::DiffHunk;

    fn line(origin: LineOrigin, old: Option<u32>, new: Option<u32>, text: &str) -> DiffLine {
        DiffLine {
            origin,
            old_lineno: old,
            new_lineno: new,
            text: text.to_owned(),
        }
    }

    /// Context 1, then `-2 +2 +3`, context 4 (old) / 5 (new).
    fn sample() -> FileDiff {
        use LineOrigin::{Added, Context, Deleted};
        FileDiff {
            hunks: vec![DiffHunk {
                header: "@@ -1,3 +1,4 @@".to_owned(),
                lines: vec![
                    line(Context, Some(1), Some(1), "a"),
                    line(Deleted, Some(2), None, "b"),
                    line(Added, None, Some(2), "B"),
                    line(Added, None, Some(3), "C"),
                    line(Context, Some(3), Some(4), "d"),
                ],
            }],
            ..FileDiff::default()
        }
    }

    fn describe(rows: &[Row<'_>]) -> Vec<String> {
        let side = |line: Option<LineRef<'_>>| line.map_or("_".to_owned(), |l| l.line.text.clone());
        rows.iter()
            .map(|row| {
                if row.hunk_header {
                    format!("@{}", row.hunk)
                } else {
                    format!("{}|{}", side(row.left), side(row.right))
                }
            })
            .collect()
    }

    #[test]
    fn a_sha_always_gets_the_same_lane_colour() {
        assert_eq!(sha_color_index(""), 0);
        assert_eq!(sha_color_index("a"), 97 % GRAPH_COLOR_COUNT);
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(sha_color_index(sha), sha_color_index(sha));
        assert!(sha_color_index(sha) < GRAPH_COLOR_COUNT);
    }

    #[test]
    fn the_line_after_the_final_newline_is_numbered_after_the_last_ones() {
        let last = final_line(&sample()).expect("lines");
        assert_eq!((last.old_lineno, last.new_lineno), (Some(4), Some(5)));
        assert_eq!(last.origin, LineOrigin::Context);
        assert!(final_line(&FileDiff::default()).is_none());
    }

    #[test]
    fn rows_of_the_inline_hunk_and_file_views() {
        let diff = sample();
        let last = final_line(&diff);
        let inline = build_rows(&diff, DiffMode::Inline, false, last.as_ref());
        assert_eq!(describe(&inline), ["a|_", "b|_", "B|_", "C|_", "d|_", "|_"]);
        // The hunk view: a header per hunk, no final empty line.
        let hunk = build_rows(&diff, DiffMode::Hunk, false, last.as_ref());
        assert_eq!(describe(&hunk), ["@0", "a|_", "b|_", "B|_", "C|_", "d|_"]);
        // The file view keeps the new side only, in any mode, and ends with the empty line.
        let file = build_rows(&diff, DiffMode::Hunk, true, last.as_ref());
        assert_eq!(describe(&file), ["a|_", "B|_", "C|_", "d|_", "|_"]);
        assert_eq!(file[0].left.and_then(|line| line.index), Some(0));
        assert_eq!(file[1].left.and_then(|line| line.index), Some(2));
        assert_eq!(
            file.last().and_then(|row| row.left).map(|l| l.index),
            Some(None)
        );
    }

    #[test]
    fn the_split_view_pairs_deletions_with_additions() {
        let diff = sample();
        let last = final_line(&diff);
        let rows = build_rows(&diff, DiffMode::Split, false, last.as_ref());
        assert_eq!(describe(&rows), ["a|a", "b|B", "_|C", "d|d", "|"]);
        assert!(!rows[0].changed() && rows[1].changed() && rows[2].changed());
    }

    #[test]
    fn word_wrap_counts_the_visual_lines() {
        assert_eq!(segment_count(0.0, 100.0), 1);
        assert_eq!(segment_count(100.0, 100.0), 1);
        assert_eq!(segment_count(100.5, 100.0), 2);
        assert_eq!(segment_count(250.0, 100.0), 3);
    }

    #[test]
    fn change_navigation_finds_the_run_above_or_below_the_top() {
        let diff = sample();
        let rows = build_rows(&diff, DiffMode::Inline, false, None);
        // Changes start at row 1 only (rows 1..=3 are one run).
        assert_eq!(change_target(&rows, 0, false, true), Some(1));
        assert_eq!(change_target(&rows, 1, false, true), None);
        assert_eq!(change_target(&rows, 3, true, false), Some(1));
        assert_eq!(change_target(&rows, 1, true, false), None);
        assert_eq!(change_target(&rows, 0, false, false), None);
    }

    #[test]
    fn shift_click_picks_the_range_from_the_last_click() {
        let diff = sample();
        let rows = build_rows(&diff, DiffMode::Inline, false, None);
        let mut picked = Vec::new();
        let mut anchor = None;
        let mut staging = Staging {
            picked: &mut picked,
            anchor: &mut anchor,
        };
        staging.click(&rows, 1, false);
        staging.click(&rows, 3, true);
        assert_eq!(staging.picked.len(), 3);
        // A plain click on a picked line unpicks it; the anchor follows.
        staging.click(&rows, 2, false);
        assert_eq!(staging.picked.len(), 2);
        assert_eq!(*staging.anchor, Some(2));
    }

    #[test]
    fn the_anchor_does_not_survive_a_reload_of_the_diff() {
        let (first, second) = (Revision::next(), Revision::next());
        let mut memory = EditorMemory::default();
        memory.follow_revision(Some(first));
        memory.anchor = Some(4);
        // Drawn again, same load: kept.
        memory.follow_revision(Some(first));
        assert_eq!(memory.anchor, Some(4));
        // Another load: forgotten, with the selection `draw_contents` clears.
        memory.follow_revision(Some(second));
        assert_eq!(memory.anchor, None);
    }
}
