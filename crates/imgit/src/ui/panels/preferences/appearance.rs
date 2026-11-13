//! The UI Customization page: the theme, the date formats, the tab group colour and what the graph shows.

use std::time::{SystemTime, UNIX_EPOCH};

use imgit_app::Preference;
use imgit_app::date_format::{DateTime, locale_code, moment_format};
use imgit_app::pref;
use imgit_app::preferences::{
    DEFAULT_DATE_FORMAT, DEFAULT_DATE_TIME_FORMAT, DEFAULT_DATE_VERBOSE_FORMAT,
    DEFAULT_DATE_WORD_FORMAT,
};

use super::controls::Field;
use super::form::{
    Form, Row, paragraph, row, row_checkbox, setting_checkbox, stored_checkbox, stored_field,
    stored_select, title, value_of,
};
use super::page::{BoxStyle, Face, HELP, LABEL, Page, Style};
use super::store::save_settings;
use crate::theme::GRAPH_COLOR_COUNT;
use crate::ui::menu::{MenuItems, menu_checkbox, popup_menu};

/// The tab group colour a choice names: "Next color" (`None`) or one of the graph's lane colours.
pub(super) fn group_color_name(color: Option<usize>) -> String {
    match color {
        None => "Next color".to_owned(),
        Some(color) => format!("Lane color {}", color + 1),
    }
}

/// The colour each group gets when every group is reset to `current`: that colour, or with "Next color" one
/// lane colour after another.
pub(super) fn reset_group_colors(current: Option<usize>, groups: usize) -> Vec<usize> {
    (0..groups)
        .map(|next| current.unwrap_or(next % GRAPH_COLOR_COUNT))
        .collect()
}

/// The default tab group colour: a react-select-like box (200px) whose menu lists "Next color" (each new group
/// takes the colour after the others') and the lane colours, and a reset button 10px after it that gives every
/// group that colour.
fn color_row(page: &mut Page<'_, '_>, r: &mut Row, id: &str, reset: &str) {
    let ui = page.cx.ui;
    let current = page.app.settings.tab_group_color;
    let (fill, border) = (page.cx.palette.input_bg, page.ink(0.08));
    page.boxed(r.x(), r.top, 200.0, 34.0, BoxStyle::new(fill, border, 4.0));
    if let Some(color) = current {
        let swatch = page.cx.palette.graph_color(color as i32);
        page.boxed(
            r.x() + 11.0,
            r.top + 7.0,
            20.0,
            20.0,
            BoxStyle::fill(swatch),
        );
    }
    let name_x = r.x() + if current.is_some() { 36.0 } else { 11.0 };
    page.text(name_x, r.top + 7.0, LABEL, &group_color_name(current));
    let separator = page.ink(0.08);
    page.boxed(
        r.x() + 170.0,
        r.top + 7.0,
        1.0,
        20.0,
        BoxStyle::fill(separator),
    );
    let chevron = page.ink(0.75);
    page.chevron(r.x() + 175.0, r.top + 7.0, 20.0, chevron);
    let _id = ui.push_id(id);
    if page.hit("##color", r.x(), r.top, 200.0, 34.0, true) {
        let mut items = MenuItems::new();
        for color in std::iter::once(None).chain((0..GRAPH_COLOR_COUNT).map(Some)) {
            items.push(menu_checkbox(
                group_color_name(color),
                color == current,
                move |_cx, app| {
                    app.settings.tab_group_color = color;
                    save_settings(app);
                },
            ));
        }
        popup_menu(page.cx, items, None);
    }
    // btn-default.btn-custom with a far:sync icon (fa-fw, 12px) and a 5px caption margin.
    let (clicked, _) = page.icon_button("##reset", r.x() + 210.0, r.top, "far:sync", reset);
    if clicked {
        let colors = reset_group_colors(current, page.app.tab_groups.len());
        for (group, color) in page.app.tab_groups.iter_mut().zip(colors) {
            group.color = color;
        }
        if page.app.settings.remember_tabs {
            page.app.save_state();
        }
    }
    r.column = 34.0;
}

/// A graph column's checkbox, as its header menu shows and hides it.
fn column_checkbox(page: &mut Page<'_, '_>, r: &mut Row, id: &str, zone: &str) {
    let shown = !page.app.settings.hidden_graph_columns.contains(zone);
    if row_checkbox(page, r, id, shown, "", false) {
        let hidden = &mut page.app.settings.hidden_graph_columns;
        if shown {
            hidden.insert(zone.to_owned());
        } else {
            hidden.remove(zone);
        }
        save_settings(page.app);
    }
}

/// The Date/Time Locale options.
const LOCALES: [&str; 11] = [
    "System [system]",
    "English [en]",
    "English (United Kingdom) [en-gb]",
    "English (United States) [en-us]",
    "French [fr]",
    "German [de]",
    "Spanish [es]",
    "Italian [it]",
    "Japanese [ja]",
    "Russian [ru]",
    "Chinese (China) [zh-cn]",
];

pub(super) fn ui_customization_page(page: &mut Page<'_, '_>) {
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "UI Customization");
    row(page, form, "Theme", |page, r| {
        let names = page.cx.state.theme_names.clone();
        let theme = page.app.settings.theme.clone();
        page.native_select(
            "##theme",
            r.x(),
            r.top,
            300.0,
            &theme,
            &names,
            |cx, app, option| {
                app.settings.theme = option.to_owned();
                app.apply_settings(cx);
            },
        );
        r.column = 34.0;
    });
    row(page, form, "Custom Themes", |page, r| {
        // Outline buttons like the colour rows' reset: Reload reads the folder again, Open Folder shows it
        // (made first if needed).
        let mut x = r.x();
        let (reload, width) = page.icon_button("##reload_themes", x, r.top, "far:sync", "Reload");
        if reload {
            page.app.reload_custom_themes(page.cx, true);
        }
        x += width + 10.0;
        let (open, _) = page.icon_button(
            "##themes_folder",
            x,
            r.top,
            "far:folder-open",
            "Open Folder",
        );
        if open {
            let folder = page.app.config_path("themes");
            let _ = std::fs::create_dir_all(&folder);
            let _ = imgit_platform::shell::open_path(&folder);
        }
        r.column = 34.0;
    });
    paragraph(
        page,
        form,
        "Theme files (*.json) in your profile's themes folder show up in the Theme list. Each holds one \
         theme: a \"name\", the built-in \"base\" theme it starts from, optional \"background\" and \"accent\" \
         colours, and \"colors\" by palette name.",
        10.0,
    );
    row(page, form, "Notification Location", |page, r| {
        stored_select(
            page,
            r,
            "##notification_location",
            pref::NOTIFICATION_LOCATION,
            &["Top Left", "Top Right", "Bottom Left", "Bottom Right"],
            300.0,
        );
    });

    // Date/Time: a sub-setting group with a bottom border, its header underlined.
    let group_top = form.place(0.0);
    form.advance(page, group_top, 0.0, 0.0);
    let header_top = form.place(10.0);
    page.text(
        55.0,
        header_top + Page::leading(Style::ink(18.0, 0.75), 19.796_875),
        Style::ink(18.0, 0.75).with_face(Face::Semibold),
        "Date/Time",
    );
    let rule = page.ink(0.6);
    page.boxed(55.0, header_top + 29.8, 1140.0, 1.0, BoxStyle::fill(rule));
    form.advance(page, header_top, 30.8, 5.0);
    paragraph(
        page,
        form,
        "See [here](https://momentjs.com/docs/#/displaying/format/) for formatting options. The graph, the \
         commit details and the file history show dates this way.",
        10.0,
    );
    row(page, form, "Date/Time Locale", |page, r| {
        stored_select(page, r, "##locale", pref::LOCALE, &LOCALES, 300.0);
    });
    let locale = locale_code(
        &value_of(pref::LOCALE),
        page.cx.state.system_locale.as_deref(),
    );
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0));
    let local = imgit_platform::system::local_time(now).map(DateTime::from);
    let formats: [(&str, Preference, &str); 4] = [
        ("Date Format", pref::DATE_FORMAT, DEFAULT_DATE_FORMAT),
        (
            "Date word Format",
            pref::DATE_WORD_FORMAT,
            DEFAULT_DATE_WORD_FORMAT,
        ),
        (
            "Date/Time Format",
            pref::DATE_TIME_FORMAT,
            DEFAULT_DATE_TIME_FORMAT,
        ),
        (
            "Date verbose Format",
            pref::DATE_VERBOSE_FORMAT,
            DEFAULT_DATE_VERBOSE_FORMAT,
        ),
    ];
    let ui = page.cx.ui;
    for (label, preference, fallback) in formats {
        row(page, form, label, |page, r| {
            let _id = ui.push_id(label);
            let field = Field::default().placeholder(fallback).inset();
            stored_field(page, r, "##format", preference, &field, 300.0);
            let value = value_of(preference);
            let shown = local.as_ref().map_or_else(String::new, |local| {
                moment_format(
                    if value.is_empty() { fallback } else { &value },
                    local,
                    &locale,
                )
            });
            page.text(r.x() + 305.0, r.top + 7.0, HELP, &shown);
        });
    }
    form.contain();
    page.boxed(55.0, form.y, 1140.0, 1.0, BoxStyle::fill(rule));
    let y = form.y;
    form.advance(page, y, 1.0, 15.0);

    row(page, form, "Default color for new tab groups", |page, r| {
        color_row(page, r, "groups", "Reset all groups to this color");
    });
    row(
        page,
        form,
        "Automatically group worktree tabs together",
        |page, r| {
            stored_checkbox(page, r, "##group_worktrees", pref::GROUP_WORKTREES, "");
        },
    );
    row(page, form, "Show toolbar icon labels", |page, r| {
        setting_checkbox(page, r, "##toolbar_labels", |s| &mut s.show_toolbar_labels);
    });
    row(
        page,
        form,
        "Display author initials and generic remote icons instead of avatars",
        |page, r| {
            setting_checkbox(page, r, "##initials", |s| &mut s.author_initials);
        },
    );
    row(
        page,
        form,
        "Show ghost branch/tag when hovering over or selecting a commit",
        |page, r| {
            setting_checkbox(page, r, "##ghost", |s| &mut s.show_ghost_refs);
        },
    );
    row(
        page,
        form,
        "Highlight associated rows when hovering over a branch",
        |page, r| {
            setting_checkbox(page, r, "##highlight", |s| {
                &mut s.highlight_rows_on_ref_hover
            });
        },
    );
    row(
        page,
        form,
        "Branch visibility in commit graph",
        |page, r| {
            let value = if page.app.settings.smart_branch_visibility {
                "Smart Branch Visibility"
            } else {
                "All"
            };
            page.native_select(
                "##branch_visibility",
                r.x(),
                r.top,
                300.0,
                value,
                &["All", "Smart Branch Visibility"],
                |_, app, option| {
                    app.settings.smart_branch_visibility = option != "All";
                    save_settings(app);
                },
            );
            r.column = 34.0;
        },
    );
    // The graph's columns, as its header menu shows and hides them.
    row(page, form, "Show branches and tags in graph", |page, r| {
        column_checkbox(page, r, "##refs", "ref");
    });
    row(page, form, "Show commit author in graph", |page, r| {
        setting_checkbox(page, r, "##author", |s| &mut s.show_author);
    });
    row(page, form, "Show commit date/time in graph", |page, r| {
        setting_checkbox(page, r, "##date", |s| &mut s.show_date);
    });
    row(page, form, "Show commit message in graph", |page, r| {
        column_checkbox(page, r, "##message", "message");
    });
    row(page, form, "Show commit description in graph", |page, r| {
        let s = &page.app.settings;
        let value = if !s.show_description {
            "Never"
        } else if s.description_on_hover {
            "On hover"
        } else {
            "Always"
        };
        page.native_select(
            "##description",
            r.x(),
            r.top,
            300.0,
            value,
            &["Always", "On hover", "Never"],
            |_, app, option| {
                app.settings.show_description = option != "Never";
                app.settings.description_on_hover = option == "On hover";
                save_settings(app);
            },
        );
        r.column = 34.0;
    });
    row(page, form, "Show commit sha in graph", |page, r| {
        setting_checkbox(page, r, "##sha", |s| &mut s.show_sha);
    });
    row(page, form, "Show commit tree in graph", |page, r| {
        column_checkbox(page, r, "##tree", "graph");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_colour_names() {
        assert_eq!(group_color_name(None), "Next color");
        assert_eq!(group_color_name(Some(0)), "Lane color 1");
        assert_eq!(group_color_name(Some(9)), "Lane color 10");
    }

    #[test]
    fn resetting_groups() {
        assert_eq!(reset_group_colors(Some(3), 3), [3, 3, 3]);
        // "Next color": one lane colour after another, wrapping round.
        let colors = reset_group_colors(None, GRAPH_COLOR_COUNT + 2);
        assert_eq!(&colors[..3], &[0, 1, 2]);
        assert_eq!(colors[GRAPH_COLOR_COUNT], 0);
        assert!(reset_group_colors(None, 0).is_empty());
    }
}
