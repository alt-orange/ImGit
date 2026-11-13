//! The widgets of the UI, as methods of [`Cx`](crate::ui::Cx): text, pills, the button families, inputs,
//! tooltips, resize handles, the horizontal scrollbar, file status marks, date formatting and transitions.

pub mod anim;
pub mod buttons;
pub mod format;
pub mod inputs;
pub mod resize;
pub mod scrollbar;
pub mod status;
pub mod text;
pub mod tooltip;

// The names the panels use; not all of them have a caller yet.
#[allow(unused_imports)]
pub use anim::{css_ease, ease_in_out};
#[allow(unused_imports)]
pub use buttons::{Pill, SEGMENTED_HEIGHT};
#[allow(unused_imports)]
pub use format::{
    contains_ci, directory_name, file_name, format_relative, fuzzy_score, relative_time,
    shortcut_label, to_upper,
};
#[allow(unused_imports)]
pub use resize::{ResizeAxis, ResizeHandle};
#[allow(unused_imports)]
pub use scrollbar::ScrollbarX;
#[allow(unused_imports)]
pub use text::wrap_lines;
