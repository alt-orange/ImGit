//! macOS unified title bar. Winit keeps the native traffic lights above the tab strip.

/// Space reserved for the three native window buttons, in logical points.
#[must_use]
pub fn title_bar_inset() -> f32 {
    72.0
}
