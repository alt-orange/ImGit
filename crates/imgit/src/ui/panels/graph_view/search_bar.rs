//! The commit search bar (Ctrl+F) over the graph.
//!
//! Ports `src/ui/graph_view.cpp`.

use dear_imgui_rs::{
    Condition, InputTextFlags, Key, MouseCursor, StyleColor, StyleVar, WindowFlags, sys,
};
use imgit_app::RepoState;

use crate::theme::Color;
use crate::ui::Cx;
use crate::ui::geom::{Vec2, vec2};

const WIDTH: f32 = 411.6;
const HEIGHT: f32 = 34.0;

/// What a click on one of the bar's icons does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Previous,
    Next,
    Close,
}

/// The `.pointer.px1` icons: arrow-up (12.25px) `.mr1`, arrow-down (12.25px) `.mr1`, times (9.625px).
const BUTTONS: [(&str, &str, f32, f32, Step); 3] = [
    (
        "##search_prev",
        "fas:arrow-up",
        327.5,
        22.25,
        Step::Previous,
    ),
    ("##search_next", "fas:arrow-down", 354.75, 22.25, Step::Next),
    ("##search_close", "fas:times", 382.0, 19.625, Step::Close),
];

/// `ImGui::GetActiveID()`.
fn active_id() -> sys::ImGuiID {
    // SAFETY: only called while a frame is open (the caller holds a `Cx`), so the context is current.
    unsafe { sys::igGetActiveID() }
}

/// "3 of 12" / "0 results".
#[must_use]
pub fn match_count_label(index: Option<usize>, matches: usize) -> String {
    if matches == 0 {
        "0 results".to_owned()
    } else {
        format!("{} of {matches}", index.map_or(0, |i| i + 1))
    }
}

/// The commit search (Ctrl+F), absolute 4px below the graph's top and 12px from its right edge, over the headers
/// and rows: a `.filtering` (blue 50% over panel-bg1) rad4 bar holding a fixed-width far:search, a 200px input
/// (black 20%, 8% white border), the 85px "{0} of {1}" / "0 results" and the up / down / close icons, in 14px
/// text with 5px (`.px1` / `.mr1` / `.mx1`) and 10px (`.px2`) spacing. Its own window keeps the rows under it from
/// hovering. `refocus` keeps the input focused after Enter (a single-line input lets go on it).
pub fn commit_search_bar(
    cx: &mut Cx<'_>,
    state: &mut RepoState,
    refocus: &mut bool,
    origin: Vec2,
    size: Vec2,
) {
    if !state.search_open {
        return;
    }
    let ui = cx.ui;
    let min = vec2(
        (origin.x + size.x - cx.px(12.0 + WIDTH)).round(),
        (origin.y + cx.px(4.0)).round(),
    );
    let max = vec2(min.x + cx.px(WIDTH).round(), min.y + cx.px(HEIGHT));
    let _min_size = ui.push_style_var(StyleVar::WindowMinSize([1.0, 1.0]));
    let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
    let _border = ui.push_style_var(StyleVar::WindowBorderSize(0.0));
    ui.window("##commit_search_bar")
        .position(min.into(), Condition::Always)
        .size((max - min).into(), Condition::Always)
        .flags(
            WindowFlags::NO_DECORATION
                | WindowFlags::NO_BACKGROUND
                | WindowFlags::NO_SAVED_SETTINGS
                | WindowFlags::NO_FOCUS_ON_APPEARING
                | WindowFlags::NO_NAV
                | WindowFlags::NO_MOVE
                | WindowFlags::NO_SCROLL_WITH_MOUSE,
        )
        .build(|| draw_bar(cx, state, refocus, min, max));
}

fn draw_bar(cx: &mut Cx<'_>, state: &mut RepoState, refocus: &mut bool, min: Vec2, max: Vec2) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    draw.add_rect_filled(min, max, p.panel_bg1, cx.px(4.0));
    draw.add_rect_filled(min, max, p.accent.with_alpha(0.5), cx.px(4.0));
    let white = p.text_selected;
    let mid = min.y + cx.px(HEIGHT / 2.0);
    let baseline = min.y + cx.px(22.0);
    let ascent = cx.font_px(cx.fonts.regular, 14.0) * 2189.0 / 2789.0;
    let scale = cx.scale;
    let at = move |x: f32| min.x + (x * scale).round();
    cx.draw_fa_icon(draw, "far:search", vec2(at(18.75), mid), cx.px(14.0), white);

    // The input: Enter / Shift+Enter and Down / Up step through the matches; Ctrl+F selects its text.
    let input_min = vec2(at(32.5).round(), min.y + cx.px(5.0));
    let input_max = vec2(input_min.x + cx.px(200.0), input_min.y + cx.px(24.0));
    // The border over the fill: the whole box black 20% + white 8%, its inside black 20% alone (opaque, so the
    // border pixels under it don't show through).
    let bar = p.accent.mix(p.panel_bg1, 0.5);
    let fill = Color::BLACK.mix(bar, 0.2);
    draw.add_rect_filled(input_min, input_max, p.ink.mix(fill, 0.08), cx.px(2.0));
    draw.add_rect_filled(
        input_min + vec2(1.0, 1.0),
        input_max - vec2(1.0, 1.0),
        fill,
        cx.px(1.0),
    );
    let input_id = cx.id("##commit_search");
    let was_active = active_id() == input_id;
    let select_all = state.search_focus_requested;
    if state.search_focus_requested || *refocus {
        ui.set_keyboard_focus_here();
        state.search_focus_requested = false;
        *refocus = false;
    }
    let pad_y = (baseline - ascent - (input_min.y + 1.0)).max(0.0);
    cx.set_cursor(input_min + vec2(1.0, 1.0));
    let changed = {
        let _pad = ui.push_style_var(StyleVar::FramePadding([cx.px(5.0), pad_y]));
        let _border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
        let _bg = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
        let _bg_hovered = ui.push_style_color(StyleColor::FrameBgHovered, Color::TRANSPARENT);
        let _bg_active = ui.push_style_color(StyleColor::FrameBgActive, Color::TRANSPARENT);
        let _text = ui.push_style_color(StyleColor::Text, p.text_selected);
        let _disabled = ui.push_style_color(StyleColor::TextDisabled, p.text_disabled);
        let _font = cx.push_font(cx.fonts.regular, 14.0);
        ui.set_next_item_width(cx.px(200.0) - 2.0);
        ui.input_text("##commit_search", &mut state.search_text)
            .hint("find commit")
            .flags(if select_all {
                InputTextFlags::AUTO_SELECT_ALL
            } else {
                InputTextFlags::NONE
            })
            .build()
    };
    if changed {
        state.run_search();
    }
    if was_active {
        if ui.is_key_pressed(Key::Enter) || ui.is_key_pressed(Key::KeypadEnter) {
            state.select_search_match(if ui.io().key_shift() { -1 } else { 1 });
            *refocus = true; // a single-line input lets go on Enter
        } else if ui.is_key_pressed(Key::DownArrow) {
            state.select_search_match(1);
        } else if ui.is_key_pressed(Key::UpArrow) {
            state.select_search_match(-1);
        }
    }

    let count = match_count_label(state.search_index, state.search_matches.len());
    cx.draw_text(
        draw,
        cx.fonts.regular,
        14.0,
        vec2(at(237.5), baseline - ascent),
        white,
        &count,
    );

    for (id, icon, left, width, step) in BUTTONS {
        cx.set_cursor(vec2(at(left), min.y));
        if ui.invisible_button(id, vec2(cx.px(width), cx.px(HEIGHT))) {
            match step {
                Step::Close => {
                    state.search_open = false;
                    state.run_search();
                }
                Step::Previous => state.select_search_match(-1),
                Step::Next => state.select_search_match(1),
            }
        }
        if ui.is_item_hovered() {
            ui.set_mouse_cursor(Some(MouseCursor::Hand));
        }
        cx.draw_fa_icon(
            draw,
            icon,
            vec2(at(left + width / 2.0), mid),
            cx.px(14.0),
            white,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_counts_read_like_the_original() {
        assert_eq!(match_count_label(None, 0), "0 results");
        assert_eq!(match_count_label(Some(0), 0), "0 results");
        assert_eq!(match_count_label(Some(2), 12), "3 of 12");
        assert_eq!(match_count_label(None, 5), "0 of 5");
    }
}
