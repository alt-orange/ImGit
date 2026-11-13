//! Development aids: the command line, scripted input and framebuffer captures.
//!
//! `imgit <repo> [--size WxH] --screenshot out.bmp [--select-row N] [--open-file path [--file-history
//! [--blame]]]` renders a few frames, saves the framebuffer and exits. `--script file --shots dir` instead replays
//! scripted input (move / click / rclick / dblclick / press / release / drag / key / keydown / keyup / type /
//! wait / shot / row / wheel), one step per frame.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use dear_imgui_rs::{Io, Key, MouseButton};
use glow::HasContext as _;

/// Frames drawn before `--screenshot` captures: enough for the layout and the font atlas to settle.
pub const SCREENSHOT_FRAME: u32 = 8;
/// The frame on which `--select-row` / `--open-file` are applied, once the first layout exists.
pub const DEV_COMMAND_FRAME: u32 = 2;
/// How many extra frames (~5 s) a screenshot waits for the app's background work, see
/// [`Frame::busy`](crate::shell::Frame::busy).
pub const SCREENSHOT_MAX_WAIT_FRAMES: u32 = 300;

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct DevOptions {
    /// The repository to open; `None` restores the last session.
    pub repo: Option<PathBuf>,
    /// `--screenshot`: where to save the framebuffer after [`SCREENSHOT_FRAME`] frames, then exit.
    pub screenshot: Option<PathBuf>,
    /// `--select-row`: the graph row to select before the screenshot.
    pub select_row: Option<usize>,
    /// `--open-file`: the file whose diff (or history) to open before the screenshot.
    pub open_file: Option<String>,
    /// `--file-history`: `--open-file` opens the File History view instead of the diff ...
    pub file_history: bool,
    /// ... `--blame`: with the blame columns on.
    pub blame: bool,
    /// `--script`: the input script to replay.
    pub script: Option<PathBuf>,
    /// `--shots`: where the script's `shot` steps are saved.
    pub shots_dir: PathBuf,
    /// `--size WxH`: the window's client size in logical pixels.
    pub width: u32,
    pub height: u32,
    /// `--terminal`: start with the terminal panel open.
    pub terminal: bool,
}

impl Default for DevOptions {
    fn default() -> Self {
        Self {
            repo: None,
            screenshot: None,
            select_row: None,
            open_file: None,
            file_history: false,
            blame: false,
            script: None,
            shots_dir: PathBuf::from("."),
            width: 1440,
            height: 900,
            terminal: false,
        }
    }
}

impl DevOptions {
    /// Parses the arguments after the program name. Unknown flags are taken as the repository path when none
    /// was given yet, like the C++ app does; malformed values are ignored.
    pub fn parse<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut options = Self::default();
        let mut args = args.into_iter().map(Into::into).peekable();
        while let Some(arg) = args.next() {
            let has_value = args.peek().is_some();
            match arg.as_str() {
                "--screenshot" if has_value => options.screenshot = args.next().map(PathBuf::from),
                "--select-row" if has_value => {
                    options.select_row = args.next().and_then(|row| row.parse().ok());
                }
                "--open-file" if has_value => options.open_file = args.next(),
                "--file-history" => options.file_history = true,
                "--blame" => options.blame = true,
                "--script" if has_value => options.script = args.next().map(PathBuf::from),
                "--shots" if has_value => {
                    options.shots_dir = args.next().map(PathBuf::from).unwrap_or_default();
                }
                "--terminal" => options.terminal = true,
                "--size" if has_value => {
                    let size = args.next().unwrap_or_default();
                    if let Some((width, height)) = size.split_once('x')
                        && let (Ok(width), Ok(height)) = (width.parse(), height.parse())
                        && width > 0
                        && height > 0
                    {
                        options.width = width;
                        options.height = height;
                    }
                }
                _ if options.repo.is_none() => options.repo = Some(PathBuf::from(arg)),
                _ => {}
            }
        }
        options
    }

    /// Whether the run takes its input from a script instead of the user.
    #[must_use]
    pub fn scripted(&self) -> bool {
        self.script.is_some()
    }

    /// Whether frames must be drawn continuously (no waiting for input): captures count frames.
    #[must_use]
    pub fn continuous(&self) -> bool {
        self.screenshot.is_some() || self.scripted()
    }
}

/// One primitive input step of a dev script.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptStep {
    /// Moves the mouse to a position in ImGui (window) coordinates.
    Move {
        x: f32,
        y: f32,
    },
    Button {
        button: MouseButton,
        down: bool,
    },
    Key {
        key: Key,
        down: bool,
    },
    /// Text typed into the focused field.
    Chars(String),
    /// Draws this many frames before the next step.
    Wait(u32),
    /// Saves the framebuffer as `<shots dir>/<name>.bmp`.
    Shot(String),
    /// Selects a graph row (handled by the app, see [`DevCommand::SelectRow`]).
    Row(usize),
    /// Wheel notches, horizontal then vertical (positive: left / up, as ImGui counts them).
    Wheel {
        x: f32,
        y: f32,
    },
}

/// The key a script names. `ctrl` is the shortcut modifier as ImGui takes it from a backend: Super on macOS,
/// which ImGui reads as Ctrl there.
fn key_from_name(name: &str) -> Option<Key> {
    const LETTERS: [Key; 26] = [
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::H,
        Key::I,
        Key::J,
        Key::K,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
        Key::Z,
    ];
    const DIGITS: [Key; 10] = [
        Key::Key0,
        Key::Key1,
        Key::Key2,
        Key::Key3,
        Key::Key4,
        Key::Key5,
        Key::Key6,
        Key::Key7,
        Key::Key8,
        Key::Key9,
    ];
    let key = match name {
        "ctrl" if cfg!(target_os = "macos") => Key::ModSuper,
        "ctrl" => Key::ModCtrl,
        "shift" => Key::ModShift,
        "alt" => Key::ModAlt,
        "enter" => Key::Enter,
        "escape" | "esc" => Key::Escape,
        "tab" => Key::Tab,
        "delete" => Key::Delete,
        "backspace" => Key::Backspace,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "home" => Key::Home,
        "end" => Key::End,
        "f5" => Key::F5,
        "space" => Key::Space,
        "=" => Key::Equal,
        "-" => Key::Minus,
        "`" => Key::GraveAccent,
        "/" => Key::Slash,
        _ => {
            let &[byte] = name.as_bytes() else {
                return None;
            };
            return match byte {
                b'a'..=b'z' => Some(LETTERS[usize::from(byte - b'a')]),
                b'0'..=b'9' => Some(DIGITS[usize::from(byte - b'0')]),
                _ => None,
            };
        }
    };
    Some(key)
}

/// Expands a script's lines into primitive steps. Blank lines and `#` comments are skipped; a missing or
/// malformed number reads as 0, an unknown key name is dropped.
#[must_use]
pub fn parse_script(source: &str) -> Vec<ScriptStep> {
    let mut steps = Vec::new();
    for line in source.lines() {
        let mut words = line.split_whitespace();
        let Some(command) = words.next() else {
            continue;
        };
        if command.starts_with('#') {
            continue;
        }
        let mut number = || words.next().and_then(|word| word.parse::<f32>().ok());
        match command {
            "move" => {
                let (x, y) = (number().unwrap_or(0.0), number().unwrap_or(0.0));
                steps.push(ScriptStep::Move { x, y });
            }
            "click" | "rclick" | "dblclick" => {
                let (x, y) = (number().unwrap_or(0.0), number().unwrap_or(0.0));
                let button = if command == "rclick" {
                    MouseButton::Right
                } else {
                    MouseButton::Left
                };
                push_click(&mut steps, x, y, button);
                if command == "dblclick" {
                    push_click(&mut steps, x, y, button);
                }
            }
            "press" | "release" => {
                // A held button for mid-drag shots: "press [x y]", moves, shots, then "release".
                let down = command == "press";
                if down && let (Some(x), Some(y)) = (number(), number()) {
                    steps.push(ScriptStep::Move { x, y });
                    steps.push(ScriptStep::Wait(2));
                }
                steps.push(ScriptStep::Button {
                    button: MouseButton::Left,
                    down,
                });
                steps.push(ScriptStep::Wait(2));
            }
            "drag" => {
                let (x1, y1) = (number().unwrap_or(0.0), number().unwrap_or(0.0));
                let (x2, y2) = (number().unwrap_or(0.0), number().unwrap_or(0.0));
                steps.extend([
                    ScriptStep::Move { x: x1, y: y1 },
                    ScriptStep::Wait(2),
                    ScriptStep::Button {
                        button: MouseButton::Left,
                        down: true,
                    },
                    ScriptStep::Wait(2),
                    ScriptStep::Move {
                        x: (x1 + x2) / 2.0,
                        y: (y1 + y2) / 2.0,
                    },
                    ScriptStep::Wait(2),
                    ScriptStep::Move { x: x2, y: y2 },
                    ScriptStep::Wait(6),
                    ScriptStep::Button {
                        button: MouseButton::Left,
                        down: false,
                    },
                    ScriptStep::Wait(4),
                ]);
            }
            "key" => {
                let chord = words.next().unwrap_or_default();
                let keys: Vec<Key> = chord.split('+').filter_map(key_from_name).collect();
                steps.extend(keys.iter().map(|&key| ScriptStep::Key { key, down: true }));
                steps.push(ScriptStep::Wait(2));
                steps.extend(
                    keys.iter()
                        .rev()
                        .map(|&key| ScriptStep::Key { key, down: false }),
                );
                steps.push(ScriptStep::Wait(3));
            }
            "keydown" | "keyup" => {
                if let Some(key) = words.next().and_then(key_from_name) {
                    steps.push(ScriptStep::Key {
                        key,
                        down: command == "keydown",
                    });
                }
                steps.push(ScriptStep::Wait(2));
            }
            "type" => {
                // The rest of the line, verbatim, after the one separating space.
                let rest = line.trim_start().strip_prefix("type").unwrap_or_default();
                steps.push(ScriptStep::Chars(
                    rest.strip_prefix(' ').unwrap_or(rest).to_owned(),
                ));
                steps.push(ScriptStep::Wait(2));
            }
            "wait" => {
                let frames = words.next().and_then(|word| word.parse().ok()).unwrap_or(1);
                steps.push(ScriptStep::Wait(frames));
            }
            "shot" => steps.push(ScriptStep::Shot(
                words.next().unwrap_or_default().to_owned(),
            )),
            "row" => {
                if let Some(row) = words.next().and_then(|word| word.parse().ok()) {
                    steps.push(ScriptStep::Row(row));
                }
            }
            "wheel" => {
                let (x, y) = (number().unwrap_or(0.0), number().unwrap_or(0.0));
                steps.push(ScriptStep::Wheel { x, y });
            }
            _ => {}
        }
    }
    steps
}

fn push_click(steps: &mut Vec<ScriptStep>, x: f32, y: f32, button: MouseButton) {
    steps.extend([
        ScriptStep::Move { x, y },
        ScriptStep::Wait(2),
        ScriptStep::Button { button, down: true },
        ScriptStep::Wait(2),
        ScriptStep::Button {
            button,
            down: false,
        },
        ScriptStep::Wait(4),
    ]);
}

/// Reads and expands the script at `path`.
pub fn load_script(path: &Path) -> io::Result<Vec<ScriptStep>> {
    fs::read_to_string(path).map(|source| parse_script(&source))
}

/// A request of the dev options or of a script that only the application can carry out.
#[derive(Debug, Clone, PartialEq)]
pub enum DevCommand {
    /// Select the graph row and scroll it into view (`--select-row`, a script's `row`).
    SelectRow(usize),
    /// Open a file's diff, or its history (`--open-file`, `--file-history`, `--blame`).
    OpenFile {
        path: String,
        file_history: bool,
        blame: bool,
    },
}

/// What a script step asks of the shell beyond feeding ImGui's input queue.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptAction {
    /// Nothing more: the step was input, or the script is waiting.
    None,
    /// Capture this frame as `<name>.bmp` once it is drawn.
    Shot(String),
    /// Hand the command to the application.
    Command(DevCommand),
    /// The script has run out: close the window.
    Finished,
}

/// Replays a script, one step per frame.
#[derive(Debug)]
pub struct ScriptPlayer {
    steps: Vec<ScriptStep>,
    next: usize,
    wait: u32,
}

impl ScriptPlayer {
    #[must_use]
    pub fn new(steps: Vec<ScriptStep>) -> Self {
        // Let the first frames settle.
        Self {
            steps,
            next: 0,
            wait: 5,
        }
    }

    /// Feeds the next step (if the script is not waiting) into `io`, before the frame begins.
    pub fn advance(&mut self, io: &mut Io) -> ScriptAction {
        if self.wait > 0 {
            self.wait -= 1;
            return ScriptAction::None;
        }
        let Some(step) = self.steps.get(self.next) else {
            return ScriptAction::Finished;
        };
        self.next += 1;
        match step {
            ScriptStep::Move { x, y } => io.add_mouse_pos_event([*x, *y]),
            ScriptStep::Button { button, down } => io.add_mouse_button_event(*button, *down),
            ScriptStep::Key { key, down } => io.add_key_event(*key, *down),
            ScriptStep::Chars(text) => io.add_input_characters_utf8(text),
            ScriptStep::Wait(frames) => self.wait = *frames,
            ScriptStep::Shot(name) => return ScriptAction::Shot(name.clone()),
            ScriptStep::Row(row) => return ScriptAction::Command(DevCommand::SelectRow(*row)),
            ScriptStep::Wheel { x, y } => io.add_mouse_wheel_event([*x, *y]),
        }
        ScriptAction::None
    }
}

/// Encodes bottom-up, tightly packed RGB rows (what `glReadPixels` returns) as a 24-bit BMP file.
#[must_use]
pub fn encode_bmp(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    const HEADER_SIZE: u32 = 54;
    let row_bytes = width as usize * 3;
    let padded = (row_bytes + 3) & !3;
    let image_size = (padded * height as usize) as u32;
    let mut file = Vec::with_capacity(HEADER_SIZE as usize + image_size as usize);
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&(HEADER_SIZE + image_size).to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes()); // reserved
    file.extend_from_slice(&HEADER_SIZE.to_le_bytes()); // pixel data offset
    file.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER
    file.extend_from_slice(&width.to_le_bytes());
    file.extend_from_slice(&height.to_le_bytes()); // positive: rows are bottom-up, like glReadPixels
    file.extend_from_slice(&1u16.to_le_bytes()); // planes
    file.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    file.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    file.extend_from_slice(&image_size.to_le_bytes());
    file.extend_from_slice(&[0; 16]); // resolution, palette
    if row_bytes > 0 {
        for row in rgb.chunks_exact(row_bytes).take(height as usize) {
            for &[red, green, blue] in row.as_chunks::<3>().0 {
                file.extend_from_slice(&[blue, green, red]);
            }
            file.resize(file.len() + (padded - row_bytes), 0);
        }
    }
    file
}

/// Saves the current framebuffer (`width` x `height` pixels) as a BMP file.
pub fn save_bmp(gl: &glow::Context, path: &Path, width: u32, height: u32) -> io::Result<()> {
    let mut pixels = vec![0u8; width as usize * height as usize * 3];
    // SAFETY: the GL context is current on this thread and `pixels` holds width * height tightly packed RGB
    // texels (PACK_ALIGNMENT 1), which is exactly what the call writes.
    unsafe {
        gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
        gl.read_pixels(
            0,
            0,
            width as i32,
            height as i32,
            glow::RGB,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut pixels)),
        );
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, encode_bmp(width, height, &pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_command_line() {
        let options = DevOptions::parse([
            "C:/repo",
            "--size",
            "1280x800",
            "--screenshot",
            "out.bmp",
            "--select-row",
            "3",
            "--open-file",
            "src/main.rs",
            "--file-history",
            "--blame",
            "--terminal",
            "ignored",
        ]);
        assert_eq!(options.repo.as_deref(), Some(Path::new("C:/repo")));
        assert_eq!((options.width, options.height), (1280, 800));
        assert_eq!(options.screenshot.as_deref(), Some(Path::new("out.bmp")));
        assert_eq!(options.select_row, Some(3));
        assert_eq!(options.open_file.as_deref(), Some("src/main.rs"));
        assert!(options.file_history && options.blame && options.terminal);
        assert!(options.continuous() && !options.scripted());
    }

    #[test]
    fn keeps_the_defaults_for_malformed_values() {
        let options =
            DevOptions::parse(["--size", "wide", "--script", "run.txt", "--shots", "shots"]);
        assert_eq!((options.width, options.height), (1440, 900));
        assert!(options.scripted());
        assert_eq!(options.shots_dir, Path::new("shots"));
        // A flag without its value is taken as the repository, as the C++ parser does.
        assert_eq!(
            DevOptions::parse(["--screenshot"]).repo.as_deref(),
            Some(Path::new("--screenshot"))
        );
    }

    #[test]
    fn expands_clicks_and_chords() {
        let steps = parse_script(
            "# comment\n\nclick 10 20\nkey ctrl+shift+p\ntype  hello world\nshot main\n",
        );
        let ctrl = if cfg!(target_os = "macos") {
            Key::ModSuper
        } else {
            Key::ModCtrl
        };
        assert_eq!(
            steps,
            [
                ScriptStep::Move { x: 10.0, y: 20.0 },
                ScriptStep::Wait(2),
                ScriptStep::Button {
                    button: MouseButton::Left,
                    down: true
                },
                ScriptStep::Wait(2),
                ScriptStep::Button {
                    button: MouseButton::Left,
                    down: false
                },
                ScriptStep::Wait(4),
                ScriptStep::Key {
                    key: ctrl,
                    down: true
                },
                ScriptStep::Key {
                    key: Key::ModShift,
                    down: true
                },
                ScriptStep::Key {
                    key: Key::P,
                    down: true
                },
                ScriptStep::Wait(2),
                ScriptStep::Key {
                    key: Key::P,
                    down: false
                },
                ScriptStep::Key {
                    key: Key::ModShift,
                    down: false
                },
                ScriptStep::Key {
                    key: ctrl,
                    down: false
                },
                ScriptStep::Wait(3),
                ScriptStep::Chars(" hello world".to_owned()),
                ScriptStep::Wait(2),
                ScriptStep::Shot("main".to_owned()),
            ]
        );
    }

    #[test]
    fn expands_drags_presses_and_wheels() {
        let steps =
            parse_script("drag 0 0 10 20\npress 5 6\nrelease\nwheel 0 -3\nrow 4\nwait\nrclick 1 2");
        assert_eq!(steps[4], ScriptStep::Move { x: 5.0, y: 10.0 });
        assert_eq!(steps[10], ScriptStep::Move { x: 5.0, y: 6.0 });
        assert_eq!(
            steps[12],
            ScriptStep::Button {
                button: MouseButton::Left,
                down: true
            }
        );
        assert_eq!(
            steps[14],
            ScriptStep::Button {
                button: MouseButton::Left,
                down: false
            }
        );
        assert_eq!(steps[16], ScriptStep::Wheel { x: 0.0, y: -3.0 });
        assert_eq!(steps[17], ScriptStep::Row(4));
        assert_eq!(steps[18], ScriptStep::Wait(1));
        assert_eq!(
            steps[21],
            ScriptStep::Button {
                button: MouseButton::Right,
                down: true
            }
        );
    }

    #[test]
    fn encodes_a_padded_bottom_up_bmp() {
        // 1x2: bottom row red, top row blue.
        let bmp = encode_bmp(1, 2, &[255, 0, 0, 0, 0, 255]);
        assert_eq!(bmp.len(), 54 + 8);
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[2..6].try_into().unwrap()), 62);
        assert_eq!(&bmp[54..58], &[0, 0, 255, 0]); // BGR + one byte of padding
        assert_eq!(&bmp[58..62], &[255, 0, 0, 0]);
    }
}
