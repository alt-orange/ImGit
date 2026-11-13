//! Text drawn straight into a draw list: measuring, wrapping, ellipsis, syntax colours; and the plain labels.

use dear_imgui_rs::{StyleColor, sys};
use imgit_syntax::Span;

use crate::fonts::Font;
use crate::theme::{Color, FONT_SIZE_TINY};
use crate::ui::draw::{DrawList, text_range};
use crate::ui::geom::{Vec2, vec2};
use crate::ui::{Cx, widgets::format::to_upper};

/// `font->CalcTextSizeA(font_px, FLT_MAX, 0, text)`: the extent of one unwrapped run of text.
fn calc_text_size(font: Font, font_px: f32, text: &str) -> Vec2 {
    let (begin, end) = text_range(text);
    // SAFETY: the font belongs to the live ImGui context (see `Font::raw`); the range is valid for the call.
    unsafe {
        sys::ImFont_CalcTextSizeA(
            font.raw(),
            font_px,
            f32::MAX,
            0.0,
            begin,
            end,
            std::ptr::null_mut(),
        )
    }
    .into()
}

/// The byte length of the UTF-8 character `text` starts with (1 for an empty or broken start, so a caller
/// always moves on).
fn first_char_len(text: &str) -> usize {
    text.chars().next().map_or(1, char::len_utf8)
}

/// `text` broken into lines: at its newlines (an empty line for each blank one), then wherever `wrap_at` says
/// a line ends. `wrap_at(rest)` returns how many bytes of `rest` (a newline-free piece) fit on one line; for 0
/// one whole character is taken (never half of one), and the spaces a wrapped line ends at are dropped. The
/// lines point into `text`.
pub fn wrap_lines(text: &str, mut wrap_at: impl FnMut(&str) -> usize) -> Vec<&str> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(paragraph);
        }
        let mut line = paragraph;
        while !line.is_empty() {
            let mut wrap = wrap_at(line).min(line.len());
            if wrap == 0 {
                // Not even one character fits: take one whole UTF-8 character.
                wrap = first_char_len(line);
            }
            while !line.is_char_boundary(wrap) {
                wrap += 1;
            }
            lines.push(&line[..wrap]);
            line = line[wrap..].trim_start_matches(' ');
        }
    }
    lines
}

impl Cx<'_> {
    /// The extent of `text` drawn in `font` at the CSS size `size`, in device pixels.
    #[must_use]
    pub fn text_size(&self, font: Font, size: f32, text: &str) -> Vec2 {
        calc_text_size(font, self.font_px(font, size), text)
    }

    /// `text` broken into lines `width` device px wide at the CSS size `size`: at its newlines (an empty line
    /// for each blank one), then between words; a word wider than the line is cut between two characters. The
    /// lines point into `text`.
    #[must_use]
    pub fn wrap_text<'t>(&self, font: Font, size: f32, text: &'t str, width: f32) -> Vec<&'t str> {
        let glyph_height = self.font_px(font, size);
        wrap_lines(text, |line| {
            let (begin, end) = text_range(line);
            // SAFETY: the font belongs to the live ImGui context; ImGui returns a pointer into
            // `[begin, end]`, the range it was given.
            let wrap = unsafe {
                sys::ImFont_CalcWordWrapPosition(font.raw(), glyph_height, begin, end, width)
            };
            (wrap as usize).saturating_sub(begin as usize)
        })
    }

    /// Text drawn with `font` at the CSS size `size`, its top-left corner at `pos`.
    ///
    /// ImGui truncates the start position; Chromium places text at fractional x (the LCD renderer resolves
    /// 1/3 px), so the quads are moved by the dropped fraction.
    pub fn draw_text(
        &self,
        draw: DrawList<'_>,
        font: Font,
        size: f32,
        pos: Vec2,
        color: Color,
        text: &str,
    ) {
        let first = draw.vertex_count();
        draw.add_text(font, self.font_px(font, size), pos, color, text);
        let fraction = pos.x - pos.x.floor();
        if fraction > 0.01 {
            draw.shift_vertices_x(first, fraction);
        }
    }

    /// Text drawn with `font` at the CSS size `size`, clipped with an ellipsis to `max_width` device px.
    /// Returns the width used.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_ellipsis(
        &self,
        draw: DrawList<'_>,
        font: Font,
        size: f32,
        pos: Vec2,
        max_width: f32,
        color: Color,
        text: &str,
    ) -> f32 {
        if max_width <= 0.0 || text.is_empty() {
            return 0.0;
        }
        let font_px = self.font_px(font, size);
        let full = calc_text_size(font, font_px, text);
        if full.x <= max_width {
            draw.add_text(font, font_px, pos, color, text);
            return full.x;
        }
        const ELLIPSIS: &str = "\u{2026}"; // as CSS text-overflow: ellipsis
        let ellipsis_width = calc_text_size(font, font_px, ELLIPSIS).x;
        let budget = max_width - ellipsis_width;
        let mut fit = 0;
        for (index, character) in text.char_indices() {
            let next = index + character.len_utf8();
            if calc_text_size(font, font_px, &text[..next]).x > budget {
                break;
            }
            fit = next;
        }
        let mut used = 0.0;
        if fit > 0 {
            draw.add_text(font, font_px, pos, color, &text[..fit]);
            used = calc_text_size(font, font_px, &text[..fit]).x;
        }
        draw.add_text(font, font_px, vec2(pos.x + used, pos.y), color, ELLIPSIS);
        used + ellipsis_width
    }

    /// A line of code: each of `spans` in its token's syntax colour (the palette's `syntax_*`), plain text in
    /// `plain`; without spans, all of it in `plain`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_code_text(
        &self,
        draw: DrawList<'_>,
        font: Font,
        size: f32,
        pos: Vec2,
        text: &str,
        spans: &[Span],
        plain: Color,
    ) {
        if spans.is_empty() {
            self.draw_text(draw, font, size, pos, plain, text);
            return;
        }
        // Span by span, each after the width of the ones before; nothing past the clip rect.
        let clip_right = draw.clip_rect().max.x;
        let mut x = pos.x;
        for span in spans {
            if x > clip_right {
                break;
            }
            let Some(piece) = text.get(span.start()..span.end()) else {
                break;
            };
            let color = self.palette.syntax_color(span.token).unwrap_or(plain);
            self.draw_text(draw, font, size, vec2(x, pos.y), color, piece);
            x += self.text_size(font, size, piece).x;
        }
    }

    /// A dashed line from `a` to `b`: dashes of `dash` logical px `gap` logical px apart (3 and 3 in the
    /// stylesheets).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_dashed_line(
        &self,
        draw: DrawList<'_>,
        a: Vec2,
        b: Vec2,
        color: Color,
        thickness: f32,
        dash: f32,
        gap: f32,
    ) {
        let delta = b - a;
        let length = (delta.x * delta.x + delta.y * delta.y).sqrt();
        if length <= 0.0 {
            return;
        }
        let unit = delta / length;
        let dash = self.px(dash);
        let step = dash + self.px(gap);
        if step <= 0.0 {
            return;
        }
        let mut t = 0.0;
        while t < length {
            let end = (t + dash).min(length);
            draw.add_line(a + unit * t, a + unit * end, color, thickness);
            t += step;
        }
    }

    /// An uppercase small section header in the secondary colour.
    pub fn section_label(&self, text: &str) {
        let _font = self.push_font(self.fonts.semibold, FONT_SIZE_TINY);
        let _color = self
            .ui
            .push_style_color(StyleColor::Text, self.palette.text_secondary);
        self.ui.text(to_upper(text));
    }

    /// A line of text in the secondary colour.
    pub fn secondary_text(&self, text: &str) {
        let _color = self
            .ui
            .push_style_color(StyleColor::Text, self.palette.text_secondary);
        self.ui.text(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wraps like a fixed-pitch font `columns` characters wide, at spaces where there are any.
    fn wrap_columns(columns: usize) -> impl FnMut(&str) -> usize {
        move |line: &str| {
            let fits: usize = line.chars().take(columns).map(char::len_utf8).sum();
            if fits == line.len() || line[fits..].starts_with(' ') {
                return fits;
            }
            match line[..fits].rfind(' ') {
                Some(space) if space > 0 => space,
                _ => fits,
            }
        }
    }

    #[test]
    fn wraps_at_newlines_and_words() {
        assert_eq!(
            wrap_lines("one two three", wrap_columns(7)),
            ["one two", "three"]
        );
        assert_eq!(
            wrap_lines("one\n\ntwo three four\n", wrap_columns(9)),
            ["one", "", "two three", "four", ""]
        );
        assert_eq!(wrap_lines("", wrap_columns(9)), [""]);
        // The spaces a line is wrapped at are dropped, however many.
        assert_eq!(wrap_lines("ab    cd", wrap_columns(2)), ["ab", "cd"]);
        // A word wider than the line is cut between characters.
        assert_eq!(
            wrap_lines("abcdefgh", wrap_columns(3)),
            ["abc", "def", "gh"]
        );
    }

    #[test]
    fn never_cuts_a_character_in_half() {
        // Nothing fits: one whole character per line, multi-byte ones included.
        assert_eq!(wrap_lines("aé€", |_| 0), ["a", "é", "€"]);
        // A measurer answering in the middle of a character is moved to its end.
        assert_eq!(wrap_lines("é€", |line| line.len().min(1)), ["é", "€"]);
        // One answering past the end is clamped.
        assert_eq!(wrap_lines("abc", |_| 99), ["abc"]);
        assert_eq!(first_char_len(""), 1);
        assert_eq!(first_char_len("€x"), 3);
    }
}
