//! The Profiles page: the "keep my .gitconfig updated" panel, the profiles table with its row menus, and the
//! Add / Edit / Delete profile modals (react-bootstrap Modal.Dialog).

use dear_imgui_rs::{Condition, Key, StyleVar, WindowFlags};
use imgit_app::profiles::Profile;

use super::controls::{BTN, Field, Tone};
use super::form::{Form, paragraph, title};
use super::page::{BoxStyle, Face, HELP, LABEL, Page, Style};
use crate::app::application::Application;
use crate::app::dialog::ProfileDialogKind;
use crate::theme::Color;
use crate::ui::draw::DrawList;
use crate::ui::popover::draw_box_shadow;
use crate::ui::profile_widgets::draw_profile_icon;
use crate::ui::{Cx, Vec2, raw, vec2};

/// The current profile's author as the placeholders of a new profile.
fn open_add_profile(app: &mut Application) {
    // openAddProfileModal: the current profile's author as the placeholders.
    let current = app.profiles.current().clone();
    let d = &mut app.profile_dialog;
    d.close();
    d.kind = Some(ProfileDialogKind::AddEdit);
    d.just_opened = true;
    d.defaults = Profile {
        guid: String::new(),
        name: "New Profile".to_owned(),
        user_name: current.user_name,
        user_email: current.user_email,
    };
}

fn open_edit_profile(app: &mut Application, profile: &Profile) {
    let d = &mut app.profile_dialog;
    d.close();
    d.kind = Some(ProfileDialogKind::AddEdit);
    d.editing = true;
    d.just_opened = true;
    d.values = profile.clone();
    d.defaults = profile.clone();
    d.gravatar_email = profile.user_email.clone();
}

fn open_delete_profile(app: &mut Application, profile: &Profile) {
    let d = &mut app.profile_dialog;
    d.close();
    d.kind = Some(ProfileDialogKind::Delete);
    d.just_opened = true;
    d.values = profile.clone();
}

const ROW_MENU: &str = "##profile_row_menu";

/// The profile row's DropdownButton menu (pullRight, 160px): Switch to this profile, Edit Profile, Delete.
fn profile_row_menu(page: &mut Page<'_, '_>) {
    let ui = page.cx.ui;
    if !ui.is_popup_open(ROW_MENU) {
        return;
    }
    let guid = page.app.panels.preferences.menu_guid.clone();
    let profile = page.app.profiles.find(&guid).cloned();
    let (_, button_max) = page.app.panels.preferences.menu_button;
    /// 12px text, 17.14px line, 10px padding.
    const ITEM: f32 = 37.140_625;
    let min = vec2(button_max.x - page.px(160.0), button_max.y);
    let max = vec2(button_max.x, min.y + page.px(ITEM * 3.0));
    raw::set_next_window_pos(ui, min, Vec2::ZERO);
    raw::set_next_window_size(ui, max - min);
    // Held for the whole popup: the safe API pops style in the scope it pushed it.
    let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
    let _border = ui.push_style_var(StyleVar::PopupBorderSize(0.0));
    let _spacing = ui.push_style_var(StyleVar::ItemSpacing([0.0, 0.0]));
    let _background = ui.push_style_color(dear_imgui_rs::StyleColor::PopupBg, Color::TRANSPARENT);
    let Some(popup) = ui.begin_popup_with_flags(ROW_MENU, WindowFlags::NO_MOVE) else {
        return;
    };
    let draw = page.cx.window_draw_list();
    let bg1 = page.cx.palette.panel_bg1;
    let (red, hover_row) = (page.cx.palette.red, page.cx.palette.hover_row);
    draw.push_clip_rect_full_screen();
    draw_box_shadow(
        draw,
        min,
        max,
        vec2(0.0, page.px(6.0)),
        page.px(12.0),
        0.0,
        Color::new(0.0, 0.0, 0.0, 0.176),
        0.0,
    );
    draw.add_rect_filled(min, max, bg1, 0.0);
    // Bootstrap's 1px rgba(0, 0, 0, .15) border shows at the sides.
    let side = Color::new(0.0, 0.0, 0.0, 0.15);
    draw.add_rect_filled(min, vec2(min.x + page.px(1.0), max.y), side, 0.0);
    draw.add_rect_filled(vec2(max.x - page.px(1.0), min.y), max, side, 0.0);
    draw.pop_clip_rect();
    let is_current = profile.is_some() && guid == page.app.profiles.current().guid;
    let items = [
        ("Switch to this profile", is_current, Style::ink(12.0, 0.75)),
        ("Edit Profile", false, Style::ink(12.0, 0.75)),
        ("Delete", is_current, Style::fixed(12.0, red)),
    ];
    let mut chosen = None;
    {
        let mut menu = page.sub(draw, min, 160.0);
        for (i, (label, disabled, style)) in items.into_iter().enumerate() {
            let top = ITEM * i as f32;
            let clicked = menu.hit(label, 1.0, top, 158.0, ITEM, !disabled);
            if menu.cx.ui.is_item_hovered() && !disabled {
                menu.boxed(1.0, top, 158.0, ITEM, BoxStyle::fill(hover_row));
            }
            let style = if disabled && i == 0 {
                Style::ink(12.0, 0.4)
            } else {
                style
            };
            menu.text(
                21.0,
                top + 10.0 + Page::leading(style, 17.140_625),
                style,
                label,
            );
            if clicked {
                chosen = Some(i);
            }
        }
    }
    if ui.is_key_pressed(Key::Escape) || chosen.is_some() {
        ui.close_current_popup();
    }
    drop(popup);
    let (Some(profile), Some(chosen)) = (profile, chosen) else {
        return;
    };
    match chosen {
        0 => page.app.switch_profile(&profile.guid),
        1 => open_edit_profile(page.app, &profile),
        _ => open_delete_profile(page.app, &profile),
    }
}

pub(super) fn profiles_page(page: &mut Page<'_, '_>) {
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "Profiles");
    paragraph(
        page,
        form,
        "ImGit Desktop uses profiles to store your application preferences as well as git config information. \
         The preferences you set after installing are set as your default profile. Create additional profiles \
         to quickly switch between them for different work environments.",
        10.0,
    );
    // div.panel-bg1.py1.px3.my3: as wide as its text.
    let heading_style = Style::ink(18.0, 0.75);
    let keep = "Keep my .gitconfig updated with my ImGit Desktop Profile preferences";
    let note = "When checked, ImGit Desktop will update your global .gitconfig to match ImGit Preferences, \
                including commit author name and email, and any GPG settings.";
    let panel_top = form.place(20.0);
    let panel_w = 40.0 + (page.measure(LABEL, keep) + 20.0).max(page.measure(HELP, note));
    let bg1 = page.cx.palette.panel_bg1;
    page.boxed(
        55.0,
        panel_top,
        panel_w,
        96.94,
        BoxStyle::fill(bg1).with_radius(2.0),
    );
    page.text(75.0, panel_top + 12.0, heading_style, "My Profiles");
    let keep_config = page.app.profiles.keep_git_config();
    let keep_width = 8.0 + page.measure(LABEL, keep);
    if page.checkbox(
        "##keep_config",
        75.0,
        panel_top + 48.8,
        keep_config,
        false,
        keep_width,
    ) {
        page.app.set_keep_git_config(!keep_config);
    }
    page.text(95.0, panel_top + 44.8, LABEL, keep);
    page.text(75.0, panel_top + 64.8, HELP, note);
    form.advance(page, panel_top, 96.94, 20.0);

    let add_top = form.place(0.0);
    let green = page.cx.palette.green;
    page.boxed(
        55.0,
        add_top,
        112.45,
        30.0,
        BoxStyle::new(green.with_alpha(0.2), green, 2.0),
    );
    let ink = page.ink(0.75);
    page.icon("far:plus", 66.0, add_top + 9.0, 9.0, 12.0, 12.0, ink);
    page.text(85.0, add_top + 6.0, Style::ink(12.0, 0.75), "Add a Profile");
    if page.hit("##add_profile", 55.0, add_top, 112.45, 30.0, true) {
        open_add_profile(page.app);
    }

    // The profiles table: a 41px header with a bottom border, then a panel-bg1 row per profile, sorted by name;
    // the current one is outlined and checked. Its five cells (20% wide, 10px margins) are spread with
    // justify-content: space-between.
    // The table is 1140px wide in a wide window and narrows with it (the cells shrink, the names truncate), so
    // the row menu at its right end stays on screen; the page does not scroll sideways.
    let table_w = (page.width - page.scrollbar - 55.0 - 40.0).clamp(400.0, 1140.0);
    let scale = ((table_w - 70.0) / 992.0).clamp(0.3, 1.0);
    let cell_w = 248.0 * scale;
    let header = add_top + 40.0;
    let rule = page.ink(0.08);
    page.boxed(55.0, header + 40.0, table_w, 1.0, BoxStyle::fill(rule));
    let columns = [
        55.0 + 10.0 * scale,
        55.0 + 285.0 * scale,
        55.0 + 560.0 * scale,
    ];
    let titles = ["Profile Name", "Author Name", "Author Email"];
    for (column, title) in columns.into_iter().zip(titles) {
        page.text(column, header + 10.0, LABEL, title);
    }
    let caret_x = 65.0 + page.measure(LABEL, titles[0]) + 5.0;
    page.icon(
        "fas:caret-up",
        caret_x,
        header + 12.75,
        8.75,
        14.0,
        14.0,
        ink,
    );

    let ui = page.cx.ui;
    let blue = page.cx.palette.accent;
    let mut row_top = header + 51.0;
    let profiles: Vec<Profile> = page.app.profiles.all().to_vec();
    let current_guid = page.app.profiles.current().guid.clone();
    for profile in &profiles {
        let current = profile.guid == current_guid;
        let border = if current { 2.0 } else { 0.0 };
        let height = 50.0 + border * 2.0;
        let gap = (table_w - border * 2.0 - (4.0 * cell_w + 66.0)) / 4.0;
        let cell = |i: usize| 55.0 + border + 10.0 + i as f32 * (cell_w + gap);
        let menu_open =
            ui.is_popup_open(ROW_MENU) && page.app.panels.preferences.menu_guid == profile.guid;
        let hovered = page.hovering(
            page.at(55.0, row_top),
            page.at(55.0 + table_w, row_top + height),
        ) || menu_open;
        if current {
            page.boxed(
                55.0,
                row_top,
                table_w,
                height,
                BoxStyle::new(blue.with_alpha(0.1), blue, 2.0).border_width(2.0),
            );
            page.check_badge(49.0, row_top - 6.0);
        } else {
            let fill = if hovered { blue.with_alpha(0.1) } else { bg1 };
            page.boxed(
                55.0,
                row_top,
                table_w,
                height,
                BoxStyle::fill(fill).with_radius(2.0),
            );
        }
        let content = row_top + border + 10.0;
        let icon_min = page.at(cell(0), content);
        let icon_size = page.px(30.0);
        draw_profile_icon(page.cx, page.draw, icon_min, icon_size, profile);
        page.text_truncated(
            cell(0) + 40.0,
            content + 5.0,
            LABEL,
            &profile.name,
            cell_w - 60.0,
        );
        let dim = Style::ink(14.0, 0.4);
        page.text_truncated(
            cell(1),
            content + 5.0,
            dim,
            &profile.user_name,
            cell_w - 20.0,
        );
        page.text_truncated(
            cell(2),
            content + 5.0,
            dim,
            &profile.user_email,
            cell_w - 20.0,
        );
        let menu_x = cell(3) + cell_w + gap;
        let (open_menu, button_hot) = {
            let _id = ui.push_id(profile.guid.as_str());
            let open_menu = page.hit("##profile_menu", menu_x, content, 46.0, 30.0, true);
            (open_menu, ui.is_item_hovered() || menu_open)
        };
        if open_menu {
            page.app.panels.preferences.menu_guid = profile.guid.clone();
            page.app.panels.preferences.menu_button = (
                page.at(menu_x, content),
                page.at(menu_x + 46.0, content + 30.0),
            );
            ui.open_popup(ROW_MENU);
        }
        let dots = page.ink(if button_hot { 1.0 } else { 0.75 });
        page.icon(
            "far:ellipsis-v",
            menu_x + 13.0,
            content + 7.0,
            20.0,
            16.0,
            16.0,
            dots,
        );
        row_top += height + 10.0;
    }
    profile_row_menu(page);
    page.reach(row_top + 10.0);
}

/// Draws the frame of a modal `width` x `height` CSS px (82px from the top over a 50% black backdrop, panel-bg1
/// with a 1px rgba(0, 0, 0, .2) border and Bootstrap's 0 5px 15px rgba(0, 0, 0, .5) shadow, a 51.56px header);
/// returns its page (positions relative to its top-left) and whether its × was clicked.
#[allow(clippy::too_many_arguments)]
fn modal_frame<'a, 'c>(
    cx: &'a mut Cx<'c>,
    app: &'a mut Application,
    draw: DrawList<'c>,
    width: f32,
    height: f32,
    title_icon: &str,
    icon_width: f32,
    title: &str,
) -> (Page<'a, 'c>, bool) {
    let (work_pos, work_size) = raw::work_area(cx.ui);
    draw.add_rect_filled(
        work_pos,
        work_pos + work_size,
        Color::from_rgba8(0, 0, 0, 128),
        0.0,
    );
    let min = vec2(
        (work_pos.x + (work_size.x - cx.px(width)) / 2.0).round(),
        work_pos.y + cx.px(82.0),
    );
    let mut page = Page::new(cx, app, draw, min, width);
    let max = page.at(width, height);
    draw_box_shadow(
        draw,
        min,
        max,
        vec2(0.0, page.px(5.0)),
        page.px(15.0),
        0.0,
        Color::new(0.0, 0.0, 0.0, 0.5),
        0.0,
    );
    let (bg1, bg2) = (page.cx.palette.panel_bg1, page.cx.palette.panel_bg2);
    page.boxed(
        0.0,
        0.0,
        width,
        height,
        BoxStyle::new(bg1, Color::new(0.0, 0.0, 0.0, 0.2), 0.0),
    );
    page.boxed(1.0, 1.0, width - 2.0, 51.5625, BoxStyle::fill(bg2));
    let title_style = Style::ink(18.0, 0.75);
    let ink = page.ink(0.75);
    page.icon(title_icon, 21.0, 17.265_625, icon_width, 18.0, 18.0, ink);
    page.text(
        21.0 + icon_width + 10.0,
        13.421_875 + Page::leading(title_style, 25.718_75),
        title_style,
        title,
    );
    let close = page.hit("##close", width - 39.5, 11.0, 18.5, 30.5625, true);
    let times = page.ink(if page.cx.ui.is_item_hovered() {
        1.0
    } else {
        0.75
    });
    page.icon("far:times", width - 36.5, 15.5, 12.5, 20.0, 20.0, times);
    (page, close)
}

/// A plausible email: one @, no spaces, a domain with a dot and a two-letter end.
pub(super) fn is_email(email: &str) -> bool {
    let Some(at) = email.find('@') else {
        return false;
    };
    if at == 0 || email[at + 1..].contains('@') || email.contains(' ') {
        return false;
    }
    let domain = &email[at + 1..];
    domain
        .rfind('.')
        .is_some_and(|dot| dot > 0 && domain.len() - dot > 2)
}

fn add_edit_profile_modal<'c>(cx: &mut Cx<'c>, app: &mut Application, draw: DrawList<'c>) {
    let mut d = app.profile_dialog.clone();
    let ui = cx.ui;
    // gravatarEmail follows the email field 500ms after typing stops.
    if d.gravatar_email != d.values.user_email && cx.time() - d.email_changed_at > 0.5 {
        d.gravatar_email = d.values.user_email.clone();
    }
    let valid_email = d.values.user_email.is_empty() || is_email(&d.values.user_email);
    let error_h = if valid_email { 0.0 } else { 17.140_625 };
    let now = cx.time();
    let (mut page, mut close) = modal_frame(
        cx,
        app,
        draw,
        750.0,
        336.703_13 + error_h,
        "far:user",
        15.75,
        if d.editing {
            "Edit Profile"
        } else {
            "Add Profile"
        },
    );

    // The preview (the author's Gravatar or initials), then the fields with their placeholders: what an empty
    // field falls back to.
    let controls = 67.5625;
    let preview = Profile {
        guid: String::new(),
        name: String::new(),
        user_name: if d.values.user_name.is_empty() {
            d.defaults.user_name.clone()
        } else {
            d.values.user_name.clone()
        },
        user_email: if d.gravatar_email.is_empty() {
            d.defaults.user_email.clone()
        } else {
            d.gravatar_email.clone()
        },
    };
    let preview_min = page.at(21.0, controls);
    let preview_size = page.px(55.0);
    draw_profile_icon(page.cx, page.draw, preview_min, preview_size, &preview);
    page.text(86.0, controls, LABEL, "Profile Name");
    let name_field = Field::default().placeholder(&d.defaults.name).inset();
    if d.just_opened {
        ui.set_keyboard_focus_here();
    }
    page.form_text_field(
        "##profile_name",
        86.0,
        controls + 20.0,
        640.0,
        &mut d.values.name,
        &name_field,
    );
    page.text(21.0, controls + 69.0, LABEL, "Author Name");
    let user_field = Field::default().placeholder(&d.defaults.user_name).inset();
    page.form_text_field(
        "##profile_user_name",
        21.0,
        controls + 89.0,
        710.0,
        &mut d.values.user_name,
        &user_field,
    );
    page.text(21.0, controls + 138.0, LABEL, "Author Email");
    let email_field = Field::default().placeholder(&d.defaults.user_email).inset();
    let red = page.cx.palette.red;
    if !valid_email {
        // .has-error
        page.boxed(
            21.0,
            controls + 158.0,
            710.0,
            34.0,
            BoxStyle::new(BoxStyle::CLEAR, red, 4.0),
        );
    }
    if page.form_text_field(
        "##profile_user_email",
        21.0,
        controls + 158.0,
        710.0,
        &mut d.values.user_email,
        &email_field,
    ) {
        d.email_changed_at = now;
    }
    if !valid_email {
        let error = Style::fixed(12.0, red);
        let message = "Must use a valid email.";
        let x = 731.0 - 20.0 - page.measure(error, message);
        page.text(x, controls + 192.0, error, message);
    }

    // The footer: "Copy settings from current profile" (adding only), Cancel and the submit button, aligned to
    // the right on their baseline.
    let footer = controls + 222.0 + error_h;
    let submit_label = if d.editing {
        "Save Changes"
    } else {
        "Add Profile"
    };
    let button_style = Style::ink(12.0, 0.75);
    let submit_w = 26.0 + page.measure(button_style, submit_label);
    let cancel_w = 26.0 + page.measure(button_style, "Cancel");
    let submit_x = 729.0 - submit_w;
    let cancel_x = submit_x - 10.0 - cancel_w;
    if !d.editing {
        let copy = "Copy settings from current profile";
        let copy_x = cancel_x - 10.0 - (20.0 + page.measure(LABEL, copy));
        let copy_width = 8.0 + page.measure(LABEL, copy);
        if page.checkbox(
            "##copy_profile",
            copy_x,
            footer + 7.33,
            d.copy_current,
            false,
            copy_width,
        ) {
            d.copy_current = !d.copy_current;
        }
        page.text(copy_x + 20.0, footer + 3.33, LABEL, copy);
    }
    let can_submit = !(d.defaults.name.is_empty() && d.values.name.is_empty())
        && !(d.defaults.user_name.is_empty() && d.values.user_name.is_empty())
        && !(d.defaults.user_email.is_empty() && d.values.user_email.is_empty())
        && valid_email;
    if page.button(
        "##cancel_profile",
        cancel_x,
        footer,
        "Cancel",
        Tone::Default,
        BTN,
        false,
    ) {
        close = true;
    }
    let mut submit = page.button(
        "##submit_profile",
        submit_x,
        footer,
        submit_label,
        Tone::Success,
        BTN,
        !can_submit,
    );
    submit = submit || (can_submit && ui.is_key_pressed(Key::Enter));
    d.just_opened = false;
    if ui.is_key_pressed(Key::Escape) {
        close = true;
    }
    if submit {
        // The defaults stand in for the fields left empty.
        let mut profile = d.values.clone();
        for (field, fallback) in [
            (&mut profile.name, &d.defaults.name),
            (&mut profile.user_name, &d.defaults.user_name),
            (&mut profile.user_email, &d.defaults.user_email),
        ] {
            if field.is_empty() {
                field.clone_from(fallback);
            }
        }
        if d.editing {
            page.app.edit_profile(&profile);
        } else {
            page.app.add_profile(profile, d.copy_current);
        }
        close = true;
    }
    if close {
        d.close();
    }
    page.app.profile_dialog = d;
}

fn delete_profile_modal<'c>(cx: &mut Cx<'c>, app: &mut Application, draw: DrawList<'c>) {
    let mut d = app.profile_dialog.clone();
    let ui = cx.ui;
    let (mut page, mut close) = modal_frame(
        cx,
        app,
        draw,
        600.0,
        244.703_13,
        "far:users",
        22.5,
        "Delete profile",
    );
    // "Are you sure you want to **delete** this profile?"
    let mut x = 41.0;
    x += page.text(x, 82.5625, LABEL, "Are you sure you want to ");
    x += page.text(
        x,
        82.5625,
        Style::ink(14.0, 0.75).with_face(Face::Semibold),
        "delete",
    );
    page.text(x, 82.5625, LABEL, " this profile?");
    let icon_min = page.at(41.0, 112.5625);
    let icon_size = page.px(30.0);
    draw_profile_icon(page.cx, page.draw, icon_min, icon_size, &d.values);
    page.text_truncated(81.0, 117.5625, LABEL, &d.values.name, 478.0);
    let label = "Delete profile";
    let w = 26.0 + page.measure(Style::ink(12.0, 0.75), label);
    let confirm = page.button(
        "##delete_profile",
        559.0 - w,
        152.5625,
        label,
        Tone::Danger,
        BTN,
        false,
    );
    let warning = "This action cannot be undone";
    let warning_x = 559.0 - page.measure(LABEL, warning);
    page.text(warning_x, 193.703_13, Style::ink(14.0, 0.4), warning);
    d.just_opened = false;
    if confirm {
        page.app.delete_profile(&d.values.guid);
        close = true;
    }
    if close || ui.is_key_pressed(Key::Escape) {
        d.close();
    }
    page.app.profile_dialog = d;
}

/// The profile modals, over everything in a window of their own like the command palette.
pub(super) fn draw_profile_dialogs(cx: &mut Cx<'_>, app: &mut Application) {
    let Some(kind) = app.profile_dialog.kind else {
        return;
    };
    let ui = cx.ui;
    let (work_pos, work_size) = raw::work_area(ui);
    raw::set_next_window_focus(ui);
    let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
    let _border = ui.push_style_var(StyleVar::WindowBorderSize(0.0));
    let _spacing = ui.push_style_var(StyleVar::ItemSpacing([0.0, 0.0]));
    ui.window("##profile_modal")
        .position(work_pos.into(), Condition::Always)
        .size(work_size.into(), Condition::Always)
        .flags(
            WindowFlags::NO_DECORATION
                | WindowFlags::NO_MOVE
                | WindowFlags::NO_SAVED_SETTINGS
                | WindowFlags::NO_BACKGROUND
                | WindowFlags::NO_NAV
                | WindowFlags::NO_SCROLL_WITH_MOUSE,
        )
        .build(|| {
            let draw = cx.window_draw_list();
            match kind {
                ProfileDialogKind::AddEdit => add_edit_profile_modal(cx, app, draw),
                ProfileDialogKind::Delete => delete_profile_modal(cx, app, draw),
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails() {
        assert!(is_email("ada@example.com"));
        assert!(is_email("a@b.co"));
        assert!(!is_email("ada"));
        assert!(!is_email("@example.com"));
        assert!(!is_email("ada@example"));
        assert!(!is_email("ada@example.c"));
        assert!(!is_email("ada@@example.com"));
        assert!(!is_email("ada @example.com"));
        assert!(!is_email("ada@.com"));
    }
}
