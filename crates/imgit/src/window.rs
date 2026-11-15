//! Platform touches on the main window: its frame, its cursors and the scale of the UI's logical pixels.
//!
//! The portable part lives here; `window/windows.rs` holds the Win32 calls, `window/portable.rs` what winit
//! alone can do elsewhere, and `window/macos.rs` the unified macOS title bar.
//!
//! The C++ `platform::native_window()` has no counterpart: winit's `Window` implements
//! `raw_window_handle::HasWindowHandle`, which is what system dialogs take as their parent
//! (see [`ShellContext::window`](crate::shell::ShellContext::window)).

use std::sync::Arc;

use winit::window::{Theme, Window};

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(not(windows))]
mod portable;
#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
use portable as imp;
#[cfg(windows)]
use windows as imp;

/// A cursor the UI shows instead of ImGui's own while it asks for it each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorShape {
    /// Drag-and-drop: drop allowed.
    Move,
    /// Drag-and-drop: no drop here.
    NoDrop,
    /// A vertical splitter.
    ColResize,
    /// A horizontal splitter.
    RowResize,
    /// The open hand over something that can be dragged (CSS `grab`).
    Grab,
}

/// The factor the UI's logical (CSS) pixels are multiplied by for `window`: the monitor's content scale where
/// window coordinates are physical pixels (Windows, X11), 1 where they are points and the framebuffer alone is
/// scaled (macOS: ImGui rasterises the fonts at the framebuffer's density).
#[must_use]
pub fn layout_scale(window: &Window) -> f32 {
    if cfg!(target_os = "macos") {
        return 1.0;
    }
    let scale = window.scale_factor() as f32;
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// The main window's frame and custom cursors.
pub struct WindowChrome {
    window: Arc<Window>,
    dark: bool,
    imp: imp::Chrome,
    /// Whether one of our cursors is up (so [`Self::reset_cursor`] has something to undo).
    custom_cursor: bool,
    #[cfg(target_os = "macos")]
    drag_region: ([f32; 2], [f32; 2]),
}

impl WindowChrome {
    /// Called once the main window exists: gives it the dark caption (Windows) or appearance (macOS) until
    /// [`Self::set_dark`] says otherwise, and hooks up the custom cursors.
    #[must_use]
    pub fn new(window: Arc<Window>) -> Self {
        let imp = imp::Chrome::new(&window);
        let mut chrome = Self {
            window,
            dark: true,
            imp,
            custom_cursor: false,
            #[cfg(target_os = "macos")]
            drag_region: ([0.0; 2], [0.0; 2]),
        };
        chrome.apply_frame();
        chrome
    }

    /// The window frame follows the theme: Windows' dark or light caption (DWM immersive dark mode), macOS'
    /// dark or light appearance; the window manager's frame elsewhere is left alone.
    pub fn set_dark(&mut self, dark: bool) {
        if dark != self.dark {
            self.dark = dark;
            self.apply_frame();
            self.imp.frame_changed();
        }
    }

    fn apply_frame(&mut self) {
        // Keeps winit's idea of the theme in step, so it does not put the system's caption back when the
        // system setting changes.
        self.window
            .set_theme(Some(if self.dark { Theme::Dark } else { Theme::Light }));
        self.imp.apply_frame(self.dark);
    }

    /// Shows `shape` over the window instead of ImGui's cursor until [`Self::reset_cursor`]: the system
    /// drag-and-drop cursors, and on Windows Chromium's own resize cursors (inverting masks an RGBA cursor
    /// cannot show).
    pub fn set_cursor(&mut self, shape: CursorShape) {
        self.imp.set_cursor(&self.window, shape);
        self.custom_cursor = true;
    }

    /// Hands the cursor back to the platform backend (the one ImGui last asked for).
    pub fn reset_cursor(&mut self) {
        if self.custom_cursor {
            self.custom_cursor = false;
            self.imp.reset_cursor(&self.window);
        }
    }

    /// Logical pixels the tabs bar leaves free at its left for the window's own controls: the traffic lights on
    /// macOS, where the system title bar is hidden and the tabs bar stands in for it; 0 elsewhere.
    #[must_use]
    pub fn title_bar_inset(&self) -> f32 {
        #[cfg(target_os = "macos")]
        {
            macos::title_bar_inset()
        }
        #[cfg(not(target_os = "macos"))]
        {
            0.0
        }
    }

    /// Starts a native window drag only in the unused portion of the tabs bar.
    #[cfg(target_os = "macos")]
    pub fn drag_title_bar(&self, position: [f32; 2]) {
        let (min, max) = self.drag_region;
        if position[0] >= min[0]
            && position[0] < max[0]
            && position[1] >= min[1]
            && position[1] < max[1]
        {
            let _ = self.window.drag_window();
        }
    }

    /// Where the tabs bar is the title bar: the part of it (window coordinates, logical px) that acts as one,
    /// moving the window when dragged and zooming or minimising on a double-click, as the system is set. Set
    /// every frame, empty (max <= min) when there is none. A no-op off macOS.
    pub fn set_title_bar_drag_region(&mut self, min: [f32; 2], max: [f32; 2]) {
        #[cfg(target_os = "macos")]
        {
            self.drag_region = (min, max);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (min, max);
    }
}
