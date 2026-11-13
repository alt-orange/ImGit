//! The Repo-Specific Preferences pages: Gitflow, Git Hooks, Commit, LFS and Sparse Checkout.

use std::fs;
use std::path::Path;

use super::controls::{BTN, BTN_34, Field, Tone, focused};
use super::form::{
    Form, git_checkbox, git_field, heading, help, help_with, paragraph, row, row_checkbox, title,
};
use super::page::{BoxStyle, Face, HELP, LABEL, Page, Style};
use super::store::{
    GITFLOW_FIELDS, Scope, context, git_flag, initialize_gitflow, load_gitflow_draft,
    load_sparse_draft, load_template_draft, save_settings, set_git_value,
};
use crate::app::toasts::ToastKind;

pub(super) fn gitflow_page(page: &mut Page<'_, '_>) {
    load_gitflow_draft(page.app);
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "Gitflow");
    // section-header h4s (medium weight); the fields are text inputs: a form-control div around an input.
    let field_row = |page: &mut Page<'_, '_>, form: &mut Form, i: usize| {
        row(page, form, GITFLOW_FIELDS[i].label, |page, r| {
            let _id = page.cx.ui.push_id(i as i32);
            let field = Field::default()
                .inset()
                .placeholder(GITFLOW_FIELDS[i].fallback);
            let mut value = std::mem::take(&mut page.app.panels.preferences.gitflow.values[i]);
            page.text_field("##field", r.x(), r.top, 300.0, &mut value, &field, 34.0);
            page.app.panels.preferences.gitflow.values[i] = value;
            r.column = 34.0;
        });
    };
    heading(page, form, "Branches", Face::Regular);
    field_row(page, form, 0);
    field_row(page, form, 1);
    heading(page, form, "Prefixes", Face::Regular);
    for i in 2..GITFLOW_FIELDS.len() {
        field_row(page, form, i);
    }
    let top = form.place(0.0);
    let no_repo = page.app.active().is_none();
    if page.button(
        "##init_gitflow",
        form.control_x(),
        top,
        "Initialize Gitflow",
        Tone::Success,
        BTN,
        no_repo,
    ) {
        initialize_gitflow(page.app);
    }
    form.advance(page, top, 31.140_625, 0.0);
}

pub(super) fn git_hooks_page(page: &mut Page<'_, '_>) {
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "Git Hooks");
    paragraph(
        page,
        form,
        "Git hooks are shell scripts that execute after an event such as a commit or push. For more details, \
         see the [Git hooks documentation](https://git-scm.com/docs/githooks).",
        20.0,
    );
    row(page, form, "Git Hooks directory", |page, r| {
        let hooks_path = context(page.app).hooks_path;
        let field = Field::default().placeholder(&hooks_path);
        git_field(
            page,
            r,
            "##hooks_dir",
            Scope::Repo,
            "core.hooksPath",
            &field,
            227.28,
        );
        let no_repo = page.app.active().is_none();
        if page.button(
            "##browse_hooks",
            r.x() + 232.28,
            r.top,
            "Browse",
            Tone::Primary,
            BTN_34,
            no_repo,
        ) && let Some(path) = imgit_platform::shell::pick_folder_dialog(
            Some(page.cx.shell.window),
            Some(Path::new(&hooks_path)),
        ) {
            set_git_value(
                page.app,
                Scope::Repo,
                "core.hooksPath",
                &path.to_string_lossy(),
            );
        }
        help(
            page,
            r,
            "The directory that Git hooks will use for this repository and its linked worktrees. Sets \
             core.hooksPath in the repository's config.",
        );
    });
    row(
        page,
        form,
        "Skip git hooks by default when committing",
        |page, r| {
            git_checkbox(
                page,
                r,
                "##skip_hooks",
                Scope::Repo,
                "imgit.skipHooks",
                false,
                false,
            );
        },
    );
}

/// The commit template file's text: the summary, then (when there is one) a blank line and the description.
pub(super) fn template_text(summary: &str, description: &str) -> String {
    if description.is_empty() {
        format!("{summary}\n")
    } else {
        format!("{summary}\n\n{description}\n")
    }
}

pub(super) fn commit_page(page: &mut Page<'_, '_>) {
    load_template_draft(page.app);
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "Commit");
    row(page, form, "Push after each commit", |page, r| {
        let push = page.app.settings.push_after_commit;
        if row_checkbox(page, r, "##push_after_commit", push, "", false) {
            page.app.settings.push_after_commit = !push;
            page.app
                .for_each_repo_state_mut(|tab| tab.push_after_commit = !push);
            save_settings(page.app);
        }
    });
    row(
        page,
        form,
        "Skip git hooks by default when committing",
        |page, r| {
            git_checkbox(
                page,
                r,
                "##skip_hooks",
                Scope::Repo,
                "imgit.skipHooks",
                false,
                false,
            );
        },
    );
    heading(page, form, "Merge Behavior", Face::Semibold);
    row(page, form, "Squash", |page, r| {
        git_checkbox(
            page,
            r,
            "##squash",
            Scope::Repo,
            "imgit.squashMerges",
            false,
            false,
        );
        help_with(
            page,
            r,
            "When enabled, merging branches locally will stage all changes without creating a merge commit \
             automatically. You will need to commit manually.",
            18.0,
            HELP,
        );
    });
    heading(page, form, "Commit Template", Face::Semibold);
    paragraph(
        page,
        form,
        "Create and apply a default initial message for commits to maintain consistency in commit summaries \
         and descriptions. For more information see the [Git commit template \
         documentation](https://git-scm.com/docs/git-commit#Documentation/git-commit.txt---templateltfilegt)",
        10.0,
    );
    row(
        page,
        form,
        "Apply this template to commit messages",
        |page, r| {
            git_checkbox(
                page,
                r,
                "##apply_template",
                Scope::Repo,
                "imgit.applyCommitTemplate",
                true,
                false,
            );
        },
    );
    row(
        page,
        form,
        "Remove comments from commit messages",
        |page, r| {
            git_checkbox(
                page,
                r,
                "##strip_comments",
                Scope::Repo,
                "imgit.stripComments",
                false,
                false,
            );
            help(
                page,
                r,
                "Lines starting with # are left out of the commits you make (git commit --cleanup=strip).",
            );
        },
    );
    // The template being edited: commit.template's text, read again for another repository; Save writes it.
    let mut summary = std::mem::take(&mut page.app.panels.preferences.template.summary);
    let mut description = std::mem::take(&mut page.app.panels.preferences.template.description);
    row(page, form, "Summary", |page, r| {
        let field = Field {
            pad_right: 21.0,
            ..Field::default()
        };
        page.text_field(
            "##template_summary",
            r.x(),
            r.top,
            300.0,
            &mut summary,
            &field,
            34.0,
        );
        let left = (72 - summary.len() as i64).to_string();
        let x = r.x() + 294.0 - page.measure(LABEL, &left);
        page.text(x, r.top + 6.0, Style::ink(14.0, 0.4), &left);
        r.column = 34.0;
    });
    row(page, form, "Description", |page, r| {
        let focus = focused(page.cx, "##template_description");
        page.field_box(r.x(), r.top, 825.0, 400.0, focus);
        page.text_area(
            "##template_description",
            r.x(),
            r.top,
            825.0,
            400.0,
            &mut description,
            None,
            7.0,
            20.0,
            LABEL,
        );
        r.column = 400.0;
    });
    let draft = &mut page.app.panels.preferences.template;
    draft.summary = summary;
    draft.description = description;
    let changed =
        draft.summary != draft.saved_summary || draft.saved_description != draft.description;
    let top = form.place(0.0);
    let no_repo = page.app.active().is_none();
    if page.button(
        "##save_template",
        form.control_x(),
        top,
        "Save Commit Template",
        Tone::Success,
        BTN,
        !changed || no_repo,
    ) && !no_repo
    {
        save_template(page);
    }
    form.advance(page, top, 31.140_625, 0.0);
}

/// Writes the template into the repository's own template file, which `commit.template` (in its config) names.
fn save_template(page: &mut Page<'_, '_>) {
    let app = &mut *page.app;
    let Some(common_dir) = app.active().map(|state| state.repo.common_dir()) else {
        return;
    };
    let draft = &app.panels.preferences.template;
    let text = template_text(&draft.summary, &draft.description);
    let file = Path::new(&common_dir).join("imgit_template");
    if fs::write(&file, text).is_err() {
        app.toasts.push_with_detail(
            ToastKind::Error,
            "Could not save the commit template",
            file.to_string_lossy().into_owned(),
        );
        return;
    }
    let path = file.to_string_lossy().replace('\\', "/");
    set_git_value(app, Scope::Repo, "commit.template", &path);
    let draft = &mut app.panels.preferences.template;
    draft.saved_summary.clone_from(&draft.summary);
    draft.saved_description.clone_from(&draft.description);
    // The commit forms read it again.
    app.for_each_repo_state_mut(|tab| {
        if tab.repo.common_dir() == common_dir {
            tab.commit_form_ready = false;
        }
    });
    app.toasts.push(ToastKind::Success, "Commit template saved");
}

pub(super) fn lfs_page(page: &mut Page<'_, '_>) {
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "LFS");
    form.contain();
    heading(page, form, "Initialize LFS", Face::Regular);
    // Plain text, not markup: the repository's name may hold markup characters.
    {
        let line_top = form.place(0.0);
        let name = context(page.app).repo_name;
        page.text(
            form.left,
            line_top + Page::leading(LABEL, 20.0),
            LABEL,
            &format!("LFS has not been initialized on {name}"),
        );
        form.advance(page, line_top, 20.0, 10.0);
    }
    let top = form.place(0.0);
    if page.button(
        "##init_lfs",
        form.left,
        top,
        "Initialize LFS on This Repo",
        Tone::Success,
        BTN,
        false,
    ) {
        page.app.with_active(|app, state| {
            app.run_git_cli(state, "Initialize LFS", ["lfs", "install", "--local"]);
        });
    }
    form.advance(page, top, 31.140_625, 0.0);
}

/// The sparse-checkout paths as `git sparse-checkout set` arguments: one per non-empty line.
pub(super) fn sparse_set_arguments(paths: &str) -> Vec<String> {
    let mut args = vec!["sparse-checkout".to_owned(), "set".to_owned()];
    args.extend(
        paths
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned),
    );
    args
}

pub(super) fn sparse_checkout_page(page: &mut Page<'_, '_>) {
    load_sparse_draft(page.app);
    let ds_text = Style::ink(14.0, 0.8);
    let ds_hint = Style::ink(12.0, 0.6);
    page.text(40.0, 19.0, Style::ink(24.0, 0.8), "Sparse checkout");
    page.text(40.0, 64.2, ds_text, "Paths to include");
    let right = 857.69_f32.min(page.width - 40.0);
    let (fill, border) = (page.cx.palette.input_bg, page.ink(0.2));
    page.boxed(
        40.0,
        96.2,
        right - 40.0,
        238.0,
        BoxStyle::new(fill, border, 2.0),
    );
    let mut paths = std::mem::take(&mut page.app.panels.preferences.sparse.paths);
    page.text_area(
        "##paths",
        40.0,
        96.2,
        right - 40.0,
        238.0,
        &mut paths,
        Some("e.g.\nsrc/\ndocs/\nREADME.md"),
        9.0,
        21.0,
        ds_text,
    );
    page.text(
        40.0,
        346.2,
        ds_hint,
        "Enter one path per line. Only these paths will be checked out in your working directory. If no paths \
         are provided, only root files will be checked out.",
    );
    let enabled = git_flag(page.app, Scope::Repo, "core.sparseCheckout", false);
    let no_repo = page.app.active().is_none();
    let mut x = 40.0;
    let label = if enabled {
        "Update sparse checkout"
    } else {
        "Enable sparse checkout"
    };
    let (clicked, w) = page.ds_button("##enable", x, 376.2, label, true, no_repo);
    if clicked && !no_repo {
        let args = sparse_set_arguments(&paths);
        page.app.with_active(|app, state| {
            app.run_git_cli(state, "Enable sparse checkout", args);
        });
        page.app.panels.preferences.git.invalidate();
    }
    x += w + 8.0;
    let (clicked, w) = page.ds_button(
        "##disable",
        x,
        376.2,
        "Disable sparse checkout",
        true,
        !enabled,
    );
    if clicked && !no_repo {
        page.app.with_active(|app, state| {
            app.run_git_cli(
                state,
                "Disable sparse checkout",
                ["sparse-checkout", "disable"],
            );
        });
        page.app.panels.preferences.git.invalidate();
    }
    x += w + 8.0;
    let (clicked, _) = page.ds_button(
        "##reapply",
        x,
        376.2,
        "Reapply sparse checkout",
        true,
        !enabled,
    );
    if clicked && !no_repo {
        page.app.with_active(|app, state| {
            app.run_git_cli(
                state,
                "Reapply sparse checkout",
                ["sparse-checkout", "reapply"],
            );
        });
    }
    page.app.panels.preferences.sparse.paths = paths;
    page.reach(376.2 + 27.14 + 20.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_files() {
        assert_eq!(template_text("Fix:", ""), "Fix:\n");
        assert_eq!(template_text("Fix:", "Why\nhow"), "Fix:\n\nWhy\nhow\n");
    }

    #[test]
    fn sparse_arguments() {
        assert_eq!(
            sparse_set_arguments("src/\n\ndocs/\n"),
            ["sparse-checkout", "set", "src/", "docs/"]
        );
        assert_eq!(sparse_set_arguments(""), ["sparse-checkout", "set"]);
    }
}
