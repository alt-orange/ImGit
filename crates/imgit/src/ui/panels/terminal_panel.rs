//! The terminal panel (`app.terminal`, `app.show_terminal_panel`): a 30px toolbar with the shell tabs, +, and
//! Minimize / Kill All / Hide, over xterm.js (by default Consolas 12px, line height 1.2: 6x16 cells, 1.2rem
//! padding) running PowerShell on a pseudo console. The content dims to 60% while the terminal does not have
//! focus. Preferences > In-App Terminal pick the face, size, line height, cursor, dimming and the shell new tabs
//! run.
//!
//! Ports `src/ui/terminal_panel.cpp`.

use dear_imgui_rs::{Key, MouseButton, MouseCursor, WindowFlags, WindowHoveredFlags, sys};
use imgit_app::RepoState;
use imgit_app::preferences::{parse_tenths, pref, preferences};
use imgit_platform::pseudo_console::PseudoConsole;
use imgit_platform::system::terminal_shell;
use imgit_vt::{Attributes, Cell, Color as VtColor, Point, Screen};

use crate::app::application::Application;
use crate::app::terminal::{Selection, TerminalSession, TerminalTabs};
use crate::fonts::Font;
use crate::theme::Color;
use crate::ui::draw::DrawList;
use crate::ui::icons::svg_path;
use crate::ui::menu::popup_menu;
use crate::ui::panels::menus;
use crate::ui::{Cx, Vec2, raw, vec2};

const TOOLBAR_HEIGHT: f32 = 30.0;
/// `.terminal-panel { padding: 1.2rem 0 1.2rem 1.2rem }`.
const PADDING: f32 = 12.0;
/// The overviewRuler / scrollbar column.
const RULER_WIDTH: f32 = 10.0;

/// The terminal colour theme.
const ANSI: [u32; 16] = [
    0x1A1A1A, 0xF0716A, 0x5CCB7A, 0xEBC45A, 0x5A9CF0, 0xC77DE8, 0x4FC3D2, 0xDCDCDC, 0x6C6C6C,
    0xF79A94, 0x86DE9E, 0xF4D78A, 0x8BBBF7, 0xDDA6F2, 0x7FD7E2, 0xF5F5F5,
];
/// The same on a light background: darker, and white turned grey.
const ANSI_LIGHT: [u32; 16] = [
    0x1A1A1A, 0xC4312A, 0x2E8B48, 0x9A7A00, 0x2F6FD0, 0x9A48C4, 0x1F8A98, 0x6C6C6C, 0x5A5A5A,
    0xD9473F, 0x3BA35A, 0xB38B00, 0x4A86E0, 0xB062D8, 0x2AA0AE, 0x2B2B2B,
];

/// The terminal panel's own state between frames: everything per session lives in `TerminalSession`, the panel
/// flags in `Application::terminal`.
#[derive(Default)]
pub struct State {}

/// The cell grid for the font preferences, in CSS px: xterm's cell is the font's advance wide and its line box
/// times the line height tall (both floored), the baseline centred in it. 6x16, baseline 12, for Consolas 12px
/// at 1.2.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CellMetrics {
    font: Font,
    /// Device px, as ImGui sizes the font.
    glyph_size: f32,
    width: f32,
    height: f32,
    /// From the cell top.
    baseline: f32,
}

/// What ImGui knows about a font at one size.
#[derive(Debug, Clone, Copy)]
struct BakedMetrics {
    ascent: f32,
    descent: f32,
    /// The advance of 'M', device px.
    advance: f32,
}

/// The ascent, descent and 'M' advance of `font` baked at `glyph_size` device px.
fn baked_metrics(font: Font, glyph_size: f32) -> BakedMetrics {
    // SAFETY: the font belongs to the live ImGui context (see `Font::raw`); ImGui returns the baked size, which
    // stays valid for the frame, and three values are read from it.
    unsafe {
        let baked = sys::ImFont_GetFontBaked(font.raw(), glyph_size, -1.0);
        BakedMetrics {
            ascent: (*baked).Ascent,
            descent: (*baked).Descent,
            advance: sys::ImFontBaked_GetCharAdvance(baked, 'M' as sys::ImWchar),
        }
    }
}

/// The cell size and baseline (CSS px) for a font of CSS size `size` whose line box is `em_scale` em, a line
/// height of `line_height` times that, drawn at DPI scale `scale`.
fn cell_geometry(
    size: f32,
    em_scale: f32,
    line_height: f32,
    scale: f32,
    baked: BakedMetrics,
) -> (f32, f32, f32) {
    let line_box = size * em_scale;
    let width = (baked.advance / scale).floor().max(1.0);
    let height = (line_box * line_height).floor().max(1.0);
    let total = baked.ascent - baked.descent;
    let ascent_share = if total > 0.0 {
        baked.ascent / total
    } else {
        0.8
    };
    let baseline = ((height - line_box) / 2.0).round() + (line_box * ascent_share).round();
    (width, height, baseline)
}

fn cell_metrics(cx: &Cx<'_>) -> CellMetrics {
    let prefs = preferences();
    let font = cx.fonts.terminal;
    let size = prefs.number(pref::TERMINAL_FONT_SIZE, 6..=40) as f32;
    let tenths = parse_tenths(&prefs.get(pref::TERMINAL_LINE_HEIGHT), 10..=30).unwrap_or(12);
    let glyph_size = cx.font_px(font, size);
    let (width, height, baseline) = cell_geometry(
        size,
        font.em_scale,
        tenths as f32 / 10.0,
        cx.scale,
        baked_metrics(font, glyph_size),
    );
    CellMetrics {
        font,
        glyph_size,
        width,
        height,
        baseline,
    }
}

/// `ImFont::RenderChar`: one glyph with its top-left corner at `pos`.
fn render_char(draw: DrawList<'_>, font: Font, size: f32, pos: Vec2, color: Color, ch: char) {
    // SAFETY: the font and the draw list belong to the live ImGui context; no clip rectangle override.
    unsafe {
        sys::ImFont_RenderChar(
            font.raw(),
            draw.raw(),
            size,
            pos.into(),
            color.to_u32(),
            u32::from(ch) as sys::ImWchar,
            std::ptr::null(),
        );
    }
}

/// An xterm colour resolved against the theme: `fallback` for the default colour.
fn resolve(color: VtColor, fallback: Color, light: bool) -> Color {
    match color {
        VtColor::Default => fallback,
        VtColor::Rgb(r, g, b) => Color::from_rgba8(r, g, b, 255),
        VtColor::Indexed(index) if index < 16 => {
            Color::rgb(if light { ANSI_LIGHT } else { ANSI }[usize::from(index)])
        }
        VtColor::Indexed(index) if index < 232 => {
            const LEVELS: [u32; 6] = [0, 95, 135, 175, 215, 255];
            let i = u32::from(index) - 16;
            Color::rgb(
                (LEVELS[(i / 36) as usize] << 16)
                    | (LEVELS[((i / 6) % 6) as usize] << 8)
                    | LEVELS[(i % 6) as usize],
            )
        }
        VtColor::Indexed(index) => {
            let gray = 8 + (u32::from(index) - 232) * 10;
            Color::rgb((gray << 16) | (gray << 8) | gray)
        }
    }
}

/// The selection with its start before its end.
fn ordered(selection: Selection) -> (Point, Point) {
    if selection.anchor <= selection.head {
        (selection.anchor, selection.head)
    } else {
        (selection.head, selection.anchor)
    }
}

/// The selection when it covers something (a click leaves an empty one behind).
fn active_selection(session: &TerminalSession) -> Option<Selection> {
    session
        .selection
        .filter(|selection| selection.anchor != selection.head)
}

fn selection_text(session: &TerminalSession) -> Option<String> {
    let (start, end) = ordered(active_selection(session)?);
    Some(session.screen.text_between(start, end))
}

/// xterm normalises line breaks to CR and brackets the paste when the program asked for it.
fn paste_text(text: &str, bracketed: bool) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => normalized.push('\r'),
            ch => normalized.push(ch),
        }
    }
    if bracketed {
        format!("\x1b[200~{normalized}\x1b[201~")
    } else {
        normalized
    }
}

fn paste(session: &mut TerminalSession, text: &str) {
    let bytes = paste_text(text, session.screen.bracketed_paste());
    session.write(&bytes);
    session.scroll = 0;
}

/// The VT bytes for a key, xterm's way (application cursor mode, modifier parameters).
fn key_sequence(key: Key, ctrl: bool, shift: bool, alt: bool, application_cursor: bool) -> String {
    let modifiers = 1 + u8::from(shift) + if alt { 2 } else { 0 } + if ctrl { 4 } else { 0 };
    let cursor = |last: char| {
        if modifiers > 1 {
            format!("\x1b[1;{modifiers}{last}")
        } else {
            format!(
                "{}{last}",
                if application_cursor { "\x1bO" } else { "\x1b[" }
            )
        }
    };
    let tilde = |code: u32| {
        if modifiers > 1 {
            format!("\x1b[{code};{modifiers}~")
        } else {
            format!("\x1b[{code}~")
        }
    };
    let function = |last: char| {
        if modifiers > 1 {
            format!("\x1b[1;{modifiers}{last}")
        } else {
            format!("\x1bO{last}")
        }
    };
    match key {
        Key::Enter | Key::KeypadEnter => return if alt { "\x1b\r" } else { "\r" }.to_owned(),
        Key::Backspace => {
            return format!(
                "{}{}",
                if alt { "\x1b" } else { "" },
                if ctrl { "\x08" } else { "\x7f" }
            );
        }
        Key::Tab => return if shift { "\x1b[Z" } else { "\t" }.to_owned(),
        Key::Escape => return "\x1b".to_owned(),
        Key::UpArrow => return cursor('A'),
        Key::DownArrow => return cursor('B'),
        Key::RightArrow => return cursor('C'),
        Key::LeftArrow => return cursor('D'),
        Key::Home => return cursor('H'),
        Key::End => return cursor('F'),
        Key::Insert => return tilde(2),
        Key::Delete => return tilde(3),
        Key::PageUp => return tilde(5),
        Key::PageDown => return tilde(6),
        Key::F1 => return function('P'),
        Key::F2 => return function('Q'),
        Key::F3 => return function('R'),
        Key::F4 => return function('S'),
        Key::F5 => return tilde(15),
        Key::F6 => return tilde(17),
        Key::F7 => return tilde(18),
        Key::F8 => return tilde(19),
        Key::F9 => return tilde(20),
        Key::F10 => return tilde(21),
        Key::F11 => return tilde(23),
        Key::F12 => return tilde(24),
        _ => {}
    }
    if ctrl && !alt {
        let letter = key as i32 - Key::A as i32;
        if (0..26).contains(&letter) {
            return char::from(1 + letter as u8).to_string();
        }
        match key {
            Key::Space | Key::Key2 => return "\0".to_owned(),
            Key::LeftBracket => return "\x1b".to_owned(),
            Key::Backslash => return "\x1c".to_owned(),
            Key::RightBracket => return "\x1d".to_owned(),
            _ => {}
        }
    }
    String::new()
}

/// The keys the terminal reacts to, in `ImGuiKey` order: those with a VT sequence, and the ones of the app's own
/// chords and the clipboard shortcuts, which are looked at (and kept from being typed).
const HANDLED_KEYS: &[Key] = &[
    Key::Tab,
    Key::LeftArrow,
    Key::RightArrow,
    Key::UpArrow,
    Key::DownArrow,
    Key::PageUp,
    Key::PageDown,
    Key::Home,
    Key::End,
    Key::Insert,
    Key::Delete,
    Key::Backspace,
    Key::Space,
    Key::Enter,
    Key::Escape,
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
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
    Key::Minus,
    Key::Equal,
    Key::LeftBracket,
    Key::Backslash,
    Key::RightBracket,
    Key::GraveAccent,
    Key::KeypadEnter,
];

/// The app's own chords (command palette, zoom, tabs, terminal tabs): not typed into the shell.
fn is_app_chord(key: Key, cmd: bool, shift: bool) -> bool {
    cmd && (matches!(
        key,
        Key::P
            | Key::Tab
            | Key::GraveAccent
            | Key::PageUp
            | Key::PageDown
            | Key::Equal
            | Key::Minus
    ) || (!shift && key == Key::Key0)
        || (shift
            && (matches!(key, Key::N | Key::W)
                || (Key::Key1 as i32..=Key::Key9 as i32).contains(&(key as i32)))))
}

/// The characters typed since the last frame, taken out of ImGui's queue so that nothing else reads them.
fn take_typed_characters(_ui: &dear_imgui_rs::Ui) -> Vec<char> {
    // SAFETY: a frame is open (`_ui`), so the IO of the current context exists; the queue's `Size` characters
    // at `Data` are read, then it is emptied (`resize(0)`), with no reference into it kept.
    unsafe {
        let queue = &mut (*sys::igGetIO_Nil()).InputQueueCharacters;
        let typed = if queue.Data.is_null() || queue.Size <= 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(queue.Data, queue.Size as usize)
                .iter()
                .filter_map(|&ch| char::from_u32(ch))
                .collect()
        };
        queue.Size = 0;
        typed
    }
}

/// What the keyboard asked of the application while it typed into the session.
#[derive(Default)]
struct KeyboardResult {
    /// Ctrl+C with a selection: the text to put on the clipboard.
    copy: Option<String>,
}

fn handle_keyboard(cx: &Cx<'_>, session: &mut TerminalSession) -> KeyboardResult {
    let ui = cx.ui;
    let io = ui.io();
    // ImGui swaps Ctrl and Super on macOS: io.KeyCtrl is the shortcut modifier (Cmd there), and the Control key
    // itself, which the shell's control sequences need, reads as Super.
    let cmd = io.key_ctrl();
    let ctrl = if cx.macos_behaviors() {
        io.key_super()
    } else {
        io.key_ctrl()
    };
    let (shift, alt) = (io.key_shift(), io.key_alt());
    let mut result = KeyboardResult::default();
    let mut out = String::new();
    for &key in HANDLED_KEYS {
        if !ui.is_key_pressed(key) || is_app_chord(key, cmd, shift) {
            continue;
        }
        // Ctrl+C (Cmd+C) copies a selection (Ctrl+C else interrupts); Ctrl+V (Cmd+V) pastes.
        if cmd && !shift && key == Key::C && active_selection(session).is_some() {
            result.copy = selection_text(session);
            session.selection = None;
            continue;
        }
        if cmd && key == Key::V {
            if let Some(text) = raw::clipboard_text(ui) {
                paste(session, &text);
            }
            continue;
        }
        out += &key_sequence(
            key,
            ctrl,
            shift,
            alt,
            session.screen.application_cursor_keys(),
        );
    }
    // Typed characters. Ctrl alone makes control sequences (above), not text, but Ctrl+Alt is AltGr on Windows,
    // whose characters (@ { } [ ] \ | ~ on German, French or Nordic layouts) must reach the shell. Alt alone is
    // Meta: ESC before an ASCII character; anything else (an Option-typed character on macOS) goes through as
    // typed.
    let typed = take_typed_characters(ui);
    if !ctrl || alt {
        for ch in typed {
            if ch >= ' ' && ch != '\x7f' {
                if alt && !ctrl && ch.is_ascii() {
                    out.push('\x1b');
                }
                out.push(ch);
            }
        }
    }
    if !out.is_empty() {
        session.write(&out);
        session.scroll = 0;
    }
    result
}

/// Whether the active repository's active terminal has a selection (the context menu's commands).
#[must_use]
pub fn terminal_has_selection(app: &mut Application) -> bool {
    let workdir = active_workdir(app);
    app.active_terminal(workdir.as_deref())
        .is_some_and(|session| active_selection(session).is_some())
}

/// The working directory of the active repository's terminal tabs (`None`: no repository).
fn active_workdir(app: &Application) -> Option<std::path::PathBuf> {
    app.active().map(|state| state.repo.workdir().to_path_buf())
}

/// Copies the selection to the clipboard.
pub fn terminal_copy(cx: &mut Cx<'_>, app: &mut Application) {
    let workdir = active_workdir(app);
    let text = app
        .active_terminal(workdir.as_deref())
        .and_then(|session| selection_text(session));
    if let Some(text) = text {
        app.copy_to_clipboard(cx, &text);
    }
}

/// Pastes the clipboard into the shell.
pub fn terminal_paste(cx: &mut Cx<'_>, app: &mut Application) {
    let workdir = active_workdir(app);
    if let Some(text) = raw::clipboard_text(cx.ui)
        && let Some(session) = app.active_terminal(workdir.as_deref())
    {
        paste(session, &text);
    }
    app.terminal.focus_requested = true;
}

pub fn terminal_select_all(app: &mut Application) {
    let workdir = active_workdir(app);
    if let Some(session) = app.active_terminal(workdir.as_deref()) {
        let screen = &session.screen;
        let first = screen.first_line_id();
        session.selection = Some(Selection {
            anchor: Point {
                line: first,
                column: 0,
            },
            head: Point {
                line: first + screen.line_count().saturating_sub(1) as u64,
                column: screen.columns(),
            },
        });
    }
}

pub fn terminal_clear(app: &mut Application) {
    let workdir = active_workdir(app);
    if let Some(session) = app.active_terminal(workdir.as_deref()) {
        session.screen.clear();
        session.scroll = 0;
        session.selection = None;
    }
}

/// Types `bytes` into the shell.
pub fn terminal_write(app: &mut Application, bytes: &[u8]) {
    let workdir = active_workdir(app);
    if let Some(session) = app.active_terminal(workdir.as_deref()) {
        session.write(&String::from_utf8_lossy(bytes));
        session.scroll = 0;
    }
    app.terminal.focus_requested = true;
}

/// What the toolbar and the body ask for once the session list is back in the application.
#[derive(Default)]
struct Outcome {
    new_tab: bool,
    close_tab: Option<usize>,
    kill_all: bool,
    context_menu: bool,
    /// Ctrl+C with a selection typed in the body.
    copy: Option<String>,
}

/// The terminal panel of `state`'s repository (`None`: no repository, the current directory), `size` device px.
pub fn draw_terminal_panel(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: Option<&mut RepoState>,
    size: Vec2,
) {
    let workdir = state.map(|state| state.repo.workdir().to_path_buf());
    let workdir = workdir.as_deref();
    // Opening the panel starts a shell; while it closes (the last shell exited) it stays empty.
    let empty = app.terminal_tabs(workdir).sessions.is_empty();
    if empty && app.show_terminal_panel {
        app.new_terminal_tab(workdir);
    }
    // The tabs leave the application while they draw (the panel's flags are in it too).
    let mut tabs = std::mem::take(app.terminal_tabs(workdir));
    let mut outcome = Outcome::default();
    let ui = cx.ui;
    let origin = cx.cursor();
    ui.child_window("##terminal_panel")
        .size(size.into())
        .flags(WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE)
        .build(ui, || {
            draw_contents(cx, app, &mut tabs, &mut outcome, origin, size);
        });
    *app.terminal_tabs(workdir) = tabs;
    if outcome.new_tab {
        app.new_terminal_tab(workdir);
    }
    if let Some(index) = outcome.close_tab {
        app.close_terminal_tab(workdir, index);
    }
    if outcome.kill_all {
        app.kill_terminals(workdir);
    }
    if let Some(text) = outcome.copy {
        app.copy_to_clipboard(cx, &text);
    }
    if outcome.context_menu {
        let items = menus::terminal_menu(cx, app);
        popup_menu(cx, items, None);
    }
}

/// `text` 12px with its line box's top at `box_top`, centred like Chromium.
fn text_at(cx: &Cx<'_>, draw: DrawList<'_>, x: f32, box_top: f32, color: Color, text: &str) {
    let ascent = 12.0 * 2189.0 / 2048.0 * cx.scale;
    cx.draw_text(
        draw,
        cx.fonts.regular,
        12.0,
        vec2(x, (box_top + ascent.round()).round() - ascent.ceil()),
        color,
        text,
    );
}

fn draw_contents(
    cx: &mut Cx<'_>,
    app: &mut Application,
    tabs: &mut TerminalTabs,
    outcome: &mut Outcome,
    origin: Vec2,
    size: Vec2,
) {
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let border = p.ink_255(20); // --section-border
    let background = p.terminal_bg.with_alpha(1.0);
    let foreground = p.terminal_fg.with_alpha(1.0);
    let right = origin.x + size.x;
    draw.add_rect_filled(origin, vec2(right, origin.y + size.y), background, 0.0);

    // Toolbar: 1px borders top and bottom, 10px padding.
    let bar_h = cx.px(TOOLBAR_HEIGHT);
    draw.add_rect_filled(origin, vec2(right, origin.y + 1.0), border, 0.0);
    draw.add_rect_filled(
        vec2(origin.x, origin.y + bar_h - 1.0),
        vec2(right, origin.y + bar_h),
        border,
        0.0,
    );
    let minimized = app.terminal.minimized;
    if minimized {
        cx.draw_fa_icon(
            draw,
            "far:terminal",
            vec2(origin.x + cx.px(17.5), origin.y + cx.px(14.92)),
            cx.px(12.0),
            foreground,
        );
        text_at(
            cx,
            draw,
            origin.x + cx.px(30.0),
            origin.y + cx.px(6.42),
            foreground,
            "Terminal",
        );
    } else {
        draw_tabs(
            cx,
            tabs,
            outcome,
            origin,
            foreground,
            &mut app.terminal.focus_requested,
        );
    }
    draw_toolbar_actions(cx, app, outcome, origin, right);

    // The session the panel shows.
    let ui = cx.ui;
    let Some(last) = tabs.sessions.len().checked_sub(1) else {
        return;
    };
    tabs.active = tabs.active.min(last);
    if app.terminal.minimized {
        return;
    }
    let session = &mut tabs.sessions[tabs.active];
    draw_body(cx, app, session, outcome, origin, size, border, bar_h);
    let _ = ui;
}

/// The shell tabs and the + button.
fn draw_tabs(
    cx: &mut Cx<'_>,
    tabs: &mut TerminalTabs,
    outcome: &mut Outcome,
    origin: Vec2,
    foreground: Color,
    focus_requested: &mut bool,
) {
    let ui = cx.ui;
    let draw = cx.window_draw_list();
    // Tabs: 21.14px, 4px inline padding, 2px apart; active white 30%, hovered 10%.
    let mut x = origin.x + cx.px(10.0);
    let tab_y = origin.y + cx.px(4.42);
    let tab_h = cx.px(21.14);
    let default_label = terminal_shell(&preferences().get(pref::TERMINAL_SHELL)).label;
    for (i, session) in tabs.sessions.iter().enumerate() {
        let _id = ui.push_id(i as i32);
        let label = session.shell_label.as_deref().unwrap_or(&default_label);
        let label_w = cx.text_size(cx.fonts.regular, 12.0, label).x;
        let w = cx
            .px(48.0)
            .max(cx.px(4.0 + 12.0 + 2.0) + label_w + cx.px(4.0 + 12.0 + 4.0));
        let tmin = vec2(x, tab_y);
        let tmax = vec2(x + w, tab_y + tab_h);
        let active = i == tabs.active;
        cx.set_cursor(tmin);
        let clicked = ui.invisible_button("##term_tab", [w - cx.px(16.0), tab_h]);
        let hovered = ui.is_item_hovered();
        cx.set_cursor(vec2(tmax.x - cx.px(16.0), tmin.y));
        let closed = ui.invisible_button("##term_tab_close", [cx.px(16.0), tab_h]);
        let close_hovered = ui.is_item_hovered();
        if active || hovered || close_hovered {
            draw.add_rect_filled(
                tmin,
                tmax,
                cx.palette.ink_255(if active { 77 } else { 26 }),
                cx.px(2.0),
            );
        }
        let color = if active {
            cx.palette.ink_255(255)
        } else {
            foreground
        };
        cx.draw_fa_icon(
            draw,
            "far:terminal",
            vec2(x + cx.px(10.0), tab_y + cx.px(10.56)),
            cx.px(9.6),
            color,
        );
        text_at(cx, draw, x + cx.px(18.0), tab_y + cx.px(2.0), color, label);
        let close_x = x + cx.px(18.0) + label_w + cx.px(4.0);
        if let Some(path) = svg_path("close") {
            cx.draw_svg_path(
                draw,
                "close",
                path,
                24.0,
                24.0,
                vec2(close_x, tab_y + cx.px(4.56)),
                vec2(close_x + cx.px(12.0), tab_y + cx.px(16.56)),
                foreground,
            );
        }
        if clicked {
            tabs.active = i;
            *focus_requested = true;
        }
        if closed {
            outcome.close_tab = Some(i);
        }
        x += w + cx.px(2.0);
    }
    // New tab: 16px button, 2px padding, 12px icon.
    let plus_min = vec2(x, origin.y + cx.px(6.98));
    cx.set_cursor(plus_min);
    if ui.invisible_button("##term_new_tab", [cx.px(16.0), cx.px(16.0)]) {
        outcome.new_tab = true;
    }
    if let Some(path) = svg_path("plus") {
        cx.draw_svg_path(
            draw,
            "plus",
            path,
            24.0,
            24.0,
            plus_min + vec2(cx.px(2.0), cx.px(2.0)),
            plus_min + vec2(cx.px(14.0), cx.px(14.0)),
            foreground,
        );
    }
}

/// Right: Minimize (Expand), Kill All Terminals, Hide; 5px padding, 12px icons, 60% white.
fn draw_toolbar_actions(
    cx: &mut Cx<'_>,
    app: &mut Application,
    outcome: &mut Outcome,
    origin: Vec2,
    right: f32,
) {
    let ui = cx.ui;
    let draw = cx.window_draw_list();
    let p = cx.palette;
    let icon_y = origin.y + cx.px(14.92);
    let action = |cx: &mut Cx<'_>, id: &str, icon: &str, x: f32, icon_w: f32, tip: Option<&str>| {
        let w = cx.px(5.0 + 5.0 + 5.0) + icon_w;
        cx.set_cursor(vec2(x, origin.y + cx.px(6.42)));
        let pressed = ui.invisible_button(id, [w, cx.px(17.14)]);
        let hovered = ui.is_item_hovered();
        let rect = cx.item_rect();
        cx.draw_fa_icon(
            draw,
            icon,
            vec2(x + cx.px(10.0) + icon_w / 2.0, icon_y),
            cx.px(12.0),
            if hovered {
                p.text_selected
            } else {
                p.text_secondary
            },
        );
        if let (true, Some(tip)) = (hovered, tip) {
            cx.tooltip_on_rect(rect.min, rect.max, tip, true);
        }
        pressed
    };
    let hide_x = right - cx.px(10.0) - cx.px(22.5);
    let kill_x = hide_x - cx.px(25.5);
    let minimize_x = kill_x - cx.px(25.5);
    let minimized = app.terminal.minimized;
    if action(
        cx,
        "##term_minimize",
        if minimized {
            "far:chevron-up"
        } else {
            "far:chevron-down"
        },
        minimize_x,
        cx.px(10.5),
        Some(if minimized {
            "Expand Terminal"
        } else {
            "Minimize Terminal"
        }),
    ) {
        app.terminal.minimized = !minimized;
        app.terminal.focus_requested = minimized;
    }
    if action(
        cx,
        "##term_kill",
        "far:trash",
        kill_x,
        cx.px(10.5),
        Some("Kill All Terminals"),
    ) {
        outcome.kill_all = true;
    }
    if action(cx, "##term_hide", "far:times", hide_x, cx.px(7.5), None) {
        app.show_terminal_panel = false;
    }
}

/// The word under a double click: the run of cells around `column` that are not blank or brackets / quotes.
fn word_range(cells: &[Cell], column: usize) -> Option<(usize, usize)> {
    let word = |x: usize| {
        cells.get(x).is_some_and(|cell| {
            cell.ch
                .is_some_and(|ch| !matches!(ch, ' ' | '(' | ')' | '"' | '\''))
        })
    };
    let mut from = column.min(cells.len().checked_sub(1)?);
    let mut to = from;
    if !word(from) {
        return None;
    }
    while from > 0 && word(from - 1) {
        from -= 1;
    }
    while to + 1 < cells.len() && word(to + 1) {
        to += 1;
    }
    Some((from, to + 1))
}

/// The terminal: a 1px top / left border, 12px padding, then the cell grid and a 10px ruler on the right.
#[allow(clippy::too_many_arguments)]
fn draw_body(
    cx: &mut Cx<'_>,
    app: &mut Application,
    session: &mut TerminalSession,
    outcome: &mut Outcome,
    origin: Vec2,
    size: Vec2,
    border: Color,
    bar_h: f32,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let right = origin.x + size.x;
    let body_top = origin.y + bar_h;
    draw.add_rect_filled(
        vec2(origin.x, body_top),
        vec2(right, body_top + 1.0),
        border,
        0.0,
    );
    draw.add_rect_filled(
        vec2(origin.x, body_top),
        vec2(origin.x + 1.0, origin.y + size.y),
        border,
        0.0,
    );
    let grid = vec2(
        origin.x + 1.0 + cx.px(PADDING),
        body_top + 1.0 + cx.px(PADDING),
    );
    let metrics = cell_metrics(cx);
    let cell_w = cx.px(metrics.width).floor();
    let cell_h = cx.px(metrics.height).floor();
    let grid_w = right - grid.x - cx.px(RULER_WIDTH);
    let grid_h = origin.y + size.y - cx.px(PADDING) - grid.y;
    let columns = ((grid_w / cell_w) as usize).max(2);
    let rows = ((grid_h / cell_h) as usize).max(1);
    if !session.started() && !session.failed {
        session.screen.resize(columns, rows);
        let shell = terminal_shell(&preferences().get(pref::TERMINAL_SHELL));
        session.shell_label = Some(shell.label.clone());
        match PseudoConsole::spawn(
            &shell.args,
            Some(&session.workdir),
            columns.min(usize::from(u16::MAX)) as u16,
            rows.min(usize::from(u16::MAX)) as u16,
        ) {
            Ok(console) => session.console = Some(console),
            Err(_) => session.failed = true,
        }
        let pending = std::mem::take(&mut session.pending_input);
        session.write(&pending);
    } else if session.started()
        && (columns != session.screen.columns() || rows != session.screen.rows())
    {
        session.screen.resize(columns, rows);
        if let Some(console) = &session.console {
            let _ = console.resize(
                columns.min(usize::from(u16::MAX)) as u16,
                rows.min(usize::from(u16::MAX)) as u16,
            );
        }
        session.selection = None;
    }
    if session.failed {
        text_at(
            cx,
            draw,
            grid.x,
            grid.y,
            Color::rgb(ANSI[1]),
            "Failed to start shell process",
        );
        return;
    }

    // Focus: a click in the panel takes it, a click anywhere else (or another text field) gives it up.
    let (area_min, area_max) = (vec2(origin.x, body_top), vec2(right, origin.y + size.y));
    let mouse_inside =
        ui.is_mouse_hovering_rect(area_min.into(), area_max.into()) && ui.is_window_hovered();
    let was_focused = app.terminal.focused;
    if app.terminal.focus_requested {
        app.terminal.focused = true;
        app.terminal.focus_requested = false;
    } else if ui.is_mouse_clicked(MouseButton::Left) || ui.is_mouse_clicked(MouseButton::Right) {
        app.terminal.focused = ui.is_window_hovered_with_flags(WindowHoveredFlags::CHILD_WINDOWS);
    } else if ui.io().want_text_input() {
        app.terminal.focused = false;
    }
    let focused = app.terminal.focused;
    if focused != was_focused && session.screen.focus_reporting() {
        session.write(if focused { "\x1b[I" } else { "\x1b[O" });
    }

    // Scrolling: the wheel moves through the scrollback (arrow keys on the alternate screen); output that
    // arrives while scrolled up does not move the view.
    let scrollback = session
        .screen
        .line_count()
        .saturating_sub(session.screen.rows());
    if session.scroll > 0 {
        let scrolled = session.screen.scrolled_lines();
        session.scroll += scrolled.saturating_sub(session.scrolled_seen) as usize;
    }
    session.scrolled_seen = session.screen.scrolled_lines();
    let wheel = ui.io().mouse_wheel();
    if mouse_inside && wheel != 0.0 {
        let lines = (wheel * 100.0 / metrics.height).round() as isize;
        if session.screen.alternate_screen() {
            let application = session.screen.application_cursor_keys();
            let key = match (lines > 0, application) {
                (true, true) => "\x1bOA",
                (true, false) => "\x1b[A",
                (false, true) => "\x1bOB",
                (false, false) => "\x1b[B",
            };
            session.write(&key.repeat(lines.unsigned_abs()));
        } else {
            session.scroll = session.scroll.saturating_add_signed(lines);
        }
    }
    session.scroll = session.scroll.min(scrollback);
    let top_index = scrollback - session.scroll;

    // Selection: drag to select, double-click selects a word; a plain click clears it.
    cx.set_cursor(grid);
    ui.invisible_button(
        "##terminal_grid",
        [grid_w + cx.px(RULER_WIDTH), cell_h * rows as f32],
    );
    let screen_columns = session.screen.columns();
    let first_line = session.screen.first_line_id();
    let cell_at = |mouse: Vec2| {
        let x = (((mouse.x - grid.x) / cell_w + 0.5).floor().max(0.0) as usize).min(screen_columns);
        let y = (((mouse.y - grid.y) / cell_h).floor().max(0.0) as usize).min(rows - 1);
        Point {
            line: first_line + (top_index + y) as u64,
            column: x,
        }
    };
    if ui.is_item_activated() {
        let at = cell_at(cx.mouse_pos());
        session.selection = Some(Selection {
            anchor: at,
            head: at,
        });
        session.selecting = true;
        if ui.is_mouse_double_clicked(MouseButton::Left)
            && let Some(line) = session.screen.line_by_id(at.line)
            && let Some((from, to)) = word_range(line.cells(), at.column)
        {
            session.selection = Some(Selection {
                anchor: Point {
                    line: at.line,
                    column: from,
                },
                head: Point {
                    line: at.line,
                    column: to,
                },
            });
            session.selecting = false;
        }
    }
    if session.selecting
        && ui.is_item_active()
        && let Some(selection) = &mut session.selection
    {
        selection.head = cell_at(cx.mouse_pos());
    }
    if !ui.is_item_active() {
        session.selecting = false;
    }
    if ui.is_item_hovered() {
        ui.set_mouse_cursor(Some(MouseCursor::TextInput));
    }
    if ui.is_window_hovered() && ui.is_mouse_released(MouseButton::Right) && mouse_inside {
        outcome.context_menu = true;
    }

    if focused && let Some(text) = handle_keyboard(cx, session).copy {
        outcome.copy = Some(text);
    }

    draw_cells(
        cx, session, focused, grid, cell_w, cell_h, rows, top_index, &metrics,
    );

    // The scrollbar: a 10px slider in the ruler column while there is scrollback.
    if scrollback > 0 {
        let track_h = cell_h * rows as f32;
        let total = session.screen.line_count() as f32;
        let slider_h = cx.px(20.0).max(track_h * rows as f32 / total);
        let slider_top = grid.y + (track_h - slider_h) * top_index as f32 / scrollback as f32;
        let sx = right - cx.px(RULER_WIDTH) - 1.0;
        draw.add_rect_filled(
            vec2(sx, slider_top.round()),
            vec2(sx + cx.px(RULER_WIDTH), (slider_top + slider_h).round()),
            p.terminal_fg.with_alpha(0.2),
            0.0,
        );
    }
}

/// The cells: backgrounds, the selection, glyphs, underlines, then the cursor; 60% while unfocused (unless Dim
/// when unfocused is off).
#[allow(clippy::too_many_arguments)]
fn draw_cells(
    cx: &Cx<'_>,
    session: &TerminalSession,
    focused: bool,
    grid: Vec2,
    cell_w: f32,
    cell_h: f32,
    rows: usize,
    top_index: usize,
    metrics: &CellMetrics,
) {
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let screen: &Screen = &session.screen;
    let opacity = if focused || !preferences().flag(pref::TERMINAL_DIM) {
        1.0
    } else {
        0.6
    };
    let (default_fg, default_bg, cursor_color) = (p.terminal_fg, p.terminal_bg, p.accent);
    let light = p.light;
    draw.push_clip_rect(
        grid,
        vec2(
            grid.x + cell_w * screen.columns() as f32,
            grid.y + cell_h * rows as f32,
        ),
        true,
    );
    let (font, glyph_size) = (metrics.font, metrics.glyph_size);
    let ascent = baked_metrics(font, glyph_size).ascent;
    let selection = active_selection(session).map(ordered);
    for y in 0..rows {
        let index = top_index + y;
        let Some(line) = screen.line(index) else {
            break;
        };
        let id = screen.first_line_id() + index as u64;
        let cy = grid.y + cell_h * y as f32;
        let baseline = (cy + cx.px(metrics.baseline)).round();
        let cells = line.cells();
        for (x, cell) in cells.iter().enumerate() {
            let mut fg_color = cell.fg;
            // drawBoldTextInBrightColors
            if cell.attributes.contains(Attributes::BOLD)
                && let VtColor::Indexed(i) = fg_color
                && i < 8
            {
                fg_color = VtColor::Indexed(i + 8);
            }
            let mut fg = resolve(fg_color, default_fg, light);
            let mut bg = resolve(cell.bg, default_bg, light);
            let mut paint_bg = cell.bg != VtColor::Default;
            if cell.attributes.contains(Attributes::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
                paint_bg = true;
            }
            let cell_x = grid.x + cell_w * x as f32;
            let wide = cell.ch.is_some() && cells.get(x + 1).is_some_and(|next| next.ch.is_none());
            let w = if wide { cell_w * 2.0 } else { cell_w };
            if paint_bg && cell.ch.is_some() {
                draw.add_rect_filled(
                    vec2(cell_x, cy),
                    vec2(cell_x + w, cy + cell_h),
                    bg.with_alpha(opacity),
                    0.0,
                );
            }
            if let Some((start, end)) = selection {
                let after_start = id > start.line || (id == start.line && x >= start.column);
                let before_end = id < end.line || (id == end.line && x < end.column);
                if after_start && before_end {
                    draw.add_rect_filled(
                        vec2(cell_x, cy),
                        vec2(cell_x + cell_w, cy + cell_h),
                        cursor_color.with_alpha(64.0 / 255.0 * opacity),
                        0.0,
                    );
                }
            }
            let Some(ch) = cell.ch else { continue };
            if ch == ' ' || cell.attributes.contains(Attributes::INVISIBLE) {
                continue;
            }
            let alpha = opacity
                * if cell.attributes.contains(Attributes::DIM) {
                    0.5
                } else {
                    1.0
                };
            let color = fg.with_alpha(alpha);
            render_char(
                draw,
                font,
                glyph_size,
                vec2(cell_x, baseline - ascent),
                color,
                ch,
            );
            if cell.attributes.contains(Attributes::UNDERLINE) {
                draw.add_rect_filled(
                    vec2(cell_x, baseline + 1.0),
                    vec2(cell_x + w, baseline + 2.0),
                    color,
                    0.0,
                );
            }
            if cell.attributes.contains(Attributes::STRIKETHROUGH) {
                let sy = (cy + cell_h / 2.0).round();
                draw.add_rect_filled(vec2(cell_x, sy), vec2(cell_x + w, sy + 1.0), color, 0.0);
            }
        }
    }
    // While focused the Cursor Style's block (the glyph over it in the background colour), underline or bar; an
    // outlined cell otherwise.
    let cursor = screen.cursor();
    let cursor_row = cursor.row + session.scroll;
    if screen.cursor_visible() && cursor_row < rows {
        let cmin = vec2(
            grid.x + cell_w * cursor.column as f32,
            grid.y + cell_h * cursor_row as f32,
        );
        let style = preferences().get(pref::TERMINAL_CURSOR);
        if focused && style == "Block" {
            draw.add_rect_filled(
                cmin,
                vec2(cmin.x + cell_w, cmin.y + cell_h),
                cursor_color.with_alpha(1.0),
                0.0,
            );
            let glyph = screen
                .line(top_index + cursor_row)
                .and_then(|line| line.cells().get(cursor.column))
                .and_then(|cell| cell.ch)
                .filter(|&ch| ch != ' ');
            if let Some(ch) = glyph {
                render_char(
                    draw,
                    font,
                    glyph_size,
                    vec2(cmin.x, (cmin.y + cx.px(metrics.baseline)).round() - ascent),
                    default_bg.with_alpha(1.0),
                    ch,
                );
            }
        } else if focused && style == "Underline" {
            let thickness = cx.px(2.0).round().max(1.0);
            draw.add_rect_filled(
                vec2(cmin.x, cmin.y + cell_h - thickness),
                vec2(cmin.x + cell_w, cmin.y + cell_h),
                cursor_color.with_alpha(1.0),
                0.0,
            );
        } else if focused {
            draw.add_rect_filled(
                cmin,
                vec2(cmin.x + 1.0, cmin.y + cell_h),
                cursor_color.with_alpha(1.0),
                0.0,
            );
        } else {
            draw.add_rect(
                cmin,
                vec2(cmin.x + cell_w, cmin.y + cell_h),
                cursor_color.with_alpha(opacity),
                0.0,
                1.0,
            );
        }
    }
    draw.pop_clip_rect();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(text: &str) -> Vec<Cell> {
        text.chars()
            .map(|ch| Cell {
                ch: Some(ch),
                ..Cell::default()
            })
            .collect()
    }

    #[test]
    fn cell_grid_of_consolas_12_at_1_2_is_6_by_16() {
        // Consolas: advance 0.5498 em, ascent 0.743 / descent 0.257 of 1.1699 em lines.
        let baked = BakedMetrics {
            ascent: 12.0,
            descent: -3.0,
            advance: 6.6,
        };
        let (width, height, baseline) = cell_geometry(12.0, 1.1699, 1.2, 1.0, baked);
        assert_eq!((width, height), (6.0, 16.0));
        // 16px line, 14.04px line box: 1px of half-leading, 12 * 0.8 ascent share.
        assert_eq!(baseline, 1.0 + (14.0388_f32 * 0.8).round());
    }

    #[test]
    fn cells_are_at_least_one_pixel() {
        let baked = BakedMetrics {
            ascent: 0.0,
            descent: 0.0,
            advance: 0.2,
        };
        let (width, height, baseline) = cell_geometry(6.0, 0.1, 1.0, 2.0, baked);
        assert_eq!((width, height), (1.0, 1.0));
        assert!(baseline.is_finite());
    }

    #[test]
    fn resolves_the_xterm_palette() {
        let fallback = Color::rgb(0x123456);
        assert_eq!(resolve(VtColor::Default, fallback, false), fallback);
        assert_eq!(
            resolve(VtColor::Rgb(1, 2, 3), fallback, false),
            Color::from_rgba8(1, 2, 3, 255)
        );
        assert_eq!(
            resolve(VtColor::Indexed(1), fallback, false),
            Color::rgb(0xF0716A)
        );
        assert_eq!(
            resolve(VtColor::Indexed(1), fallback, true),
            Color::rgb(0xC4312A)
        );
        // The colour cube: 16 is black, 231 white, 196 pure red.
        assert_eq!(
            resolve(VtColor::Indexed(16), fallback, false),
            Color::rgb(0)
        );
        assert_eq!(
            resolve(VtColor::Indexed(231), fallback, false),
            Color::rgb(0xFFFFFF)
        );
        assert_eq!(
            resolve(VtColor::Indexed(196), fallback, false),
            Color::rgb(0xFF0000)
        );
        // The greys: 232 is 8, 255 is 238.
        assert_eq!(
            resolve(VtColor::Indexed(232), fallback, false),
            Color::rgb(0x080808)
        );
        assert_eq!(
            resolve(VtColor::Indexed(255), fallback, false),
            Color::rgb(0xEEEEEE)
        );
    }

    #[test]
    fn pastes_with_carriage_returns_and_brackets() {
        assert_eq!(paste_text("a\r\nb\nc\rd", false), "a\rb\rc\rd");
        assert_eq!(paste_text("x\ny", true), "\x1b[200~x\ry\x1b[201~");
        assert_eq!(paste_text("", false), "");
    }

    #[test]
    fn keys_become_vt_sequences() {
        let k = |key, ctrl, shift, alt, app| key_sequence(key, ctrl, shift, alt, app);
        assert_eq!(k(Key::Enter, false, false, false, false), "\r");
        assert_eq!(k(Key::Enter, false, false, true, false), "\x1b\r");
        assert_eq!(k(Key::Backspace, false, false, false, false), "\x7f");
        assert_eq!(k(Key::Backspace, true, false, true, false), "\x1b\x08");
        assert_eq!(k(Key::Tab, false, true, false, false), "\x1b[Z");
        assert_eq!(k(Key::UpArrow, false, false, false, false), "\x1b[A");
        assert_eq!(k(Key::UpArrow, false, false, false, true), "\x1bOA");
        // A modifier makes the parameter form, even in application mode: shift 2, alt 3, ctrl 5.
        assert_eq!(k(Key::LeftArrow, true, false, false, true), "\x1b[1;5D");
        assert_eq!(k(Key::RightArrow, false, true, true, false), "\x1b[1;4C");
        assert_eq!(k(Key::Delete, false, false, false, false), "\x1b[3~");
        assert_eq!(k(Key::PageUp, false, true, false, false), "\x1b[5;2~");
        assert_eq!(k(Key::F1, false, false, false, false), "\x1bOP");
        assert_eq!(k(Key::F1, true, false, false, false), "\x1b[1;5P");
        assert_eq!(k(Key::F5, false, false, false, false), "\x1b[15~");
        assert_eq!(k(Key::F12, false, false, false, false), "\x1b[24~");
        // Control letters and the C0 punctuation; Ctrl+Alt is not a control sequence.
        assert_eq!(k(Key::C, true, false, false, false), "\x03");
        assert_eq!(k(Key::Z, true, false, false, false), "\x1a");
        assert_eq!(k(Key::Space, true, false, false, false), "\0");
        assert_eq!(k(Key::LeftBracket, true, false, false, false), "\x1b");
        assert_eq!(k(Key::C, true, false, true, false), "");
        assert_eq!(k(Key::C, false, false, false, false), "");
    }

    #[test]
    fn the_apps_chords_are_not_typed() {
        assert!(is_app_chord(Key::P, true, false));
        assert!(is_app_chord(Key::Tab, true, true));
        assert!(is_app_chord(Key::Key0, true, false));
        assert!(!is_app_chord(Key::Key0, true, true));
        assert!(is_app_chord(Key::Key3, true, true));
        assert!(!is_app_chord(Key::Key3, true, false));
        assert!(is_app_chord(Key::N, true, true));
        assert!(!is_app_chord(Key::N, true, false));
        assert!(!is_app_chord(Key::P, false, false));
    }

    #[test]
    fn every_handled_key_is_listed_once() {
        let mut seen = std::collections::HashSet::new();
        for &key in HANDLED_KEYS {
            assert!(seen.insert(key as i32), "{key:?} twice");
        }
    }

    #[test]
    fn a_double_click_selects_the_word() {
        let line = cells("git status (now) ");
        assert_eq!(word_range(&line, 5), Some((4, 10)));
        assert_eq!(word_range(&line, 0), Some((0, 3)));
        assert_eq!(word_range(&line, 3), None);
        assert_eq!(word_range(&line, 11), None);
        assert_eq!(word_range(&line, 12), Some((12, 15)));
        // Past the line, the last cell counts; nothing to select in an empty one.
        assert_eq!(word_range(&line, 99), None);
        assert_eq!(word_range(&[], 0), None);
    }

    #[test]
    fn selections_are_ordered() {
        let a = Point { line: 3, column: 5 };
        let b = Point { line: 2, column: 9 };
        assert_eq!(ordered(Selection { anchor: a, head: b }), (b, a));
        assert_eq!(ordered(Selection { anchor: b, head: a }), (b, a));
    }
}
