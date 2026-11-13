//! The page being drawn: CSS-pixel positions from its top-left corner, text placed the way Chromium places it,
//! boxes with crisp borders and the rich-text paragraphs.
//!
//! Positions are CSS pixels (fractions are Chromium's LayoutUnits, 1/64 px, so heights such as 19.796875 add up
//! to its pixel rounding).

use dear_imgui_rs::MouseCursor;

use crate::app::application::Application;
use crate::fonts::Font;
use crate::theme::Color;
use crate::ui::draw::DrawList;
use crate::ui::{Cx, Vec2, vec2};

/// The typefaces the pages use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Face {
    Regular,
    Semibold,
    Italic,
    Mono,
}

/// hhea ascent over the em (Open Sans, Consolas).
pub(super) fn ascent_of(face: Face) -> f32 {
    if face == Face::Mono {
        1884.0 / 2048.0
    } else {
        2189.0 / 2048.0
    }
}

/// hhea descent over the em.
pub(super) fn descent_of(face: Face) -> f32 {
    if face == Face::Mono {
        514.0 / 2048.0
    } else {
        600.0 / 2048.0
    }
}

/// A text colour: a fixed one, or the theme's ink at an opacity (resolved when drawn, so the styles follow theme
/// switches).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum TextColor {
    Ink(f32),
    Fixed(Color),
}

impl TextColor {
    pub(super) fn resolve(self, cx: &Cx<'_>) -> Color {
        match self {
            Self::Ink(alpha) => cx.palette.ink(alpha),
            Self::Fixed(color) => color,
        }
    }
}

/// Size (CSS px), colour and face of a run of text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Style {
    pub size: f32,
    pub color: TextColor,
    pub face: Face,
}

impl Style {
    /// The theme's ink at `alpha`, regular face.
    pub(super) const fn ink(size: f32, alpha: f32) -> Self {
        Self {
            size,
            color: TextColor::Ink(alpha),
            face: Face::Regular,
        }
    }

    pub(super) const fn fixed(size: f32, color: Color) -> Self {
        Self {
            size,
            color: TextColor::Fixed(color),
            face: Face::Regular,
        }
    }

    pub(super) const fn with_face(self, face: Face) -> Self {
        Self { face, ..self }
    }

    pub(super) const fn with_color(self, color: TextColor) -> Self {
        Self { color, ..self }
    }
}

/// The body text: 14px, 75% ink.
pub(super) const LABEL: Style = Style::ink(14.0, 0.75);
/// Help blocks: 12px, 60% ink.
pub(super) const HELP: Style = Style::ink(12.0, 0.6);

/// How a box is filled and bordered.
#[derive(Debug, Clone, Copy)]
pub(super) struct BoxStyle {
    pub fill: Color,
    pub border: Color,
    pub radius: f32,
    /// Bootstrap's form-control: rgba(0, 0, 0, .075) 0 1px 1px inset.
    pub inset_shadow: bool,
    pub border_width: f32,
}

impl BoxStyle {
    pub(super) const CLEAR: Color = Color::TRANSPARENT;

    pub(super) const fn fill(fill: Color) -> Self {
        Self {
            fill,
            border: Self::CLEAR,
            radius: 0.0,
            inset_shadow: false,
            border_width: 1.0,
        }
    }

    pub(super) const fn new(fill: Color, border: Color, radius: f32) -> Self {
        Self {
            fill,
            border,
            radius,
            inset_shadow: false,
            border_width: 1.0,
        }
    }

    pub(super) const fn inset(self) -> Self {
        Self {
            inset_shadow: true,
            ..self
        }
    }

    pub(super) const fn border_width(self, border_width: f32) -> Self {
        Self {
            border_width,
            ..self
        }
    }
}

/// What a page draws into.
pub(super) struct Page<'a, 'c> {
    pub cx: &'a mut Cx<'c>,
    pub app: &'a mut Application,
    pub draw: DrawList<'c>,
    /// Screen position of (0, 0), scrolled.
    pub origin: Vec2,
    /// CSS px.
    pub width: f32,
    /// Content height, for the scroll range.
    pub extent: f32,
    /// The page's scrollbar, when it scrolls (CSS px).
    pub scrollbar: f32,
    /// A window under Bootstrap's md breakpoint (992px): labels above their controls.
    pub stacked: bool,
}

impl<'a, 'c> Page<'a, 'c> {
    pub(super) fn new(
        cx: &'a mut Cx<'c>,
        app: &'a mut Application,
        draw: DrawList<'c>,
        origin: Vec2,
        width: f32,
    ) -> Self {
        Self {
            cx,
            app,
            draw,
            origin,
            width,
            extent: 0.0,
            scrollbar: 0.0,
            stacked: false,
        }
    }

    /// A page over the same context and application drawing elsewhere (a popup, a modal).
    pub(super) fn sub(&mut self, draw: DrawList<'c>, origin: Vec2, width: f32) -> Page<'_, 'c> {
        Page::new(&mut *self.cx, &mut *self.app, draw, origin, width)
    }

    /// Screen position of CSS (x, y).
    pub(super) fn at(&self, x: f32, y: f32) -> Vec2 {
        vec2(self.origin.x + self.cx.px(x), self.origin.y + self.cx.px(y))
    }

    pub(super) fn px(&self, logical: f32) -> f32 {
        self.cx.px(logical)
    }

    pub(super) fn reach(&mut self, y: f32) {
        self.extent = self.extent.max(y);
    }

    /// The theme's ink at `alpha`.
    pub(super) fn ink(&self, alpha: f32) -> Color {
        self.cx.palette.ink(alpha)
    }

    pub(super) fn font(&self, face: Face) -> Font {
        let fonts = self.cx.fonts;
        match face {
            Face::Semibold => fonts.semibold,
            Face::Italic => fonts.italic,
            Face::Mono => fonts.mono,
            Face::Regular => fonts.regular,
        }
    }

    /// CSS px.
    pub(super) fn measure(&self, style: Style, text: &str) -> f32 {
        self.cx.text_size(self.font(style.face), style.size, text).x / self.cx.scale
    }

    /// Where Chromium puts the text box in a line box `line_height` tall: half the leading, floored
    /// (NGLineHeightMetrics::CalculateLeadingSpace), over the rounded ascent + descent.
    pub(super) fn leading(style: Style, line_height: f32) -> f32 {
        let content = (style.size * ascent_of(style.face)).round()
            + (style.size * descent_of(style.face)).round();
        ((line_height - content) / 2.0).floor()
    }

    /// Plain text broken like Chromium: at spaces (dropped) and after a hyphen inside a word; '\n' forces a
    /// break.
    pub(super) fn wrap<'t>(&self, style: Style, text: &'t str, max_width: f32) -> Vec<&'t str> {
        let bytes = text.as_bytes();
        let mut lines = Vec::new();
        let mut start = 0;
        loop {
            let newline = text[start..].find('\n').map(|at| at + start);
            let end = newline.unwrap_or(text.len());
            let mut line = start;
            loop {
                let mut cut = end;
                let mut next = end;
                if self.measure(style, &text[line..end]) > max_width {
                    let mut best = None;
                    let mut best_next = end;
                    for i in line + 1..end {
                        let space = bytes[i] == b' ';
                        let hyphen = bytes[i - 1] == b'-'
                            && bytes[i] != b' '
                            && i >= line + 2
                            && bytes[i - 2] != b' ';
                        if !space && !hyphen {
                            continue;
                        }
                        if self.measure(style, &text[line..i]) > max_width {
                            break;
                        }
                        best = Some(i);
                        best_next = if space { i + 1 } else { i };
                    }
                    if let Some(best) = best {
                        cut = best;
                        next = best_next;
                    }
                }
                lines.push(&text[line..cut]);
                if cut >= end {
                    break;
                }
                line = next;
            }
            match newline {
                Some(newline) => start = newline + 1,
                None => break,
            }
        }
        lines
    }

    /// The top-left of a text box at (x, y); x keeps its fraction (the LCD renderer places glyphs at 1/3 px),
    /// the baseline is rounded to a pixel like Chromium's.
    fn text_origin(&self, style: Style, x: f32, y: f32) -> Vec2 {
        let ascent = style.size * ascent_of(style.face) * self.cx.scale;
        vec2(
            self.origin.x + x * self.cx.scale,
            self.origin.y + self.cx.px(y) + ascent.round() - ascent.ceil(),
        )
    }

    /// Text with its Chromium text box (the inline box's content area) at (x, y). Returns the width (CSS px).
    pub(super) fn text(&mut self, x: f32, y: f32, style: Style, text: &str) -> f32 {
        self.text_faded(x, y, style, text, 1.0)
    }

    pub(super) fn text_faded(
        &mut self,
        x: f32,
        y: f32,
        style: Style,
        text: &str,
        opacity: f32,
    ) -> f32 {
        let pos = self.text_origin(style, x, y);
        let color = style.color.resolve(self.cx).faded(opacity);
        self.cx.draw_text(
            self.draw,
            self.font(style.face),
            style.size,
            pos,
            color,
            text,
        );
        self.measure(style, text)
    }

    /// [`Self::text`] cut with an ellipsis at `max_width` CSS px (.truncate).
    pub(super) fn text_truncated(
        &mut self,
        x: f32,
        y: f32,
        style: Style,
        text: &str,
        max_width: f32,
    ) {
        let pos = self.text_origin(style, x, y);
        let color = style.color.resolve(self.cx);
        let width = self.px(max_width);
        self.cx.draw_text_ellipsis(
            self.draw,
            self.font(style.face),
            style.size,
            pos,
            width,
            color,
            text,
        );
    }

    /// A Font Awesome icon whose box (CSS px) is at (x, y), `size` its em.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn icon(
        &mut self,
        name: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        size: f32,
        color: Color,
    ) {
        let min = self.at(x, y);
        let center = vec2(min.x + self.px(w) / 2.0, min.y + self.px(h) / 2.0);
        let size = self.px(size);
        self.cx.draw_fa_icon(self.draw, name, center, size, color);
    }

    /// fad:check-circle: a white check on a blue circle (the current profile, the chosen profile icon).
    pub(super) fn check_badge(&mut self, x: f32, y: f32) {
        let min = self.at(x, y);
        let center = vec2(min.x + self.px(7.0), min.y + self.px(7.0));
        let size = self.px(14.0);
        let (white, blue) = (self.cx.palette.white, self.cx.palette.accent);
        self.cx
            .draw_fa_duotone(self.draw, "fad:check-circle", center, size, white, blue);
    }

    /// An SVG path of `assets/svg` in the box (x, y, w, h).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn svg(
        &mut self,
        key: &str,
        view_w: f32,
        view_h: f32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
    ) {
        let min = self.at(x, y);
        let max = vec2(min.x + self.px(w), min.y + self.px(h));
        let path = crate::ui::icons::svg_path(key).unwrap_or("");
        self.cx
            .draw_svg_path(self.draw, key, path, view_w, view_h, min, max, color);
    }

    /// The expand chevron of the selects (`chevron-down`, 20x20).
    pub(super) fn chevron(&mut self, x: f32, y: f32, size: f32, color: Color) {
        self.svg("chevron-down", 20.0, 20.0, x, y, size, size, color);
    }

    /// Whether the mouse is in `[min, max)` of the hovered window.
    pub(super) fn hovering(&self, min: Vec2, max: Vec2) -> bool {
        let mouse = self.cx.mouse_pos();
        self.cx.ui.is_window_hovered()
            && mouse.x >= min.x
            && mouse.x < max.x
            && mouse.y >= min.y
            && mouse.y < max.y
    }

    /// A clickable rectangle (CSS px); shows the hand cursor.
    pub(super) fn hit(&mut self, id: &str, x: f32, y: f32, w: f32, h: f32, enabled: bool) -> bool {
        let ui = self.cx.ui;
        self.cx.set_cursor(self.at(x, y));
        ui.set_next_item_allow_overlap();
        let clicked = ui.invisible_button(id, vec2(self.px(w).max(1.0), self.px(h).max(1.0)));
        if ui.is_item_hovered() && enabled {
            ui.set_mouse_cursor(Some(MouseCursor::Hand));
        }
        clicked && enabled
    }

    /// A filled and bordered box (CSS px).
    pub(super) fn boxed(&mut self, x: f32, y: f32, w: f32, h: f32, style: BoxStyle) {
        self.boxed_faded(x, y, w, h, style, 1.0);
    }

    pub(super) fn boxed_faded(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        style: BoxStyle,
        opacity: f32,
    ) {
        let draw = self.draw;
        let min = self.at(x, y);
        let max = vec2(min.x + self.px(w), min.y + self.px(h));
        let rounding = self.px(style.radius);
        if style.fill.a > 0.0 {
            draw.add_rect_filled(min, max, style.fill.faded(opacity), rounding);
        }
        if style.inset_shadow {
            draw.add_rect_filled(
                vec2(min.x + rounding, min.y + self.px(1.0)),
                vec2(max.x - rounding, min.y + self.px(2.0)),
                Color::new(0.0, 0.0, 0.0, 0.075).faded(opacity),
                0.0,
            );
        }
        if style.border.a <= 0.0 {
            return;
        }
        // Crisp straight edges like Chromium's (ImGui's anti-aliased 1px lines spread over two pixels); only the
        // rounded corners are anti-aliased.
        let b = self.px(style.border_width);
        let color = style.border.faded(opacity);
        let r = if rounding >= b { rounding } else { 0.0 };
        draw.add_rect_filled(
            vec2(min.x + r, min.y),
            vec2(max.x - r, min.y + b),
            color,
            0.0,
        );
        draw.add_rect_filled(
            vec2(min.x + r, max.y - b),
            vec2(max.x - r, max.y),
            color,
            0.0,
        );
        draw.add_rect_filled(
            vec2(min.x, min.y + r.max(b)),
            vec2(min.x + b, max.y - r.max(b)),
            color,
            0.0,
        );
        draw.add_rect_filled(
            vec2(max.x - b, min.y + r.max(b)),
            vec2(max.x, max.y - r.max(b)),
            color,
            0.0,
        );
        if r <= 0.0 {
            return;
        }
        let arc = r - b / 2.0;
        let pi = std::f32::consts::PI;
        let centers = [
            vec2(min.x + r, min.y + r),
            vec2(max.x - r, min.y + r),
            vec2(max.x - r, max.y - r),
            vec2(min.x + r, max.y - r),
        ];
        let starts = [pi, pi * 1.5, 0.0, pi * 0.5];
        for (center, start) in centers.into_iter().zip(starts) {
            draw.path_arc_to(center, arc, start, start + pi * 0.5, 6);
            draw.path_stroke(color, false, b);
        }
    }

    /// Bootstrap's .form-control: 20% black, an 8% white border (the accent when focused), 4px corners.
    pub(super) fn field_box(&mut self, x: f32, y: f32, w: f32, h: f32, focused: bool) {
        let border = if focused {
            self.cx.palette.accent
        } else {
            self.ink(0.08)
        };
        let fill = self.cx.palette.input_bg;
        self.boxed(x, y, w, h, BoxStyle::new(fill, border, 4.0).inset());
    }
}

/// What a markup atom is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AtomKind {
    Word,
    Space,
    Break,
    /// fas:exclamation-triangle with mr1.
    Warning,
    Code,
}

/// A piece of a paragraph that is placed as a whole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Atom<'t> {
    pub kind: AtomKind,
    pub text: &'t str,
    pub face: Face,
    pub url: &'t str,
    pub lead: f32,
    pub trail: f32,
}

impl<'t> Atom<'t> {
    fn new(kind: AtomKind, text: &'t str, face: Face, url: &'t str) -> Self {
        Self {
            kind,
            text,
            face,
            url,
            lead: 0.0,
            trail: 0.0,
        }
    }
}

/// `markup` as atoms: `**bold**`, `[links](url)`, `` `code` `` and `{warning}` icons; the rest is words and spaces.
pub(super) fn parse_markup(markup: &str, base: Face) -> Vec<Atom<'_>> {
    fn words<'t>(atoms: &mut Vec<Atom<'t>>, run: &'t str, face: Face, url: &'t str) {
        let bytes = run.as_bytes();
        let mut a = 0;
        while a < run.len() {
            match bytes[a] {
                b'\n' => {
                    atoms.push(Atom::new(AtomKind::Break, "", Face::Regular, ""));
                    a += 1;
                }
                b' ' => {
                    atoms.push(Atom::new(AtomKind::Space, &run[a..=a], face, url));
                    a += 1;
                }
                _ => {
                    let b = run[a..]
                        .find([' ', '\n'])
                        .map_or(run.len(), |found| found + a);
                    atoms.push(Atom::new(AtomKind::Word, &run[a..b], face, url));
                    a = b;
                }
            }
        }
    }
    let mut atoms = Vec::new();
    let mut i = 0;
    while i < markup.len() {
        let rest = &markup[i..];
        // An unmatched marker: the rest is plain text.
        if let Some(inner) = rest.strip_prefix("**") {
            let Some(close) = inner.find("**") else {
                words(&mut atoms, rest, base, "");
                break;
            };
            words(&mut atoms, &inner[..close], Face::Semibold, "");
            i += 2 + close + 2;
            continue;
        }
        if let Some(inner) = rest.strip_prefix('[') {
            let Some(middle) = inner.find("](") else {
                words(&mut atoms, rest, base, "");
                break;
            };
            let Some(close) = inner[middle..].find(')').map(|at| at + middle) else {
                words(&mut atoms, rest, base, "");
                break;
            };
            let first = atoms.len();
            words(
                &mut atoms,
                &inner[..middle],
                base,
                &inner[middle + 2..close],
            );
            if let Some(atom) = atoms.get_mut(first) {
                atom.lead = 3.0;
            }
            if let Some(atom) = atoms.last_mut() {
                atom.trail = 3.0;
            }
            i += 1 + close + 1;
            continue;
        }
        if let Some(inner) = rest.strip_prefix('`') {
            let Some(close) = inner.find('`') else {
                words(&mut atoms, rest, base, "");
                break;
            };
            atoms.push(Atom::new(AtomKind::Code, &inner[..close], Face::Mono, ""));
            i += 1 + close + 1;
            continue;
        }
        if let Some(inner) = rest.strip_prefix('{') {
            let Some(close) = inner.find('}') else {
                words(&mut atoms, rest, base, "");
                break;
            };
            atoms.push(Atom::new(AtomKind::Warning, "", Face::Regular, ""));
            i += 1 + close + 1;
            continue;
        }
        // Plain words up to the next marker (the character at `i` itself is always plain here).
        let first_len = rest.chars().next().map_or(1, char::len_utf8);
        let next = rest[first_len..]
            .find(['*', '[', '`', '{'])
            .map_or(rest.len(), |found| found + first_len);
        words(&mut atoms, &rest[..next], base, "");
        i += next;
    }
    atoms
}

impl Page<'_, '_> {
    /// Draws the markup in a box `width` wide from (x, y); returns its height. A link keeps its 2px padding +
    /// 1px border at both ends.
    pub(super) fn rich(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        line_height: f32,
        style: Style,
        markup: &str,
    ) -> f32 {
        let atoms = parse_markup(markup, style.face);
        let palette = *self.cx.palette;
        let style_of = |atom: &Atom<'_>| {
            let mut s = style.with_face(atom.face);
            if atom.face == Face::Mono {
                s = s.with_color(TextColor::Fixed(palette.text_accent));
            }
            if !atom.url.is_empty() {
                s = s.with_color(TextColor::Fixed(palette.link));
            }
            s
        };
        let widths: Vec<f32> = atoms
            .iter()
            .map(|atom| match atom.kind {
                AtomKind::Word | AtomKind::Space => {
                    self.measure(style_of(atom), atom.text) + atom.lead + atom.trail
                }
                AtomKind::Warning => style.size * 1.125 + 5.0,
                AtomKind::Code => self.measure(style_of(atom), atom.text) + 8.0,
                AtomKind::Break => 0.0,
            })
            .collect();

        // Lay out: units of atoms with no space between them can't be broken.
        struct Placed {
            atom: usize,
            x: f32,
            line: usize,
        }
        let mut placed = Vec::new();
        let mut line = 0;
        let mut cx = 0.0_f32;
        let mut pending = 0.0_f32; // spaces not yet placed
        let mut pending_spaces: Vec<usize> = Vec::new();
        let mut i = 0;
        while i < atoms.len() {
            let atom = &atoms[i];
            if atom.kind == AtomKind::Break {
                line += 1;
                cx = 0.0;
                pending = 0.0;
                pending_spaces.clear();
                i += 1;
                continue;
            }
            if atom.kind == AtomKind::Space {
                // Spaces at the start of a wrapped line collapse.
                if cx > 0.0 || line == 0 {
                    pending += widths[i];
                    pending_spaces.push(i);
                }
                i += 1;
                continue;
            }
            let mut end = i;
            let mut unit = 0.0;
            while end < atoms.len() && !matches!(atoms[end].kind, AtomKind::Space | AtomKind::Break)
            {
                unit += widths[end];
                end += 1;
            }
            if cx > 0.0 && cx + pending + unit > width {
                line += 1;
                cx = 0.0;
            } else {
                for &space in &pending_spaces {
                    placed.push(Placed {
                        atom: space,
                        x: cx,
                        line,
                    });
                    cx += widths[space];
                }
            }
            pending = 0.0;
            pending_spaces.clear();
            for (k, width) in widths.iter().enumerate().take(end).skip(i) {
                placed.push(Placed {
                    atom: k,
                    x: cx,
                    line,
                });
                cx += width;
            }
            i = end;
        }

        // Link IDs: one scope per paragraph position.
        let ui = self.cx.ui;
        let _scope_x = ui.push_id((x * 16.0).round() as i32);
        let _scope_y = ui.push_id((y * 16.0).round() as i32);
        let top_of = |line: usize| y + line_height * line as f32;
        for p in &placed {
            let atom = &atoms[p.atom];
            let s = style_of(atom);
            let top = top_of(p.line);
            let text_top = top + Self::leading(style, line_height);
            let ax = x + p.x;
            // Font Awesome's svg-inline sits 0.125em under the baseline.
            let baseline = text_top + (style.size * ascent_of(style.face)).round();
            match atom.kind {
                AtomKind::Word | AtomKind::Space => {
                    let w = self.text(ax + atom.lead, text_top, s, atom.text);
                    if !atom.url.is_empty() {
                        let _id = ui.push_id(p.atom as i32);
                        if self.hit(
                            "##link",
                            ax,
                            top,
                            w + atom.lead + atom.trail,
                            line_height,
                            true,
                        ) {
                            let _ = imgit_platform::shell::open_url(atom.url);
                        }
                    }
                }
                AtomKind::Warning => {
                    let color = style.color.resolve(self.cx);
                    self.icon(
                        "fas:exclamation-triangle",
                        ax,
                        baseline + style.size * 0.125 - style.size,
                        style.size * 1.125,
                        style.size,
                        style.size,
                        color,
                    );
                }
                AtomKind::Code => {
                    let w = self.measure(s, atom.text);
                    let fill = palette.input_bg;
                    self.boxed(
                        ax,
                        top,
                        w + 8.0,
                        21.0,
                        BoxStyle::fill(fill).with_radius(2.0),
                    );
                    self.text(ax + 4.0, top + 2.0, s, atom.text);
                }
                AtomKind::Break => {}
            }
        }
        top_of(line) - y + line_height
    }
}

impl BoxStyle {
    pub(super) const fn with_radius(self, radius: f32) -> Self {
        Self { radius, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds<'a>(atoms: &[Atom<'a>]) -> Vec<(AtomKind, &'a str)> {
        atoms.iter().map(|atom| (atom.kind, atom.text)).collect()
    }

    #[test]
    fn markup_words_and_spaces() {
        let atoms = parse_markup("one two\nthree", Face::Regular);
        assert_eq!(
            kinds(&atoms),
            [
                (AtomKind::Word, "one"),
                (AtomKind::Space, " "),
                (AtomKind::Word, "two"),
                (AtomKind::Break, ""),
                (AtomKind::Word, "three"),
            ]
        );
    }

    #[test]
    fn markup_runs() {
        let atoms = parse_markup("a **b c** [d](http://x) `e` {w}", Face::Regular);
        let found: Vec<_> = atoms
            .iter()
            .filter(|atom| atom.kind != AtomKind::Space)
            .map(|atom| (atom.kind, atom.text, atom.face, atom.url))
            .collect();
        assert_eq!(
            found,
            [
                (AtomKind::Word, "a", Face::Regular, ""),
                (AtomKind::Word, "b", Face::Semibold, ""),
                (AtomKind::Word, "c", Face::Semibold, ""),
                (AtomKind::Word, "d", Face::Regular, "http://x"),
                (AtomKind::Code, "e", Face::Mono, ""),
                (AtomKind::Warning, "", Face::Regular, ""),
            ]
        );
        let link = atoms.iter().find(|atom| !atom.url.is_empty()).unwrap();
        assert_eq!((link.lead, link.trail), (3.0, 3.0));
    }

    #[test]
    fn unmatched_markers_are_plain_text() {
        let atoms = parse_markup("a **b", Face::Regular);
        assert_eq!(
            kinds(&atoms),
            [
                (AtomKind::Word, "a"),
                (AtomKind::Space, " "),
                (AtomKind::Word, "**b"),
            ]
        );
        let atoms = parse_markup("x [y z", Face::Regular);
        assert_eq!(atoms.last().unwrap().text, "z");
        // A lone star is ordinary text.
        let atoms = parse_markup("5 * 3", Face::Regular);
        assert_eq!(atoms.len(), 5);
        assert_eq!(atoms[2].text, "*");
        let atoms = parse_markup("é**b**", Face::Regular);
        assert_eq!(atoms[0].text, "é");
        assert_eq!(atoms[1].face, Face::Semibold);
    }
}
