//! Line-drawn icons, for the ones not drawn with Font Awesome (LFS, the headphones, expand / collapse) and as
//! the fallback for a glyph missing from the embedded table.

use crate::theme::Color;
use crate::ui::draw::DrawList;
use crate::ui::geom::{Vec2, vec2};
use crate::ui::{Cx, Icon};

impl Cx<'_> {
    /// Draws `icon` with strokes into a box of `size` device px centred on `c`. Icons that exist only as
    /// Font Awesome paths draw nothing.
    // The angles are the C++ literals (3.14159, 6.28318), digit for digit.
    #[allow(clippy::approx_constant)]
    pub fn draw_icon_vector(
        &self,
        draw: DrawList<'_>,
        icon: Icon,
        c: Vec2,
        size: f32,
        color: Color,
    ) {
        let s = size / 2.0; // half size
        let t = self.px(1.5).max(1.0);
        let round = self.px(1.0);
        // A point `dx`, `dy` half sizes from the centre.
        let at = |dx: f32, dy: f32| vec2(c.x + s * dx, c.y + s * dy);
        let line = |a: Vec2, b: Vec2| draw.add_line(a, b, color, t);
        let rect =
            |min: Vec2, max: Vec2, rounding: f32| draw.add_rect(min, max, color, rounding, t);
        match icon {
            Icon::Pull | Icon::Push => {
                let dir = if icon == Icon::Pull { 1.0 } else { -1.0 };
                let tip = c.y + dir * s * 0.55;
                line(vec2(c.x, c.y - dir * s * 0.9), vec2(c.x, tip));
                line(vec2(c.x - s * 0.5, tip - dir * s * 0.5), vec2(c.x, tip));
                line(vec2(c.x + s * 0.5, tip - dir * s * 0.5), vec2(c.x, tip));
                line(
                    vec2(c.x - s * 0.8, c.y + dir * s * 0.95),
                    vec2(c.x + s * 0.8, c.y + dir * s * 0.95),
                );
            }
            Icon::Branch => {
                let r = s * 0.28;
                line(at(-0.45, -0.55), at(-0.45, 0.6));
                draw.add_circle(at(-0.45, -0.75), r, color, 12, t);
                draw.add_circle(at(-0.45, 0.8), r, color, 12, t);
                draw.add_circle(at(0.55, -0.4), r, color, 12, t);
                draw.path_line_to(vec2(c.x + s * 0.55, c.y - s * 0.4 + r));
                draw.path_bezier_cubic_curve_to(at(0.55, 0.3), at(-0.45, 0.1), at(-0.45, 0.5), 10);
                draw.path_stroke(color, false, t);
            }
            Icon::Stash | Icon::Pop => {
                rect(at(-0.85, -0.35), at(0.85, 0.85), round);
                line(at(-0.85, -0.35), at(-0.55, -0.8));
                line(at(0.85, -0.35), at(0.55, -0.8));
                line(at(-0.55, -0.8), at(0.55, -0.8));
                if icon == Icon::Pop {
                    line(at(0.0, 0.5), at(0.0, -0.1));
                    line(at(-0.3, 0.2), at(0.0, -0.1));
                    line(at(0.3, 0.2), at(0.0, -0.1));
                }
            }
            Icon::Tag => {
                let points = [
                    at(-0.8, -0.8),
                    at(0.1, -0.8),
                    at(0.85, 0.0),
                    at(0.1, 0.8),
                    at(-0.8, 0.8),
                ];
                draw.add_polyline(&points, color, true, t);
                draw.add_circle_filled(at(-0.35, -0.35), t, color, 0);
            }
            Icon::Remote => {
                draw.add_circle(c, s * 0.8, color, 16, t);
                line(at(-0.8, 0.0), at(0.8, 0.0));
                draw.add_ellipse(c, vec2(s * 0.35, s * 0.8), color, 0.0, 16, t);
            }
            Icon::Cloud => {
                draw.path_arc_to(at(-0.4, 0.1), s * 0.45, 2.2, 4.4, 10);
                draw.path_arc_to(at(0.2, -0.1), s * 0.55, 3.5, 5.9, 10);
                draw.path_arc_to(at(0.4, 0.25), s * 0.35, 5.6, 7.6, 8);
                draw.path_line_to(at(-0.4, 0.55));
                draw.path_stroke(color, true, t);
            }
            Icon::Folder => {
                rect(at(-0.85, -0.45), at(0.85, 0.65), round);
                line(at(-0.85, -0.45), at(-0.6, -0.75));
                line(at(-0.6, -0.75), at(-0.05, -0.75));
                line(at(-0.05, -0.75), at(0.2, -0.45));
            }
            Icon::Laptop => {
                rect(at(-0.65, -0.6), at(0.65, 0.35), round);
                line(at(-0.95, 0.65), at(0.95, 0.65));
            }
            Icon::Close => {
                line(at(-0.6, -0.6), at(0.6, 0.6));
                line(at(-0.6, 0.6), at(0.6, -0.6));
            }
            Icon::Plus => {
                line(at(-0.7, 0.0), at(0.7, 0.0));
                line(at(0.0, -0.7), at(0.0, 0.7));
            }
            Icon::Minus => line(at(-0.7, 0.0), at(0.7, 0.0)),
            Icon::Back => {
                line(at(-0.8, 0.0), at(0.8, 0.0));
                line(at(-0.8, 0.0), at(-0.2, -0.6));
                line(at(-0.8, 0.0), at(-0.2, 0.6));
            }
            Icon::ArrowRight => {
                line(at(-0.8, 0.0), at(0.8, 0.0));
                line(at(0.8, 0.0), at(0.2, -0.6));
                line(at(0.8, 0.0), at(0.2, 0.6));
            }
            Icon::Refresh => {
                draw.path_arc_to(c, s * 0.75, 0.4, 5.6, 20);
                draw.path_stroke(color, false, t);
                draw.add_triangle_filled(at(0.95, -0.75), at(0.95, 0.05), at(0.25, -0.3), color);
            }
            Icon::Undo | Icon::Redo => {
                let undo = icon == Icon::Undo;
                let dir = if undo { 1.0 } else { -1.0 };
                draw.path_arc_to(
                    vec2(c.x + dir * s * 0.1, c.y + s * 0.15),
                    s * 0.7,
                    if undo { 3.4 } else { -0.3 },
                    if undo { 6.6 } else { 2.9 },
                    16,
                );
                draw.path_stroke(color, false, t);
                let tip = vec2(c.x - dir * s * 0.75, c.y - s * 0.25);
                line(tip, vec2(tip.x, tip.y - s * 0.6));
                line(tip, vec2(tip.x + dir * s * 0.6, tip.y));
            }
            Icon::Terminal => {
                line(at(-0.8, -0.5), at(-0.2, 0.0));
                line(at(-0.2, 0.0), at(-0.8, 0.5));
                line(at(0.05, 0.6), at(0.85, 0.6));
            }
            Icon::Search => {
                draw.add_circle(at(-0.15, -0.15), s * 0.55, color, 16, t);
                line(at(0.25, 0.25), at(0.8, 0.8));
            }
            Icon::Gear => {
                for spoke in 0..8 {
                    let a = spoke as f32 * 0.785;
                    draw.add_line(
                        vec2(c.x + a.cos() * s * 0.55, c.y + a.sin() * s * 0.55),
                        vec2(c.x + a.cos() * s * 0.9, c.y + a.sin() * s * 0.9),
                        color,
                        t * 1.3,
                    );
                }
                draw.add_circle(c, s * 0.5, color, 16, t);
                draw.add_circle(c, s * 0.18, color, 8, t);
            }
            Icon::Bell => {
                draw.path_arc_to(at(0.0, -0.1), s * 0.55, 3.14159, 6.28318, 12);
                draw.path_line_to(at(0.7, 0.5));
                draw.path_line_to(at(-0.7, 0.5));
                draw.path_stroke(color, true, t);
                line(at(-0.2, 0.8), at(0.2, 0.8));
            }
            Icon::Rocket => {
                draw.path_line_to(at(0.0, -0.9));
                draw.path_bezier_quadratic_curve_to(at(0.6, -0.2), at(0.3, 0.5), 8);
                draw.path_line_to(at(-0.3, 0.5));
                draw.path_bezier_quadratic_curve_to(at(-0.6, -0.2), at(0.0, -0.9), 8);
                draw.path_stroke(color, true, t);
                line(at(-0.3, 0.5), at(-0.6, 0.85));
                line(at(0.3, 0.5), at(0.6, 0.85));
                draw.add_circle_filled(at(0.0, -0.15), t, color, 0);
            }
            Icon::Trash => {
                line(at(-0.8, -0.5), at(0.8, -0.5));
                line(at(-0.25, -0.5), at(-0.25, -0.8));
                line(at(-0.25, -0.8), at(0.25, -0.8));
                line(at(0.25, -0.8), at(0.25, -0.5));
                rect(at(-0.6, -0.5), at(0.6, 0.85), 0.0);
                line(at(-0.2, -0.2), at(-0.2, 0.55));
                line(at(0.2, -0.2), at(0.2, 0.55));
            }
            Icon::Eye | Icon::EyeOff => {
                draw.path_arc_to(at(0.0, 0.6), s * 1.1, 3.6, 5.8, 12);
                draw.path_stroke(color, false, t);
                draw.path_arc_to(at(0.0, -0.6), s * 1.1, 0.5, 2.65, 12);
                draw.path_stroke(color, false, t);
                draw.add_circle_filled(c, s * 0.28, color, 10);
                if icon == Icon::EyeOff {
                    line(at(-0.8, 0.8), at(0.8, -0.8));
                }
            }
            Icon::Pencil => {
                draw.add_line(at(-0.7, 0.7), at(0.5, -0.5), color, t * 1.2);
                draw.add_line(at(0.3, -0.7), at(0.7, -0.3), color, t * 1.2);
                line(at(-0.7, 0.7), at(-0.85, 0.85));
            }
            Icon::Sort => {
                line(at(-0.35, -0.8), at(-0.35, 0.8));
                line(at(-0.35, -0.8), at(-0.75, -0.4));
                line(at(0.35, -0.8), at(0.35, 0.8));
                line(at(0.35, 0.8), at(0.75, 0.4));
            }
            Icon::Expand | Icon::Collapse => {
                let expand = icon == Icon::Expand;
                let d = if expand { 1.0 } else { -1.0 };
                line(at(-0.7, -0.7), at(0.7, 0.7));
                let a = if expand { at(0.7, 0.7) } else { at(0.1, 0.1) };
                line(a, vec2(a.x - d * s * 0.55, a.y));
                line(a, vec2(a.x, a.y - d * s * 0.55));
                let b = if expand {
                    at(-0.7, -0.7)
                } else {
                    at(-0.1, -0.1)
                };
                line(b, vec2(b.x + d * s * 0.55, b.y));
                line(b, vec2(b.x, b.y + d * s * 0.55));
            }
            Icon::List => {
                for row in -1..=1 {
                    let y = c.y + row as f32 * s * 0.55;
                    draw.add_circle_filled(vec2(c.x - s * 0.7, y), t * 0.8, color, 0);
                    line(vec2(c.x - s * 0.3, y), vec2(c.x + s * 0.8, y));
                }
            }
            Icon::Commit => {
                draw.add_circle(c, s * 0.4, color, 12, t);
                line(at(-0.9, 0.0), at(-0.4, 0.0));
                line(at(0.4, 0.0), at(0.9, 0.0));
            }
            Icon::Headphones => {
                draw.path_arc_to(c, s * 0.75, 3.14159, 6.28318, 14);
                draw.path_stroke(color, false, t);
                draw.add_rect_filled(at(-0.9, 0.0), at(-0.5, 0.6), color, round);
                draw.add_rect_filled(at(0.5, 0.0), at(0.9, 0.6), color, round);
            }
            Icon::Plug => {
                line(at(-0.3, -0.9), at(-0.3, -0.3));
                line(at(0.3, -0.9), at(0.3, -0.3));
                rect(at(-0.7, -0.3), at(0.7, 0.3), round);
                line(at(0.0, 0.3), at(0.0, 0.9));
            }
            Icon::User => {
                draw.add_circle(at(0.0, -0.35), s * 0.35, color, 12, t);
                draw.path_arc_to(at(0.0, 0.9), s * 0.75, 3.4, 6.0, 12);
                draw.path_stroke(color, false, t);
            }
            Icon::Worktree => {
                rect(at(-0.8, -0.8), at(0.8, 0.8), round);
                line(at(-0.8, 0.0), at(0.8, 0.0));
                line(at(0.0, 0.0), at(0.0, 0.8));
            }
            Icon::Submodule => {
                rect(at(-0.8, -0.8), at(0.2, 0.2), round);
                rect(at(-0.2, -0.2), at(0.8, 0.8), round);
            }
            Icon::Patch => {
                rect(at(-0.6, -0.8), at(0.6, 0.8), round);
                line(at(-0.3, -0.3), at(0.3, -0.3));
                line(at(-0.3, 0.1), at(0.3, 0.1));
            }
            Icon::Issue => {
                draw.add_circle(c, s * 0.8, color, 16, t);
                line(at(0.0, -0.4), at(0.0, 0.1));
                draw.add_circle_filled(at(0.0, 0.45), t * 0.8, color, 0);
            }
            Icon::Team => {
                draw.add_circle(at(-0.35, -0.3), s * 0.3, color, 10, t);
                draw.add_circle(at(0.4, -0.3), s * 0.3, color, 10, t);
                draw.path_arc_to(at(-0.35, 0.9), s * 0.65, 3.4, 6.0, 10);
                draw.path_stroke(color, false, t);
                draw.path_arc_to(at(0.4, 0.9), s * 0.65, 3.4, 6.0, 10);
                draw.path_stroke(color, false, t);
            }
            Icon::Lfs => {
                rect(at(-0.8, -0.6), at(0.8, 0.6), self.px(2.0));
                line(at(-0.5, -0.2), at(-0.5, 0.3));
                line(at(-0.5, 0.3), at(-0.2, 0.3));
                line(at(0.1, -0.2), at(0.1, 0.3));
                line(at(0.1, -0.2), at(0.5, -0.2));
                line(at(0.1, 0.05), at(0.4, 0.05));
            }
            Icon::Spinner => {
                // The C++ `fmod(time * 5, 6.28318)`.
                let a = (self.time() * 5.0 % 6.28318) as f32;
                draw.path_arc_to(c, s * 0.75, a, a + 4.2, 16);
                draw.path_stroke(color, false, t);
            }
            Icon::External => {
                rect(at(-0.8, -0.4), at(0.4, 0.8), round);
                line(at(0.0, 0.0), at(0.8, -0.8));
                line(at(0.2, -0.8), at(0.8, -0.8));
                line(at(0.8, -0.8), at(0.8, -0.2));
            }
            Icon::File => {
                rect(at(-0.6, -0.85), at(0.6, 0.85), round);
                line(at(-0.3, -0.2), at(0.3, -0.2));
                line(at(-0.3, 0.2), at(0.3, 0.2));
            }
            Icon::ChevronDown => {
                line(at(-0.5, -0.25), at(0.0, 0.25));
                line(at(0.5, -0.25), at(0.0, 0.25));
            }
            Icon::ChevronUp => {
                line(at(-0.5, 0.25), at(0.0, -0.25));
                line(at(0.5, 0.25), at(0.0, -0.25));
            }
            Icon::ChevronRight => {
                line(at(-0.25, -0.5), at(0.25, 0.0));
                line(at(-0.25, 0.5), at(0.25, 0.0));
            }
            Icon::ChevronLeft => {
                line(at(0.25, -0.5), at(-0.25, 0.0));
                line(at(0.25, 0.5), at(-0.25, 0.0));
            }
            Icon::Check => {
                line(at(-0.6, 0.0), at(-0.15, 0.5));
                line(at(-0.15, 0.5), at(0.65, -0.5));
            }
            Icon::Warning => {
                draw.add_triangle(at(0.0, -0.85), at(0.9, 0.75), at(-0.9, 0.75), color, t);
                line(at(0.0, -0.3), at(0.0, 0.2));
                draw.add_circle_filled(at(0.0, 0.5), t * 0.8, color, 0);
            }
            Icon::Copy => {
                rect(at(-0.75, -0.75), at(0.25, 0.25), round);
                rect(at(-0.25, -0.25), at(0.75, 0.75), round);
            }
            // Drawn from their Font Awesome paths only.
            Icon::Magic
            | Icon::Robot
            | Icon::Keyboard
            | Icon::Globe
            | Icon::SearchPlus
            | Icon::CheckedOut
            | Icon::CaretDown
            | Icon::Sparkles
            | Icon::Bars
            | Icon::FolderTree
            | Icon::Gift => {}
        }
    }
}
