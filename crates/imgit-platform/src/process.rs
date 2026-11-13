//! Child processes: run-and-capture (the git executable) and detached launches (relaunch).

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// What a finished process left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    /// The exit code; -1 when the process was ended by a signal.
    pub exit_code: i32,
    /// stdout + stderr, in order, lossy UTF-8.
    pub text: String,
}

/// The optional parts of [`run_process`].
#[derive(Default)]
pub struct RunOptions<'a> {
    /// Written to stdin, which is then closed.
    pub input: &'a [u8],
    /// Variables added to (or replacing) the inherited environment.
    pub environment: &'a [(String, String)],
    /// Called on the calling thread with each chunk of output as it arrives (git's progress lines). A chunk never
    /// splits a UTF-8 sequence.
    pub on_output: Option<&'a mut dyn FnMut(&str)>,
}

/// Serialises process creation. On Windows a child inherits every handle that is inheritable at the moment it
/// is created, and std makes the pipe ends inheritable just for the spawn: without the lock a child started
/// meanwhile from another thread (auto-fetch next to a user's fetch) could take a copy of this child's output
/// pipe, and the read loop would not see EOF until that other child's tree has exited. (On POSIX every pipe end
/// is close-on-exec, so the lock is only a formality there.)
static SPAWN_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn spawn_lock() -> MutexGuard<'static, ()> {
    SPAWN_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

fn no_program() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "no program to run")
}

/// Runs `args` (`args[0]` looked up on PATH, passed verbatim, no shell) in `workdir` without a console window
/// and waits for it to finish. An empty `workdir` inherits the current directory.
///
/// The input is fed from a helper thread while the output is drained here: writing it all first deadlocks on a
/// child that fills the output pipe before it has read its input, and a child that exits early simply leaves
/// the rest unread.
///
/// # Errors
/// The process could not be started.
pub fn run_process(
    workdir: &Path,
    args: &[String],
    options: RunOptions<'_>,
) -> io::Result<ProcessOutput> {
    let RunOptions {
        input,
        environment,
        mut on_output,
    } = options;
    let (program, rest) = args.split_first().ok_or_else(no_program)?;

    // One pipe for both streams keeps stdout and stderr in the order they were written.
    let (mut reader, writer) = io::pipe()?;
    let mut command = Command::new(program);
    command
        .args(rest)
        .envs(environment.iter().map(|(name, value)| (name, value)))
        .stdin(if input.is_empty() {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(writer.try_clone()?)
        .stderr(writer);
    if !workdir.as_os_str().is_empty() {
        command.current_dir(workdir);
    }
    hide_console(&mut command);

    let mut child = {
        let _guard = spawn_lock();
        command.spawn()?
    };
    // The command holds this side's copies of the pipe's write end: EOF only comes once they are closed.
    drop(command);
    let stdin = child.stdin.take();

    let mut bytes = Vec::new();
    std::thread::scope(|scope| {
        if let Some(mut stdin) = stdin {
            scope.spawn(move || {
                // A child that exits before reading everything makes this fail (EPIPE), which is fine.
                let _ = stdin.write_all(input);
            });
        }
        let mut emitted = 0;
        let mut buffer = [0u8; 4096];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(sink) = on_output.as_mut() {
                let end = bytes.len() - incomplete_suffix(&bytes[emitted..]);
                if end > emitted {
                    sink(&String::from_utf8_lossy(&bytes[emitted..end]));
                    emitted = end;
                }
            }
        }
        if let Some(sink) = on_output.as_mut()
            && emitted < bytes.len()
        {
            sink(&String::from_utf8_lossy(&bytes[emitted..]));
        }
    });

    let status = child.wait()?;
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
    Ok(ProcessOutput {
        exit_code: status.code().unwrap_or(-1),
        text,
    })
}

/// How many bytes at the end of `bytes` start a UTF-8 sequence that the rest of the output has yet to complete.
fn incomplete_suffix(bytes: &[u8]) -> usize {
    for back in 1..=bytes.len().min(3) {
        let byte = bytes[bytes.len() - back];
        if byte & 0xC0 == 0x80 {
            continue; // a continuation byte: its lead byte is further back
        }
        let length = match byte {
            0xF0.. => 4,
            0xE0.. => 3,
            0xC0.. => 2,
            _ => 1,
        };
        return if length > back { back } else { 0 };
    }
    0
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

/// Starts `args` (`args[0]` looked up on PATH) without waiting for it, in `workdir` or the current directory.
/// On Windows it gets a console of its own.
///
/// # Errors
/// The process could not be started.
pub fn spawn_detached(args: &[String], workdir: Option<&Path>) -> io::Result<()> {
    imp::spawn_detached(args, workdir)
}

/// The running executable's path (empty when the system does not tell).
#[must_use]
pub fn executable_path() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

#[cfg(windows)]
pub(crate) mod imp {
    use super::{no_program, spawn_lock};
    use std::ffi::OsStr;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_CONSOLE, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
    };

    /// `text` as a NUL-terminated UTF-16 string.
    pub(crate) fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
        text.as_ref()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// One argument quoted the way `CommandLineToArgvW` (and the MSVC CRT) splits it back: backslashes are
    /// literal unless they precede a quote.
    pub(crate) fn quote_argument(arg: &str) -> String {
        if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0B', '"']) {
            return arg.to_owned();
        }
        let mut out = String::from("\"");
        let mut backslashes = 0;
        for c in arg.chars() {
            if c == '\\' {
                backslashes += 1;
                continue;
            }
            let escapes = if c == '"' {
                backslashes * 2 + 1
            } else {
                backslashes
            };
            out.extend(std::iter::repeat_n('\\', escapes));
            backslashes = 0;
            out.push(c);
        }
        out.extend(std::iter::repeat_n('\\', backslashes * 2));
        out.push('"');
        out
    }

    pub(crate) fn command_line(args: &[String]) -> String {
        args.iter()
            .map(|arg| quote_argument(arg))
            .collect::<Vec<_>>()
            .join(" ")
    }

    // CreateProcessW by hand rather than std's Command: the child must get the standard handles of its own new
    // console, where Command would hand it (copies of) ours.
    pub(super) fn spawn_detached(args: &[String], workdir: Option<&Path>) -> io::Result<()> {
        if args.is_empty() {
            return Err(no_program());
        }
        let mut command = wide(command_line(args));
        let directory = workdir
            .filter(|path| !path.as_os_str().is_empty())
            .map(wide);
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process = PROCESS_INFORMATION::default();
        let _guard = spawn_lock();
        // SAFETY: `command` is a mutable NUL-terminated buffer and `directory` a NUL-terminated string (or
        // null), both alive across the call; `startup` and `process` are valid for their types; the other
        // pointers may be null.
        let created = unsafe {
            CreateProcessW(
                std::ptr::null(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NEW_CONSOLE,
                std::ptr::null(),
                directory
                    .as_ref()
                    .map_or(std::ptr::null(), |directory| directory.as_ptr()),
                &startup,
                &mut process,
            )
        };
        if created == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateProcessW succeeded, so both handles are valid and owned by us.
        unsafe {
            CloseHandle(process.hThread);
            CloseHandle(process.hProcess);
        }
        Ok(())
    }
}

#[cfg(unix)]
pub(crate) mod imp {
    use super::no_program;
    use std::ffi::OsStr;
    use std::io;
    use std::path::Path;
    use std::process::Command;

    pub(super) fn spawn_detached(args: &[String], workdir: Option<&Path>) -> io::Result<()> {
        spawn_detached_os(args, workdir)
    }

    /// [`super::spawn_detached`] for arguments that need not be UTF-8 (paths).
    pub(crate) fn spawn_detached_os<S: AsRef<OsStr>>(
        args: &[S],
        workdir: Option<&Path>,
    ) -> io::Result<()> {
        let (program, rest) = args.split_first().ok_or_else(no_program)?;
        let mut command = Command::new(program);
        command.args(rest);
        if let Some(workdir) = workdir.filter(|path| !path.as_os_str().is_empty()) {
            command.current_dir(workdir);
        }
        let mut child = command.spawn()?;
        // Reap it when it ends so it does not stay a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|&arg| arg.to_owned()).collect()
    }

    /// A shell one-liner: `cmd /c` on Windows, `sh -c` elsewhere.
    fn shell(script: &str) -> Vec<String> {
        if cfg!(windows) {
            strings(&["cmd.exe", "/d", "/c", script])
        } else {
            strings(&["sh", "-c", script])
        }
    }

    fn run(args: &[String]) -> ProcessOutput {
        run_process(Path::new(""), args, RunOptions::default()).expect("the process starts")
    }

    #[test]
    fn reports_the_exit_code() {
        assert_eq!(run(&shell("exit 0")).exit_code, 0);
        assert_eq!(run(&shell("exit 7")).exit_code, 7);
    }

    #[test]
    fn fails_for_a_missing_program_or_no_program() {
        let missing = strings(&["imgit-no-such-program-4f2a"]);
        assert!(run_process(Path::new(""), &missing, RunOptions::default()).is_err());
        assert!(run_process(Path::new(""), &[], RunOptions::default()).is_err());
        assert!(spawn_detached(&[], None).is_err());
        assert!(spawn_detached(&missing, None).is_err());
    }

    #[test]
    fn merges_stdout_and_stderr_in_order() {
        let output = run(&shell("echo one&& echo two 1>&2&& echo three"));
        let lines: Vec<&str> = output.text.lines().map(str::trim).collect();
        assert_eq!(lines, ["one", "two", "three"]);
    }

    #[test]
    fn feeds_stdin_and_closes_it() {
        let args = if cfg!(windows) {
            strings(&["sort.exe"])
        } else {
            strings(&["sort"])
        };
        let options = RunOptions {
            input: b"pear\napple\nmango\n",
            ..RunOptions::default()
        };
        let output = run_process(Path::new(""), &args, options).unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(
            output.text.lines().collect::<Vec<_>>(),
            ["apple", "mango", "pear"]
        );
    }

    #[test]
    fn feeds_more_input_than_a_pipe_holds() {
        // The child echoes while it reads: without the helper thread both sides would block on a full pipe.
        let line = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde\n";
        let input = line.repeat(8192); // 512 KiB
        let args = if cfg!(windows) {
            strings(&["findstr.exe", "^"])
        } else {
            strings(&["cat"])
        };
        let options = RunOptions {
            input: input.as_bytes(),
            ..RunOptions::default()
        };
        let output = run_process(Path::new(""), &args, options).unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.text.lines().count(), 8192);
        assert!(output.text.lines().all(|read| read == line.trim_end()));
    }

    #[test]
    fn a_child_that_ignores_its_input_is_no_error() {
        let input = vec![b'x'; 1 << 20];
        let options = RunOptions {
            input: &input,
            ..RunOptions::default()
        };
        let output = run_process(Path::new(""), &shell("exit 3"), options).unwrap();
        assert_eq!(output.exit_code, 3);
    }

    #[test]
    fn applies_the_environment() {
        let environment = [
            (
                "IMGIT_TEST_VALUE".to_owned(),
                "a value with spaces".to_owned(),
            ),
            // Replaces an inherited variable (names are case-insensitive on Windows).
            ("PATHEXT".to_owned(), ".EXE".to_owned()),
        ];
        let script = if cfg!(windows) {
            "echo [%IMGIT_TEST_VALUE%][%PATHEXT%]"
        } else {
            "echo [$IMGIT_TEST_VALUE][$PATHEXT]"
        };
        let options = RunOptions {
            environment: &environment,
            ..RunOptions::default()
        };
        let output = run_process(Path::new(""), &shell(script), options).unwrap();
        assert_eq!(output.text.trim(), "[a value with spaces][.EXE]");
        // The rest of the environment is inherited.
        let script = if cfg!(windows) {
            "echo [%PATH%]"
        } else {
            "echo [$PATH]"
        };
        assert!(run(&shell(script)).text.trim().len() > 2);
    }

    #[test]
    fn runs_in_the_work_directory() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("marker.txt"), "found").unwrap();
        let script = if cfg!(windows) {
            "type marker.txt"
        } else {
            "cat marker.txt"
        };
        let output = run_process(folder.path(), &shell(script), RunOptions::default()).unwrap();
        assert_eq!((output.exit_code, output.text.as_str()), (0, "found"));
    }

    /// Arguments reach the child verbatim: this test binary is run again as the child and prints its own.
    #[test]
    fn passes_arguments_verbatim() {
        const MARKER: &str = "IMGIT_PRINT_ARGUMENTS";
        if std::env::var_os(MARKER).is_some() {
            let args: Vec<String> = std::env::args()
                .skip_while(|arg| arg != "--")
                .skip(1)
                .collect();
            print!("<<{}>>", args.join("\u{1f}"));
            return;
        }
        let tricky = [
            "plain",
            "with space",
            "",
            "quo\"te",
            "trailing\\",
            "back\\slash and space\\",
            "\\\"both\\\\\"",
            "%PATH% $HOME `x` ^ & | < >",
            "tab\there",
            "ünïcödé ✓",
        ];
        let mut args = vec![
            executable_path().to_str().unwrap().to_owned(),
            "--exact".to_owned(),
            "process::tests::passes_arguments_verbatim".to_owned(),
            "--nocapture".to_owned(),
            "--".to_owned(),
        ];
        args.extend(tricky.iter().map(|&arg| arg.to_owned()));
        let environment = [(MARKER.to_owned(), "1".to_owned())];
        let options = RunOptions {
            environment: &environment,
            ..RunOptions::default()
        };
        let output = run_process(Path::new(""), &args, options).unwrap();
        assert_eq!(output.exit_code, 0, "{}", output.text);
        let start = output
            .text
            .find("<<")
            .expect("the child printed its arguments")
            + 2;
        let end = output.text.rfind(">>").unwrap();
        let received: Vec<&str> = output.text[start..end].split('\u{1f}').collect();
        assert_eq!(received, tricky);
    }

    #[test]
    fn streams_output_without_splitting_utf8() {
        let mut chunks = Vec::new();
        let mut sink = |chunk: &str| chunks.push(chunk.to_owned());
        let options = RunOptions {
            on_output: Some(&mut sink),
            ..RunOptions::default()
        };
        let output =
            run_process(Path::new(""), &shell("echo first&& echo second"), options).unwrap();
        assert!(!chunks.is_empty());
        assert_eq!(chunks.concat(), output.text);
    }

    #[test]
    fn finds_incomplete_utf8_suffixes() {
        assert_eq!(incomplete_suffix(b""), 0);
        assert_eq!(incomplete_suffix(b"abc"), 0);
        assert_eq!(incomplete_suffix("é".as_bytes()), 0);
        assert_eq!(incomplete_suffix(&"é".as_bytes()[..1]), 1);
        assert_eq!(incomplete_suffix("a✓".as_bytes()), 0);
        assert_eq!(incomplete_suffix(&"a✓".as_bytes()[..3]), 2);
        assert_eq!(incomplete_suffix(&"a✓".as_bytes()[..2]), 1);
        assert_eq!(incomplete_suffix("😀".as_bytes()), 0);
        assert_eq!(incomplete_suffix(&"😀".as_bytes()[..3]), 3);
        // Stray continuation bytes are not waited for.
        assert_eq!(incomplete_suffix(&[0x80, 0x80, 0x80, 0x80]), 0);
    }

    #[cfg(windows)]
    #[test]
    fn quotes_arguments_for_command_line_to_argv() {
        use imp::{command_line, quote_argument};
        assert_eq!(quote_argument("plain"), "plain");
        assert_eq!(quote_argument(""), "\"\"");
        assert_eq!(quote_argument("a b"), "\"a b\"");
        assert_eq!(quote_argument("C:\\dir\\"), "C:\\dir\\");
        assert_eq!(quote_argument("C:\\my dir\\"), "\"C:\\my dir\\\\\"");
        assert_eq!(quote_argument("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quote_argument("a\\\"b"), "\"a\\\\\\\"b\"");
        assert_eq!(
            command_line(&strings(&["git", "commit", "-m", "two words"])),
            "git commit -m \"two words\""
        );
    }

    #[test]
    fn knows_its_executable() {
        assert!(executable_path().is_file());
    }
}
