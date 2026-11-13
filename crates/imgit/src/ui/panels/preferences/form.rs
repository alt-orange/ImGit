//! Bootstrap's form-horizontal (the classic pages): the grid, titles, rows with their label column and help
//! blocks, and the rows bound to preferences, settings and the git configuration.

use dear_imgui_rs::InputTextFlags;
use imgit_app::Preference;
use imgit_app::preferences::preferences;

use super::controls::{BTN, Field, Tone};
use super::page::{Face, HELP, LABEL, Page, Style};
use super::store::{Scope, git_flag, git_value, save_settings, set_git_value};
use crate::app::application::Application;
use crate::app::settings::{Settings, atoi};

/// The layout cursor of a page: the bottom of the last block and its margin, which collapses with the next
/// block's top margin.
#[derive(Debug, Clone, Copy)]
pub(super) struct Form {
    /// The .container's padding.
    pub left: f32,
    /// The .container: 1170px, or what the page leaves inside its 40px padding and scrollbar; 15px padding.
    pub width: f32,
    /// Bottom of the last block.
    pub y: f32,
    /// Its bottom margin.
    pub margin: f32,
    stacked: bool,
}

impl Form {
    pub(super) fn new(page: &Page<'_, '_>) -> Self {
        Self {
            left: 55.0,
            width: (page.width - 80.0 - page.scrollbar - 30.0).min(1140.0),
            y: 0.0,
            margin: 0.0,
            stacked: page.stacked,
        }
    }

    /// col-md-3: the label column (the whole row when stacked).
    pub(super) fn column(&self) -> f32 {
        if self.stacked {
            self.width
        } else {
            self.width / 4.0
        }
    }

    /// col-md-9: the control column.
    pub(super) fn control_x(&self) -> f32 {
        if self.stacked {
            self.left + 15.0
        } else {
            self.left + self.column() + 15.0
        }
    }

    pub(super) fn control_width(&self) -> f32 {
        if self.stacked {
            self.width
        } else {
            self.width - self.column()
        }
    }

    pub(super) fn place(&self, margin_top: f32) -> f32 {
        self.y + self.margin.max(margin_top)
    }

    pub(super) fn advance(
        &mut self,
        page: &mut Page<'_, '_>,
        top: f32,
        height: f32,
        margin_bottom: f32,
    ) {
        self.y = top + height;
        self.margin = margin_bottom;
        // The form's padding-bottom.
        page.reach(self.y + self.margin + 20.0);
    }

    /// A clearfix / border: the last block's margin stays inside its parent.
    pub(super) fn contain(&mut self) {
        self.y += self.margin;
        self.margin = 0.0;
    }
}

/// A setting row being laid out.
#[derive(Debug, Clone, Copy)]
pub(super) struct Row {
    pub top: f32,
    /// The label's height.
    pub label: f32,
    /// The control column's height.
    pub column: f32,
    /// The help block is open.
    pub help: bool,
    control_x: f32,
    control_width: f32,
}

impl Row {
    pub(super) fn x(&self) -> f32 {
        self.control_x
    }

    /// The control column's width (CSS px).
    pub(super) fn control_width(&self) -> f32 {
        self.control_width
    }
}

/// h3.bold: 24px semibold on a 26.4px line, margins 20 / 10.
pub(super) fn title(page: &mut Page<'_, '_>, form: &mut Form, text: &str) {
    let top = form.place(20.0);
    let style = Style::ink(24.0, 0.75).with_face(Face::Semibold);
    page.text(
        form.left,
        top + Page::leading(style, 26.390_625),
        style,
        text,
    );
    form.advance(page, top, 26.390_625, 10.0);
}

/// h4: 18px on a 19.8px line, margins 10 / 10.
pub(super) fn heading(page: &mut Page<'_, '_>, form: &mut Form, text: &str, face: Face) {
    let top = form.place(10.0);
    let style = Style::ink(18.0, 0.75).with_face(face);
    page.text(
        form.left,
        top + Page::leading(style, 19.796_875),
        style,
        text,
    );
    form.advance(page, top, 19.796_875, 10.0);
}

/// p: 14px on 20px lines.
pub(super) fn paragraph(
    page: &mut Page<'_, '_>,
    form: &mut Form,
    markup: &str,
    margin_bottom: f32,
) {
    let top = form.place(0.0);
    let height = page.rich(form.left, top, form.width, 20.0, LABEL, markup);
    form.advance(page, top, height, margin_bottom);
}

/// A setting row: the label right-aligned in its column (7px padding-top, 20px lines), `body` draws the control
/// column; 15px under the taller of the two. Stacked, the label sits left-aligned above the column.
pub(super) fn row(
    page: &mut Page<'_, '_>,
    form: &mut Form,
    label: &str,
    body: impl FnOnce(&mut Page<'_, '_>, &mut Row),
) {
    let top = form.place(0.0);
    let mut r = Row {
        top,
        label: 0.0,
        column: 0.0,
        help: false,
        control_x: form.control_x(),
        control_width: form.control_width(),
    };
    let right = form.left + form.column() - 15.0;
    let lines = page.wrap(LABEL, label, form.column() - 30.0);
    for (i, line) in lines.iter().enumerate() {
        let x = if form.stacked {
            form.left + 15.0
        } else {
            right - page.measure(LABEL, line)
        };
        page.text(x, top + 7.0 + 20.0 * i as f32, LABEL, line);
    }
    r.label = 7.0 + 20.0 * lines.len() as f32;
    if form.stacked {
        r.top += r.label;
        r.label = 0.0;
    }
    body(page, &mut r);
    if r.help {
        r.column += 10.0;
    }
    form.advance(page, top, r.top - top + r.label.max(r.column), 15.0);
}

/// One div.mt1 of the row's help block under the control (12px, 60% white).
pub(super) fn help(page: &mut Page<'_, '_>, r: &mut Row, markup: &str) {
    help_with(page, r, markup, 17.140_625, HELP);
}

pub(super) fn help_with(
    page: &mut Page<'_, '_>,
    r: &mut Row,
    markup: &str,
    line_height: f32,
    style: Style,
) {
    if !r.help {
        r.column += 5.0;
        r.help = true;
    }
    r.column += 5.0;
    r.column += page.rich(
        r.x(),
        r.top + r.column,
        525.0_f32.min(r.control_width - 30.0),
        line_height,
        style,
        markup,
    );
}

/// A checkbox in its div.checkbox (7px padding-top, a 20px line), with an optional 12px label after it.
pub(super) fn row_checkbox(
    page: &mut Page<'_, '_>,
    r: &mut Row,
    id: &str,
    checked: bool,
    label: &str,
    disabled: bool,
) -> bool {
    let mut label_width = 0.0;
    if !label.is_empty() {
        label_width = 8.0
            + page.text(
                r.x() + 20.0,
                r.top + 7.0 + Page::leading(HELP, 20.0),
                HELP,
                label,
            );
    }
    r.column = 27.0;
    page.checkbox(id, r.x(), r.top + 11.0, checked, disabled, label_width)
}

pub(super) fn value_of(preference: Preference) -> String {
    preferences().get(preference)
}

pub(super) fn flag_of(preference: Preference) -> bool {
    preferences().flag(preference)
}

pub(super) fn set_value(preference: Preference, value: &str) {
    let _ = preferences().set(preference, value);
}

/// A checkbox row backed by a stored value.
pub(super) fn stored_checkbox(
    page: &mut Page<'_, '_>,
    r: &mut Row,
    id: &str,
    preference: Preference,
    label: &str,
) {
    let value = flag_of(preference);
    if row_checkbox(page, r, id, value, label, false) {
        let _ = preferences().set_flag(preference, !value);
    }
}

/// A checkbox row over a git configuration boolean; `inverted`: checked while it is false.
pub(super) fn git_checkbox(
    page: &mut Page<'_, '_>,
    r: &mut Row,
    id: &str,
    scope: Scope,
    key: &str,
    fallback: bool,
    inverted: bool,
) {
    let disabled = scope == Scope::Repo && page.app.active().is_none();
    let checked = git_flag(page.app, scope, key, fallback) != inverted;
    if row_checkbox(page, r, id, checked, "", disabled) {
        let value = if !checked != inverted {
            "true"
        } else {
            "false"
        };
        set_git_value(page.app, scope, key, value);
    }
}

/// A checkbox row bound to a setting.
pub(super) fn setting_checkbox(
    page: &mut Page<'_, '_>,
    r: &mut Row,
    id: &str,
    setting: fn(&mut Settings) -> &mut bool,
) {
    let value = *setting(&mut page.app.settings);
    if row_checkbox(page, r, id, value, "", false) {
        *setting(&mut page.app.settings) = !value;
        save_settings(page.app);
    }
}

/// The edit buffer of the field `id`: kept between frames, refilled from `current` while the field is not being
/// edited.
fn take_buffer(
    page: &mut Page<'_, '_>,
    id: &str,
    current: impl FnOnce(&mut Application) -> String,
) -> (u32, String) {
    let key = page.cx.id(id);
    let mut buffer = page
        .app
        .panels
        .preferences
        .buffers
        .remove(&key)
        .unwrap_or_default();
    if !super::controls::focused(page.cx, id) {
        buffer = current(page.app);
    }
    (key, buffer)
}

/// A form-control text field backed by a stored value: edited in a buffer, stored as it changes.
pub(super) fn stored_field(
    page: &mut Page<'_, '_>,
    r: &mut Row,
    id: &str,
    preference: Preference,
    field: &Field<'_>,
    width: f32,
) {
    let (key, mut buffer) = take_buffer(page, id, |_| value_of(preference));
    if page.text_field(id, r.x(), r.top, width, &mut buffer, field, 34.0) {
        set_value(preference, &buffer);
    }
    page.app.panels.preferences.buffers.insert(key, buffer);
    r.column = 34.0;
}

/// A text field over a git configuration value, written when the field is left (empty: removed).
pub(super) fn git_field(
    page: &mut Page<'_, '_>,
    r: &mut Row,
    id: &str,
    scope: Scope,
    key: &str,
    field: &Field<'_>,
    width: f32,
) {
    let (buffer_key, mut buffer) = take_buffer(page, id, |app| git_value(app, scope, key));
    page.text_field(id, r.x(), r.top, width, &mut buffer, field, 34.0);
    if page.cx.ui.is_item_deactivated_after_edit() {
        set_git_value(page.app, scope, key, &buffer);
    }
    page.app
        .panels
        .preferences
        .buffers
        .insert(buffer_key, buffer);
    r.column = 34.0;
}

/// A number field bound to a setting: edited as text, applied when the field is left.
pub(super) fn setting_number(
    page: &mut Page<'_, '_>,
    r: &mut Row,
    id: &str,
    value: i32,
    apply: impl FnOnce(&mut Page<'_, '_>, i32),
) {
    let (key, mut buffer) = take_buffer(page, id, |_| value.to_string());
    let field = Field::default().flags(InputTextFlags::CHARS_DECIMAL);
    page.text_field(id, r.x(), r.top, 300.0, &mut buffer, &field, 34.0);
    if page.cx.ui.is_item_deactivated_after_edit() {
        apply(page, atoi(&buffer));
        save_settings(page.app);
    }
    page.app.panels.preferences.buffers.insert(key, buffer);
    r.column = 34.0;
}

/// A native select over a stored value.
pub(super) fn stored_select(
    page: &mut Page<'_, '_>,
    r: &mut Row,
    id: &str,
    preference: Preference,
    options: &[&str],
    width: f32,
) {
    let value = value_of(preference);
    page.native_select(
        id,
        r.x(),
        r.top,
        width,
        &value,
        options,
        move |_, _, option| {
            set_value(preference, option);
        },
    );
    r.column = 34.0;
}

/// A Browse button with the chosen path after it (5px apart, the path on the button's text line), or `unset`
/// while there is none; a set path has a Clear button after it. Returns the path's right edge.
pub(super) fn browse_row(
    page: &mut Page<'_, '_>,
    r: &mut Row,
    preference: Preference,
    unset: &str,
    extension: &str,
) -> f32 {
    if page.button(
        "##browse",
        r.x(),
        r.top,
        "Browse",
        Tone::Primary,
        BTN,
        false,
    ) {
        let extension = (!extension.is_empty()).then_some(extension);
        if let Some(path) =
            imgit_platform::shell::open_file_dialog(Some(page.cx.shell.window), extension)
        {
            set_value(preference, &path.to_string_lossy());
        }
    }
    let browse_w = 26.0 + page.measure(Style::ink(12.0, 0.75), "Browse");
    let path = value_of(preference);
    let (style, shown) = if path.is_empty() {
        (HELP, unset)
    } else {
        (LABEL, path.as_str())
    };
    let mut right =
        r.x() + browse_w + 5.0 + page.text(r.x() + browse_w + 5.0, r.top + 4.32, style, shown);
    if !path.is_empty() {
        if page.button(
            "##clear",
            right + 10.0,
            r.top,
            "Clear",
            Tone::Default,
            BTN,
            false,
        ) {
            set_value(preference, "");
        }
        right += 10.0 + 26.0 + page.measure(Style::ink(12.0, 0.75), "Clear");
    }
    r.column = 31.140_625;
    right
}

/// div.sub-setting-group (a clearfix) with its header: the h4's margins stay inside, 5px under the header.
pub(super) fn group(
    page: &mut Page<'_, '_>,
    form: &mut Form,
    name: &str,
    body: impl FnOnce(&mut Page<'_, '_>, &mut Form),
) {
    let top = form.place(0.0);
    form.advance(page, top, 0.0, 0.0);
    heading(page, form, name, Face::Semibold);
    body(page, form);
    form.contain();
    let y = form.y;
    form.advance(page, y, 0.0, 15.0);
}
