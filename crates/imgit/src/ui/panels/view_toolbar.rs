//! Layout shared by the diff and file-history toolbars.

use crate::ui::{Vec2, vec2};

pub(super) const TOOLBAR_H: f32 = 53.0;

pub(super) struct ToolbarLayout {
    /// Button positions relative to the toolbar's top-left corner, in group order.
    pub groups: Vec<Vec<Vec2>>,
    pub height: f32,
}

/// Each group supplies its preferred x position and its button widths, in device pixels. Keep that
/// alignment when it fits; otherwise flow the groups onto additional rows, keeping segments together
/// where possible. A group wider than the pane can also break between buttons.
pub(super) fn layout_toolbar(width: f32, scale: f32, groups: &[(f32, &[f32])]) -> ToolbarLayout {
    let padding = 10.0 * scale;
    let right = width - padding;
    let row_step = 29.0 * scale;
    let mut end = padding;
    let aligned = groups.iter().all(|(x, widths)| {
        if widths.is_empty() {
            return true;
        }
        let fits = *x >= end && *x + widths.iter().sum::<f32>() <= right;
        end = *x + widths.iter().sum::<f32>() + padding;
        fits
    });
    let mut x = padding;
    let mut y = 0.0;
    let mut positions = Vec::with_capacity(groups.len());
    for (preferred_x, widths) in groups {
        let mut group = Vec::with_capacity(widths.len());
        if !widths.is_empty() {
            if aligned {
                x = *preferred_x;
            } else if x > padding && x + widths.iter().sum::<f32>() > right {
                x = padding;
                y += row_step;
            }
            for &button_width in *widths {
                if !aligned && x > padding && x + button_width > right {
                    x = padding;
                    y += row_step;
                }
                group.push(vec2(x, y));
                x += button_width;
            }
            x += padding;
        }
        positions.push(group);
    }
    ToolbarLayout {
        groups: positions,
        height: TOOLBAR_H * scale + y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_toolbar_keeps_its_alignment() {
        let layout = layout_toolbar(
            1200.0,
            1.0,
            &[
                (10.0, &[100.0]),
                (480.0, &[80.0, 80.0]),
                (900.0, &[100.0, 100.0]),
            ],
        );
        assert_eq!(layout.groups[0], [vec2(10.0, 0.0)]);
        assert_eq!(layout.groups[1], [vec2(480.0, 0.0), vec2(560.0, 0.0)]);
        assert_eq!(layout.groups[2], [vec2(900.0, 0.0), vec2(1000.0, 0.0)]);
        assert_eq!(layout.height, TOOLBAR_H);
    }

    #[test]
    fn colliding_center_and_right_groups_wrap_below_the_view_controls() {
        let layout = layout_toolbar(
            650.0,
            1.0,
            &[
                (10.0, &[100.0]),
                (205.0, &[80.0, 80.0, 80.0]),
                (270.0, &[60.0, 60.0, 70.0, 100.0, 80.0]),
            ],
        );
        assert_eq!(layout.groups[1][0], vec2(120.0, 0.0));
        assert_eq!(layout.groups[2][0], vec2(10.0, 29.0));
        assert_eq!(layout.height, TOOLBAR_H + 29.0);
    }

    #[test]
    fn buttons_stay_apart_and_above_the_editor_at_different_widths_and_scales() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let left = [100.0 * scale];
            let center = [80.0 * scale; 3];
            let right = [60.0 * scale; 6];
            for width in [160.0, 250.0, 400.0, 650.0, 800.0, 1200.0] {
                let width = width * scale;
                let groups = [
                    (10.0 * scale, left.as_slice()),
                    ((width - 240.0 * scale) / 2.0, center.as_slice()),
                    (width - 370.0 * scale, right.as_slice()),
                ];
                let layout = layout_toolbar(width, scale, &groups);
                let mut previous_end = vec2(0.0, -1.0);
                for ((_, widths), positions) in groups.iter().zip(&layout.groups) {
                    assert_eq!(widths.len(), positions.len());
                    for (&button_width, &pos) in widths.iter().zip(positions) {
                        assert!(pos.x >= 10.0 * scale);
                        assert!(pos.x + button_width <= width - 10.0 * scale);
                        assert!(pos.y > previous_end.y || pos.x >= previous_end.x);
                        assert!(10.0 * scale + pos.y + 23.0 * scale <= layout.height);
                        previous_end = vec2(pos.x + button_width, pos.y);
                    }
                }
            }
        }
    }
}
