//! The External Tools, Editor and In-App Terminal pages.

use dear_imgui_rs::InputTextFlags;
use imgit_app::Preference;
use imgit_app::pref;

use super::controls::Field;
use super::form::{
    Form, Row, group, help, paragraph, row, set_value, setting_checkbox, stored_checkbox,
    stored_field, stored_select, title, value_of,
};
use super::page::{Face, Page};

/// A form-horizontal select over a stored value (External Tools' design-system selects).
fn tool_select(
    page: &mut Page<'_, '_>,
    r: &mut Row,
    id: &str,
    preference: Preference,
    options: &[&str],
) {
    let value = value_of(preference);
    page.ds_select(
        id,
        r.x(),
        r.top,
        &value,
        "",
        options,
        move |_, _, option| {
            set_value(preference, option);
        },
    );
    r.column = 31.0;
}

/// The Command row of a `<Custom>` tool.
fn custom_command(page: &mut Page<'_, '_>, form: &mut Form, preference: Preference, id: &str) {
    row(page, form, "Command", |page, r| {
        let field = Field::default()
            .placeholder("program --option %path%")
            .inset();
        stored_field(page, r, id, preference, &field, 300.0);
        help(
            page,
            r,
            "%path% is replaced by the folder or file (added at the end when missing). Quote a path with \
             spaces.",
        );
    });
}

pub(super) fn external_tools_page(page: &mut Page<'_, '_>) {
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "External Tools");
    group(page, form, "Merge Tool", |page, form| {
        row(page, form, "External Merge Tool", |page, r| {
            tool_select(
                page,
                r,
                "##merge_tool",
                pref::MERGE_TOOL,
                &["<None>", "Git Config Default"],
            );
            help(
                page,
                r,
                "Git Config Default: conflicted files offer \"Open in external merge tool\" (git mergetool, \
                 the tool merge.tool names).",
            );
        });
    });
    group(page, form, "Diff Tool", |page, form| {
        row(page, form, "External Diff Tool", |page, r| {
            tool_select(
                page,
                r,
                "##diff_tool",
                pref::DIFF_TOOL,
                &["<Use Merge Tool>", "<None>", "Git Config Default"],
            );
            help(
                page,
                r,
                "Files offer \"Open in external diff tool\" (git difftool, the tool diff.tool or else \
                 merge.tool names).",
            );
        });
    });
    group(page, form, "Editor", |page, form| {
        row(page, form, "External Editor", |page, r| {
            tool_select(
                page,
                r,
                "##editor",
                pref::EDITOR,
                &["System Default", "Visual Studio Code", "Rider", "<Custom>"],
            );
        });
        if value_of(pref::EDITOR) == "<Custom>" {
            custom_command(page, form, pref::EDITOR_COMMAND, "##editor_command");
        }
    });
    group(page, form, "Terminal", |page, form| {
        row(page, form, "External Terminal", |page, r| {
            #[cfg(windows)]
            let options = [
                "System Default",
                "In-App Terminal",
                "Command Prompt",
                "Git Bash",
                "<Custom>",
            ];
            #[cfg(not(windows))]
            let options = ["System Default", "In-App Terminal", "<Custom>"];
            tool_select(page, r, "##terminal", pref::TERMINAL, &options);
        });
        if value_of(pref::TERMINAL) == "<Custom>" {
            custom_command(page, form, pref::TERMINAL_COMMAND, "##terminal_command");
        }
    });
}

/// A font select over a stored font name ("" the system's monospace face), from the fonts installed.
fn font_row(page: &mut Page<'_, '_>, r: &mut Row, preference: Preference) {
    let options = page
        .app
        .panels
        .preferences
        .font_options
        .take()
        .unwrap_or_else(|| {
            std::iter::once("System Default".to_owned())
                .chain(
                    imgit_platform::system::monospace_font_names()
                        .into_iter()
                        .map(str::to_owned),
                )
                .collect()
        });
    let stored = value_of(preference);
    let shown = if stored.is_empty() {
        "System Default"
    } else {
        &stored
    };
    page.combo(
        "##font",
        r.x(),
        r.top,
        300.0,
        34.0,
        shown,
        false,
        &options,
        move |_, _, option| {
            set_value(
                preference,
                if option == "System Default" {
                    ""
                } else {
                    option
                },
            );
        },
        Face::Mono,
    );
    page.app.panels.preferences.font_options = Some(options);
    r.column = 34.0;
}

/// A whole number field (the font sizes, the tab size).
fn number_field() -> Field<'static> {
    Field::default().flags(InputTextFlags::CHARS_DECIMAL)
}

pub(super) fn editor_page(page: &mut Page<'_, '_>) {
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "Editor");
    paragraph(page, form, "The diff, file and merge views.", 10.0);
    row(page, form, "Font", |page, r| {
        font_row(page, r, pref::EDITOR_FONT);
        help(
            page,
            r,
            "Changes to this setting apply after restarting ImGit Desktop",
        );
    });
    row(page, form, "Font Size", |page, r| {
        stored_field(
            page,
            r,
            "##font_size",
            pref::EDITOR_FONT_SIZE,
            &number_field(),
            300.0,
        );
        help(page, r, "Between 6 and 40.");
    });
    row(page, form, "Tab Size", |page, r| {
        stored_field(
            page,
            r,
            "##tab_size",
            pref::EDITOR_TAB_SIZE,
            &number_field(),
            300.0,
        );
        help(page, r, "Columns a tab character takes, between 1 and 16.");
    });
    row(page, form, "End of Line Character", |page, r| {
        stored_select(page, r, "##eol", pref::EDITOR_EOL, &["LF", "CRLF"], 300.0);
        help(
            page,
            r,
            "For files saved from the editor that have no line endings of their own yet.",
        );
    });
    row(page, form, "Syntax Highlighting", |page, r| {
        setting_checkbox(page, r, "##syntax", |s| &mut s.syntax_highlighting);
    });
    row(page, form, "Show Line Numbers", |page, r| {
        stored_checkbox(page, r, "##line_numbers", pref::EDITOR_LINE_NUMBERS, "");
    });
    row(page, form, "Word Wrap", |page, r| {
        setting_checkbox(page, r, "##word_wrap", |s| &mut s.word_wrap);
    });
}

pub(super) fn terminal_page(page: &mut Page<'_, '_>) {
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "In-App Terminal");
    row(page, form, "Font", |page, r| {
        font_row(page, r, pref::TERMINAL_FONT);
        help(
            page,
            r,
            "Changes to this setting apply after restarting ImGit Desktop",
        );
    });
    row(page, form, "Font Size", |page, r| {
        stored_field(
            page,
            r,
            "##font_size",
            pref::TERMINAL_FONT_SIZE,
            &number_field(),
            300.0,
        );
        help(page, r, "Between 6 and 40.");
    });
    row(page, form, "Line Height", |page, r| {
        let field = number_field().flags(InputTextFlags::CHARS_SCIENTIFIC);
        stored_field(
            page,
            r,
            "##line_height",
            pref::TERMINAL_LINE_HEIGHT,
            &field,
            300.0,
        );
        help(page, r, "Between 1 and 3.");
    });
    row(page, form, "Cursor Style", |page, r| {
        stored_select(
            page,
            r,
            "##cursor",
            pref::TERMINAL_CURSOR,
            &["Block", "Underline", "Bar"],
            300.0,
        );
    });
    row(page, form, "Dim terminal when unfocused", |page, r| {
        stored_checkbox(page, r, "##dim", pref::TERMINAL_DIM, "");
    });
    row(page, form, "Default In-App Terminal", |page, r| {
        let shells = imgit_platform::system::terminal_shell_names();
        let value = value_of(pref::TERMINAL_SHELL);
        let shown = match shells.first() {
            Some(first) if value.is_empty() => first.as_str(),
            _ => value.as_str(),
        };
        page.native_select(
            "##shell",
            r.x(),
            r.top,
            300.0,
            shown,
            shells,
            |_, _, option| set_value(pref::TERMINAL_SHELL, option),
        );
        r.column = 34.0;
        help(
            page,
            r,
            "Changes to this setting will only apply to new tabs or after restarting ImGit Desktop",
        );
    });
}
