//! The horizontal scrollbar drawn over a view that scrolls itself (the graph, wide diffs).

use dear_imgui_rs::StyleColor;

use crate::theme::Color;
use crate::ui::Cx;
use crate::ui::draw::DrawList;
use crate::ui::geom::{Rect, Vec2, vec2};

/// Where the thumb of a [`Cx::scrollbar_x`] is and what the mouse does with it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScrollbarX {
    /// The thumb's left edge, in screen coordinates.
    pub thumb_x: f32,
    /// The thumb's width; 0 when there is nothing to scroll.
    pub thumb_w: f32,
    pub hovered: bool,
    pub held: bool,
}

/// The thumb `(x, width)` on a track from `min_x` of `track` px, for `view` of `content` scrolled by `scroll`
/// of `range`; never narrower than `min_width`.
fn place_thumb(
    min_x: f32,
    track: f32,
    scroll: f32,
    range: f32,
    content: f32,
    view: f32,
    min_width: f32,
) -> (f32, f32) {
    let width = min_width.max(track * view / content);
    let x = min_x + (track - width) * (scroll / range).clamp(0.0, 1.0);
    (x, width)
}

impl Cx<'_> {
    /// A horizontal scrollbar over `[min, max)` for `view` of `content` (any unit, `scroll` in the same):
    /// dragging the thumb follows the mouse, a press on the track pages towards it. It takes no layout space;
    /// submit it before what it covers so it wins the hover, and draw it with [`Self::draw_scrollbar_x`] over
    /// them.
    pub fn scrollbar_x(
        &mut self,
        id: &str,
        min: Vec2,
        max: Vec2,
        scroll: &mut f32,
        content: f32,
        view: f32,
    ) -> ScrollbarX {
        let mut bar = ScrollbarX::default();
        let track = max.x - min.x;
        let range = (content - view).max(0.0);
        if range <= 0.0 || track <= 0.0 {
            return bar;
        }
        let min_width = self.px(20.0);
        let place =
            |scroll: f32| place_thumb(min.x, track, scroll, range, content, view, min_width);
        (bar.thumb_x, bar.thumb_w) = place(*scroll);
        let item = self.id(id);
        let rect = Rect::new(min, max);
        if !self.item_add(rect, item) {
            return bar;
        }
        let button = self.button_behavior(rect, item);
        bar.hovered = button.hovered;
        bar.held = button.held;
        let grab = self.id(&format!("{id}#grab"));
        let mouse = self.mouse_pos().x;
        if self.ui.is_item_activated() {
            if mouse >= bar.thumb_x && mouse < bar.thumb_x + bar.thumb_w {
                self.set_storage_float(grab, mouse - bar.thumb_x);
            } else {
                // Chromium pages by 87.5% of the view towards the press.
                self.set_storage_float(grab, -1.0);
                let page = view * 0.875;
                let step = if mouse < bar.thumb_x { -page } else { page };
                *scroll = (*scroll + step).clamp(0.0, range);
            }
        } else if bar.held && self.storage_float(grab, -1.0) >= 0.0 && track > bar.thumb_w {
            let left = mouse - self.storage_float(grab, 0.0) - min.x;
            *scroll = (left / (track - bar.thumb_w)).clamp(0.0, 1.0) * range;
        }
        (bar.thumb_x, bar.thumb_w) = place(*scroll);
        bar
    }

    /// The `.scrollbar-outer.horizontal`: `app_bg0` under a thumb rounded by 5/6 of the thickness.
    pub fn draw_scrollbar_x(&self, draw: DrawList<'_>, min: Vec2, max: Vec2, bar: &ScrollbarX) {
        if bar.thumb_w <= 0.0 {
            return;
        }
        draw.add_rect_filled(min, max, self.palette.app_bg0, 0.0);
        let color = if bar.held {
            StyleColor::ScrollbarGrabActive
        } else if bar.hovered {
            StyleColor::ScrollbarGrabHovered
        } else {
            StyleColor::ScrollbarGrab
        };
        draw.add_rect_filled(
            vec2(bar.thumb_x, min.y),
            vec2(bar.thumb_x + bar.thumb_w, max.y),
            // `ImGui::GetColorU32()`: the style's alpha applies.
            Color::from_u32(self.ui.get_color_u32(color)),
            (max.y - min.y) * 5.0 / 6.0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_placement() {
        // Half the content in view: a thumb half the track wide, moving over the other half.
        assert_eq!(
            place_thumb(10.0, 200.0, 0.0, 500.0, 1000.0, 500.0, 20.0),
            (10.0, 100.0)
        );
        assert_eq!(
            place_thumb(10.0, 200.0, 250.0, 500.0, 1000.0, 500.0, 20.0),
            (60.0, 100.0)
        );
        assert_eq!(
            place_thumb(10.0, 200.0, 500.0, 500.0, 1000.0, 500.0, 20.0),
            (110.0, 100.0)
        );
        // Overscroll is clamped; a tiny view keeps the minimum width.
        assert_eq!(
            place_thumb(0.0, 200.0, 9999.0, 9900.0, 10_000.0, 100.0, 20.0),
            (180.0, 20.0)
        );
        assert_eq!(
            place_thumb(0.0, 200.0, -5.0, 9900.0, 10_000.0, 100.0, 20.0),
            (0.0, 20.0)
        );
    }
}
