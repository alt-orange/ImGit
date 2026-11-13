//! The marks of a file's status: its colour, the lettered badge and the glyph.

use imgit_git::FileStatus;
use imgit_git::repository::status_letter;

use crate::theme::Color;
use crate::ui::draw::DrawList;
use crate::ui::geom::{Vec2, vec2};
use crate::ui::{Cx, Icon};

impl Cx<'_> {
    /// The colour a file of `status` is marked with.
    #[must_use]
    pub fn status_color(&self, status: FileStatus) -> Color {
        let p = self.palette;
        match status {
            FileStatus::Added | FileStatus::Untracked => p.green,
            FileStatus::Modified => p.stats_files,
            FileStatus::Deleted | FileStatus::Conflicted => p.red,
            FileStatus::Renamed => p.ltblue,
            FileStatus::TypeChange => p.purple,
            FileStatus::Unknown => p.text_secondary,
        }
    }

    /// A square badge of `size` device px at `pos` holding the status letter (A, M, D, R, T, !).
    pub fn draw_status_badge(&self, draw: DrawList<'_>, pos: Vec2, size: f32, status: FileStatus) {
        let color = self.status_color(status);
        draw.add_rect_filled(
            pos,
            pos + Vec2::splat(size),
            color.with_alpha(0.25),
            self.px(2.0),
        );
        let letter = status_letter(status);
        let font = self.fonts.semibold;
        let font_size = size * 0.7 / self.scale;
        let text = self.text_size(font, font_size, letter);
        draw.add_text(
            font,
            self.font_px(font, font_size),
            vec2(pos.x + (size - text.x) / 2.0, pos.y + (size - text.y) / 2.0),
            color,
            letter,
        );
    }

    /// The file status glyphs: pencil (modified), plus (added), minus (deleted), arrow (renamed), warning
    /// (conflicted); `size` is the glyph's em height in device px.
    pub fn draw_status_icon(
        &mut self,
        draw: DrawList<'_>,
        center: Vec2,
        size: f32,
        status: FileStatus,
    ) {
        let color = self.status_color(status);
        let icon = match status {
            FileStatus::Added | FileStatus::Untracked => Icon::Plus,
            FileStatus::Deleted => Icon::Minus,
            FileStatus::Renamed => Icon::ArrowRight,
            FileStatus::Conflicted => Icon::Warning,
            FileStatus::Modified | FileStatus::TypeChange | FileStatus::Unknown => Icon::Pencil,
        };
        self.draw_icon(draw, icon, center, size, color);
    }
}
