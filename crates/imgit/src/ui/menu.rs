//! Context menus and the menu bar in the style of Electron's (Chromium views) menus with the Windows 11 look,
//! drawn inside the ImGui window with the same geometry, colours, shadow and behaviour (hover, delayed
//! submenus, keyboard navigation, clicks outside closing the menu and then reaching what is under the mouse).
//!
//! Geometry and colours of Electron 41's menus on Windows 11, 100% scale: Segoe UI 9pt, rows 28px, 12px above
//! the first and below the last row, separators 8 + 1 + 8, labels 20px from the left (48 when the menu has
//! check marks), 8px corner radius, #1F1F1F background, #363635 hot row, #E3E3E3 text, #C7C7C7 accelerators,
//! #9AA0A6 disabled text, #5E5E5E full-width separators and a DWM style shadow offset downwards.
//!
//! The menus that are open live in [`MenuState`] (`UiState::menus`). A menu is built as [`MenuItems`] whose
//! clicks are [`Action`]s; the click of the chosen item runs from [`draw_menus`], at the end of the frame and
//! after the menu has closed.

use dear_imgui_rs::{Condition, Key, MouseButton, StyleVar, WindowFlags};

use crate::app::action::Action;
use crate::app::application::Application;
use crate::theme::Color;
use crate::ui::draw::DrawList;
use crate::ui::geom::{Rect, Vec2, vec2};
use crate::ui::{Cx, raw};

const ROW_HEIGHT: f32 = 28.0;
const VERTICAL_PADDING: f32 = 12.0;
const SEPARATOR_HEIGHT: f32 = 17.0;
const LABEL_START: f32 = 20.0;
const CHECK_COLUMN: f32 = 28.0;
const RIGHT_MARGIN: f32 = 22.0;
const ARROW_COLUMN: f32 = 33.0;
const ACCELERATOR_PADDING: f32 = 6.0;
const CORNER_RADIUS: f32 = 8.0;
const FONT_SIZE: f32 = 12.0;

/// Shadow opacity per pixel of distance outside each edge (the corners combine them). Above the menu it is
/// below one grey level.
const SHADOW_SIDE: [f32; 11] = [
    0.151, 0.14, 0.108, 0.097, 0.065, 0.065, 0.043, 0.032, 0.032, 0.032, 0.011,
];
const SHADOW_BOTTOM: [f32; 23] = [
    0.258, 0.247, 0.237, 0.226, 0.204, 0.194, 0.172, 0.172, 0.172, 0.14, 0.14, 0.129, 0.108, 0.097,
    0.086, 0.075, 0.065, 0.065, 0.043, 0.032, 0.032, 0.032, 0.011,
];
const SHADOW_PEAK: f32 = 0.36;

struct MenuColors {
    background: Color,
    hot_row: Color,
    text: Color,
    accelerator: Color,
    disabled: Color,
    separator: Color,
}

/// The dark menus, or Windows' light ones under a light theme.
fn menu_colors(light: bool) -> MenuColors {
    let rgb = Color::rgb;
    if light {
        MenuColors {
            background: rgb(0xF9F9F9),
            hot_row: rgb(0xE6E6E6),
            text: rgb(0x1A1A1A),
            accelerator: rgb(0x5F5F5F),
            disabled: rgb(0x9A9A9A),
            separator: rgb(0xD9D9D9),
        }
    } else {
        MenuColors {
            background: rgb(0x1F1F1F),
            hot_row: rgb(0x363635),
            text: rgb(0xE3E3E3),
            accelerator: rgb(0xC7C7C7),
            disabled: rgb(0x9AA0A6),
            separator: rgb(0x5E5E5E),
        }
    }
}

/// What kind of row a [`MenuItem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuItemKind {
    #[default]
    Normal,
    Checkbox,
    Separator,
}

/// One row of a menu.
pub struct MenuItem {
    pub kind: MenuItemKind,
    /// `&` marks the mnemonic in menu bar menus (context menus show it as it is).
    pub label: String,
    /// Right-aligned shortcut text.
    pub accelerator: String,
    pub enabled: bool,
    pub checked: bool,
    /// Runs after the menu has closed (not for an item with a submenu).
    pub click: Option<Action>,
    pub submenu: MenuItems,
}

pub type MenuItems = Vec<MenuItem>;

impl Default for MenuItem {
    fn default() -> Self {
        Self {
            kind: MenuItemKind::Normal,
            label: String::new(),
            accelerator: String::new(),
            enabled: true,
            checked: false,
            click: None,
            submenu: Vec::new(),
        }
    }
}

impl std::fmt::Debug for MenuItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuItem")
            .field("kind", &self.kind)
            .field("label", &self.label)
            .field("accelerator", &self.accelerator)
            .field("enabled", &self.enabled)
            .field("checked", &self.checked)
            .field("click", &self.click.is_some())
            .field("submenu", &self.submenu)
            .finish()
    }
}

impl MenuItem {
    /// A separator line.
    #[must_use]
    pub fn separator() -> Self {
        Self {
            kind: MenuItemKind::Separator,
            ..Self::default()
        }
    }

    /// A greyed-out row that does nothing (the "Version 0.2.0" line).
    #[must_use]
    pub fn disabled(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: false,
            ..Self::default()
        }
    }

    /// The same item enabled or disabled.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The same item with its shortcut text (see `Cx::shortcut_label`).
    #[must_use]
    pub fn accelerator(mut self, accelerator: impl Into<String>) -> Self {
        self.accelerator = accelerator.into();
        self
    }

    /// The same item shown as a checkbox, ticked or not.
    #[must_use]
    pub fn checkbox(mut self, checked: bool) -> Self {
        self.kind = MenuItemKind::Checkbox;
        self.checked = checked;
        self
    }

    /// Whether the row can be picked.
    fn selectable(&self) -> bool {
        self.kind != MenuItemKind::Separator && self.enabled
    }
}

/// A row that runs `click` when picked.
#[must_use]
pub fn menu_item(
    label: impl Into<String>,
    click: impl FnOnce(&mut Cx<'_>, &mut Application) + 'static,
) -> MenuItem {
    MenuItem {
        label: label.into(),
        click: Some(Box::new(click)),
        ..MenuItem::default()
    }
}

/// A row with a check mark when `checked`.
#[must_use]
pub fn menu_checkbox(
    label: impl Into<String>,
    checked: bool,
    click: impl FnOnce(&mut Cx<'_>, &mut Application) + 'static,
) -> MenuItem {
    menu_item(label, click).checkbox(checked)
}

/// A row that opens `submenu`.
#[must_use]
pub fn menu_submenu(label: impl Into<String>, submenu: MenuItems) -> MenuItem {
    MenuItem {
        label: label.into(),
        submenu,
        ..MenuItem::default()
    }
}

/// The non-empty groups joined by separators.
#[must_use]
pub fn separated_menu(groups: Vec<MenuItems>) -> MenuItems {
    let mut result = MenuItems::new();
    for group in groups.into_iter().filter(|group| !group.is_empty()) {
        if !result.is_empty() {
            result.push(MenuItem::separator());
        }
        result.extend(group);
    }
    result
}

/// Labels without the `&` mnemonic markers (`&&` stays a literal `&`).
#[must_use]
pub fn strip_mnemonic(label: &str) -> String {
    let mut text = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            if chars.peek() == Some(&'&') {
                text.push('&');
                chars.next();
            }
            continue;
        }
        text.push(c);
    }
    text
}

/// One open menu: the whole menu of a popup, or one submenu of it. Its items are those of the root menu at
/// `path` (the indices of the items whose submenus lead here).
#[derive(Debug)]
struct Level {
    path: Vec<usize>,
    pos: Vec2,
    size: Vec2,
    label_start: f32,
    accelerator_right: f32,
    hot: Option<usize>,
    /// The item whose submenu is the next level.
    child_item: Option<usize>,
    /// The item whose submenu opens / closes when the delay elapses.
    pending_item: Option<usize>,
    pending_since: f64,
}

/// The menu bar entry whose menu is showing, and its button.
#[derive(Debug, Clone, Copy)]
struct BarEntry {
    index: usize,
    button: Rect,
}

/// The menu bar's entries (see [`set_menu_bar`]).
struct MenuBar {
    build: fn(&Cx<'_>, usize) -> MenuItems,
    rects: Vec<Rect>,
}

/// The menus that are open, and what the bar lets them do. Lives in `UiState::menus`.
#[derive(Default)]
pub struct MenuState {
    root: MenuItems,
    levels: Vec<Level>,
    bar: Option<BarEntry>,
    bar_menus: Option<MenuBar>,
    keyboard: bool,
    take_focus: bool,
    pending_click: Option<Action>,
    opened_frame: Option<i32>,
    /// A menu opens on a press; the release of that same click must not pick the item that happens to land
    /// under the cursor (a tall menu is moved up over it). A release counts once the mouse was dragged away
    /// from where the menu opened, or after a fresh press.
    release_armed: bool,
    open_mouse: Vec2,
}

/// The items of the level whose path is `path`.
fn items_at<'a>(root: &'a [MenuItem], path: &[usize]) -> &'a [MenuItem] {
    let mut items = root;
    for &index in path {
        items = &items[index].submenu;
    }
    items
}

fn items_at_mut<'a>(root: &'a mut [MenuItem], path: &[usize]) -> &'a mut [MenuItem] {
    let mut items = root;
    for &index in path {
        items = &mut items[index].submenu;
    }
    items
}

impl MenuState {
    fn items(&self, depth: usize) -> &[MenuItem] {
        items_at(&self.root, &self.levels[depth].path)
    }

    /// A label as shown: `&` marks the mnemonic in the menu bar's menus; a context menu (its labels built from
    /// branch, tag and file names) shows it as it is.
    fn menu_text(&self, label: &str) -> String {
        if self.bar.is_some() {
            strip_mnemonic(label)
        } else {
            label.to_owned()
        }
    }

    fn label_width(&self, cx: &Cx<'_>, label: &str) -> f32 {
        cx.text_size(cx.fonts.system, FONT_SIZE, &self.menu_text(label))
            .x
    }

    /// Where the labels and accelerators of `items` go, and how big their menu is: `(label_start,
    /// accelerator_right, size)`.
    fn layout(&self, cx: &Cx<'_>, items: &[MenuItem]) -> (f32, f32, Vec2) {
        let mut checks = false;
        let mut submenus = false;
        let mut max_label = 0.0_f32;
        let mut max_accelerator = 0.0_f32;
        let mut height = cx.px(VERTICAL_PADDING) * 2.0;
        for item in items {
            if item.kind == MenuItemKind::Separator {
                height += cx.px(SEPARATOR_HEIGHT);
                continue;
            }
            height += cx.px(ROW_HEIGHT);
            checks |= item.kind == MenuItemKind::Checkbox;
            submenus |= !item.submenu.is_empty();
            max_label = max_label.max(self.label_width(cx, &item.label));
            if !item.accelerator.is_empty() {
                max_accelerator = max_accelerator.max(
                    cx.text_size(cx.fonts.system, FONT_SIZE, &item.accelerator)
                        .x,
                );
            }
        }
        let label_start = cx.px(LABEL_START + if checks { CHECK_COLUMN } else { 0.0 });
        // The check column moves the labels 28px but widens the menu by 25 (Chromium's check icon overlaps the
        // left padding).
        let mut width = label_start - if checks { cx.px(3.0) } else { 0.0 }
            + max_label.ceil()
            + cx.px(RIGHT_MARGIN);
        if submenus {
            width += cx.px(ARROW_COLUMN);
        }
        if max_accelerator > 0.0 {
            width += max_accelerator.ceil() + cx.px(ACCELERATOR_PADDING);
        }
        (
            label_start,
            width - cx.px(LABEL_START + 1.0),
            vec2(width.round(), height.round()),
        )
    }

    /// Places a menu inside the viewport like Chromium: below / right of the anchor, flipped when it would not
    /// fit.
    fn place(cx: &Cx<'_>, size: Vec2, anchor: Vec2, flip_x: f32, flip_y: f32) -> Vec2 {
        let (viewport_pos, viewport_size) = raw::viewport(cx.ui);
        let min = viewport_pos;
        let max = viewport_pos + viewport_size;
        let mut pos = anchor;
        if pos.x + size.x > max.x {
            pos.x = flip_x - size.x;
        }
        if pos.y + size.y > max.y {
            pos.y = min.y.max((flip_y - size.y).min(max.y - size.y));
        }
        pos.x = pos.x.max(min.x);
        pos.y = pos.y.max(min.y);
        pos.round()
    }

    fn new_level(&self, cx: &Cx<'_>, path: Vec<usize>, items: &[MenuItem]) -> Level {
        let (label_start, accelerator_right, size) = self.layout(cx, items);
        Level {
            path,
            pos: Vec2::ZERO,
            size,
            label_start,
            accelerator_right,
            hot: None,
            child_item: None,
            pending_item: None,
            pending_since: 0.0,
        }
    }

    /// Y of the top of row `index` of level `depth`.
    fn item_top(&self, cx: &Cx<'_>, depth: usize, index: usize) -> f32 {
        let mut y = self.levels[depth].pos.y + cx.px(VERTICAL_PADDING);
        for item in &self.items(depth)[..index] {
            y += cx.px(if item.kind == MenuItemKind::Separator {
                SEPARATOR_HEIGHT
            } else {
                ROW_HEIGHT
            });
        }
        y
    }

    fn level_rect(&self, depth: usize) -> Rect {
        let level = &self.levels[depth];
        Rect::new(level.pos, level.pos + level.size)
    }

    /// The row under `mouse` in level `depth`, `None` between rows, on separators and outside.
    fn item_at(&self, cx: &Cx<'_>, depth: usize, mouse: Vec2) -> Option<usize> {
        if !self.level_rect(depth).contains(mouse) {
            return None;
        }
        let mut y = self.levels[depth].pos.y + cx.px(VERTICAL_PADDING);
        for (index, item) in self.items(depth).iter().enumerate() {
            let separator = item.kind == MenuItemKind::Separator;
            let height = cx.px(if separator {
                SEPARATOR_HEIGHT
            } else {
                ROW_HEIGHT
            });
            if mouse.y >= y && mouse.y < y + height {
                return (!separator).then_some(index);
            }
            y += height;
        }
        None
    }

    /// The innermost level under `mouse`.
    fn level_at(&self, mouse: Vec2) -> Option<usize> {
        (0..self.levels.len())
            .rev()
            .find(|&depth| self.level_rect(depth).contains(mouse))
    }

    fn close_children(&mut self, depth: usize) {
        self.levels.truncate(depth + 1);
        self.levels[depth].child_item = None;
    }

    fn open_child(&mut self, cx: &Cx<'_>, depth: usize, index: usize, select_first: bool) {
        self.close_children(depth);
        let item = &self.items(depth)[index];
        if item.submenu.is_empty() || !item.enabled {
            return;
        }
        let mut path = self.levels[depth].path.clone();
        path.push(index);
        let mut child = self.new_level(cx, path, &self.items(depth)[index].submenu);
        let parent = &self.levels[depth];
        let parent_pos = parent.pos;
        let parent_size = parent.size;
        let top = self.item_top(cx, depth, index);
        child.pos = Self::place(
            cx,
            child.size,
            vec2(parent_pos.x + parent_size.x, top - cx.px(VERTICAL_PADDING)),
            parent_pos.x,
            top + cx.px(ROW_HEIGHT) + cx.px(VERTICAL_PADDING),
        );
        if select_first {
            child.hot = items_at(&self.root, &child.path)
                .iter()
                .position(MenuItem::selectable);
        }
        let parent = &mut self.levels[depth];
        parent.child_item = Some(index);
        parent.pending_item = None;
        self.levels.push(child);
    }

    fn activate(&mut self, cx: &Cx<'_>, depth: usize, index: usize) {
        let item = &self.items(depth)[index];
        if !item.selectable() {
            return;
        }
        if !item.submenu.is_empty() {
            self.open_child(cx, depth, index, false);
            return;
        }
        let path = self.levels[depth].path.clone();
        self.pending_click = items_at_mut(&mut self.root, &path)[index].click.take();
        self.close();
    }

    fn move_hot(&mut self, depth: usize, direction: isize) {
        let count = self.items(depth).len();
        if count == 0 {
            return;
        }
        let mut index = self.levels[depth].hot;
        for _ in 0..count {
            let next = match index {
                None if direction > 0 => 0,
                None => count - 1,
                Some(current) => (current as isize + direction).rem_euclid(count as isize) as usize,
            };
            index = Some(next);
            if self.items(depth)[next].selectable() {
                self.levels[depth].hot = Some(next);
                return;
            }
        }
    }

    fn close(&mut self) {
        self.levels.clear();
        self.root.clear();
        self.bar = None;
    }

    fn popup(&mut self, cx: &Cx<'_>, items: MenuItems, pos: Option<Vec2>) {
        self.close();
        if items.is_empty() {
            return;
        }
        self.root = items;
        let mut level = self.new_level(cx, Vec::new(), &self.root);
        let anchor = pos.unwrap_or_else(|| cx.mouse_pos());
        level.pos = Self::place(cx, level.size, anchor.round(), anchor.x, anchor.y);
        self.levels.push(level);
        self.opened(cx, false);
    }

    fn popup_bar(
        &mut self,
        cx: &Cx<'_>,
        items: MenuItems,
        bar_index: usize,
        button: Rect,
        keyboard: bool,
    ) {
        self.close();
        // Before the layout: it measures the labels without their '&'.
        self.bar = Some(BarEntry {
            index: bar_index,
            button,
        });
        self.root = items;
        let mut level = self.new_level(cx, Vec::new(), &self.root);
        // The dropdown overlaps the bar's last pixel row, like Chromium's.
        level.pos = Self::place(
            cx,
            level.size,
            vec2(button.min.x, button.max.y - cx.px(1.0)),
            button.max.x,
            button.min.y,
        );
        self.levels.push(level);
        if keyboard {
            self.move_hot(0, 1);
        }
        self.opened(cx, keyboard);
    }

    /// Bookkeeping of a menu that has just opened.
    fn opened(&mut self, cx: &Cx<'_>, keyboard: bool) {
        self.keyboard = keyboard;
        self.take_focus = true;
        self.opened_frame = Some(cx.frame_count());
        self.release_armed = false;
        self.open_mouse = cx.mouse_pos();
    }

    fn switch_bar_menu(&mut self, cx: &Cx<'_>, index: isize, keyboard: bool) {
        let Some(bar) = &self.bar_menus else {
            return;
        };
        let count = bar.rects.len();
        if count == 0 {
            return;
        }
        let index = index.rem_euclid(count as isize) as usize;
        let (build, rect) = (bar.build, bar.rects[index]);
        self.popup_bar(cx, build(cx, index), index, rect, keyboard);
    }

    fn handle_keyboard(&mut self, cx: &Cx<'_>) {
        let ui = cx.ui;
        let Some(depth) = self.levels.len().checked_sub(1) else {
            return;
        };
        let bar_index = self.bar.map(|bar| bar.index);
        let pressed = |key: Key| ui.is_key_pressed(key);
        if pressed(Key::DownArrow) {
            self.move_hot(depth, 1);
            self.keyboard = true;
        } else if pressed(Key::UpArrow) {
            self.move_hot(depth, -1);
            self.keyboard = true;
        } else if pressed(Key::RightArrow) {
            let hot = self.levels[depth].hot;
            if let Some(hot) = hot.filter(|&hot| !self.items(depth)[hot].submenu.is_empty()) {
                self.open_child(cx, depth, hot, true);
            } else if let Some(bar) = bar_index {
                self.switch_bar_menu(cx, bar as isize + 1, true);
            }
        } else if pressed(Key::LeftArrow) {
            if depth > 0 {
                self.levels.pop();
                if let Some(parent) = self.levels.last_mut() {
                    parent.child_item = None;
                }
            } else if let Some(bar) = bar_index {
                self.switch_bar_menu(cx, bar as isize - 1, true);
            }
        } else if pressed(Key::Escape) {
            if depth > 0 {
                self.levels.pop();
                if let Some(parent) = self.levels.last_mut() {
                    parent.child_item = None;
                }
            } else {
                self.close();
            }
        } else if pressed(Key::Enter) || pressed(Key::KeypadEnter) || pressed(Key::Space) {
            if let Some(hot) = self.levels[depth].hot {
                self.activate(cx, depth, hot);
            }
        } else if bar_index.is_some() {
            // Mnemonics (menu bar menus only): the underlined letter of an item activates it.
            for (offset, &key) in LETTER_KEYS.iter().enumerate() {
                if !ui.is_key_pressed_with_repeat(key, false) {
                    continue;
                }
                let letter = char::from(b'a' + offset as u8);
                let found = self.items(depth).iter().position(|item| {
                    item.label
                        .split_once('&')
                        .and_then(|(_, after)| after.chars().next())
                        .is_some_and(|c| c.to_ascii_lowercase() == letter)
                });
                if let Some(index) = found {
                    self.levels[depth].hot = Some(index);
                    self.activate(cx, depth, index);
                    return;
                }
            }
        }
    }

    fn handle_mouse(&mut self, cx: &Cx<'_>) {
        let ui = cx.ui;
        let mouse = cx.mouse_pos();
        let now = cx.time();
        let over_level = self.level_at(mouse);

        // A press outside every menu closes them; the press is then replayed so that it also reaches what is
        // under the mouse, as Chromium reposts it to the window below.
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            if !ui.is_mouse_clicked(button)
                || over_level.is_some()
                || self.opened_frame == Some(cx.frame_count())
            {
                continue;
            }
            let on_bar_button = self.bar.is_some_and(|bar| bar.button.contains(mouse));
            self.close();
            if !on_bar_button {
                raw::add_mouse_button_event(ui, button, false);
                raw::add_mouse_button_event(ui, button, true);
            }
            return;
        }

        // Hovering another menu bar entry while a bar menu is open switches to it.
        if let (Some(bar), Some(menus), None) = (self.bar, &self.bar_menus, over_level) {
            let hovered = menus
                .rects
                .iter()
                .position(|rect| rect.contains(mouse))
                .filter(|&index| index != bar.index);
            if let Some(index) = hovered {
                self.switch_bar_menu(cx, index as isize, false);
                return;
            }
        }

        let mouse_delta = ui.io().mouse_delta();
        let moved = mouse_delta[0] != 0.0 || mouse_delta[1] != 0.0;
        if let Some(depth) = over_level {
            let index = self.item_at(cx, depth, mouse);
            if moved || index != self.levels[depth].hot {
                if moved {
                    self.keyboard = false;
                }
                let enabled_item = index.is_some_and(|index| self.items(depth)[index].enabled);
                if moved || enabled_item {
                    self.levels[depth].hot = if enabled_item { index } else { None };
                }
                // Submenus open, and other items' submenus close, after the system menu delay.
                let level = &mut self.levels[depth];
                if index != level.child_item && level.pending_item != index {
                    level.pending_item = index;
                    level.pending_since = now;
                }
            }
            let travel = mouse - self.open_mouse;
            let fresh_press = self.opened_frame != Some(cx.frame_count())
                && (ui.is_mouse_clicked(MouseButton::Left)
                    || ui.is_mouse_clicked(MouseButton::Right));
            if fresh_press || travel.x * travel.x + travel.y * travel.y > cx.px(4.0) * cx.px(4.0) {
                self.release_armed = true;
            }
            if (ui.is_mouse_released(MouseButton::Left) || ui.is_mouse_released(MouseButton::Right))
                && let Some(index) = index.filter(|_| self.release_armed)
            {
                self.activate(cx, depth, index);
                return;
            }
            // Deeper levels keep their hot item only while the mouse is over them.
            for deeper in &mut self.levels[depth + 1..] {
                if moved && deeper.child_item.is_none() {
                    deeper.hot = None;
                }
            }
        } else if moved && let Some(top) = self.levels.last_mut() {
            // Leaving the menu clears the hot row unless it shows its submenu.
            top.hot = top.child_item;
            top.pending_item = None;
        }

        let delay = imgit_platform::system::menu_show_delay().as_secs_f64();
        for depth in 0..self.levels.len() {
            let level = &self.levels[depth];
            if level.pending_item.is_none() && level.child_item.is_none() {
                continue;
            }
            if level.pending_item != level.child_item && now - level.pending_since >= delay {
                let pending = level.pending_item;
                self.levels[depth].pending_item = self.levels[depth].child_item;
                match pending {
                    Some(pending) if !self.items(depth)[pending].submenu.is_empty() => {
                        self.open_child(cx, depth, pending, false);
                    }
                    Some(_) => self.close_children(depth),
                    None => {}
                }
                break;
            }
        }
        if ui.io().mouse_wheel() != 0.0 {
            raw::consume_mouse_wheel(ui);
        }
    }

    fn draw_shadow(draw: DrawList<'_>, min: Vec2, max: Vec2, radius: f32) {
        let shadow = |alpha: f32| Color::from_rgba8(0, 0, 0, (alpha * 255.0 + 0.5) as u8);
        for (d, &a) in SHADOW_SIDE.iter().enumerate() {
            let d = d as f32;
            draw.add_rect_filled(
                vec2(min.x - 1.0 - d, min.y),
                vec2(min.x - d, max.y),
                shadow(a),
                0.0,
            );
            draw.add_rect_filled(
                vec2(max.x + d, min.y),
                vec2(max.x + d + 1.0, max.y),
                shadow(a),
                0.0,
            );
        }
        for (d, &a) in SHADOW_BOTTOM.iter().enumerate() {
            let d = d as f32;
            draw.add_rect_filled(
                vec2(min.x, max.y + d),
                vec2(max.x, max.y + d + 1.0),
                shadow(a),
                0.0,
            );
        }
        for (dx, &side) in SHADOW_SIDE.iter().enumerate() {
            for (dy, &bottom) in SHADOW_BOTTOM.iter().enumerate() {
                let (dx, dy) = (dx as f32, dy as f32);
                let c = shadow(side * bottom / SHADOW_PEAK);
                draw.add_rect_filled(
                    vec2(min.x - 1.0 - dx, max.y + dy),
                    vec2(min.x - dx, max.y + dy + 1.0),
                    c,
                    0.0,
                );
                draw.add_rect_filled(
                    vec2(max.x + dx, max.y + dy),
                    vec2(max.x + dx + 1.0, max.y + dy + 1.0),
                    c,
                    0.0,
                );
            }
        }
        // Under the rounded corners, where the menu does not cover its bounding box.
        let corner = shadow(SHADOW_SIDE[0] * SHADOW_BOTTOM[0] / SHADOW_PEAK);
        draw.add_rect_filled(
            vec2(min.x, max.y - radius),
            vec2(min.x + radius, max.y),
            corner,
            0.0,
        );
        draw.add_rect_filled(vec2(max.x - radius, max.y - radius), max, corner, 0.0);
    }

    fn draw_label(
        &self,
        cx: &Cx<'_>,
        draw: DrawList<'_>,
        pos: Vec2,
        color: Color,
        label: &str,
        underline: bool,
    ) {
        let font = cx.fonts.system;
        let text = self.menu_text(label);
        cx.draw_text(draw, font, FONT_SIZE, pos, color, &text);
        if !underline || self.bar.is_none() {
            return;
        }
        // Position of the mnemonic letter in the stripped text.
        let mut stripped = 0;
        let mut chars = label.char_indices().peekable();
        while let Some((_, c)) = chars.next() {
            if c == '&' {
                match chars.peek() {
                    Some(&(_, '&')) => {
                        chars.next();
                        stripped += 1;
                        continue;
                    }
                    Some(_) => {
                        let before: String = text.chars().take(stripped).collect();
                        let through: String = text.chars().take(stripped + 1).collect();
                        let x0 = cx.text_size(font, FONT_SIZE, &before).x;
                        let x1 = cx.text_size(font, FONT_SIZE, &through).x;
                        let y = (pos.y + cx.font_px(font, FONT_SIZE) * 0.86).round();
                        draw.add_rect_filled(
                            vec2((pos.x + x0).round(), y),
                            vec2((pos.x + x1).round(), y + 1.0),
                            color,
                            0.0,
                        );
                        return;
                    }
                    None => return,
                }
            }
            stripped += 1;
        }
    }

    fn draw_level(&self, cx: &Cx<'_>, draw: DrawList<'_>, depth: usize, colors: &MenuColors) {
        let level = &self.levels[depth];
        let min = level.pos;
        let max = level.pos + level.size;
        Self::draw_shadow(draw, min, max, cx.px(CORNER_RADIUS));
        draw.add_rect_filled(min, max, colors.background, cx.px(CORNER_RADIUS));

        let font = cx.fonts.system;
        let line = cx.font_px(font, FONT_SIZE);
        let mut y = min.y + cx.px(VERTICAL_PADDING);
        for (index, item) in self.items(depth).iter().enumerate() {
            if item.kind == MenuItemKind::Separator {
                let sy = y + cx.px(8.0);
                draw.add_rect_filled(
                    vec2(min.x, sy),
                    vec2(max.x, sy + cx.px(1.0)),
                    colors.separator,
                    0.0,
                );
                y += cx.px(SEPARATOR_HEIGHT);
                continue;
            }
            let row = cx.px(ROW_HEIGHT);
            if level.hot == Some(index) && item.enabled {
                draw.add_rect_filled(vec2(min.x, y), vec2(max.x, y + row), colors.hot_row, 0.0);
            }
            let color = if item.enabled {
                colors.text
            } else {
                colors.disabled
            };
            let text_y = (y + (row - line) / 2.0).round();
            self.draw_label(
                cx,
                draw,
                vec2(min.x + level.label_start, text_y),
                color,
                &item.label,
                self.keyboard,
            );
            if !item.accelerator.is_empty() {
                let w = cx.text_size(font, FONT_SIZE, &item.accelerator).x;
                cx.draw_text(
                    draw,
                    font,
                    FONT_SIZE,
                    vec2((min.x + level.accelerator_right - w).round(), text_y),
                    if item.enabled {
                        colors.accelerator
                    } else {
                        colors.disabled
                    },
                    &item.accelerator,
                );
            }
            if item.kind == MenuItemKind::Checkbox && item.checked {
                // Chromium's check icon: a 1.6px stroke at about 75% of the text colour.
                let points = [
                    vec2(min.x + cx.px(22.6), y + cx.px(15.2)),
                    vec2(min.x + cx.px(25.9), y + cx.px(18.4)),
                    vec2(min.x + cx.px(33.4), y + cx.px(10.4)),
                ];
                draw.add_polyline(
                    &points,
                    color.with_alpha(f32::from(0xC0_u8) / 255.0),
                    false,
                    cx.px(1.6),
                );
            }
            if !item.submenu.is_empty() {
                let ax = max.x - cx.px(30.0);
                let ay = y + row / 2.0;
                let points = [
                    vec2(ax + cx.px(0.25), ay - cx.px(3.25)),
                    vec2(ax + cx.px(3.5), ay),
                    vec2(ax + cx.px(0.25), ay + cx.px(3.25)),
                ];
                draw.add_polyline(&points, color, false, cx.px(1.0));
            }
            y += row;
        }
    }
}

/// `A`..`Z`, for the mnemonics.
const LETTER_KEYS: [Key; 26] = [
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

/// Runs `f` on the menu state, taken out of the UI state so that `f` can use the context freely.
fn with_menus<R>(cx: &mut Cx<'_>, f: impl FnOnce(&mut MenuState, &Cx<'_>) -> R) -> R {
    let mut menus = std::mem::take(&mut cx.state.menus);
    let result = f(&mut menus, cx);
    // A menu opened by `f` replaces what was taken; nothing else can have touched the slot meanwhile.
    cx.state.menus = menus;
    result
}

/// Shows `items` as a context menu with its top-left corner at `pos` (the mouse when `None`), replacing any
/// open menu. Clicks run after the menu has closed.
pub fn popup_menu(cx: &mut Cx<'_>, items: MenuItems, pos: Option<Vec2>) {
    with_menus(cx, |menus, cx| menus.popup(cx, items, pos));
}

/// A menu bar menu, dropped down from the bar button `[button_min, button_max]`. `keyboard` opens it with the
/// first item selected and mnemonics underlined (Alt / F10 navigation).
pub fn popup_bar_menu(
    cx: &mut Cx<'_>,
    items: MenuItems,
    bar_index: usize,
    button_min: Vec2,
    button_max: Vec2,
    keyboard: bool,
) {
    with_menus(cx, |menus, cx| {
        menus.popup_bar(
            cx,
            items,
            bar_index,
            Rect::new(button_min, button_max),
            keyboard,
        );
    });
}

/// Closes every open menu.
pub fn close_menus(cx: &mut Cx<'_>) {
    cx.state.menus.close();
}

/// The menu bar's entries: `build(i)` makes entry `i`'s menu, `rects[i]` is its button. Lets an open bar menu
/// switch to another entry on hover and with the Left / Right keys. Call every frame the bar is drawn.
pub fn set_menu_bar(cx: &mut Cx<'_>, build: fn(&Cx<'_>, usize) -> MenuItems, rects: Vec<Rect>) {
    cx.state.menus.bar_menus = Some(MenuBar { build, rects });
}

/// Whether a menu is showing.
#[must_use]
pub fn menu_open(cx: &Cx<'_>) -> bool {
    !cx.state.menus.levels.is_empty()
}

/// The menu bar entry whose menu is showing, `None` when none (or a context menu is showing).
#[must_use]
pub fn open_bar_menu(cx: &Cx<'_>) -> Option<usize> {
    let menus = &cx.state.menus;
    if menus.levels.is_empty() {
        None
    } else {
        menus.bar.map(|bar| bar.index)
    }
}

/// Whether the open bar menu was opened from the keyboard (the bar then underlines mnemonics).
#[must_use]
pub fn menu_keyboard_mode(cx: &Cx<'_>) -> bool {
    cx.state.menus.keyboard
}

/// Draws the open menus and handles their input; then runs the click of the item picked. Call once per frame,
/// after everything else.
pub fn draw_menus(cx: &mut Cx<'_>, app: &mut Application) {
    let mut menus = std::mem::take(&mut cx.state.menus);
    if !menus.levels.is_empty() {
        menus.handle_keyboard(cx);
    }
    if !menus.levels.is_empty() {
        menus.handle_mouse(cx);
    }
    if !menus.levels.is_empty() {
        let ui = cx.ui;
        // A window over the whole viewport: while a menu is open, the rest of the UI gets no hover or input.
        let (viewport_pos, viewport_size) = raw::viewport(ui);
        let take_focus = std::mem::take(&mut menus.take_focus);
        let colors = menu_colors(cx.palette.light);
        let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
        let _border = ui.push_style_var(StyleVar::WindowBorderSize(0.0));
        let mut window = ui
            .window("##native_menus")
            .position(viewport_pos.into(), Condition::Always)
            .size(viewport_size.into(), Condition::Always)
            .flags(
                WindowFlags::NO_DECORATION
                    | WindowFlags::NO_BACKGROUND
                    | WindowFlags::NO_MOVE
                    | WindowFlags::NO_SAVED_SETTINGS
                    | WindowFlags::NO_NAV
                    | WindowFlags::NO_SCROLL_WITH_MOUSE,
            );
        if take_focus {
            window = window.focused(true);
        }
        window.build(|| {
            raw::bring_current_window_to_display_front(ui);
            let draw = cx.window_draw_list();
            draw.push_clip_rect_full_screen();
            for depth in 0..menus.levels.len() {
                menus.draw_level(cx, draw, depth, &colors);
            }
            draw.pop_clip_rect();
        });
    }
    let click = menus.pending_click.take();
    // Back before the click runs: it may open another menu.
    cx.state.menus = menus;
    if let Some(click) = click {
        click(cx, app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(label: &str) -> MenuItem {
        MenuItem {
            label: label.to_owned(),
            ..MenuItem::default()
        }
    }

    #[test]
    fn mnemonic_markers_are_stripped() {
        assert_eq!(strip_mnemonic("&File"), "File");
        assert_eq!(strip_mnemonic("Open &Repo..."), "Open Repo...");
        assert_eq!(strip_mnemonic("Fish && Chips"), "Fish & Chips");
        assert_eq!(strip_mnemonic("No marker"), "No marker");
        assert_eq!(strip_mnemonic("Trailing &"), "Trailing ");
        assert_eq!(strip_mnemonic("Naïve &ünïcode"), "Naïve ünïcode");
        assert_eq!(strip_mnemonic("&&&A"), "&A");
    }

    #[test]
    fn groups_are_joined_by_separators_and_empty_ones_skipped() {
        let menu = separated_menu(vec![
            vec![plain("a"), plain("b")],
            Vec::new(),
            vec![plain("c")],
            Vec::new(),
        ]);
        let shape: Vec<(MenuItemKind, &str)> = menu
            .iter()
            .map(|item| (item.kind, item.label.as_str()))
            .collect();
        assert_eq!(
            shape,
            [
                (MenuItemKind::Normal, "a"),
                (MenuItemKind::Normal, "b"),
                (MenuItemKind::Separator, ""),
                (MenuItemKind::Normal, "c"),
            ]
        );
        assert!(separated_menu(vec![Vec::new(), Vec::new()]).is_empty());
        assert_eq!(separated_menu(vec![vec![plain("only")]]).len(), 1);
    }

    #[test]
    fn builders_set_their_fields() {
        let item = menu_checkbox("Show", true, |_, _| {}).accelerator("Ctrl+J");
        assert_eq!(item.kind, MenuItemKind::Checkbox);
        assert!(item.checked && item.enabled && item.click.is_some());
        assert_eq!(item.accelerator, "Ctrl+J");
        let sub = menu_submenu("More", vec![plain("x")]);
        assert_eq!(sub.submenu.len(), 1);
        assert!(sub.click.is_none());
        assert!(!MenuItem::disabled("Version").selectable());
        assert!(!MenuItem::separator().selectable());
    }

    #[test]
    fn items_are_found_by_path() {
        let mut root = vec![
            plain("a"),
            menu_submenu(
                "b",
                vec![plain("b0"), menu_submenu("b1", vec![plain("deep")])],
            ),
        ];
        assert_eq!(items_at(&root, &[]).len(), 2);
        assert_eq!(items_at(&root, &[1, 1])[0].label, "deep");
        items_at_mut(&mut root, &[1])[0].enabled = false;
        assert!(!root[1].submenu[0].enabled);
    }
}
