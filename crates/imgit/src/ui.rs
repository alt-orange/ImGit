//! The layer every panel of the UI is built on: the per-frame context ([`Cx`]), the draw list, the widgets,
//! the icons and the avatars, the menus and popovers, and (in [`panels`]) the regions of the window.
//!
//! # Conventions
//!
//! * Every draw function takes a `&mut Cx` (or is a method of it): there are no globals. A `Frame` builds one
//!   `Cx` per frame from its [`UiState`] (see [`Cx::new`]).
//! * Windows, child windows and popups are opened through `cx.ui` (`let ui = cx.ui;` first, the reference is
//!   `Copy`), and their closures use `cx` freely.
//! * Positions and sizes are [`Vec2`], colours [`crate::theme::Color`], ids and labels `&str`.
//! * Sizes named "logical" are CSS pixels: `cx.px()` turns them into device pixels. Font sizes are CSS font
//!   sizes (the em box): `cx.font_px()` / `cx.push_font()`.
//! * Draw lists are [`DrawList`] handles (`cx.window_draw_list()`), never `dear_imgui_rs::DrawListMut`.

pub mod avatars;
pub mod cx;
pub mod draw;
pub mod geom;
pub mod icons;
pub mod menu;
pub mod panels;
pub mod popover;
pub mod profile_widgets;
pub mod raw;
pub mod widgets;

pub use cx::{Cx, UiState};
#[allow(unused_imports)] // for the panels
pub use draw::DrawList;
#[allow(unused_imports)] // for the panels
pub use geom::{Rect, Vec2, vec2};
pub use icons::Icon;
