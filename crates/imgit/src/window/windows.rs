//! Windows: the dark caption (DWM), Chromium's resize cursors through a `WM_SETCURSOR` hook, ole32's
//! drag-and-drop cursors.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;

use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use windows_sys::Win32::Foundation::{FreeLibrary, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows_sys::Win32::System::LibraryLoader::{LOAD_LIBRARY_AS_DATAFILE, LoadLibraryExW};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, CreateIconIndirect, GetSystemMetrics, HCURSOR, HTCLIENT, ICONINFO,
    IDC_ARROW, IDC_NO, IDC_SIZENS, IDC_SIZEWE, IMAGE_CURSOR, LR_DEFAULTCOLOR, LR_DEFAULTSIZE,
    LoadCursorW, LoadImageW, SM_CXCURSOR, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SendMessageW, SetCursor, SetWindowPos, WM_MOUSEMOVE, WM_SETCURSOR,
};
use windows_sys::core::{PCWSTR, w};
use winit::window::Window;

use super::CursorShape;
use crate::assets;

/// Identifies our subclass among the window's ("IMGT").
const SUBCLASS_ID: usize = 0x494D_4754;

/// The cursor `WM_SETCURSOR` shows instead of winit's while it is non-null: winit's own cursors cannot be
/// swapped from outside its event handling without it putting its cursor back on the next mouse move.
type CursorSlot = Cell<HCURSOR>;

pub(super) struct Chrome {
    /// Null when the window has no Win32 handle (never the case on Windows; everything is a no-op then).
    hwnd: HWND,
    /// Shared with [`cursor_subclass_proc`] through the subclass' reference data.
    shown: Box<CursorSlot>,
    cursors: Cursors,
}

/// The custom cursors, loaded on first use. They live as long as the process.
#[derive(Default)]
struct Cursors {
    col_resize: Option<HCURSOR>,
    row_resize: Option<HCURSOR>,
    move_: Option<HCURSOR>,
    no_drop: Option<HCURSOR>,
    grab: Option<HCURSOR>,
}

impl Chrome {
    pub(super) fn new(window: &Window) -> Self {
        let hwnd = match window.window_handle().map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get() as HWND,
            _ => ptr::null_mut(),
        };
        let shown = Box::new(Cell::new(ptr::null_mut()));
        if !hwnd.is_null() {
            // SAFETY: `hwnd` is the live main window, created on this thread. The reference data points into
            // the box `Chrome` owns, and `Drop` removes the subclass before the box is freed.
            unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(cursor_subclass_proc),
                    SUBCLASS_ID,
                    ptr::from_ref::<CursorSlot>(&shown) as usize,
                );
            }
        }
        Self {
            hwnd,
            shown,
            cursors: Cursors::default(),
        }
    }

    /// The window frame is the Windows caption in the theme's shade.
    pub(super) fn apply_frame(&mut self, dark: bool) {
        const USE_IMMERSIVE_DARK_MODE: u32 = 20;
        if self.hwnd.is_null() {
            return;
        }
        let value: i32 = dark.into();
        // SAFETY: `hwnd` is a live window; the attribute is a BOOL of the size passed.
        unsafe {
            DwmSetWindowAttribute(
                self.hwnd,
                USE_IMMERSIVE_DARK_MODE,
                ptr::from_ref(&value).cast::<c_void>(),
                size_of::<i32>() as u32,
            );
        }
    }

    /// Windows 10 repaints the caption only once the frame changes.
    pub(super) fn frame_changed(&mut self) {
        if self.hwnd.is_null() {
            return;
        }
        // SAFETY: `hwnd` is a live window; the flags leave its position, size and z-order alone.
        unsafe {
            SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    pub(super) fn set_cursor(&mut self, _window: &Window, shape: CursorShape) {
        let cursors = &mut self.cursors;
        let cursor = match shape {
            CursorShape::ColResize => *cursors
                .col_resize
                .get_or_insert_with(|| chromium_cursor("cursors/col_resize.cur", IDC_SIZEWE)),
            CursorShape::RowResize => *cursors
                .row_resize
                .get_or_insert_with(|| chromium_cursor("cursors/row_resize.cur", IDC_SIZENS)),
            CursorShape::Move => *cursors
                .move_
                .get_or_insert_with(|| drag_drop_cursor(3, IDC_ARROW)),
            CursorShape::NoDrop => *cursors
                .no_drop
                .get_or_insert_with(|| drag_drop_cursor(1, IDC_NO)),
            CursorShape::Grab => *cursors.grab.get_or_insert_with(grab_cursor),
        };
        if self.shown.replace(cursor) != cursor {
            // SAFETY: `cursor` is a live cursor handle (never destroyed) or null, which SetCursor accepts.
            unsafe { SetCursor(cursor) };
        }
    }

    pub(super) fn reset_cursor(&mut self, _window: &Window) {
        if self.shown.replace(ptr::null_mut()).is_null() || self.hwnd.is_null() {
            return;
        }
        // Let winit put its cursor (the one ImGui last asked for) back.
        // SAFETY: `hwnd` is a live window of this thread; the message is the one the system sends on a mouse
        // move over the client area.
        unsafe {
            SendMessageW(
                self.hwnd,
                WM_SETCURSOR,
                self.hwnd as WPARAM,
                (HTCLIENT | (WM_MOUSEMOVE << 16)) as LPARAM,
            );
        }
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        if !self.hwnd.is_null() {
            // SAFETY: removes exactly the subclass `new` installed; harmless when the window is already gone.
            unsafe { RemoveWindowSubclass(self.hwnd, Some(cursor_subclass_proc), SUBCLASS_ID) };
        }
    }
}

/// Answers `WM_SETCURSOR` over the client area with the custom cursor while one is set.
unsafe extern "system" fn cursor_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    reference_data: usize,
) -> LRESULT {
    if message == WM_SETCURSOR && (lparam as u32 & 0xFFFF) == HTCLIENT {
        // SAFETY: the reference data is the `CursorSlot` owned by the `Chrome` that installed this subclass
        // and removes it before dropping the slot; window procedures run on the window's (the UI) thread.
        let cursor = unsafe { (*(reference_data as *const CursorSlot)).get() };
        if !cursor.is_null() {
            // SAFETY: a live cursor handle.
            unsafe { SetCursor(cursor) };
            return 1;
        }
    }
    // SAFETY: forwards the message, unchanged, to the next procedure in the subclass chain.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

/// The system cursor size (32 at 100 %).
fn system_cursor_size() -> i32 {
    // SAFETY: no preconditions.
    unsafe { GetSystemMetrics(SM_CXCURSOR) }
}

fn system_cursor(id: PCWSTR) -> HCURSOR {
    // SAFETY: `id` is one of the IDC_* resource identifiers of the system cursors.
    unsafe { LoadCursorW(ptr::null_mut(), id) }
}

/// The cursor resource for the first image of a `.cur` file: `CreateIconFromResourceEx` takes the entry's
/// hotspot (two little-endian WORDs) followed by its DIB.
fn cursor_resource(file: &[u8]) -> Option<Vec<u8>> {
    let dword =
        |at: usize| Some(u32::from_le_bytes(file.get(at..at + 4)?.try_into().ok()?) as usize);
    // ICONDIR (6 bytes) + the first ICONDIRENTRY: hotspot at 10, byte count at 14, offset at 18.
    let bytes = dword(14)?;
    let offset = dword(18)?;
    let image = file.get(offset..offset.checked_add(bytes)?)?;
    let mut resource = Vec::with_capacity(4 + bytes);
    resource.extend_from_slice(file.get(10..14)?);
    resource.extend_from_slice(image);
    Some(resource)
}

/// One of Chromium's cursors (assets/cursors: `col_resize.cur`, `row_resize.cur`, shown by the resize handles),
/// stretched to the system cursor size as `LoadCursor` does. `fallback` is the system cursor used when the
/// asset cannot be loaded.
fn chromium_cursor(asset: &str, fallback: PCWSTR) -> HCURSOR {
    if let Some(resource) = assets::data(asset).and_then(cursor_resource) {
        let size = system_cursor_size();
        // SAFETY: `resource` is a complete cursor resource of the length passed; 0x00030000 is the only
        // resource version the function accepts.
        let cursor = unsafe {
            CreateIconFromResourceEx(
                resource.as_ptr(),
                resource.len() as u32,
                0,
                0x0003_0000,
                size,
                size,
                LR_DEFAULTCOLOR,
            )
        };
        if !cursor.is_null() {
            return cursor;
        }
    }
    system_cursor(fallback)
}

/// The system drag-and-drop cursor `id` (ole32.dll's resources: 1 no drop, 3 move), the ones a drag between
/// Windows applications shows. Unlike the C++ app, which had to redraw it into an RGBA image for GLFW, the
/// cursor is used as it is.
fn drag_drop_cursor(id: u16, fallback: PCWSTR) -> HCURSOR {
    // SAFETY: loads ole32 as a data file only (no code runs), reads one cursor resource by its integer id
    // (MAKEINTRESOURCE) and releases the module; the cursor is a copy that outlives the module.
    let cursor = unsafe {
        let ole = LoadLibraryExW(w!("ole32.dll"), ptr::null_mut(), LOAD_LIBRARY_AS_DATAFILE);
        if ole.is_null() {
            ptr::null_mut()
        } else {
            let cursor = LoadImageW(
                ole,
                id as usize as PCWSTR,
                IMAGE_CURSOR,
                0,
                0,
                LR_DEFAULTSIZE,
            );
            FreeLibrary(ole);
            cursor
        }
    };
    if cursor.is_null() {
        system_cursor(fallback)
    } else {
        cursor
    }
}

/// Chromium's grab cursor: an 18px open hand, black outlined and white filled, its hotspot in the palm.
fn grab_cursor() -> HCURSOR {
    const SIZE: usize = 18;
    const ROWS: [&[u8; SIZE]; SIZE] = [
        b".......XX.XX......",
        b"...XX.XooXooX.XX..",
        b"...XooXooXooXXooX.",
        b"...XooXooXooXooX..",
        b"...XooXooXooXooX..",
        b"...XoooooooooooX..",
        b".XXXoooooooooooX..",
        b"XooXXooooooooooX..",
        b"XoooXooooooooooX..",
        b".XoooooooooooooX..",
        b"..XooooooooooooX..",
        b"..XoooooooooooX...",
        b"...XooooooooooX...",
        b"...XoooooooooX....",
        b"....XoooooooX.....",
        b"....XoooooooX.....",
        b"....XXXXXXXXX.....",
        b"..................",
    ];
    // 32-bit BGRA colour plane; the 1-bit mask (rows padded to 16 bits) is ignored where alpha is present but
    // must exist.
    let mut bgra = [0u8; SIZE * SIZE * 4];
    let art = ROWS.iter().flat_map(|row| row.iter());
    for (pixel, &art) in bgra.as_chunks_mut::<4>().0.iter_mut().zip(art) {
        // Black outline, white fill; transparent texels are all zero (premultiplied).
        *pixel = match art {
            b'X' => [0, 0, 0, 255],
            b'o' => [255, 255, 255, 255],
            _ => [0, 0, 0, 0],
        };
    }
    let mask = [0xFFu8; SIZE * 4];
    // SAFETY: both bitmaps are created from buffers of exactly the size their dimensions need (rows are
    // WORD-aligned: 18 * 4 bytes for the colour plane, 4 bytes for the mask), the icon keeps its own copies,
    // and the bitmaps are deleted afterwards.
    let cursor = unsafe {
        let color = CreateBitmap(SIZE as i32, SIZE as i32, 1, 32, bgra.as_ptr().cast());
        let mask = CreateBitmap(SIZE as i32, SIZE as i32, 1, 1, mask.as_ptr().cast());
        let info = ICONINFO {
            fIcon: 0,
            xHotspot: 9,
            yHotspot: 9,
            hbmMask: mask,
            hbmColor: color,
        };
        let cursor = CreateIconIndirect(&info);
        DeleteObject(color);
        DeleteObject(mask);
        cursor
    };
    if cursor.is_null() {
        system_cursor(IDC_ARROW)
    } else {
        cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_cursor_resource() {
        let file = assets::data("cursors/col_resize.cur").expect("embedded");
        let resource = cursor_resource(file).expect("a well-formed .cur");
        assert_eq!(&resource[..4], &file[10..14]);
        // The DIB starts with a BITMAPINFOHEADER.
        assert_eq!(u32::from_le_bytes(resource[4..8].try_into().unwrap()), 40);
        assert!(cursor_resource(&file[..20]).is_none());
    }

    #[test]
    fn loads_every_custom_cursor() {
        for (asset, fallback) in [
            ("cursors/col_resize.cur", IDC_SIZEWE),
            ("cursors/row_resize.cur", IDC_SIZENS),
        ] {
            let cursor = chromium_cursor(asset, fallback);
            assert!(!cursor.is_null());
            assert_ne!(
                cursor,
                system_cursor(fallback),
                "{asset} fell back to the system cursor"
            );
        }
        assert_ne!(drag_drop_cursor(3, IDC_ARROW), system_cursor(IDC_ARROW));
        assert_ne!(drag_drop_cursor(1, IDC_NO), system_cursor(IDC_NO));
        assert_ne!(grab_cursor(), system_cursor(IDC_ARROW));
    }
}
