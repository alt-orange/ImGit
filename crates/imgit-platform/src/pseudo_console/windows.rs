//! The Windows side of [`super`]: ConPTY (Windows 10 1809+).
//!
//! By hand rather than through `portable-pty`: that crate always creates the console with
//! `PSEUDOCONSOLE_INHERIT_CURSOR` (which holds all output back until the terminal answers a cursor position
//! request), the resize quirk and win32-input-mode, where ImGit's terminal expects a plain one (no flags).

use super::Shared;
use crate::process::imp::{command_line, wide};
use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

/// An open pseudo console, closed on drop.
struct Console(HPCON);

// SAFETY: a pseudo console handle is not tied to the thread that created it; the functions taking it may be
// called from any thread.
unsafe impl Send for Console {}

impl Drop for Console {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreatePseudoConsole and is closed only here.
        unsafe { ClosePseudoConsole(self.0) };
    }
}

pub(super) struct Backend {
    // In drop order: closing the console ends its clients and its output pipe, which ends the reader thread. That
    // thread is left to finish on its own rather than joined, so a console that is slow to close cannot stall
    // the UI.
    console: Console,
    /// Our end of the console's input pipe.
    input: File,
    process: OwnedHandle,
}

fn coord(columns: u16, rows: u16) -> COORD {
    COORD {
        X: columns.min(i16::MAX as u16) as i16,
        Y: rows.min(i16::MAX as u16) as i16,
    }
}

fn check(result: i32) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::other(format!(
            "pseudo console: HRESULT {result:#010x}"
        )))
    } else {
        Ok(())
    }
}

/// An anonymous pipe: (read end, write end), neither inheritable.
fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read: HANDLE = std::ptr::null_mut();
    let mut write: HANDLE = std::ptr::null_mut();
    // SAFETY: both out pointers refer to live handles; the security attributes may be null.
    if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreatePipe succeeded, so both are valid handles that nothing else owns.
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read),
            OwnedHandle::from_raw_handle(write),
        )
    })
}

impl Backend {
    pub(super) fn spawn(
        args: &[String],
        workdir: Option<&Path>,
        columns: u16,
        rows: u16,
        shared: Arc<Shared>,
    ) -> io::Result<Self> {
        let (console_input, input) = pipe()?;
        let (output, console_output) = pipe()?;
        let mut console: HPCON = Default::default();
        // SAFETY: both pipe ends are valid for the call and `console` is a live out value.
        check(unsafe {
            CreatePseudoConsole(
                coord(columns, rows),
                console_input.as_raw_handle(),
                console_output.as_raw_handle(),
                0,
                &mut console,
            )
        })?;
        let console = Console(console);
        // The console holds its own references to the pipe ends it was given.
        drop((console_input, console_output));

        let process = start(&console, args, workdir)?;
        let mut output = File::from(output);
        std::thread::Builder::new()
            .name("pseudo console reader".to_owned())
            .spawn(move || {
                let mut buffer = [0u8; 8192];
                // The read fails (broken pipe) once the console is closed.
                while !shared.stop.load(Ordering::Acquire) {
                    match output.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => shared.pending().extend_from_slice(&buffer[..count]),
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
                shared.drained.store(true, Ordering::Release);
            })?;
        Ok(Self {
            console,
            input: File::from(input),
            process,
        })
    }

    pub(super) fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.input.write_all(bytes)
    }

    pub(super) fn resize(&self, columns: u16, rows: u16) -> io::Result<()> {
        // SAFETY: the console handle is open for as long as `self` lives.
        check(unsafe { ResizePseudoConsole(self.console.0, coord(columns, rows)) })
    }

    /// Whether the shell process has ended.
    pub(super) fn has_ended(&mut self) -> bool {
        // SAFETY: the process handle is open for as long as `self` lives.
        unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) == WAIT_OBJECT_0 }
    }
}

/// Starts `args` attached to `console` and returns the process handle.
fn start(console: &Console, args: &[String], workdir: Option<&Path>) -> io::Result<OwnedHandle> {
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of_val(&startup) as u32;
    // No std handles of ours: the child talks to the pseudo console even when ImGit's own are redirected.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;

    let mut list_size = 0usize;
    // SAFETY: a null list with a size out value is the documented way to ask for the size needed.
    unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut list_size) };
    // usize elements keep the list pointer-aligned.
    let mut list = vec![0usize; list_size.div_ceil(size_of::<usize>())];
    startup.lpAttributeList = list.as_mut_ptr().cast();
    // SAFETY: the buffer is at least `list_size` bytes, as the first call asked for.
    if unsafe { InitializeProcThreadAttributeList(startup.lpAttributeList, 1, 0, &mut list_size) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }

    let mut command = wide(command_line(args));
    let directory = workdir.map(wide);
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: the attribute list was initialised above and outlives the call that uses it; the attribute's value
    // is the console handle itself, which stays open; `command` is a mutable NUL-terminated buffer and
    // `directory` a NUL-terminated string (or null); `startup` (an STARTUPINFOEXW, as the flag says) and
    // `process` are live. The list is deleted exactly once, after its last use.
    let created = unsafe {
        let attached = UpdateProcThreadAttribute(
            startup.lpAttributeList,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            console.0 as *const c_void,
            size_of::<HPCON>(),
            std::ptr::null_mut(),
            std::ptr::null(),
        ) != 0;
        let created = attached
            && CreateProcessW(
                std::ptr::null(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                std::ptr::null(),
                directory
                    .as_ref()
                    .map_or(std::ptr::null(), |directory| directory.as_ptr()),
                &startup.StartupInfo,
                &mut process,
            ) != 0;
        let error = io::Error::last_os_error();
        DeleteProcThreadAttributeList(startup.lpAttributeList);
        if created { Ok(()) } else { Err(error) }
    };
    created?;
    // SAFETY: CreateProcessW succeeded, so both handles are valid and ours; the thread's is not needed.
    unsafe {
        CloseHandle(process.hThread);
        Ok(OwnedHandle::from_raw_handle(process.hProcess))
    }
}
