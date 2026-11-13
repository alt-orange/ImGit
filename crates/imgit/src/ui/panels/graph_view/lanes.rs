//! What the graph zone of a row draws: the lane edges (straight lines and rounded turns, solid or dashed), the
//! commit node, the gutters' shadows and the fade towards the zone's edges.

use std::f32::consts::PI;

use imgit_git::graph::RowLayout;
use imgit_git::{Commit, NodeType};

use super::zones::{Metrics, Zones};
use crate::theme::{self, Color};
use crate::ui::Cx;
use crate::ui::draw::DrawList;
use crate::ui::geom::{Vec2, vec2};

/// A gutter's box-shadow (`±5px 5px 10px rgba(0, 0, 0, alpha)`) as it falls beside the gutter: the gutter moved
/// 5px towards `direction` and blurred by 10px (a Gaussian of sigma 5).
pub fn gutter_shadow(
    draw: DrawList<'_>,
    scale: f32,
    edge: f32,
    direction: f32,
    top: f32,
    bottom: f32,
    alpha: f32,
) {
    if alpha <= 0.0 {
        return;
    }
    let sigma = 5.0 * scale;
    let offset = 5.0 * scale;
    let mut d = 0.0;
    while d < offset + 3.0 * sigma {
        let a = alpha * 0.5 * erfc((d + 0.5 - offset) / (sigma * std::f32::consts::SQRT_2));
        let x = if direction > 0.0 {
            edge + d
        } else {
            edge - d - 1.0
        };
        draw.add_rect_filled(
            vec2(x, top),
            vec2(x + 1.0, bottom),
            Color::BLACK.with_alpha(a),
            0.0,
        );
        d += 1.0;
    }
}

/// The complementary error function (Abramowitz and Stegun 7.1.26, accurate to 1.5e-7), `std::erfc` of C++.
fn erfc(x: f32) -> f32 {
    let x = f64::from(x);
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-x * x).exp();
    (if x >= 0.0 { 1.0 - erf } else { 1.0 + erf }) as f32
}

/// Fades what was drawn since vertex `start` (CSS opacity on a node and its avatar).
pub fn fade_vertices(draw: DrawList<'_>, start: usize, opacity: f32) {
    if opacity >= 1.0 {
        return;
    }
    // SAFETY: `draw` is a live draw list of the current frame (see `DrawList`); its vertex buffer is ImGui's own
    // allocation of `Size` initialised vertices, and nothing else references it during this call.
    unsafe {
        let buffer = &(*draw.raw()).VtxBuffer;
        let count = usize::try_from(buffer.Size).unwrap_or(0);
        if buffer.Data.is_null() || start >= count {
            return;
        }
        for vertex in &mut std::slice::from_raw_parts_mut(buffer.Data, count)[start..] {
            let alpha = (vertex.col >> 24 & 0xFF) as f32;
            let faded = (alpha * opacity + 0.5) as u32;
            vertex.col = (vertex.col & 0x00FF_FFFF) | (faded << 24);
        }
    }
}

/// The edges are SVG lines and paths, 2px wide: axis-aligned pieces land on whole pixels and are drawn as
/// filled rectangles (an anti-aliased line fades at its ends, which leaves a seam where two rows meet).
fn stroke_segment(draw: DrawList<'_>, a: Vec2, b: Vec2, color: Color, thickness: f32) {
    let vertical = (a.x - b.x).abs() < 0.01;
    let horizontal = (a.y - b.y).abs() < 0.01;
    let half = thickness.round() / 2.0;
    if vertical {
        let x = (a.x - half).round();
        draw.add_rect_filled(
            vec2(x, a.y.min(b.y).round()),
            vec2(x + 2.0 * half, a.y.max(b.y).round()),
            color,
            0.0,
        );
    } else if horizontal {
        let y = (a.y - half).round();
        draw.add_rect_filled(
            vec2(a.x.min(b.x).round(), y),
            vec2(a.x.max(b.x).round(), y + 2.0 * half),
            color,
            0.0,
        );
    } else {
        draw.add_line(a, b, color, thickness);
    }
}

/// An SVG path through `points`: solid paths draw axis pieces as rectangles and the rest as one anti-aliased
/// polyline; dashed ones walk the 2 on / 2 off pattern (stroke-dasharray "2") from the first point.
fn stroke_path(
    draw: DrawList<'_>,
    points: &[Vec2],
    color: Color,
    thickness: f32,
    dashed: bool,
    dash: f32,
) {
    if points.len() < 2 {
        return;
    }
    if !dashed {
        let mut curve: Vec<Vec2> = Vec::new();
        let flush = |curve: &mut Vec<Vec2>| {
            if curve.len() >= 2 {
                draw.add_polyline(curve, color, false, thickness);
            }
            curve.clear();
        };
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if (a.x - b.x).abs() < 0.01 || (a.y - b.y).abs() < 0.01 {
                flush(&mut curve);
                stroke_segment(draw, a, b, color, thickness);
            } else {
                if curve.is_empty() {
                    curve.push(a);
                }
                curve.push(b);
            }
        }
        flush(&mut curve);
        return;
    }
    let mut travelled = 0.0;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = (b.x - a.x).hypot(b.y - a.y);
        if length <= 0.0 {
            continue;
        }
        let dir = vec2((b.x - a.x) / length, (b.y - a.y) / length);
        let mut s = 0.0;
        while s < length - 1e-4 {
            let phase = (travelled + s) % (2.0 * dash);
            let on = phase < dash;
            let step = (length - s).min(if on { dash - phase } else { 2.0 * dash - phase });
            if on {
                stroke_segment(
                    draw,
                    vec2(a.x + dir.x * s, a.y + dir.y * s),
                    vec2(a.x + dir.x * (s + step), a.y + dir.y * (s + step)),
                    color,
                    thickness,
                );
            }
            s += step;
        }
        travelled += length;
    }
}

/// Appends the points of a counter-clockwise (SVG sweep-flag 0) arc from angle `from` to `to` around `center`.
fn append_arc(points: &mut Vec<Vec2>, center: Vec2, radius: f32, from: f32, to: f32) {
    const SEGMENTS: i32 = 8;
    for i in 1..=SEGMENTS {
        let t = from + (to - from) * i as f32 / SEGMENTS as f32;
        points.push(vec2(
            center.x + radius * t.cos(),
            center.y + radius * t.sin(),
        ));
    }
}

/// The strokes of the lanes: one line or path at a time, in the lane's colour.
struct Stroke<'a> {
    draw: DrawList<'a>,
    color: Color,
    thickness: f32,
    dash: f32,
}

impl Stroke<'_> {
    fn line(&self, a: Vec2, b: Vec2, dashed: bool) {
        stroke_path(
            self.draw,
            &[a, b],
            self.color,
            self.thickness,
            dashed,
            self.dash,
        );
    }

    fn path(&self, points: &[Vec2], dashed: bool) {
        stroke_path(
            self.draw,
            points,
            self.color,
            self.thickness,
            dashed,
            self.dash,
        );
    }
}

/// The edges of one row: pass-throughs, starting and ending edges per lane. A turn is EDGE_ARC_RADIUS (11) from
/// its corner, drawn as an arc of radius 11 - EDGE_ARC_PADDING (3).
pub fn draw_edges(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    z: &Zones,
    m: &Metrics,
    row: &RowLayout,
    y0: f32,
    alpha: f32,
) {
    let h = m.px(m.row_height);
    let turn = m.px(m.arc_radius);
    let radius = turn - m.px(m.margin);
    let pad = m.px(m.margin);
    let thickness = m.px(theme::COMMIT_ZONE_LINE_WIDTH);
    let mid_y = y0 + h / 2.0;
    let node_column = row.column;
    let node_x = z.column_x(node_column, m);

    for (&column, edges) in &row.edges {
        let stroke = Stroke {
            draw,
            color: cx.palette.graph_color(column as i32).with_alpha(alpha),
            thickness,
            dash: m.px(2.0),
        };
        let x = z.column_x(column, m);
        // +1: the node is to the right of the lane, -1: to its left.
        let side = if node_column < column { -1.0 } else { 1.0 };
        if let Some(edge) = &edges.pass_through {
            stroke.line(vec2(x, y0), vec2(x, y0 + h), edge.node_type.is_dashed());
        }
        if let Some(edge) = &edges.starting {
            let dashed = edge.node_type.is_dashed();
            if column == node_column {
                stroke.line(vec2(x, mid_y), vec2(x, y0 + h), dashed);
            } else {
                let center = vec2(x + side * radius, mid_y + radius);
                let mut path;
                if side < 0.0 {
                    path = vec![vec2(x, y0 + h - pad), vec2(x, mid_y + radius)];
                    append_arc(&mut path, center, radius, 0.0, -0.5 * PI);
                    path.push(vec2(x - turn, mid_y));
                } else {
                    path = vec![vec2(x + turn, mid_y), vec2(x + radius, mid_y)];
                    append_arc(&mut path, center, radius, 1.5 * PI, PI);
                    path.push(vec2(x, y0 + h - pad));
                }
                stroke.path(&path, dashed);
                stroke.line(vec2(x, y0 + h - pad), vec2(x, y0 + h), dashed);
                stroke.line(vec2(x + side * turn, mid_y), vec2(node_x, mid_y), dashed);
            }
        }
        if let Some(edge) = &edges.ending {
            let dashed = edge.node_type.is_dashed();
            if column == node_column {
                stroke.line(vec2(x, y0), vec2(x, mid_y), dashed);
            } else {
                stroke.line(vec2(x, y0), vec2(x, y0 + pad), dashed);
                let center = vec2(x + side * radius, mid_y - radius);
                let mut path;
                if side < 0.0 {
                    path = vec![vec2(x - turn, mid_y), vec2(x - radius, mid_y)];
                    append_arc(&mut path, center, radius, 0.5 * PI, 0.0);
                    path.push(vec2(x, y0 + pad));
                } else {
                    path = vec![vec2(x, y0 + pad), vec2(x, mid_y - radius)];
                    append_arc(&mut path, center, radius, PI, 0.5 * PI);
                    path.push(vec2(x + turn, mid_y));
                }
                stroke.path(&path, dashed);
                stroke.line(vec2(x + side * turn, mid_y), vec2(node_x, mid_y), dashed);
            }
        }
    }
}

/// The commit's node in its lane colour: a dot for a merge, a dotted box with an inbox for a stash, a dashed
/// circle for the working directory, the avatar in a ring for a commit.
#[allow(clippy::too_many_arguments)]
pub fn draw_node(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    z: &Zones,
    m: &Metrics,
    commit: &Commit,
    row: &RowLayout,
    y0: f32,
    alpha: f32,
) {
    let p = cx.palette;
    let h = m.px(m.row_height);
    let center = vec2(z.node_x(row.column, m), y0 + h / 2.0);
    let color = p.graph_color(row.column as i32).with_alpha(alpha);
    match commit.node_type {
        NodeType::Merge => {
            draw.add_circle_filled(center, m.px(m.merge_node_diameter) / 2.0, color, 16);
        }
        NodeType::Stash => {
            // .node.stash-node: the 22px node with a 2px dotted border in the lane colour around far:inbox.
            let half = m.px(m.node_diameter) / 2.0;
            let icon_em = m.node_diameter * 12.0 / theme::COMMIT_NODE_DIAMETER;
            let min = vec2(center.x - half, center.y - half);
            let max = vec2(center.x + half, center.y + half);
            draw.add_rect_filled(min, max, p.app_bg0, 0.0);
            let dot = m.px(2.0);
            let mut t = 0.0;
            while t < max.x - min.x {
                let e = (t + dot).min(max.x - min.x);
                draw.add_rect_filled(
                    vec2(min.x + t, min.y),
                    vec2(min.x + e, min.y + dot),
                    color,
                    0.0,
                );
                draw.add_rect_filled(
                    vec2(min.x + t, max.y - dot),
                    vec2(min.x + e, max.y),
                    color,
                    0.0,
                );
                draw.add_rect_filled(
                    vec2(min.x, min.y + t),
                    vec2(min.x + dot, min.y + e),
                    color,
                    0.0,
                );
                draw.add_rect_filled(
                    vec2(max.x - dot, min.y + t),
                    vec2(max.x, min.y + e),
                    color,
                    0.0,
                );
                t += dot * 2.0;
            }
            cx.draw_fa_icon(
                draw,
                "far:inbox",
                vec2(center.x - m.px(0.25), center.y + m.px(0.5)),
                m.px(icon_em),
                color,
            );
        }
        NodeType::WorkDir => {
            // .node.work-dir-changes: the empty 22px node under an SVG circle r=10, stroke-width 2,
            // stroke-dasharray "2 3" with round caps, starting at 3 o'clock and running clockwise.
            let radius = m.px(m.node_diameter / 2.0 - 1.0);
            draw.add_circle_filled(center, m.px(m.node_diameter) / 2.0, p.app_bg0, 32);
            let dash = m.px(2.0) / radius;
            let period = m.px(5.0) / radius;
            let mut a = 0.0;
            while a < 2.0 * PI - 1e-4 {
                let a1 = (a + dash).min(2.0 * PI);
                draw.path_arc_to(center, radius, a, a1, 3);
                draw.path_stroke(color, false, m.px(2.0));
                // Round caps: a 1px disc on each end of the dash.
                for angle in [a, a1] {
                    draw.add_circle_filled(
                        vec2(
                            center.x + radius * angle.cos(),
                            center.y + radius * angle.sin(),
                        ),
                        m.px(1.0),
                        color,
                        8,
                    );
                }
                a += period;
            }
        }
        NodeType::Commit => {
            // .node { border: 2px solid lane } around the 18px avatar.
            let radius = m.px(m.node_diameter) / 2.0;
            draw.add_circle_filled(center, radius, color, 32);
            if m.node_avatar_diameter > 0.0 {
                cx.draw_avatar(
                    draw,
                    center,
                    m.px(m.node_avatar_diameter),
                    &commit.author,
                    None,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erfc_matches_known_values() {
        assert!((erfc(0.0) - 1.0).abs() < 1e-6);
        assert!((erfc(1.0) - 0.157_299_2).abs() < 1e-5);
        assert!((erfc(-1.0) - 1.842_700_8).abs() < 1e-5);
        assert!(erfc(4.0) < 1e-6);
    }

    #[test]
    fn arcs_end_on_the_circle() {
        let mut points = Vec::new();
        append_arc(&mut points, vec2(10.0, 10.0), 4.0, 0.0, -0.5 * PI);
        assert_eq!(points.len(), 8);
        let last = points[7];
        assert!((last.x - 10.0).abs() < 1e-4 && (last.y - 6.0).abs() < 1e-4);
    }
}
