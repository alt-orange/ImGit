//! The draw list: ImGui's `ImDrawList` taking the UI's own [`Vec2`] and [`Color`].
//!
//! `dear_imgui_rs::DrawListMut` allows one wrapper per draw list at a time and panics on a second one, which
//! rules out a widget fetching the window's draw list while its caller holds it, as the UI does all the time.
//! [`DrawList`] is a plain copyable handle instead: get one wherever it is needed (`Cx::window_draw_list`) and
//! pass it around by value. Its colours are packed with ImGui's own rounding ([`Color::to_u32`]).

use std::ffi::c_char;
use std::marker::PhantomData;

use dear_imgui_rs::{DrawFlags, TextureId, Ui, sys};

use crate::fonts::Font;
use crate::theme::Color;
use crate::ui::geom::{Rect, Vec2, vec2};

/// A draw list of the frame `'ui` is building: a window's, or the foreground / background one.
#[derive(Debug, Clone, Copy)]
pub struct DrawList<'ui> {
    raw: *mut sys::ImDrawList,
    _frame: PhantomData<&'ui Ui>,
}

/// The begin and end pointers ImGui takes for a string slice.
pub(crate) fn text_range(text: &str) -> (*const c_char, *const c_char) {
    let range = text.as_bytes().as_ptr_range();
    (range.start.cast(), range.end.cast())
}

fn texture_ref(texture: TextureId) -> sys::ImTextureRef {
    sys::ImTextureRef {
        _TexData: std::ptr::null_mut(),
        _TexID: texture.id(),
    }
}

fn raw_flags(flags: DrawFlags) -> sys::ImDrawFlags {
    flags.bits() as sys::ImDrawFlags
}

// SAFETY (for every `unsafe` block of this impl): `self.raw` is a draw list of the frame `'ui` borrows, owned
// by the ImGui context that is current on this thread; the handle cannot outlive that frame, is neither `Send`
// nor `Sync`, and every call only appends to the list (or reads it) through ImGui's own functions. Pointers
// passed along (text ranges, point slices) are valid for the call, which copies what it needs.
impl<'ui> DrawList<'ui> {
    /// The current window's draw list (`ImGui::GetWindowDrawList()`). A window must be open.
    #[must_use]
    pub fn window(_ui: &'ui Ui) -> Self {
        // SAFETY: `_ui` proves a frame is open; ImGui returns the current window's list.
        let raw = unsafe { sys::igGetWindowDrawList() };
        assert!(!raw.is_null(), "no current window to draw into");
        Self {
            raw,
            _frame: PhantomData,
        }
    }

    /// The list drawn over every window (`ImGui::GetForegroundDrawList()`): tooltips, drag previews.
    #[must_use]
    pub fn foreground(_ui: &'ui Ui) -> Self {
        // SAFETY: `_ui` proves a frame is open, so the main viewport and its lists exist.
        let raw = unsafe { sys::igGetForegroundDrawList_ViewportPtr(sys::igGetMainViewport()) };
        assert!(!raw.is_null(), "no foreground draw list");
        Self {
            raw,
            _frame: PhantomData,
        }
    }

    /// The list drawn behind every window (`ImGui::GetBackgroundDrawList()`).
    #[must_use]
    pub fn background(_ui: &'ui Ui) -> Self {
        // SAFETY: as for `foreground`.
        let raw = unsafe { sys::igGetBackgroundDrawList(sys::igGetMainViewport()) };
        assert!(!raw.is_null(), "no background draw list");
        Self {
            raw,
            _frame: PhantomData,
        }
    }

    /// The `ImDrawList` itself, for what this wrapper lacks.
    #[must_use]
    pub fn raw(self) -> *mut sys::ImDrawList {
        self.raw
    }

    pub fn add_line(self, a: Vec2, b: Vec2, color: Color, thickness: f32) {
        unsafe {
            sys::ImDrawList_AddLine(self.raw, a.into(), b.into(), color.to_u32(), thickness, 0);
        }
    }

    /// The outline of a rectangle; `rounding` 0 for square corners.
    pub fn add_rect(self, min: Vec2, max: Vec2, color: Color, rounding: f32, thickness: f32) {
        self.add_rect_corners(min, max, color, rounding, DrawFlags::NONE, thickness);
    }

    /// [`Self::add_rect`] rounding only the corners in `corners` (`DrawFlags::ROUND_TOP`...).
    pub fn add_rect_corners(
        self,
        min: Vec2,
        max: Vec2,
        color: Color,
        rounding: f32,
        corners: DrawFlags,
        thickness: f32,
    ) {
        unsafe {
            sys::ImDrawList_AddRect(
                self.raw,
                min.into(),
                max.into(),
                color.to_u32(),
                rounding,
                thickness,
                raw_flags(corners),
            );
        }
    }

    pub fn add_rect_filled(self, min: Vec2, max: Vec2, color: Color, rounding: f32) {
        self.add_rect_filled_corners(min, max, color, rounding, DrawFlags::NONE);
    }

    /// [`Self::add_rect_filled`] rounding only the corners in `corners`.
    pub fn add_rect_filled_corners(
        self,
        min: Vec2,
        max: Vec2,
        color: Color,
        rounding: f32,
        corners: DrawFlags,
    ) {
        unsafe {
            sys::ImDrawList_AddRectFilled(
                self.raw,
                min.into(),
                max.into(),
                color.to_u32(),
                rounding,
                raw_flags(corners),
            );
        }
    }

    /// A rectangle with a colour per corner, blended between them.
    pub fn add_rect_filled_multicolor(
        self,
        min: Vec2,
        max: Vec2,
        top_left: Color,
        top_right: Color,
        bottom_right: Color,
        bottom_left: Color,
    ) {
        unsafe {
            sys::ImDrawList_AddRectFilledMultiColor(
                self.raw,
                min.into(),
                max.into(),
                top_left.to_u32(),
                top_right.to_u32(),
                bottom_right.to_u32(),
                bottom_left.to_u32(),
            );
        }
    }

    /// A circle's outline; `segments` 0 lets ImGui choose by radius.
    pub fn add_circle(
        self,
        center: Vec2,
        radius: f32,
        color: Color,
        segments: i32,
        thickness: f32,
    ) {
        unsafe {
            sys::ImDrawList_AddCircle(
                self.raw,
                center.into(),
                radius,
                color.to_u32(),
                segments,
                thickness,
                0,
            );
        }
    }

    pub fn add_circle_filled(self, center: Vec2, radius: f32, color: Color, segments: i32) {
        unsafe {
            sys::ImDrawList_AddCircleFilled(
                self.raw,
                center.into(),
                radius,
                color.to_u32(),
                segments,
            );
        }
    }

    /// An ellipse's outline with the radii `radius`, turned by `rotation` radians.
    pub fn add_ellipse(
        self,
        center: Vec2,
        radius: Vec2,
        color: Color,
        rotation: f32,
        segments: i32,
        thickness: f32,
    ) {
        unsafe {
            sys::ImDrawList_AddEllipse(
                self.raw,
                center.into(),
                radius.into(),
                color.to_u32(),
                rotation,
                segments,
                thickness,
                0,
            );
        }
    }

    pub fn add_triangle(self, a: Vec2, b: Vec2, c: Vec2, color: Color, thickness: f32) {
        unsafe {
            sys::ImDrawList_AddTriangle(
                self.raw,
                a.into(),
                b.into(),
                c.into(),
                color.to_u32(),
                thickness,
                0,
            );
        }
    }

    pub fn add_triangle_filled(self, a: Vec2, b: Vec2, c: Vec2, color: Color) {
        unsafe {
            sys::ImDrawList_AddTriangleFilled(
                self.raw,
                a.into(),
                b.into(),
                c.into(),
                color.to_u32(),
            );
        }
    }

    pub fn add_quad_filled(self, a: Vec2, b: Vec2, c: Vec2, d: Vec2, color: Color) {
        unsafe {
            sys::ImDrawList_AddQuadFilled(
                self.raw,
                a.into(),
                b.into(),
                c.into(),
                d.into(),
                color.to_u32(),
            );
        }
    }

    /// A line through `points`, back to the first one when `closed`.
    pub fn add_polyline(self, points: &[Vec2], color: Color, closed: bool, thickness: f32) {
        let flags = if closed {
            DrawFlags::CLOSED
        } else {
            DrawFlags::NONE
        };
        unsafe {
            sys::ImDrawList_AddPolyline(
                self.raw,
                points.as_ptr().cast(),
                points.len() as i32,
                color.to_u32(),
                thickness,
                raw_flags(flags),
            );
        }
    }

    /// A filled convex polygon (points in clockwise order).
    pub fn add_convex_poly_filled(self, points: &[Vec2], color: Color) {
        unsafe {
            sys::ImDrawList_AddConvexPolyFilled(
                self.raw,
                points.as_ptr().cast(),
                points.len() as i32,
                color.to_u32(),
                0,
            );
        }
    }

    /// A cubic Bézier curve from `p1` to `p4`; `segments` 0 tessellates by the style's tolerance.
    #[allow(clippy::too_many_arguments)]
    pub fn add_bezier_cubic(
        self,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
        p4: Vec2,
        color: Color,
        thickness: f32,
        segments: i32,
    ) {
        unsafe {
            sys::ImDrawList_AddBezierCubic(
                self.raw,
                p1.into(),
                p2.into(),
                p3.into(),
                p4.into(),
                color.to_u32(),
                thickness,
                segments,
                0,
            );
        }
    }

    pub fn path_clear(self) {
        unsafe { sys::ImDrawList_PathClear(self.raw) }
    }

    pub fn path_line_to(self, point: Vec2) {
        unsafe { sys::ImDrawList_PathLineTo(self.raw, point.into()) }
    }

    /// An arc from angle `a_min` to `a_max` (radians, clockwise from +x); `segments` 0 chooses by radius.
    pub fn path_arc_to(self, center: Vec2, radius: f32, a_min: f32, a_max: f32, segments: i32) {
        unsafe {
            sys::ImDrawList_PathArcTo(self.raw, center.into(), radius, a_min, a_max, segments);
        }
    }

    /// An arc between two of the 12 steps of a circle (`PathArcToFast`): 0 is +x, 3 is +y (down).
    pub fn path_arc_to_fast(self, center: Vec2, radius: f32, a_min_of_12: i32, a_max_of_12: i32) {
        unsafe {
            sys::ImDrawList_PathArcToFast(
                self.raw,
                center.into(),
                radius,
                a_min_of_12,
                a_max_of_12,
            );
        }
    }

    pub fn path_bezier_cubic_curve_to(self, p2: Vec2, p3: Vec2, p4: Vec2, segments: i32) {
        unsafe {
            sys::ImDrawList_PathBezierCubicCurveTo(
                self.raw,
                p2.into(),
                p3.into(),
                p4.into(),
                segments,
            );
        }
    }

    pub fn path_bezier_quadratic_curve_to(self, p2: Vec2, p3: Vec2, segments: i32) {
        unsafe {
            sys::ImDrawList_PathBezierQuadraticCurveTo(self.raw, p2.into(), p3.into(), segments);
        }
    }

    /// A rounded rectangle added to the path.
    pub fn path_rect(self, min: Vec2, max: Vec2, rounding: f32, corners: DrawFlags) {
        unsafe {
            sys::ImDrawList_PathRect(
                self.raw,
                min.into(),
                max.into(),
                rounding,
                raw_flags(corners),
            );
        }
    }

    /// Strokes the path and clears it.
    pub fn path_stroke(self, color: Color, closed: bool, thickness: f32) {
        let flags = if closed {
            DrawFlags::CLOSED
        } else {
            DrawFlags::NONE
        };
        unsafe {
            sys::ImDrawList_PathStroke(self.raw, color.to_u32(), thickness, raw_flags(flags));
        }
    }

    /// Fills the (convex) path and clears it.
    pub fn path_fill_convex(self, color: Color) {
        unsafe { sys::ImDrawList_PathFillConvex(self.raw, color.to_u32(), 0) }
    }

    /// Text in `font` at `font_px` device pixels (`Cx::font_px`), its top-left corner at `pos`. ImGui truncates
    /// `pos` to whole pixels: `Cx::draw_text` keeps the fraction.
    pub fn add_text(self, font: Font, font_px: f32, pos: Vec2, color: Color, text: &str) {
        let (begin, end) = text_range(text);
        unsafe {
            sys::ImDrawList_AddText_FontPtr(
                self.raw,
                font.raw(),
                font_px,
                pos.into(),
                color.to_u32(),
                begin,
                end,
                0.0,
                std::ptr::null(),
            );
        }
    }

    /// Text in the current font at its current size.
    pub fn add_text_current_font(self, pos: Vec2, color: Color, text: &str) {
        let (begin, end) = text_range(text);
        unsafe { sys::ImDrawList_AddText_Vec2(self.raw, pos.into(), color.to_u32(), begin, end) }
    }

    /// A texture stretched over `[min, max]`, tinted by `color`.
    pub fn add_image(
        self,
        texture: TextureId,
        min: Vec2,
        max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        color: Color,
    ) {
        unsafe {
            sys::ImDrawList_AddImage(
                self.raw,
                texture_ref(texture),
                min.into(),
                max.into(),
                uv_min.into(),
                uv_max.into(),
                color.to_u32(),
            );
        }
    }

    /// [`Self::add_image`] with rounded corners.
    #[allow(clippy::too_many_arguments)]
    pub fn add_image_rounded(
        self,
        texture: TextureId,
        min: Vec2,
        max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        color: Color,
        rounding: f32,
    ) {
        unsafe {
            sys::ImDrawList_AddImageRounded(
                self.raw,
                texture_ref(texture),
                min.into(),
                max.into(),
                uv_min.into(),
                uv_max.into(),
                color.to_u32(),
                rounding,
                0,
            );
        }
    }

    /// Clips what follows to `[min, max)` (within the current clip rectangle when `intersect`) until
    /// [`Self::pop_clip_rect`].
    pub fn push_clip_rect(self, min: Vec2, max: Vec2, intersect: bool) {
        unsafe { sys::ImDrawList_PushClipRect(self.raw, min.into(), max.into(), intersect) }
    }

    pub fn pop_clip_rect(self) {
        unsafe { sys::ImDrawList_PopClipRect(self.raw) }
    }

    /// `ChannelsSplit(count)`: the following draw calls go to the channel set with [`Self::channels_set_current`],
    /// merged back in channel order by [`Self::channels_merge`] (a background drawn after the content, say).
    pub fn channels_split(self, count: i32) {
        unsafe { sys::ImDrawList_ChannelsSplit(self.raw, count) }
    }

    pub fn channels_set_current(self, channel: i32) {
        unsafe { sys::ImDrawList_ChannelsSetCurrent(self.raw, channel) }
    }

    pub fn channels_merge(self) {
        unsafe { sys::ImDrawList_ChannelsMerge(self.raw) }
    }

    /// `PushClipRectFullScreen()`: draws outside the window's own clip rectangle (shadows, popups) until
    /// [`Self::pop_clip_rect`].
    pub fn push_clip_rect_full_screen(self) {
        self.push_clip_rect(vec2(-8192.0, -8192.0), vec2(8192.0, 8192.0), false);
    }

    /// Runs `f` with the list clipped to `[min, max)`.
    pub fn with_clip_rect<R>(
        self,
        min: Vec2,
        max: Vec2,
        intersect: bool,
        f: impl FnOnce() -> R,
    ) -> R {
        self.push_clip_rect(min, max, intersect);
        let result = f();
        self.pop_clip_rect();
        result
    }

    /// The clip rectangle in effect.
    #[must_use]
    pub fn clip_rect(self) -> Rect {
        unsafe {
            Rect::new(
                sys::ImDrawList_GetClipRectMin(self.raw).into(),
                sys::ImDrawList_GetClipRectMax(self.raw).into(),
            )
        }
    }

    /// The number of vertices written so far: the mark [`Self::shift_vertices_x`] takes.
    #[must_use]
    pub fn vertex_count(self) -> usize {
        unsafe { (*self.raw).VtxBuffer.Size.max(0) as usize }
    }

    /// Moves every vertex written since `first` (a [`Self::vertex_count`]) by `dx` pixels: how text gets a
    /// fractional position after ImGui truncated it.
    pub fn shift_vertices_x(self, first: usize, dx: f32) {
        unsafe {
            let buffer = &(*self.raw).VtxBuffer;
            let count = buffer.Size.max(0) as usize;
            if buffer.Data.is_null() || first >= count {
                return;
            }
            // The vertex buffer is ImGui's own allocation of `Size` initialised vertices; nothing else holds
            // a reference into it during this call.
            for vertex in &mut std::slice::from_raw_parts_mut(buffer.Data, count)[first..] {
                vertex.pos.x += dx;
            }
        }
    }

    /// Chromium draws text in composited layers (scrolling ones, faded ones) with grey-scale anti-aliasing:
    /// switches the rest of this list's text to grey-scale, `on = false` back to sub-pixel. A no-op without
    /// the sub-pixel renderer.
    pub fn set_grayscale_text(self, on: bool) {
        #[cfg(feature = "lcd-text")]
        // SAFETY: `self.raw` is a live draw list (see the impl's note).
        unsafe {
            crate::gfx::lcd_renderer::set_grayscale_text_raw(self.raw, on);
        }
        #[cfg(not(feature = "lcd-text"))]
        let _ = on;
    }
}
