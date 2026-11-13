//! macOS: the Cocoa side of the main window. Not ported yet.
//!
//! The C++ app hides the system title bar under the content (`titlebarAppearsTransparent`,
//! `NSWindowStyleMaskFullSizeContentView`, an empty compact toolbar for the traffic lights' inset), keeps a
//! transparent drag view over the tabs bar and forces the theme's `NSAppearance`. Here winit's
//! `Window::set_theme` (called by `WindowChrome`) covers the appearance; the rest is a TODO for an `objc2` port.

/// Logical pixels the traffic lights take at the left of the tabs bar. 0 until the title bar is hidden as in
/// the C++ app (72 there).
#[must_use]
pub fn title_bar_inset() -> f32 {
    0.0
}

/// The part of the tabs bar that drags the window. TODO: an `NSView` over that part that starts a window drag.
pub fn set_drag_region(_min: [f32; 2], _max: [f32; 2]) {}
