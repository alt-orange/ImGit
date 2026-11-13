//! The ref zone: the refs of a commit grouped into pills (one per short name), the pills' drawing, hover and
//! drag, the "+N" overflow count, and which rows a hovered group keeps lit.
//!
//! Ports `src/ui/graph_view.cpp`.

use std::path::PathBuf;

use dear_imgui_rs::{MouseButton, WindowHoveredFlags};
use imgit_app::RepoState;
use imgit_git::{Ref, RefType};

use super::zones::Metrics;
use crate::theme::{Color, FONT_SIZE_ROW};
use crate::ui::draw::DrawList;
use crate::ui::geom::{Vec2, vec2};
use crate::ui::panels::menus::can_drop_ref;
use crate::ui::{Cx, Icon};

const REF_ICON_EM: f32 = 12.0;
/// `.ref-node.p1`.
const REF_PAD: f32 = 5.0;

/// The refs on a commit grouped by short name (a local branch, its remote-tracking branches and a tag of the
/// same name share one node), the checked-out group first, then local over remote over tag groups, bigger
/// groups first, then by name.
#[derive(Debug, Clone, PartialEq)]
pub struct RefGroup {
    pub name: String,
    pub refs: Vec<Ref>,
    pub current: bool,
}

impl RefGroup {
    /// The ref the group stands for in menus and clicks.
    #[must_use]
    pub fn primary(&self) -> &Ref {
        &self.refs[0]
    }

    /// The group as a hovered ref group is remembered.
    #[must_use]
    pub fn key(&self) -> RefGroupKey {
        let mut key = RefGroupKey {
            name: self.name.clone(),
            ..RefGroupKey::default()
        };
        for r in &self.refs {
            match r.ref_type {
                RefType::LocalBranch => key.head = true,
                RefType::RemoteBranch => key.remotes.push(r.remote.clone()),
                RefType::Tag => key.tag = true,
                RefType::Stash => {}
            }
        }
        key
    }
}

/// The ref group whose rows stay lit: its name and the kinds of refs it holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RefGroupKey {
    pub name: String,
    pub head: bool,
    /// Remote names.
    pub remotes: Vec<String>,
    pub tag: bool,
}

/// The name refs of a group share: a remote branch without its remote.
#[must_use]
pub fn group_name(r: &Ref) -> &str {
    if r.ref_type == RefType::RemoteBranch && r.name.len() > r.remote.len() {
        r.name.get(r.remote.len() + 1..).unwrap_or(&r.name)
    } else {
        &r.name
    }
}

/// The groups of `refs` in display order.
#[must_use]
pub fn ref_groups(refs: &[&Ref]) -> Vec<RefGroup> {
    let mut groups: Vec<RefGroup> = Vec::new();
    for ty in [RefType::LocalBranch, RefType::RemoteBranch, RefType::Tag] {
        for &r in refs.iter().filter(|r| r.ref_type == ty) {
            let name = group_name(r);
            if let Some(group) = groups.iter_mut().find(|g| g.name == name) {
                group.current |= r.is_head;
                group.refs.push(r.clone());
            } else {
                groups.push(RefGroup {
                    name: name.to_owned(),
                    refs: vec![r.clone()],
                    current: r.is_head,
                });
            }
        }
    }
    let priority = |group: &RefGroup| match group.primary().ref_type {
        RefType::LocalBranch => 2,
        RefType::RemoteBranch => 1,
        _ => 0,
    };
    groups.sort_by(|a, b| {
        b.current
            .cmp(&a.current)
            .then_with(|| priority(b).cmp(&priority(a)))
            .then_with(|| b.refs.len().cmp(&a.refs.len()))
            // localeCompare: case-insensitive first.
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    groups
}

/// One `.ref-avatar` icon per ref of the group: laptop (local), cloud (remote), tag.
fn ref_icon(r: &Ref) -> Icon {
    match r.ref_type {
        RefType::RemoteBranch => Icon::Cloud,
        RefType::Tag => Icon::Tag,
        _ => Icon::Laptop,
    }
}

fn ref_icon_tooltip(r: &Ref) -> String {
    match r.ref_type {
        RefType::RemoteBranch => format!("{} -> {} (Remote Branch)", r.remote, group_name(r)),
        RefType::Tag => format!(
            "{} (Tag)",
            if r.annotated && !r.message.is_empty() {
                &r.message
            } else {
                &r.name
            }
        ),
        _ => format!("{} (local)", r.name),
    }
}

fn ref_icons_width(cx: &Cx<'_>, group: &RefGroup, m: &Metrics) -> f32 {
    let count = group.refs.len();
    group
        .refs
        .iter()
        .enumerate()
        .map(|(i, r)| {
            cx.icon_width(ref_icon(r), m.px(REF_ICON_EM))
                + if i + 1 < count { m.px(5.0) } else { 0.0 }
        })
        .sum()
}

/// Width of the `.ref-node` at its content size: padding, [check + .6rem], name + .mr1, icons, padding.
#[must_use]
pub fn ref_node_width(cx: &Cx<'_>, group: &RefGroup, m: &Metrics) -> f32 {
    let check = if group.current {
        m.px(REF_ICON_EM) + m.px(6.0)
    } else {
        0.0
    };
    m.px(REF_PAD) * 2.0
        + check
        + cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, &group.name).x
        + m.px(5.0)
        + ref_icons_width(cx, group, m)
}

/// What a ref pill reports.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RefNodeResult {
    pub clicked: bool,
    pub double_clicked: bool,
    pub right_clicked: bool,
    pub hovered: bool,
    /// As drawn: a drop target loses its check and gains its 2px band.
    pub width: f32,
}

/// A ref group being dragged: it starts 4px from the press (Windows' SM_CXDRAG) and follows the mouse as
/// Chromium's drag image, the pill at 75% opacity held where it was grabbed. Mouse events stop during a native
/// drag, so hover state stays as it was when it started.
#[derive(Debug, Clone, Default)]
pub struct RefDrag {
    pub pressed: bool,
    pub active: bool,
    pub press: Vec2,
    /// The press in the pill.
    pub grab: Vec2,
    pub width: f32,
    pub lane: Color,
    pub name: String,
    pub current: bool,
    /// Copied: a refresh may rebuild the history mid-drag.
    pub refs: Vec<Ref>,
    pub repo: PathBuf,
}

/// The drag of a ref group and the pill under the mouse this frame that accepts the drop.
#[derive(Debug, Clone, Default)]
pub struct RefInteraction {
    pub drag: RefDrag,
    pub drop_target: Option<Ref>,
}

/// 1px of a box-shadow ring `offset` px outside `[min, max]`.
fn shadow_ring(draw: DrawList<'_>, m: &Metrics, min: Vec2, max: Vec2, offset: f32, color: Color) {
    let o = m.px(offset);
    let one = m.px(1.0);
    draw.add_rect_filled(
        vec2(min.x - o - one, min.y - o - one),
        vec2(max.x + o + one, min.y - o),
        color,
        0.0,
    );
    draw.add_rect_filled(
        vec2(min.x - o - one, max.y + o),
        vec2(max.x + o + one, max.y + o + one),
        color,
        0.0,
    );
    draw.add_rect_filled(
        vec2(min.x - o - one, min.y - o),
        vec2(min.x - o, max.y + o),
        color,
        0.0,
    );
    draw.add_rect_filled(
        vec2(max.x + o, min.y - o),
        vec2(max.x + o + one, max.y + o),
        color,
        0.0,
    );
}

/// How a pill is drawn.
#[derive(Debug, Clone, Copy)]
pub struct PillStyle {
    pub lane: Color,
    /// Fades all of it.
    pub opacity: f32,
    /// The weight of the lane colour mixed into the graph background.
    pub weight: f32,
    pub show_check: bool,
    pub tooltips: bool,
}

/// The `.ref-node` pill: the lane colour mixed into the graph background at `style.weight`, the check of the
/// checked-out branch, the name and one icon per ref.
pub fn draw_ref_pill(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    m: &Metrics,
    group: &RefGroup,
    (min, max): (Vec2, Vec2),
    style: PillStyle,
) {
    let p = cx.palette;
    let fade = |c: Color| c.with_alpha(c.a * style.opacity);
    draw.add_rect_filled(
        min,
        max,
        fade(style.lane.mix(p.app_bg0, style.weight)),
        m.px(2.0),
    );
    let color = fade(if group.current {
        p.text_selected
    } else {
        p.text_normal
    });
    let mid = (min.y + max.y) / 2.0;
    let mut x = min.x + m.px(REF_PAD);
    draw.push_clip_rect(min, max, true);
    if group.current && style.show_check {
        let w = m.px(REF_ICON_EM);
        cx.draw_icon(draw, Icon::Check, vec2(x + w / 2.0, mid), w, color);
        if style.tooltips {
            cx.tooltip_on_rect(vec2(x, min.y), vec2(x + w, max.y), "current", true);
        }
        x += w + m.px(6.0);
    }
    let icons_w = ref_icons_width(cx, group, m);
    let name_room = (max.x - m.px(REF_PAD) - icons_w - m.px(5.0) - x).max(0.0);
    let line_h = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, "A").y;
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.regular,
        FONT_SIZE_ROW,
        vec2(x, mid - line_h / 2.0),
        name_room,
        color,
        &group.name,
    );
    x += name_room + m.px(5.0);
    for r in &group.refs {
        let icon = ref_icon(r);
        let w = cx.icon_width(icon, m.px(REF_ICON_EM));
        cx.draw_icon(draw, icon, vec2(x + w / 2.0, mid), m.px(REF_ICON_EM), color);
        if style.tooltips {
            cx.tooltip_on_rect(
                vec2(x, min.y),
                vec2(x + w, max.y),
                &ref_icon_tooltip(r),
                true,
            );
        }
        x += w + m.px(5.0);
    }
    draw.pop_clip_rect();
}

/// Where and how a pill is placed by [`ref_node`].
#[derive(Clone, Copy)]
pub struct RefNode<'a> {
    pub group: &'a RefGroup,
    pub pos: Vec2,
    pub width: f32,
    pub lane: Color,
    /// Dims ghost refs (`.dim-ref`).
    pub opacity: f32,
    pub id: &'a str,
}

/// `.ref-node`: lane colour mixed into the graph background at 25% (45% hovered, 50% pressed or for the
/// checked-out branch, which also gets the selected text colour). Pressing it arms a drag; while another group
/// is dragged it shows whether it takes the drop.
pub fn ref_node(
    cx: &mut Cx<'_>,
    interaction: &mut RefInteraction,
    state: &RepoState,
    m: &Metrics,
    draw: DrawList<'_>,
    node: &RefNode<'_>,
) -> RefNodeResult {
    let ui = cx.ui;
    let RefNode {
        group,
        pos,
        mut width,
        lane,
        opacity,
        id,
    } = *node;
    let h = m.px(m.ref_height);
    let primary = group.primary();
    let mut result = RefNodeResult::default();
    cx.set_cursor(pos);
    {
        let _id = ui.push_id(id);
        let _ref = ui.push_id(primary.full_name.as_str());
        ui.invisible_button("##ref_node", vec2(width, h));
        result.hovered = ui.is_item_hovered() && !interaction.drag.active;
        result.clicked = ui.is_item_clicked();
        result.right_clicked = result.hovered && ui.is_mouse_released(MouseButton::Right);
        result.double_clicked = result.hovered && ui.is_mouse_double_clicked(MouseButton::Left);
        if ui.is_item_activated() {
            let mouse = cx.mouse_pos();
            interaction.drag = RefDrag {
                pressed: true,
                active: false,
                press: mouse,
                grab: mouse - pos,
                width,
                lane,
                name: group.name.clone(),
                current: group.current,
                refs: group.refs.clone(),
                repo: state.repo.workdir().to_path_buf(),
            };
        }
    }
    let pressed = ui.is_item_active() && !interaction.drag.active;

    // While another group is dragged, the pills it can drop on (.ref-node-wrapper.droppable): a 2px gold band
    // (padding 2px 2px 2px 0) around the pill, 18px tall, with a gold glow, green under the mouse (.hovering);
    // their check icon is hidden.
    let droppable = interaction.drag.active
        && interaction.drag.repo == state.repo.workdir()
        && interaction
            .drag
            .refs
            .first()
            .is_some_and(|source| can_drop_ref(state, source, primary));
    if droppable && group.current {
        width -= m.px(REF_ICON_EM) + m.px(6.0);
    }
    if droppable {
        let p = cx.palette;
        let mouse = cx.mouse_pos();
        let over = ui.is_window_hovered_with_flags(
            WindowHoveredFlags::CHILD_WINDOWS
                | WindowHoveredFlags::ALLOW_WHEN_BLOCKED_BY_ACTIVE_ITEM,
        ) && mouse.x >= pos.x
            && mouse.x < pos.x + width + m.px(2.0)
            && mouse.y >= pos.y
            && mouse.y < pos.y + h;
        if over {
            interaction.drop_target = Some(primary.clone());
        }
        draw.add_rect_filled(
            pos,
            vec2(pos.x + width + m.px(2.0), pos.y + h),
            p.yellow.with_alpha(0.3 * opacity),
            0.0,
        );
        // box-shadow: 0 0 2px 2px.
        let glow = if over {
            p.green.with_alpha(0.5 * opacity)
        } else {
            p.yellow.with_alpha(0.3 * opacity)
        };
        let pill = (
            vec2(pos.x, pos.y + m.px(2.0)),
            vec2(pos.x + width, pos.y + h - m.px(2.0)),
        );
        shadow_ring(draw, m, pill.0, pill.1, 0.0, glow);
        shadow_ring(draw, m, pill.0, pill.1, 1.0, glow.with_alpha(glow.a * 0.5));
        shadow_ring(draw, m, pill.0, pill.1, 2.0, glow.with_alpha(glow.a * 0.12));
        draw_ref_pill(
            cx,
            draw,
            m,
            group,
            pill,
            PillStyle {
                lane,
                opacity,
                weight: if group.current { 0.50 } else { 0.25 },
                show_check: false,
                tooltips: false,
            },
        );
        result.width = width + m.px(2.0);
        return result;
    }
    let weight = if group.current || pressed {
        0.50
    } else if result.hovered {
        0.45
    } else {
        0.25
    };
    draw_ref_pill(
        cx,
        draw,
        m,
        group,
        (pos, vec2(pos.x + width, pos.y + h)),
        PillStyle {
            lane,
            opacity,
            weight,
            show_check: true,
            tooltips: !interaction.drag.active,
        },
    );
    result.width = width;
    result
}

/// `.overflow-count`: "+N" more ref groups, 3px padding, the lane colour at 25%.
#[must_use]
pub fn overflow_count_width(cx: &Cx<'_>, m: &Metrics, count: usize) -> f32 {
    cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, &format!("+{count}"))
        .x
        + m.px(6.0)
}

pub fn overflow_count(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    m: &Metrics,
    pos: Vec2,
    count: usize,
    lane: Color,
    opacity: f32,
) {
    let p = cx.palette;
    let text = format!("+{count}");
    let w = overflow_count_width(cx, m, count);
    let h = m.px(m.ref_height);
    let bg = lane.mix(p.app_bg0, 0.25);
    draw.add_rect_filled(
        pos,
        vec2(pos.x + w, pos.y + h),
        bg.with_alpha(bg.a * opacity),
        m.px(2.0),
    );
    let line_h = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, "A").y;
    cx.draw_text(
        draw,
        cx.fonts.regular,
        FONT_SIZE_ROW,
        vec2(pos.x + m.px(3.0), pos.y + (h - line_h) / 2.0),
        p.text_normal.with_alpha(p.text_normal.a * opacity),
        &text,
    );
}

fn branch_and_tag_refs<'a>(state: &'a RepoState, sha: &str) -> Vec<&'a Ref> {
    let mut refs = state.refs_at(sha);
    refs.retain(|r| r.ref_type != RefType::Stash);
    refs
}

/// The refs a row stands for: its own (a tag-only row's only while a tag is the key), else its childRefs.
fn row_refs(state: &RepoState, row: usize, tag_key: bool) -> Vec<&Ref> {
    let commits = &state.history.commits;
    let refs = branch_and_tag_refs(state, &commits[row].sha);
    let tags_only = !refs.is_empty() && refs.iter().all(|r| r.ref_type == RefType::Tag);
    if refs.is_empty() || (tags_only && !tag_key) {
        return match state.child_ref_rows.get(row).copied().flatten() {
            Some(child) => branch_and_tag_refs(state, &commits[child].sha),
            None => Vec::new(),
        };
    }
    refs
}

/// The groups of the refs a row stands for, for the Ctrl-held dimming.
#[must_use]
pub fn row_ref_groups(state: &RepoState, row: usize) -> Vec<RefGroup> {
    ref_groups(&row_refs(state, row, false))
}

/// isMissingHoveredRefGroup: none of the row's refs is the key's branch, remote branch or tag.
#[must_use]
pub fn row_misses_group(state: &RepoState, row: usize, key: &RefGroupKey) -> bool {
    !row_refs(state, row, key.tag)
        .into_iter()
        .filter(|r| group_name(r) == key.name)
        .any(|r| match r.ref_type {
            RefType::LocalBranch => key.head,
            RefType::RemoteBranch => key.remotes.contains(&r.remote),
            RefType::Tag => key.tag,
            RefType::Stash => false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(ty: RefType, name: &str, remote: &str, head: bool) -> Ref {
        Ref {
            ref_type: ty,
            name: name.to_owned(),
            remote: remote.to_owned(),
            full_name: name.to_owned(),
            is_head: head,
            ..Ref::default()
        }
    }

    #[test]
    fn remote_branches_group_under_their_short_name() {
        let origin = make(RefType::RemoteBranch, "origin/main", "origin", false);
        assert_eq!(group_name(&origin), "main");
        let local = make(RefType::LocalBranch, "main", "", true);
        assert_eq!(group_name(&local), "main");
        // A remote name as long as the whole ref name is kept as is.
        let odd = make(RefType::RemoteBranch, "origin", "origin", false);
        assert_eq!(group_name(&odd), "origin");
    }

    #[test]
    fn groups_order_current_then_local_then_size_then_name() {
        let refs = [
            make(RefType::Tag, "v1", "", false),
            make(RefType::RemoteBranch, "origin/zeta", "origin", false),
            make(RefType::LocalBranch, "beta", "", false),
            make(RefType::LocalBranch, "Alpha", "", false),
            make(RefType::LocalBranch, "main", "", true),
            make(RefType::RemoteBranch, "origin/beta", "origin", false),
        ];
        let refs: Vec<&Ref> = refs.iter().collect();
        let groups = ref_groups(&refs);
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["main", "beta", "Alpha", "zeta", "v1"]);
        assert!(groups[0].current);
        // beta has its remote with it.
        assert_eq!(groups[1].refs.len(), 2);
        assert_eq!(
            groups[1].key(),
            RefGroupKey {
                name: "beta".into(),
                head: true,
                remotes: vec!["origin".into()],
                tag: false
            }
        );
    }

    #[test]
    fn tooltips_name_the_kind_of_ref() {
        let tag = Ref {
            annotated: true,
            message: "Release".into(),
            ..make(RefType::Tag, "v1", "", false)
        };
        assert_eq!(ref_icon_tooltip(&tag), "Release (Tag)");
        assert_eq!(
            ref_icon_tooltip(&make(RefType::RemoteBranch, "origin/dev", "origin", false)),
            "origin -> dev (Remote Branch)"
        );
        assert_eq!(
            ref_icon_tooltip(&make(RefType::LocalBranch, "dev", "", false)),
            "dev (local)"
        );
    }
}
