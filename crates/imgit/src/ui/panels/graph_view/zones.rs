//! The graph's zones (columns): which are shown and in what order, their widths and how dragging one's edge
//! moves the others, and the lanes of the graph zone itself (its sideways scroll and node offsets).
//!
//! Ports `src/ui/graph_view.cpp`.

use crate::app::settings::Settings;
use crate::theme;

/// The metrics of the graph being drawn, in CSS px: the rich layout (avatars in 22px nodes on 28px rows) or the
/// compact one (`Settings::compact_graph`: plain dots on 20px rows, narrower lanes and smaller pills).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub row_height: f32,
    pub row_inner_height: f32,
    /// Above and below the inner height.
    pub margin: f32,
    pub column_width: f32,
    pub gutter_width: f32,
    pub node_diameter: f32,
    pub merge_node_diameter: f32,
    /// 0: the node is a plain dot.
    pub node_avatar_diameter: f32,
    pub arc_radius: f32,
    /// The AUTHOR zone's.
    pub avatar_diameter: f32,
    pub ref_height: f32,
    /// Logical to device pixels.
    pub scale: f32,
}

impl Metrics {
    #[must_use]
    pub const fn rich(scale: f32) -> Self {
        Self {
            row_height: theme::GRAPH_ROW_HEIGHT,
            row_inner_height: theme::GRAPH_ROW_INNER_HEIGHT,
            margin: theme::COMMIT_ZONE_MARGIN,
            column_width: theme::COMMIT_COLUMN_WIDTH,
            gutter_width: theme::COMMIT_ZONE_GUTTER_WIDTH,
            node_diameter: theme::COMMIT_NODE_DIAMETER,
            merge_node_diameter: theme::COMMIT_MERGE_NODE_DIAMETER,
            node_avatar_diameter: theme::COMMIT_ZONE_AVATAR_DIAMETER,
            arc_radius: theme::COMMIT_ZONE_EDGE_ARC_RADIUS,
            avatar_diameter: theme::AVATAR_DIAMETER,
            ref_height: theme::REF_ZONE_TEXT_HEIGHT,
            scale,
        }
    }

    #[must_use]
    pub const fn compact(scale: f32) -> Self {
        Self {
            row_height: 20.0,
            row_inner_height: 16.0,
            margin: 2.0,
            column_width: 14.0,
            gutter_width: 16.0,
            node_diameter: 10.0,
            merge_node_diameter: 6.0,
            node_avatar_diameter: 0.0,
            arc_radius: 7.0,
            avatar_diameter: 16.0,
            ref_height: 16.0,
            scale,
        }
    }

    #[must_use]
    pub const fn new(compact: bool, scale: f32) -> Self {
        if compact {
            Self::compact(scale)
        } else {
            Self::rich(scale)
        }
    }

    /// Logical pixels to whole device pixels (`ui::px()`).
    #[must_use]
    pub fn px(&self, logical: f32) -> f32 {
        (logical * self.scale).round()
    }
}

/// `--scrollable-scrollbar-thickness`.
pub const SCROLLBAR_THICKNESS: f32 = 8.0;

/// The graph zones, in CSS px. Every shown zone has a current and a preferred width; together they fill the
/// graph, the last one taking what is left. Dragging a zone's right edge moves the zones after it: they shrink
/// from the last one, or grow back towards their preferred widths from the first one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneType {
    Ref,
    Graph,
    Message,
    Author,
    Changes,
    DateTime,
    Sha,
}

impl ZoneType {
    pub const ALL: [Self; 7] = [
        Self::Ref,
        Self::Graph,
        Self::Message,
        Self::Author,
        Self::Changes,
        Self::DateTime,
        Self::Sha,
    ];

    /// The key the zone is saved under in the settings.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Ref => "ref",
            Self::Graph => "graph",
            Self::Message => "message",
            Self::Author => "author",
            Self::Changes => "changes",
            Self::DateTime => "datetime",
            Self::Sha => "sha",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ref => "BRANCH / TAG",
            Self::Graph => "GRAPH",
            Self::Message => "COMMIT MESSAGE",
            Self::Author => "AUTHOR",
            Self::Changes => "CHANGES",
            Self::DateTime => "COMMIT DATE / TIME",
            Self::Sha => "SHA",
        }
    }

    /// GraphZoneConstants' minimumWidth (the graph's is COMMIT_ZONE_VIEWPORT_WIDTH_MIN).
    #[must_use]
    pub const fn minimum(self) -> f32 {
        match self {
            Self::Ref | Self::Author => 32.0,
            Self::Graph => 56.0,
            _ => 50.0,
        }
    }

    /// The width of a zone the user never dragged.
    #[must_use]
    pub const fn default_width(self) -> f32 {
        match self {
            Self::Ref => theme::GRAPH_REF_ZONE_DEFAULT_WIDTH,
            Self::Graph => 150.0, // COMMIT_ZONE_DEFAULT_WIDTH
            Self::Message => 400.0,
            Self::Author => 170.0,
            Self::Changes => 110.0,
            Self::DateTime => 150.0,
            Self::Sha => 80.0,
        }
    }

    /// Whether the settings show the zone.
    #[must_use]
    pub fn shown(self, settings: &Settings) -> bool {
        match self {
            Self::Author => settings.show_author,
            Self::Changes => settings.show_changes,
            Self::DateTime => settings.show_date,
            Self::Sha => settings.show_sha,
            _ => !settings.hidden_graph_columns.contains(self.key()),
        }
    }
}

/// Every zone in the user's order: the saved keys first, then the rest in the default order.
#[must_use]
pub fn zone_order(settings: &Settings) -> Vec<ZoneType> {
    let mut order: Vec<ZoneType> = Vec::with_capacity(ZoneType::ALL.len());
    for key in &settings.graph_column_order {
        if let Some(&zone) = ZoneType::ALL
            .iter()
            .find(|zone| zone.key() == key && !order.contains(zone))
        {
            order.push(zone);
        }
    }
    let rest: Vec<ZoneType> = ZoneType::ALL
        .into_iter()
        .filter(|zone| !order.contains(zone))
        .collect();
    order.extend(rest);
    order
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphZone {
    pub zone: ZoneType,
    pub minimum: f32,
    /// 0: none.
    pub maximum: f32,
    pub current: f32,
    pub preferred: f32,
}

/// The shown zones, in order, and the width they fill.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ZoneLayout {
    pub zones: Vec<GraphZone>,
    pub width: f32,
    /// The graph zone's content width, its maximum.
    pub content: f32,
}

impl ZoneLayout {
    fn total(&self, skip: Option<usize>) -> f32 {
        self.zones
            .iter()
            .enumerate()
            .filter(|&(i, _)| Some(i) != skip)
            .map(|(_, zone)| zone.current)
            .sum()
    }

    /// The width a zone may take: its minimum and maximum (the last zone has no maximum).
    #[must_use]
    pub fn clamp_zone(&self, index: usize, width: f32) -> f32 {
        let zone = &self.zones[index];
        let last = index + 1 == self.zones.len();
        let capped = if zone.maximum > 0.0 && !last {
            width.min(zone.maximum)
        } else {
            width
        };
        zone.minimum.max(capped)
    }

    fn set_width(&mut self, index: usize, width: f32, preferred: bool) {
        let current = self.clamp_zone(index, width);
        let zone = &mut self.zones[index];
        zone.current = current;
        if preferred {
            zone.preferred = current;
        }
    }

    /// shrinkZoneWidthsToFitWidth: the last zones above their minimum give way first, down to `from`.
    fn shrink(&mut self, width: f32, from: usize) {
        for _ in 0..=self.zones.len() * 2 {
            if self.total(None) <= width {
                break;
            }
            let Some(index) = (from..self.zones.len())
                .rev()
                .find(|&i| self.zones[i].current > self.zones[i].minimum)
            else {
                break;
            };
            self.set_width(index, width - self.total(Some(index)), false);
        }
    }

    /// expandZoneWidthsToFitWidth: the first zones under their preferred width (capped by their maximum) grow
    /// back to it from `from` on; the last zone takes the rest.
    fn expand(&mut self, width: f32, from: usize) {
        let Some(last) = self.zones.len().checked_sub(1) else {
            return;
        };
        let cap = |zone: &GraphZone| {
            if zone.maximum > 0.0 {
                zone.maximum.min(if zone.preferred > 0.0 {
                    zone.preferred
                } else {
                    f32::MAX
                })
            } else {
                zone.preferred
            }
        };
        for _ in 0..=self.zones.len() * 2 {
            if self.total(None) >= width {
                break;
            }
            let index = (from..=last)
                .find(|&i| {
                    let zone = &self.zones[i];
                    cap(zone) > 0.0 && zone.current < cap(zone)
                })
                .unwrap_or(last);
            let mut grown = width - self.total(Some(index));
            if index != last {
                grown = grown.min(cap(&self.zones[index]));
            }
            self.set_width(index, grown, false);
        }
    }

    /// ensureZoneWidthsMatchGraphWidth.
    fn fit(&mut self) {
        let width = self.width;
        if self.total(None) > width {
            self.shrink(width, 0);
        }
        if self.total(None) < width {
            self.expand(width, 0);
        }
    }

    /// adjustResizedGraphZone: a dragged zone takes its new width and the zones after it make room or fill in.
    pub fn resize_zone(&mut self, index: usize, width: f32, release: bool) {
        let before = self.zones[index].current;
        self.set_width(index, width, release);
        let after = self.zones[index].current;
        if before > after {
            self.expand(self.width, index + 1);
        }
        if before < after {
            self.shrink(self.width, index.saturating_sub(1));
        }
    }

    /// A dragged zone's limits: its minimum, and the room left by the zones before it and the minimums after.
    #[must_use]
    pub fn max_zone_width(&self, index: usize) -> f32 {
        let room = self
            .zones
            .iter()
            .enumerate()
            .fold(self.width, |room, (i, zone)| match i.cmp(&index) {
                std::cmp::Ordering::Less => room - zone.current,
                std::cmp::Ordering::Greater => room - zone.minimum,
                std::cmp::Ordering::Equal => room,
            });
        let maximum = self.zones[index].maximum;
        if maximum > 0.0 {
            maximum.min(room)
        } else {
            room
        }
    }

    /// The index of the shown zone `zone`.
    #[must_use]
    pub fn index_of(&self, zone: ZoneType) -> Option<usize> {
        self.zones.iter().position(|z| z.zone == zone)
    }

    /// Lays the zones out for a graph `width` CSS px wide, keeping the user's widths; `max_columns` is the
    /// highest lane in use. `true` when the set of shown zones changed (they were made again from the saved
    /// widths).
    pub fn update(
        &mut self,
        settings: &Settings,
        width: f32,
        max_columns: usize,
        metrics: &Metrics,
    ) -> bool {
        let shown: Vec<ZoneType> = zone_order(settings)
            .into_iter()
            .filter(|zone| zone.shown(settings))
            .collect();
        let content = content_width(max_columns, metrics);
        let graph_maximum = if max_columns > 0 { content } else { 0.0 };
        let same = shown.len() == self.zones.len()
            && shown.iter().zip(&self.zones).all(|(a, b)| *a == b.zone);
        self.width = width;
        if !same {
            // initializeGraphZones: the saved widths, the last zone also leaving room for the scrollbar.
            self.zones = shown
                .iter()
                .map(|&zone| {
                    let preferred = settings
                        .graph_column_widths
                        .get(zone.key())
                        .copied()
                        .filter(|&saved| saved > 0.0)
                        .unwrap_or_else(|| zone.default_width());
                    GraphZone {
                        zone,
                        minimum: zone.minimum(),
                        maximum: if zone == ZoneType::Graph {
                            graph_maximum
                        } else {
                            0.0
                        },
                        current: preferred,
                        preferred,
                    }
                })
                .collect();
            if let Some(last) = self.zones.last_mut() {
                last.minimum += SCROLLBAR_THICKNESS;
            }
            for i in 0..self.zones.len() {
                self.zones[i].current = self.clamp_zone(i, self.zones[i].current);
            }
            self.content = content;
        } else if content != self.content {
            // updateCommitZoneContentWidthFromChange: the graph zone never outgrows its lanes.
            self.content = content;
            for i in 0..self.zones.len() {
                if self.zones[i].zone != ZoneType::Graph {
                    continue;
                }
                self.zones[i].maximum = graph_maximum;
                if graph_maximum > 0.0 && self.zones[i].current > graph_maximum {
                    self.zones[i].current = self.clamp_zone(i, graph_maximum);
                }
            }
        }
        self.fit();
        !same
    }

    /// Moves the shown zone `from` to `to`, its widths with it; the zone now last gets the scrollbar's room and
    /// the one it replaces its limits back, and the zones fit the graph again. Returns the order to save: the
    /// shown zones as they are now, the hidden ones after them (`None` when nothing moved).
    pub fn move_zone(
        &mut self,
        settings: &Settings,
        from: usize,
        to: usize,
    ) -> Option<Vec<String>> {
        let count = self.zones.len();
        if from == to || from >= count || to >= count {
            return None;
        }
        let moved = self.zones.remove(from);
        self.zones.insert(to, moved);
        for i in 0..count {
            let last = i + 1 == count;
            let minimum =
                self.zones[i].zone.minimum() + if last { SCROLLBAR_THICKNESS } else { 0.0 };
            self.zones[i].minimum = minimum;
            self.zones[i].current = self.clamp_zone(i, self.zones[i].current);
        }
        self.fit();
        let mut order: Vec<String> = self.zones.iter().map(|z| z.zone.key().to_owned()).collect();
        for zone in zone_order(settings) {
            if !order.iter().any(|key| key == zone.key()) {
                order.push(zone.key().to_owned());
            }
        }
        Some(order)
    }
}

/// getCommitZoneContentWidth: the lanes plus the gutters and paddings on both sides.
#[must_use]
pub fn content_width(max_columns: usize, m: &Metrics) -> f32 {
    max_columns.max(1) as f32 * m.column_width
        + 2.0 * m.gutter_width
        + 2.0 * theme::COMMIT_ZONE_PADDING
}

/// The graph zone when it is narrower than its lanes (GraphContainer's node offsets), in CSS px: the lanes
/// scroll sideways between two opaque gutters that cast a shadow over them, the nodes stay pinned inside the
/// zone and fade towards its edges, and below one lane's width (single column mode, `factor` 0..1) every node
/// slides onto one column.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GraphLanes {
    /// The zone.
    pub width: f32,
    /// getCommitZoneContentWidth.
    pub content: f32,
    /// scrollLeft.
    pub scroll: f32,
    /// getSingleColumnModeFactor.
    pub factor: f32,
    /// getRightGutterSlideOutFactor.
    pub slide_out: f32,
}

impl GraphLanes {
    /// The lanes of the graph zone in `layout`; `scroll` (its scrollLeft, CSS px) is clamped to what there is
    /// to scroll.
    #[must_use]
    pub fn new(layout: &ZoneLayout, scroll: &mut f32, max_columns: usize, m: &Metrics) -> Self {
        let gutter = m.gutter_width;
        let padding = theme::COMMIT_ZONE_PADDING;
        let column = m.column_width;
        let width = layout
            .zones
            .iter()
            .filter(|zone| zone.zone == ZoneType::Graph)
            .map(|zone| zone.current)
            .next_back()
            .unwrap_or(0.0);
        let content = content_width(max_columns, m);
        // getScrollLeft: no scrolling once the zone shows all its lanes.
        *scroll = scroll.clamp(0.0, (content - width).max(0.0));
        let scrolled = if content == width { 0.0 } else { *scroll };
        let factor = if max_columns == 1 {
            0.0
        } else {
            (1.0 - (width - 2.0 * gutter - padding) / column).clamp(0.0, 1.0)
        };
        let slide_out =
            ((width - (content - (gutter + padding))) / column).clamp(0.0, 1.0) + factor / 2.0;
        Self {
            width,
            content,
            scroll: scrolled,
            factor,
            slide_out,
        }
    }
}

/// A zone's placed edge and width, device px.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ZoneRect {
    pub x: f32,
    pub w: f32,
}

/// Horizontal layout of the graph: the zones from left to right, in device px (hidden ones stay empty).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Zones {
    rects: [ZoneRect; 7],
    pub lanes: GraphLanes,
}

impl Zones {
    /// The zones in device px from `x` (the first header's left edge).
    #[must_use]
    pub fn place(layout: &ZoneLayout, x: f32, scale: f32) -> Self {
        let mut zones = Self::default();
        let mut edge = 0.0;
        for zone in &layout.zones {
            let left = x + (scale * edge).round();
            edge += zone.current;
            let w = x + (scale * edge).round() - left;
            *zones.rect_mut(zone.zone) = ZoneRect { x: left, w };
        }
        zones
    }

    #[must_use]
    pub fn rect(&self, zone: ZoneType) -> ZoneRect {
        self.rects[zone as usize]
    }

    pub fn rect_mut(&mut self, zone: ZoneType) -> &mut ZoneRect {
        &mut self.rects[zone as usize]
    }

    #[must_use]
    pub fn x(&self, zone: ZoneType) -> f32 {
        self.rect(zone).x
    }

    #[must_use]
    pub fn w(&self, zone: ZoneType) -> f32 {
        self.rect(zone).w
    }

    /// The last zone ends at `last_end`; a narrow graph squeezes it to its minimum, which already holds the
    /// scrollbar: there the settings button only covers its header, and the zone keeps its content width (the
    /// author's avatar).
    pub fn extend_last(&mut self, layout: &ZoneLayout, last_end: f32, scale: f32) {
        let Some(last) = layout.zones.last() else {
            return;
        };
        let content_minimum = (last.zone.minimum() * scale).round();
        let rect = self.rect_mut(last.zone);
        rect.w = (last_end - rect.x).max(rect.w.min(content_minimum));
    }

    /// The zone's body (its lanes' origin, where the gutters and the scrollbar start): a node's left edge sits
    /// GUTTER - PADDING_LEFT + 1 into the graph header cell, which puts the body 2px left of the cell.
    #[must_use]
    pub fn lanes_x(&self, m: &Metrics) -> f32 {
        self.x(ZoneType::Graph) - m.px(2.0)
    }

    /// A lane's centre, scrolled with the edges (getAbsoluteNodeLeftByColumn).
    #[must_use]
    pub fn column_x(&self, column: usize, m: &Metrics) -> f32 {
        self.lanes_x(m) + m.px(m.gutter_width) + column as f32 * m.px(m.column_width)
            - (self.lanes.scroll * m.scale).round()
            + m.px(m.column_width) / 2.0
    }

    /// A node's centre: its lane kept between getMinNodeOffset and getMaxNodeOffset (getNodeOffsetByColumn).
    #[must_use]
    pub fn node_x(&self, column: usize, m: &Metrics) -> f32 {
        let gutter = m.gutter_width;
        let padding = theme::COMMIT_ZONE_PADDING;
        let l = &self.lanes;
        let lowest = (1.0 - l.factor) * 2.0 * padding + l.factor * gutter;
        let highest = (1.0 - l.factor) * (l.width + padding - gutter) + l.factor * gutter;
        let left = gutter + column as f32 * m.column_width - l.scroll;
        self.lanes_x(m)
            + (lowest.max(left.min(highest)) * m.scale).round()
            + m.px(m.column_width) / 2.0
    }

    /// getNodeOpacityByColumn: nodes within half a gutter of the zone's edges fade to 50%.
    #[must_use]
    pub fn node_opacity(&self, column: usize, m: &Metrics) -> f32 {
        let gutter = m.gutter_width;
        let padding = theme::COMMIT_ZONE_PADDING;
        let l = &self.lanes;
        let left = gutter + column as f32 * m.column_width - l.scroll;
        let from_left = left - padding;
        let from_right = l.width - padding - left - gutter * (1.0 - l.slide_out);
        let fade = (from_left.min(from_right) / (gutter / 2.0)).max(l.factor);
        (0.5 + 0.5 * fade).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(widths: &[(ZoneType, f32)], width: f32) -> ZoneLayout {
        let mut layout = ZoneLayout {
            zones: widths
                .iter()
                .map(|&(zone, current)| GraphZone {
                    zone,
                    minimum: zone.minimum(),
                    maximum: 0.0,
                    current,
                    preferred: current,
                })
                .collect(),
            width,
            content: 0.0,
        };
        if let Some(last) = layout.zones.last_mut() {
            last.minimum += SCROLLBAR_THICKNESS;
        }
        layout
    }

    fn currents(layout: &ZoneLayout) -> Vec<f32> {
        layout.zones.iter().map(|z| z.current).collect()
    }

    #[test]
    fn order_keeps_saved_keys_first() {
        let settings = Settings {
            graph_column_order: vec![
                "message".into(),
                "bogus".into(),
                "ref".into(),
                "message".into(),
            ],
            ..Settings::default()
        };
        let order = zone_order(&settings);
        assert_eq!(&order[..2], &[ZoneType::Message, ZoneType::Ref]);
        assert_eq!(order.len(), 7);
        assert_eq!(order[2], ZoneType::Graph);
    }

    #[test]
    fn shown_zones_follow_the_settings() {
        let mut settings = Settings::default();
        assert!(ZoneType::Ref.shown(&settings));
        assert!(!ZoneType::Sha.shown(&settings));
        settings.show_sha = true;
        settings.hidden_graph_columns.insert("ref".to_owned());
        assert!(ZoneType::Sha.shown(&settings));
        assert!(!ZoneType::Ref.shown(&settings));
    }

    #[test]
    fn update_fills_the_width_with_the_last_zone() {
        let settings = Settings::default();
        let m = Metrics::rich(1.0);
        let mut zones = ZoneLayout::default();
        assert!(zones.update(&settings, 1000.0, 3, &m));
        assert_eq!(
            zones.zones.iter().map(|z| z.zone).collect::<Vec<_>>(),
            [ZoneType::Ref, ZoneType::Graph, ZoneType::Message]
        );
        assert!((zones.total(None) - 1000.0).abs() < 0.01);
        // The graph zone never outgrows its lanes.
        let graph = zones.zones[1];
        assert!(graph.current <= content_width(3, &m));
        // The same zones again: not rebuilt.
        assert!(!zones.update(&settings, 900.0, 3, &m));
        assert!((zones.total(None) - 900.0).abs() < 0.01);
    }

    #[test]
    fn shrinking_takes_from_the_last_zones_first() {
        let mut l = layout(
            &[
                (ZoneType::Ref, 100.0),
                (ZoneType::Graph, 150.0),
                (ZoneType::Message, 400.0),
            ],
            500.0,
        );
        l.fit();
        assert_eq!(currents(&l), [100.0, 150.0, 250.0]);
        l.width = 300.0;
        l.fit();
        // The message hits its minimum (50 + scrollbar) and the graph gives way next.
        assert_eq!(currents(&l), [100.0, 142.0, 58.0]);
    }

    #[test]
    fn resizing_makes_room_after_the_zone() {
        let mut l = layout(
            &[
                (ZoneType::Ref, 100.0),
                (ZoneType::Graph, 150.0),
                (ZoneType::Message, 250.0),
            ],
            500.0,
        );
        l.resize_zone(0, 140.0, false);
        assert_eq!(l.zones[0].current, 140.0);
        assert!((l.total(None) - 500.0).abs() < 0.01);
        // Released, the width becomes the preferred one.
        l.resize_zone(0, 140.0, true);
        assert_eq!(l.zones[0].preferred, 140.0);
        // Below the minimum it stops.
        l.resize_zone(0, 5.0, true);
        assert_eq!(l.zones[0].current, ZoneType::Ref.minimum());
        assert!((l.total(None) - 500.0).abs() < 0.01);
    }

    #[test]
    fn max_width_leaves_the_minimums_after() {
        let l = layout(
            &[
                (ZoneType::Ref, 100.0),
                (ZoneType::Graph, 150.0),
                (ZoneType::Message, 250.0),
            ],
            500.0,
        );
        assert_eq!(l.max_zone_width(0), 500.0 - 56.0 - 58.0);
        assert_eq!(l.max_zone_width(1), 500.0 - 100.0 - 58.0);
    }

    #[test]
    fn moving_a_zone_keeps_its_width_and_saves_the_order() {
        let settings = Settings::default();
        let mut l = layout(
            &[
                (ZoneType::Ref, 100.0),
                (ZoneType::Graph, 150.0),
                (ZoneType::Message, 250.0),
            ],
            500.0,
        );
        let order = l.move_zone(&settings, 2, 0).expect("moved");
        assert_eq!(
            l.zones.iter().map(|z| z.zone).collect::<Vec<_>>(),
            [ZoneType::Message, ZoneType::Ref, ZoneType::Graph]
        );
        assert_eq!(&order[..3], ["message", "ref", "graph"]);
        assert_eq!(order.len(), 7);
        // The zone now last holds the scrollbar's room.
        assert_eq!(
            l.zones[2].minimum,
            ZoneType::Graph.minimum() + SCROLLBAR_THICKNESS
        );
        assert_eq!(l.zones[0].minimum, ZoneType::Message.minimum());
        assert!((l.total(None) - 500.0).abs() < 0.01);
        assert_eq!(l.move_zone(&settings, 1, 1), None);
        assert_eq!(l.move_zone(&settings, 0, 3), None);
    }

    #[test]
    fn placing_zones_rounds_their_edges() {
        let l = layout(&[(ZoneType::Ref, 100.5), (ZoneType::Graph, 150.25)], 250.75);
        let z = Zones::place(&l, 10.0, 1.5);
        assert_eq!(z.x(ZoneType::Ref), 10.0);
        assert_eq!(z.w(ZoneType::Ref), (1.5f32 * 100.5).round());
        assert_eq!(z.x(ZoneType::Graph), 10.0 + z.w(ZoneType::Ref));
        assert_eq!(z.w(ZoneType::Message), 0.0);
    }

    #[test]
    fn lanes_clamp_the_scroll_and_slide_into_one_column() {
        let m = Metrics::rich(1.0);
        // Wide enough for every lane: no scrolling at all.
        let wide = layout(&[(ZoneType::Graph, 400.0)], 400.0);
        let mut scroll = 50.0;
        let lanes = GraphLanes::new(&wide, &mut scroll, 3, &m);
        assert_eq!((scroll, lanes.scroll, lanes.factor), (0.0, 0.0, 0.0));
        // Narrower than the lanes: the scroll is kept up to the overflow.
        let narrow = layout(&[(ZoneType::Graph, 100.0)], 100.0);
        let mut scroll = 1000.0;
        let lanes = GraphLanes::new(&narrow, &mut scroll, 6, &m);
        assert_eq!(scroll, lanes.content - 100.0);
        assert_eq!(lanes.scroll, scroll);
        // Below one lane's width every node slides onto one column.
        let tiny = layout(&[(ZoneType::Graph, 56.0)], 56.0);
        let mut scroll = 0.0;
        let lanes = GraphLanes::new(&tiny, &mut scroll, 6, &m);
        assert!(lanes.factor > 0.0);
        let mut zones = Zones::place(&tiny, 0.0, 1.0);
        zones.lanes = lanes;
        assert_eq!(zones.node_x(0, &m), zones.node_x(5, &m));
        // In single column mode the nodes stay opaque.
        assert_eq!(zones.node_opacity(5, &m), 1.0);
    }
}
