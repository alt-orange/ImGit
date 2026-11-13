//! The General and Notifications pages.

use imgit_app::RepoState;
use imgit_app::pref;
use imgit_app::preferences::preferences;

use super::controls::{BTN, Field, Tone, focused};
use super::form::{
    Form, autocrlf_checkbox, flag_of, git_checkbox, help, row, row_checkbox, setting_checkbox,
    setting_number, stored_checkbox, title,
};
use super::page::Page;
use super::store::{Scope, save_settings};
use crate::app::toasts::ToastKind;

pub(super) fn general_page(page: &mut Page<'_, '_>) {
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "General");
    row(page, form, "Auto-Fetch Interval", |page, r| {
        let minutes = page.app.settings.auto_fetch_minutes;
        setting_number(page, r, "##auto_fetch", minutes, |page, v| {
            page.app.settings.auto_fetch_minutes = v.clamp(0, 60);
        });
        help(
            page,
            r,
            "In minutes, between 0 and 60 (0 disables auto-fetch). Fetches all visible remotes.",
        );
        help(
            page,
            r,
            "{warning}If you have many visible remotes, this may impact performance. LFS objects will not be \
             automatically fetched.",
        );
    });
    row(page, form, "Auto-Prune", |page, r| {
        stored_checkbox(page, r, "##prune", pref::AUTO_PRUNE, "");
        help(
            page,
            r,
            "Fetches remove the remote-tracking branches whose branch is gone from the remote.",
        );
    });
    row(page, form, "Keep submodules up to date", |page, r| {
        stored_checkbox(
            page,
            r,
            "##submodules",
            pref::UPDATE_SUBMODULES,
            "Automatically update all submodules after performing a Git action",
        );
    });
    row(page, form, "Default Branch Name", |page, r| {
        let mut buffer = std::mem::take(&mut page.app.panels.preferences.default_branch);
        if !focused(page.cx, "##default_branch") {
            buffer = if page.app.settings.default_branch == "main" {
                String::new()
            } else {
                page.app.settings.default_branch.clone()
            };
        }
        let field = Field::default().placeholder("main");
        if page.text_field(
            "##default_branch",
            r.x(),
            r.top,
            300.0,
            &mut buffer,
            &field,
            34.0,
        ) {
            page.app.settings.default_branch = if buffer.is_empty() {
                "main".to_owned()
            } else {
                buffer.clone()
            };
            save_settings(page.app);
        }
        page.app.panels.preferences.default_branch = buffer;
        r.column = 34.0;
        help(
            page,
            r,
            "Sets the default branch name when initializing new repositories.",
        );
    });
    row(
        page,
        form,
        "Delete \".orig\" files after merging",
        |page, r| {
            git_checkbox(
                page,
                r,
                "##orig",
                Scope::Global,
                "mergetool.keepBackup",
                true,
                true,
            );
            help(
                page,
                r,
                "Sets mergetool.keepBackup in your global Git Config: an external merge tool leaves no .orig \
             backup of the conflicted file.",
            );
        },
    );
    row(page, form, "Show All Commits in Graph", |page, r| {
        let all = flag_of(pref::SHOW_ALL_COMMITS);
        if row_checkbox(page, r, "##all_commits", all, "", false) {
            let _ = preferences().set_flag(pref::SHOW_ALL_COMMITS, !all);
            page.app.apply_settings(page.cx);
            page.app.for_each_repo_state_mut(RepoState::refresh);
        }
        help(
            page,
            r,
            "{warning}Enabling this option may adversely affect performance.",
        );
    });
    row(page, form, "Initial Commits in Graph", |page, r| {
        let commits = page.app.settings.initial_commits;
        setting_number(page, r, "##initial_commits", commits, |page, v| {
            page.app.settings.initial_commits = v.max(500);
            page.app.apply_settings(page.cx);
        });
        help(
            page,
            r,
            "Minimum 500. If commit lazy loading is enabled, ImGit Desktop will load additional commits if you \
             reach the earliest commit in the Graph.",
        );
        help(
            page,
            r,
            "{warning}Increasing this value may adversely affect performance.",
        );
    });
    row(page, form, "Lazy Load Commits in Graph", |page, r| {
        stored_checkbox(page, r, "##lazy", pref::LAZY_LOAD_COMMITS, "");
    });
    row(page, form, "Remember tabs", |page, r| {
        setting_checkbox(page, r, "##remember_tabs", |s| &mut s.remember_tabs);
    });
    #[cfg(windows)]
    row(page, form, "Longpaths", |page, r| {
        git_checkbox(
            page,
            r,
            "##longpaths",
            Scope::Global,
            "core.longpaths",
            false,
            false,
        );
        help(
            page,
            r,
            "Sets core.longpaths in your global Git Config. By default, Windows only supports file paths less \
             than 260 characters. Enable support for longer file paths.",
        );
    });
    row(page, form, "AutoCRLF", |page, r| {
        let note = autocrlf_checkbox(page, r, "##autocrlf");
        help(page, r, &note);
    });
    row(
        page,
        form,
        "Forget all Usernames and Passwords",
        |page, r| {
            if page.button(
                "##forget_all",
                r.x(),
                r.top,
                "Forget All",
                Tone::Danger,
                BTN,
                false,
            ) {
                // The ones entered this session; "Remember me" left the others to the credential helper.
                page.app.credentials.clear();
                page.app.toasts.push_with_detail(
                    ToastKind::Info,
                    "Forget All",
                    "Forgot the usernames and passwords entered this session.",
                );
            }
            r.column = 31.140_625;
        },
    );
}

pub(super) fn notifications_page(page: &mut Page<'_, '_>) {
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "Notifications");
    row(page, form, "Enable Desktop Notifications", |page, r| {
        stored_checkbox(page, r, "##desktop", pref::DESKTOP_NOTIFICATIONS, "");
        help(
            page,
            r,
            "While ImGit is in the background, a new notification makes its window ask for attention (the \
             taskbar button flashes, the Dock icon bounces).",
        );
    });
}
