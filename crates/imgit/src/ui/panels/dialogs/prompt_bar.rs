//! The prompt bar: the 48px strip over the toolbar that asks for a confirmation, a name, the upstream of a
//! pushed branch or credentials (`.user-input` in the web app). It is not a modal: the rest of the window stays
//! usable.

use dear_imgui_rs::{
    Condition, InputTextCallback, InputTextCallbackHandler, InputTextFlags, Key, StyleColor,
    StyleVar, TextCallbackData, WindowFlags,
};
use imgit_git::Remote;

use crate::app::credentials::CredentialPrompt;
use crate::app::dialog::{Dialog, DialogKind};
use crate::fonts::Font;
use crate::theme::{Color, TABS_BAR_HEIGHT, TOOLBAR_HEIGHT};
use crate::ui::{Cx, DrawList, Vec2, raw, vec2};

/// What the bar answered this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Answer {
    /// Still open.
    #[default]
    Pending,
    /// Confirmed (the primary button, Enter).
    Yes,
    /// Cancelled (the Cancel button, Escape).
    No,
}

/// Where the bar sits: under the tabs bar, as wide as the window, as high as the toolbar.
struct Bar {
    min: Vec2,
    size: Vec2,
}

impl Bar {
    fn new(cx: &Cx<'_>) -> Self {
        let (work_pos, _) = raw::work_area(cx.ui);
        let (_, viewport_size) = raw::viewport(cx.ui);
        Self {
            // Under the menu bar, if any.
            min: vec2(work_pos.x, work_pos.y + cx.px(TABS_BAR_HEIGHT)),
            size: vec2(viewport_size.x, cx.px(TOOLBAR_HEIGHT)),
        }
    }

    /// Where the content starts: centred, but at least 10px from the left.
    fn start_x(&self, cx: &Cx<'_>, total: f32) -> f32 {
        (self.min.x + cx.px(10.0).max((self.size.x - total) / 2.0)).round()
    }
}

/// What a bar window draws with.
struct Pen {
    /// The text colour: the ink at 90%.
    text: Color,
    field_bg: Color,
    field_border: Color,
}

impl Pen {
    fn new(cx: &Cx<'_>) -> Self {
        Self {
            text: cx.palette.ink_255(230),
            field_bg: cx.palette.input_bg,
            field_border: cx.palette.ink_255(51),
        }
    }
}

/// Where a line of text goes: its font and CSS size, the left edge and the top of its line box (device px).
#[derive(Clone, Copy)]
struct Spot {
    font: Font,
    size: f32,
    x: f32,
    top: f32,
}

/// Text whose line box (Chromium's) starts at `box_top`: placed on the baseline Chromium rounds to.
fn text_at(cx: &Cx<'_>, draw: DrawList<'_>, spot: Spot, color: Color, label: &str) {
    let ascent = spot.size * 2189.0 / 2048.0 * cx.scale;
    cx.draw_text(
        draw,
        spot.font,
        spot.size,
        vec2(spot.x, (spot.top + ascent.round()).round() - ascent.ceil()),
        color,
        label,
    );
}

/// The bar's window: the toolbar's colour under the accent at 20%, in front of every other window. `focus`
/// takes the keyboard focus (the frame the bar opens). `body` draws the content and returns the answer.
fn bar_window<R: Default>(
    cx: &mut Cx<'_>,
    name: &str,
    focus_on_appearing: bool,
    focus: bool,
    body: impl FnOnce(&mut Cx<'_>, DrawList<'_>, &Bar) -> R,
) -> R {
    let ui = cx.ui;
    let bar = Bar::new(cx);
    let mut flags = WindowFlags::NO_DECORATION
        | WindowFlags::NO_MOVE
        | WindowFlags::NO_SAVED_SETTINGS
        | WindowFlags::NO_BACKGROUND;
    if !focus_on_appearing {
        flags |= WindowFlags::NO_FOCUS_ON_APPEARING;
    }
    let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
    let _border = ui.push_style_var(StyleVar::WindowBorderSize(0.0));
    let _rounding = ui.push_style_var(StyleVar::WindowRounding(0.0));
    ui.window(name)
        .position(bar.min.into(), Condition::Always)
        .size(bar.size.into(), Condition::Always)
        .flags(flags)
        .build(|| {
            raw::bring_current_window_to_display_front(ui);
            if focus {
                ui.set_window_focus(None);
            }
            let draw = cx.window_draw_list();
            let max = bar.min + bar.size;
            draw.add_rect_filled(bar.min, max, cx.palette.toolbar_bg1, 0.0);
            draw.add_rect_filled(bar.min, max, cx.palette.accent.with_alpha(0.2), 0.0);
            body(cx, draw, &bar)
        })
        .unwrap_or_default()
}

/// A button of the bar: 22px high, 12px bold text.
enum ButtonKind {
    /// A tinted one; dimmed while not `enabled`.
    Primary { tone: Color, enabled: bool },
    /// Outlined in the ink at `border` / 255.
    Secondary { border: u8 },
}

/// A button of the bar: its id, label, width and look.
struct Button<'a> {
    id: &'a str,
    label: &'a str,
    width: f32,
    kind: &'a ButtonKind,
}

/// Draws the button at `*x` and moves `*x` on, 12px past it. `true` when clicked.
fn bar_button(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    pen: &Pen,
    x: &mut f32,
    top: f32,
    button: Button<'_>,
) -> bool {
    let Button {
        id,
        label,
        width,
        kind,
    } = button;
    let min = vec2(*x, top + cx.px(13.0));
    let max = vec2(*x + width, min.y + cx.px(22.0));
    cx.set_cursor(min);
    let clicked = cx.ui.invisible_button(id, max - min);
    let hovered = cx.ui.is_item_hovered();
    let rounding = cx.px(2.0);
    let label_color = match *kind {
        ButtonKind::Primary { tone, enabled } => {
            let alpha = match (enabled, hovered) {
                (false, _) => 0.1,
                (true, true) => 0.3,
                (true, false) => 0.2,
            };
            draw.add_rect_filled(min, max, tone.with_alpha(alpha), rounding);
            let border = if enabled { tone } else { tone.with_alpha(0.5) };
            draw.add_rect(min, max, border, rounding, 1.0);
            if enabled {
                pen.text
            } else {
                cx.palette.ink_255(115)
            }
        }
        ButtonKind::Secondary { border } => {
            if hovered {
                draw.add_rect_filled(min, max, cx.palette.ink(0.1), rounding);
            }
            draw.add_rect(min, max, cx.palette.ink_255(border), rounding, 1.0);
            pen.text
        }
    };
    text_at(
        cx,
        draw,
        Spot {
            font: cx.fonts.bold,
            size: 12.0,
            x: min.x + cx.px(6.0),
            top: min.y + cx.px(2.0),
        },
        label_color,
        label,
    );
    *x += width + cx.px(12.0);
    clicked
}

/// The width of a bar button holding `label`: the bold 12px text and 6px either side.
fn button_width(cx: &Cx<'_>, label: &str) -> f32 {
    cx.text_size(cx.fonts.bold, 12.0, label).x + cx.px(12.0)
}

/// Draws a 34px field over `[min, min + width]` in the bar through `field`: 12px frame padding, no frame, the
/// regular 14px font. The caller draws the box.
fn bar_field<R>(cx: &mut Cx<'_>, min: Vec2, field: impl FnOnce(&mut Cx<'_>) -> R) -> R {
    let ui = cx.ui;
    cx.set_cursor(min);
    let line = cx.text_size(cx.fonts.regular, 14.0, "A").y;
    let _padding = ui.push_style_var(StyleVar::FramePadding([
        cx.px(12.0),
        (cx.px(34.0) - line) / 2.0,
    ]));
    let _border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
    let _bg = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
    let _hovered = ui.push_style_color(StyleColor::FrameBgHovered, Color::TRANSPARENT);
    let _active = ui.push_style_color(StyleColor::FrameBgActive, Color::TRANSPARENT);
    let _hint = ui.push_style_color(StyleColor::TextDisabled, cx.palette.text_disabled);
    let _font = cx.push_font(cx.fonts.regular, 14.0);
    field(cx)
}

/// The confirm prompt (`.user-input` over the toolbar): the message at 14px, 10px on the xs buttons (12px bold,
/// 12px apart, 8px from the end), all centred.
pub fn draw_prompt_bar(
    cx: &mut Cx<'_>,
    message: &str,
    confirm_label: &str,
    danger: bool,
) -> Answer {
    bar_window(cx, "##confirm_prompt", false, false, |cx, draw, bar| {
        let ui = cx.ui;
        let pen = Pen::new(cx);
        let cancel = "Cancel";
        let message_w = cx.text_size(cx.fonts.regular, 14.0, message).x;
        let confirm_w = button_width(cx, confirm_label);
        let cancel_w = button_width(cx, cancel);
        let total =
            message_w + cx.px(10.0) + cx.px(12.0) + confirm_w + cx.px(12.0) + cancel_w + cx.px(8.0);
        let mut x = bar.start_x(cx, total);
        text_at(
            cx,
            draw,
            Spot {
                font: cx.fonts.regular,
                size: 14.0,
                x,
                top: bar.min.y + cx.px(14.0),
            },
            pen.text,
            message,
        );
        x += message_w + cx.px(10.0) + cx.px(12.0);
        let tone = if danger {
            cx.palette.red
        } else {
            cx.palette.green
        };
        let mut answer = Answer::Pending;
        let primary = ButtonKind::Primary {
            tone,
            enabled: true,
        };
        if bar_button(
            cx,
            draw,
            &pen,
            &mut x,
            bar.min.y,
            Button {
                id: "##confirm",
                label: confirm_label,
                width: confirm_w,
                kind: &primary,
            },
        ) {
            answer = Answer::Yes;
        }
        // Cancel has the focus when the prompt opens, so Enter answers it too.
        let keyed = ui.is_key_pressed(Key::Escape)
            || (!ui.io().want_text_input()
                && (ui.is_key_pressed(Key::Enter)
                    || ui.is_key_pressed(Key::KeypadEnter)
                    || ui.is_key_pressed(Key::Space)));
        let secondary = ButtonKind::Secondary { border: 255 };
        if bar_button(
            cx,
            draw,
            &pen,
            &mut x,
            bar.min.y,
            Button {
                id: "##cancel",
                label: cancel,
                width: cancel_w,
                kind: &secondary,
            },
        ) || keyed
        {
            answer = Answer::No;
        }
        answer
    })
}

/// The question the set-upstream prompt asks for `branch`.
fn push_question(branch: &str) -> String {
    format!("What remote/branch should \"{branch}\" push to and pull from?")
}

/// The set-upstream prompt for a branch without one: the question, the remote react-select (150x34, focused:
/// #2684FF ring), an 18px "/", the branch name field (191x34, the branch's own name as placeholder), Submit and
/// Cancel. `select_focused` is the select's focus (it has it when the prompt opens); `d.flag` is its menu being
/// open, `d.text_a` the remote chosen, `d.text_b` the remote branch.
pub fn draw_push_prompt(
    cx: &mut Cx<'_>,
    d: &mut Dialog,
    select_focused: &mut bool,
    remotes: &[Remote],
) -> Answer {
    let (answer, select_min, select_max) =
        bar_window(cx, "##push_prompt", true, false, |cx, draw, bar| {
            let ui = cx.ui;
            let pen = Pen::new(cx);
            let message = push_question(&d.ref_name);
            let message_w = cx.text_size(cx.fonts.regular, 14.0, &message).x;
            let slash_w = cx.text_size(cx.fonts.regular, 18.0, "/").x;
            let submit_w = button_width(cx, "Submit");
            let cancel_w = button_width(cx, "Cancel");
            let actions_w = cx.px(150.0)
                + cx.px(7.5)
                + slash_w
                + cx.px(7.5)
                + cx.px(191.0)
                + cx.px(12.0)
                + submit_w
                + cx.px(12.0)
                + cancel_w
                + cx.px(8.0);
            let total = message_w + cx.px(10.0) + actions_w;
            let mut x = bar.start_x(cx, total);
            text_at(
                cx,
                draw,
                Spot {
                    font: cx.fonts.regular,
                    size: 14.0,
                    x,
                    top: bar.min.y + cx.px(14.0),
                },
                pen.text,
                &message,
            );
            x += message_w + cx.px(10.0);

            // The remote select: value, a 1px separator, the chevron; a menu of the remotes under it.
            let smin = vec2(x.round(), bar.min.y + cx.px(7.0));
            let smax = smin + vec2(cx.px(150.0), cx.px(34.0));
            cx.set_cursor(smin);
            if ui.invisible_button("##push_remote", smax - smin) {
                d.flag = !d.flag;
                *select_focused = true;
            }
            if d.just_opened {
                *select_focused = true;
                d.just_opened = false;
            }
            draw.add_rect_filled(smin, smax, pen.field_bg, cx.px(4.0));
            draw.add_rect(smin, smax, pen.field_border, cx.px(4.0), 1.0);
            if *select_focused {
                draw.add_rect(
                    smin - vec2(1.0, 1.0),
                    smax + vec2(1.0, 1.0),
                    cx.palette.accent,
                    cx.px(5.0),
                    1.0,
                );
            }
            text_at(
                cx,
                draw,
                Spot {
                    font: cx.fonts.regular,
                    size: 14.0,
                    x: smin.x + cx.px(11.0),
                    top: smin.y + cx.px(7.0),
                },
                pen.text,
                &d.text_a,
            );
            // The select's search input has the caret, in front of the value (Chromium blinks it every 500ms).
            if *select_focused && cx.time().rem_euclid(1.0) < 0.5 {
                let caret_x = (smin.x + cx.px(11.0)).round();
                draw.add_rect_filled(
                    vec2(caret_x, smin.y + cx.px(8.0)),
                    vec2(caret_x + 1.0, smin.y + cx.px(26.0)),
                    pen.text,
                    0.0,
                );
            }
            draw.add_rect_filled(
                vec2(smin.x + cx.px(120.0), smin.y + cx.px(7.0)),
                vec2(smin.x + cx.px(121.0), smin.y + cx.px(27.0)),
                pen.field_border,
                0.0,
            );
            if let Some(path) = crate::ui::icons::svg_path("chevron-down") {
                cx.draw_svg_path(
                    draw,
                    "chevron-down",
                    path,
                    20.0,
                    20.0,
                    vec2(smin.x + cx.px(125.0), smin.y + cx.px(7.0)),
                    vec2(smin.x + cx.px(145.0), smin.y + cx.px(27.0)),
                    pen.text,
                );
            }
            x = smax.x + cx.px(7.5);
            text_at(
                cx,
                draw,
                Spot {
                    font: cx.fonts.regular,
                    size: 18.0,
                    x,
                    top: bar.min.y + cx.px(11.14),
                },
                pen.text,
                "/",
            );
            x += slash_w + cx.px(7.5);

            // The branch name: empty pushes to a branch of the same name.
            let imin = vec2(x.round(), bar.min.y + cx.px(7.0));
            let imax = imin + vec2(cx.px(191.0), cx.px(34.0));
            draw.add_rect_filled(imin, imax, pen.field_bg, cx.px(4.0));
            // The field covers the whole box; its 12px padding is the frame padding.
            let (entered, name_active) = bar_field(cx, imin, |cx| {
                let entered = cx.input_text(
                    "##push_branch",
                    &mut d.text_b,
                    Some(&d.ref_name),
                    InputTextFlags::ENTER_RETURNS_TRUE,
                    imax.x - imin.x,
                );
                (entered, ui.is_item_active())
            });
            if name_active {
                *select_focused = false;
                d.flag = false;
            }
            let border = if name_active {
                cx.palette.accent
            } else {
                pen.field_border
            };
            draw.add_rect(imin, imax, border, cx.px(4.0), 1.0);
            x = imax.x + cx.px(12.0);

            let mut answer = Answer::Pending;
            let primary = ButtonKind::Primary {
                tone: cx.palette.green,
                enabled: true,
            };
            let submit_by_enter = *select_focused && !d.flag && ui.is_key_pressed(Key::Enter);
            if bar_button(
                cx,
                draw,
                &pen,
                &mut x,
                bar.min.y,
                Button {
                    id: "##push_submit",
                    label: "Submit",
                    width: submit_w,
                    kind: &primary,
                },
            ) || entered
                || submit_by_enter
            {
                answer = Answer::Yes;
            }
            let secondary = ButtonKind::Secondary { border: 191 };
            let escape = !d.flag && ui.is_key_pressed(Key::Escape);
            if bar_button(
                cx,
                draw,
                &pen,
                &mut x,
                bar.min.y,
                Button {
                    id: "##push_cancel",
                    label: "Cancel",
                    width: cancel_w,
                    kind: &secondary,
                },
            ) || escape
            {
                answer = Answer::No;
            }
            (answer, smin, smax)
        });

    if d.flag {
        draw_remote_menu(cx, d, remotes, select_min, select_max);
    }
    answer
}

/// react-select's menu: 8px under the control, toolbar-bg0, 4px padding, 37px options.
fn draw_remote_menu(cx: &mut Cx<'_>, d: &mut Dialog, remotes: &[Remote], smin: Vec2, smax: Vec2) {
    let ui = cx.ui;
    let pen = Pen::new(cx);
    let min = vec2(smin.x, smax.y + cx.px(8.0));
    let option_h = cx.px(37.0);
    let size = vec2(
        smax.x - smin.x,
        cx.px(8.0) + option_h * remotes.len() as f32,
    );
    let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
    let _border = ui.push_style_var(StyleVar::WindowBorderSize(0.0));
    ui.window("##push_remote_menu")
        .position(min.into(), Condition::Always)
        .size(size.into(), Condition::Always)
        .flags(
            WindowFlags::NO_DECORATION
                | WindowFlags::NO_MOVE
                | WindowFlags::NO_SAVED_SETTINGS
                | WindowFlags::NO_BACKGROUND,
        )
        .build(|| {
            raw::bring_current_window_to_display_front(ui);
            let draw = cx.window_draw_list();
            let max = min + size;
            draw.add_rect_filled(min, max, cx.palette.toolbar_bg0, cx.px(4.0));
            draw.add_rect(min, max, pen.field_border, cx.px(4.0), 1.0);
            for (index, remote) in remotes.iter().enumerate() {
                let option_min = vec2(min.x + 1.0, min.y + cx.px(5.0) + option_h * index as f32);
                cx.set_cursor(option_min);
                let picked = {
                    let _id = ui.push_id(index);
                    ui.invisible_button("##remote_option", vec2(size.x - 2.0, option_h))
                };
                if ui.is_item_hovered() || remote.name == d.text_a {
                    draw.add_rect_filled(
                        option_min,
                        option_min + vec2(size.x - 2.0, option_h),
                        cx.palette.hover_row,
                        0.0,
                    );
                }
                text_at(
                    cx,
                    draw,
                    Spot {
                        font: cx.fonts.regular,
                        size: 14.0,
                        x: option_min.x + cx.px(11.0),
                        top: option_min.y + cx.px(8.0),
                    },
                    pen.text,
                    &remote.name,
                );
                if picked {
                    d.text_a.clone_from(&remote.name);
                    d.flag = false;
                }
            }
            let inside = ui.is_mouse_hovering_rect_with_clip(min.into(), max.into(), false)
                || ui.is_mouse_hovering_rect_with_clip(smin.into(), smax.into(), false);
            if ui.is_key_pressed(Key::Escape)
                || (ui.is_mouse_clicked(dear_imgui_rs::MouseButton::Left) && !inside)
            {
                d.flag = false;
            }
        });
}

/// The hosting services by host: the friendly name and what their password field asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostingService {
    pub name: &'static str,
    pub secret: &'static str,
}

/// The service behind `url` (`scheme://host[:port]...`), `None` for any other host.
pub fn hosting_service(url: &str) -> Option<HostingService> {
    let host = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = host.split(':').next().unwrap_or(host);
    let is = |domain: &str| {
        host == domain
            || host
                .strip_suffix(domain)
                .is_some_and(|rest| rest.ends_with('.'))
    };
    const TOKEN: &str = "Personal Access Token";
    if is("github.com") {
        Some(HostingService {
            name: "GitHub",
            secret: TOKEN,
        })
    } else if is("gitlab.com") {
        Some(HostingService {
            name: "GitLab",
            secret: TOKEN,
        })
    } else if is("bitbucket.org") {
        Some(HostingService {
            name: "Bitbucket",
            secret: "App Password",
        })
    } else if is("dev.azure.com") || is("visualstudio.com") {
        Some(HostingService {
            name: "Azure DevOps",
            secret: TOKEN,
        })
    } else {
        None
    }
}

/// What the credentials prompt says: the hosting service's name when it is a known one, else the host.
fn credentials_message(url: &str, rejected: bool) -> String {
    let host = url.split_once("://").map_or(url, |(_, rest)| rest);
    match hosting_service(url) {
        Some(service) => format!(
            "{}Please enter your {} credentials:",
            if rejected {
                "Authentication failed. "
            } else {
                ""
            },
            service.name
        ),
        None if rejected => format!("Invalid credentials for {host}. Please try again:"),
        None => format!("Please enter your credentials for {host}:"),
    }
}

/// The credentials form: the message, Username, Password (or token), Log In, Cancel and Remember me.
pub fn draw_credentials_prompt(cx: &mut Cx<'_>, prompt: &mut CredentialPrompt) -> Answer {
    bar_window(
        cx,
        "##credentials_prompt",
        true,
        prompt.just_opened,
        |cx, draw, bar| {
            let ui = cx.ui;
            let pen = Pen::new(cx);
            let secret = hosting_service(&prompt.url).map_or("Password", |service| service.secret);
            let message = credentials_message(&prompt.url, prompt.rejected);
            let remember_label = "Remember me";
            let field_w = cx.px(191.0);
            let message_w = cx.text_size(cx.fonts.regular, 14.0, &message).x;
            let login_w = button_width(cx, "Log In");
            let cancel_w = button_width(cx, "Cancel");
            let checkbox = cx.px(13.0);
            let remember_w =
                checkbox + cx.px(6.0) + cx.text_size(cx.fonts.regular, 14.0, remember_label).x;
            let total = message_w
                + cx.px(10.0)
                + field_w
                + cx.px(12.0)
                + field_w
                + cx.px(12.0)
                + login_w
                + cx.px(12.0)
                + cancel_w
                + cx.px(12.0)
                + remember_w
                + cx.px(8.0);
            let mut x = bar.start_x(cx, total);
            text_at(
                cx,
                draw,
                Spot {
                    font: cx.fonts.regular,
                    size: 14.0,
                    x,
                    top: bar.min.y + cx.px(14.0),
                },
                pen.text,
                &message,
            );
            x += message_w + cx.px(10.0);

            let mut submitted = false;
            let focus_username = prompt.just_opened && prompt.username.is_empty();
            let focus_password = prompt.just_opened && !prompt.username.is_empty();
            prompt.just_opened = false;
            let field = |cx: &mut Cx<'_>,
                         x: &mut f32,
                         id: &str,
                         value: &mut String,
                         hint: &str,
                         flags: InputTextFlags,
                         focus: bool| {
                let min = vec2(x.round(), bar.min.y + cx.px(7.0));
                let max = min + vec2(field_w, cx.px(34.0));
                draw.add_rect_filled(min, max, pen.field_bg, cx.px(4.0));
                let (entered, active) = bar_field(cx, min, |cx| {
                    if focus {
                        ui.set_keyboard_focus_here();
                    }
                    let entered = cx.input_text(
                        id,
                        value,
                        Some(hint),
                        flags | InputTextFlags::ENTER_RETURNS_TRUE,
                        field_w,
                    );
                    (entered, ui.is_item_active())
                });
                let border = if active {
                    cx.palette.accent
                } else {
                    pen.field_border
                };
                draw.add_rect(min, max, border, cx.px(4.0), 1.0);
                *x = max.x + cx.px(12.0);
                entered
            };
            submitted |= field(
                cx,
                &mut x,
                "##credentials_username",
                &mut prompt.username,
                "Username",
                InputTextFlags::empty(),
                focus_username,
            );
            submitted |= field(
                cx,
                &mut x,
                "##credentials_password",
                &mut prompt.password,
                secret,
                InputTextFlags::PASSWORD,
                focus_password,
            );

            let complete = !prompt.username.is_empty() && !prompt.password.is_empty();
            // Enter submits the form from anywhere in it, not only from the fields.
            submitted |= ui.is_window_focused()
                && (ui.is_key_pressed(Key::Enter) || ui.is_key_pressed(Key::KeypadEnter));
            let mut answer = Answer::Pending;
            let primary = ButtonKind::Primary {
                tone: cx.palette.green,
                enabled: complete,
            };
            let logged_in = bar_button(
                cx,
                draw,
                &pen,
                &mut x,
                bar.min.y,
                Button {
                    id: "##credentials_login",
                    label: "Log In",
                    width: login_w,
                    kind: &primary,
                },
            );
            if (logged_in || submitted) && complete {
                answer = Answer::Yes;
            }
            let secondary = ButtonKind::Secondary { border: 191 };
            let cancelled = bar_button(
                cx,
                draw,
                &pen,
                &mut x,
                bar.min.y,
                Button {
                    id: "##credentials_cancel",
                    label: "Cancel",
                    width: cancel_w,
                    kind: &secondary,
                },
            );
            if cancelled || ui.is_key_pressed(Key::Escape) {
                answer = Answer::No;
            }

            // Remember me: a 13px checkbox and its label, both clickable.
            let min = vec2(x, (bar.min.y + (bar.size.y - checkbox) / 2.0).round());
            cx.set_cursor(vec2(min.x, bar.min.y + cx.px(13.0)));
            if ui.invisible_button("##credentials_remember", vec2(remember_w, cx.px(22.0))) {
                prompt.remember = !prompt.remember;
            }
            let max = min + vec2(checkbox, checkbox);
            if prompt.remember {
                draw.add_rect_filled(min, max, cx.palette.accent, cx.px(2.0));
                draw.add_polyline(
                    &[
                        vec2(min.x + checkbox * 0.22, min.y + checkbox * 0.52),
                        vec2(min.x + checkbox * 0.42, min.y + checkbox * 0.72),
                        vec2(min.x + checkbox * 0.8, min.y + checkbox * 0.3),
                    ],
                    Color::WHITE,
                    false,
                    cx.px(1.6),
                );
            } else {
                draw.add_rect_filled(min, max, pen.field_bg, cx.px(2.0));
                draw.add_rect(min, max, cx.palette.ink_255(140), cx.px(2.0), 1.0);
            }
            text_at(
                cx,
                draw,
                Spot {
                    font: cx.fonts.regular,
                    size: 14.0,
                    x: max.x + cx.px(6.0),
                    top: bar.min.y + cx.px(14.0),
                },
                pen.text,
                remember_label,
            );
            answer
        },
    )
}

/// Moves the caret behind the text the first time the field draws (keyboard focus would select the name).
struct CaretToEnd<'a>(&'a mut bool);

impl InputTextCallbackHandler for CaretToEnd<'_> {
    fn on_always(&mut self, mut data: TextCallbackData<'_>) {
        if *self.0 {
            let end = data.str().len();
            data.set_cursor_pos(end);
            data.set_selection_start(end);
            data.set_selection_end(end);
            *self.0 = false;
        }
    }
}

/// What the name prompt asks.
fn rename_message(d: &Dialog) -> String {
    if d.kind == Some(DialogKind::CreateFile) {
        let folder = if d.text_b.is_empty() {
            "/".to_owned()
        } else {
            format!("{}/", d.text_b)
        };
        format!("Create a file in {folder}:")
    } else if d.kind == Some(DialogKind::TextPrompt) || !d.message.is_empty() {
        d.message.clone()
    } else {
        format!("Rename '{}' to:", d.ref_name)
    }
}

/// The name prompt in the bar (rename a branch, a tab, a stash message, create a file): the message, the name
/// field (230x34, 360 for a stash message; the current name in it, focused with the caret at its end), Submit
/// and Cancel.
pub fn draw_rename_prompt(cx: &mut Cx<'_>, d: &mut Dialog, caret_to_end: &mut bool) -> Answer {
    bar_window(
        cx,
        "##rename_prompt",
        true,
        d.just_opened,
        |cx, draw, bar| {
            let ui = cx.ui;
            let pen = Pen::new(cx);
            let message = rename_message(d);
            let field_w = cx.px(if d.kind == Some(DialogKind::EditStashMessage) {
                360.0
            } else {
                230.0
            });
            let message_w = cx.text_size(cx.fonts.regular, 14.0, &message).x;
            let submit_w = button_width(cx, "Submit");
            let cancel_w = button_width(cx, "Cancel");
            let total = message_w
                + cx.px(10.0)
                + field_w
                + cx.px(12.0)
                + submit_w
                + cx.px(12.0)
                + cancel_w
                + cx.px(8.0);
            let mut x = bar.start_x(cx, total);
            text_at(
                cx,
                draw,
                Spot {
                    font: cx.fonts.regular,
                    size: 14.0,
                    x,
                    top: bar.min.y + cx.px(14.0),
                },
                pen.text,
                &message,
            );
            x += message_w + cx.px(10.0);

            // The name field; keyboard focus would select the name, so the caret goes to its end instead.
            let min = vec2(x.round(), bar.min.y + cx.px(7.0));
            let max = min + vec2(field_w, cx.px(34.0));
            draw.add_rect_filled(min, max, pen.field_bg, cx.px(4.0));
            let (entered, active) = bar_field(cx, min, |cx| {
                if d.just_opened {
                    ui.set_keyboard_focus_here();
                    *caret_to_end = true;
                    d.just_opened = false;
                }
                ui.set_next_item_width(field_w);
                let entered = ui
                    .input_text("##rename_branch", &mut d.text_a)
                    .flags(InputTextFlags::ENTER_RETURNS_TRUE)
                    .callback(CaretToEnd(caret_to_end))
                    .callback_flags(InputTextCallback::ALWAYS)
                    .build();
                if let Some(menu) = cx.state.text_field_menu {
                    menu(cx, &d.text_a);
                }
                (entered, ui.is_item_active())
            });
            let border = if active {
                cx.palette.accent
            } else {
                pen.field_border
            };
            draw.add_rect(min, max, border, cx.px(4.0), 1.0);
            x = max.x + cx.px(12.0);

            // TextPrompt's `flag`: an empty answer is one too (it clears the name).
            let complete =
                !d.text_a.is_empty() || (d.kind == Some(DialogKind::TextPrompt) && d.flag);
            let mut answer = Answer::Pending;
            let primary = ButtonKind::Primary {
                tone: cx.palette.green,
                enabled: complete,
            };
            let submitted = bar_button(
                cx,
                draw,
                &pen,
                &mut x,
                bar.min.y,
                Button {
                    id: "##rename_submit",
                    label: "Submit",
                    width: submit_w,
                    kind: &primary,
                },
            );
            if (submitted || entered) && complete {
                answer = Answer::Yes;
            }
            let secondary = ButtonKind::Secondary { border: 191 };
            let cancelled = bar_button(
                cx,
                draw,
                &pen,
                &mut x,
                bar.min.y,
                Button {
                    id: "##rename_cancel",
                    label: "Cancel",
                    width: cancel_w,
                    kind: &secondary,
                },
            );
            if cancelled || ui.is_key_pressed(Key::Escape) {
                answer = Answer::No;
            }
            answer
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosting_services_are_told_by_host() {
        let github = hosting_service("https://github.com").unwrap();
        assert_eq!(github.name, "GitHub");
        assert_eq!(github.secret, "Personal Access Token");
        assert_eq!(
            hosting_service("https://gist.github.com").map(|s| s.name),
            Some("GitHub")
        );
        assert_eq!(
            hosting_service("https://bitbucket.org").map(|s| s.secret),
            Some("App Password")
        );
        assert_eq!(
            hosting_service("https://dev.azure.com").map(|s| s.name),
            Some("Azure DevOps")
        );
        assert_eq!(
            hosting_service("https://acme.visualstudio.com").map(|s| s.name),
            Some("Azure DevOps")
        );
        // Not a subdomain: the suffix must follow a dot.
        assert_eq!(hosting_service("https://notgithub.com"), None);
        assert_eq!(hosting_service("https://git.example.org:8443"), None);
    }

    #[test]
    fn credentials_messages_name_the_service_or_the_host() {
        assert_eq!(
            credentials_message("https://github.com", false),
            "Please enter your GitHub credentials:"
        );
        assert_eq!(
            credentials_message("https://gitlab.com", true),
            "Authentication failed. Please enter your GitLab credentials:"
        );
        assert_eq!(
            credentials_message("https://git.example.org", false),
            "Please enter your credentials for git.example.org:"
        );
        assert_eq!(
            credentials_message("https://git.example.org", true),
            "Invalid credentials for git.example.org. Please try again:"
        );
    }

    #[test]
    fn the_rename_prompt_asks_what_the_dialog_is_for() {
        let mut d = Dialog::default();
        d.open(DialogKind::RenameBranch);
        d.ref_name = "topic".to_owned();
        assert_eq!(rename_message(&d), "Rename 'topic' to:");
        d.message = "Name the tab:".to_owned();
        assert_eq!(rename_message(&d), "Name the tab:");
        d.open(DialogKind::CreateFile);
        assert_eq!(rename_message(&d), "Create a file in /:");
        d.text_b = "src".to_owned();
        assert_eq!(rename_message(&d), "Create a file in src/:");
    }

    #[test]
    fn the_push_prompt_quotes_the_branch() {
        assert_eq!(
            push_question("main"),
            "What remote/branch should \"main\" push to and pull from?"
        );
    }
}
