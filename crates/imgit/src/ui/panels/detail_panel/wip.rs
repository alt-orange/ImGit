//! The working-directory view: the unstaged / staged / conflicted file lists, the commit composer (summary and
//! description, amend, commit options, push after commit) and its Stash tab.

use dear_imgui_rs::{Key, KeyChord, KeyMods};
use imgit_app::RepoState;
use imgit_git::repository::CommitOptions;
use imgit_git::{FileChange, RepoState as OperationState, ResetMode};

use super::controls::{
    MessageBox, SummaryFocus, ViewToggle, css_text, css_text_in, not_allowed_cursor,
    small_checkbox, view_toggle_row,
};
use super::files::{
    FileList, ListEvent, ListKind, RowAction, SectionHeader, apply_events, build_tree,
};
use super::{State, stash_changes};
use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::theme::FONT_SIZE_ROW;
use crate::ui::{Cx, Vec2, vec2};
use imgit_app::repo_state::UndoEntry;

/// The commit options' rows under "Commit options", 22px apart.
const COMMIT_OPTION_ROW: f32 = 22.0;

/// A template's first line is the summary, the rest (without its leading blank lines) the description.
fn split_template(text: &str) -> (&str, &str) {
    match text.find('\n') {
        None => (text, ""),
        Some(newline) => (
            &text[..newline],
            text[newline + 1..].trim_start_matches('\n'),
        ),
    }
}

/// A message's first line and the rest without its surrounding line breaks (CRLF read as LF: the fields hold
/// "\n" breaks).
fn split_amend_message(message: &str) -> (String, String) {
    let message = message.replace("\r\n", "\n");
    let (summary, rest) = match message.find('\n') {
        None => (message.as_str(), ""),
        Some(newline) => (&message[..newline], &message[newline + 1..]),
    };
    (summary.to_owned(), rest.trim_matches('\n').to_owned())
}

/// The commit message of a summary and a description: separated by a blank line.
fn join_message(summary: &str, description: &str) -> String {
    if description.is_empty() {
        summary.to_owned()
    } else {
        format!("{summary}\n\n{description}")
    }
}

/// An empty commit form starts from commit.template: its first line the summary, the rest the description
/// (unless Preferences -> Commit turns "Apply this template" off).
fn apply_commit_template(state: &mut RepoState) {
    let Some(template) = state
        .commit_template
        .as_deref()
        .filter(|text| !text.is_empty())
    else {
        return;
    };
    if !state.commit_summary.is_empty()
        || !state.commit_description.is_empty()
        || !state.repo.config_bool("imgit.applyCommitTemplate", true)
    {
        return;
    }
    let template = template.to_owned();
    fill_form(state, &template);
}

/// Puts `text` in the (empty) commit form: its first line the summary, the rest the description.
fn fill_form(state: &mut RepoState, text: &str) {
    let (summary, body) = split_template(text);
    let (summary, body) = (summary.to_owned(), body.to_owned());
    state.commit_summary = summary;
    state.commit_description = body;
}

/// Fills the form from the configuration the first time it is drawn, and from the message git prepared when a
/// merge, cherry-pick or revert stopped on conflicts.
fn prepare_form(state: &mut RepoState) {
    if !state.commit_form_ready {
        state.commit_form_ready = true;
        state.commit_sign = state.repo.config_bool("commit.gpgsign", false);
        // Preferences -> Git Hooks / Commit: "Skip git hooks by default when committing".
        state.commit_skip_hooks = state.repo.config_bool("imgit.skipHooks", false);
        state.commit_template = state.repo.commit_template();
        apply_commit_template(state);
    }
    // A merge, cherry-pick or revert stopped on conflicts: an empty form starts from the message git prepared
    // for the commit that concludes it, as `git commit` would. Aborted, the message it left untouched goes with
    // it.
    if state.operation_state == state.prepared_message_for {
        return;
    }
    if !state.prepared_summary.is_empty()
        && state.commit_summary == state.prepared_summary
        && state.commit_description == state.prepared_description
    {
        state.commit_summary.clear();
        state.commit_description.clear();
        apply_commit_template(state);
    }
    state.prepared_summary.clear();
    state.prepared_description.clear();
    state.prepared_message_for = state.operation_state;
    let prepared = matches!(
        state.operation_state,
        OperationState::Merge | OperationState::CherryPick | OperationState::Revert
    );
    let untouched = (state.commit_summary.is_empty() && state.commit_description.is_empty())
        || state
            .commit_template
            .as_deref()
            .unwrap_or_default()
            .starts_with(&state.commit_summary);
    if prepared && untouched {
        state.commit_summary.clear();
        state.commit_description.clear();
        // The prepared message is the commit's own: it does not depend on the template switch.
        if let Some(message) = state
            .repo
            .prepared_message()
            .filter(|text| !text.is_empty())
        {
            fill_form(state, &message);
        }
        state.prepared_summary = state.commit_summary.clone();
        state.prepared_description = state.commit_description.clone();
    }
}

/// The label and whether the commit button is enabled.
fn commit_button_state(state: &RepoState) -> (String, bool) {
    let staged = state.status.staged.len();
    if state.amend {
        (
            "Amend Previous Commit".to_owned(),
            !state.commit_summary.is_empty() && state.history.head_sha.is_some(),
        )
    } else if staged == 0 {
        ("Stage files/changes to commit".to_owned(), false)
    } else if state.commit_summary.is_empty() {
        ("Type a Message to Commit".to_owned(), false)
    } else {
        (
            format!(
                "Commit changes to {staged} {}",
                if staged == 1 { "file" } else { "files" }
            ),
            true,
        )
    }
}

/// The big green button of the composer (the .btn-success of the commit button, 40px, 50% opacity while
/// disabled) with `icon` and `label`. Returns whether it was pressed while enabled.
fn composer_button(
    cx: &mut Cx<'_>,
    id: &str,
    icon: &str,
    label: &str,
    min: Vec2,
    width: f32,
    enabled: bool,
) -> bool {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let max = vec2(min.x + width, min.y + cx.px(40.0));
    cx.set_cursor(min);
    let pressed = ui.invisible_button(id, max - min) && enabled;
    let over = ui.is_item_hovered();
    let hovered = over && enabled;
    if over && !enabled {
        not_allowed_cursor(cx);
    }
    let alpha = if enabled { 1.0 } else { 0.5 };
    draw.add_rect_filled(
        min,
        max,
        p.green.with_alpha(if hovered { 0.3 } else { 0.2 } * alpha),
        cx.px(2.0),
    );
    draw.add_rect(min, max, p.green.with_alpha(alpha), cx.px(2.0), 1.0);
    let text_w = cx.text_size(cx.fonts.semibold, 12.0, label).x;
    let content = cx.px(15.0 + 5.0) + text_w;
    let x = min.x + (max.x - min.x - content) / 2.0;
    let color = p.text_normal.with_alpha(p.text_normal.a * alpha);
    cx.draw_fa_icon(
        draw,
        icon,
        vec2(x + cx.px(7.5), min.y + cx.px(20.0)),
        cx.px(12.0),
        color,
    );
    css_text_in(
        cx,
        draw,
        cx.fonts.semibold,
        12.0,
        vec2(x + cx.px(20.0), min.y + cx.px(11.42)),
        color,
        label,
    );
    pressed
}

/// The composer's Stash tab: a title and description over the stash button, which takes every change.
fn draw_stash_form(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    panel: &mut State,
    origin: Vec2,
    width: f32,
    c: f32,
) {
    MessageBox {
        summary: &mut state.stash_summary,
        description: &mut state.stash_description,
        min: vec2(origin.x + cx.px(11.0), c + cx.px(55.0)),
        max: vec2(origin.x + width - cx.px(11.0), c + cx.px(55.0 + 185.0)),
        summary_id: "##stash_summary",
        description_id: "##stash_description",
        focus: SummaryFocus::None,
        summary_hint: "Stash title",
        counter_shown: false,
        caret_to_end: &mut panel.caret_to_end,
    }
    .draw(cx);

    let enabled = !state.status.is_empty() && state.history.head_sha.is_some();
    let label = if enabled {
        "Stash all changes"
    } else {
        "Stage Changes to Stash"
    };
    let bmin = vec2(origin.x + cx.px(11.0), c + cx.px(260.0));
    let width = width - cx.px(22.0);
    let pressed = composer_button(cx, "##stash", "far:inbox-in", label, bmin, width, enabled);
    if pressed
        || (enabled
            && cx
                .ui
                .is_key_chord_pressed(KeyChord::new(Key::Enter).with_mods(KeyMods::CTRL)))
    {
        let message = join_message(&state.stash_summary, &state.stash_description);
        if stash_changes(cx, app, state, &message) {
            state.stash_summary.clear();
            state.stash_description.clear();
        }
    }
}

/// Which file lists one frame's events came from.
struct SectionEvents {
    kind: ListKind,
    events: Vec<ListEvent>,
}

/// The working-directory view.
pub fn draw_wip_panel(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    panel: &mut State,
    size: Vec2,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let origin = cx.cursor();
    let width = size.x;
    prepare_form(state);

    // Header (36px, border-bottom): the danger trash button and "N file changes on [branch]" centred in the
    // rest.
    {
        let tmin = vec2(origin.x + cx.px(10.0), origin.y + cx.px(8.0));
        let tmax = vec2(tmin.x + cx.px(22.5), tmin.y + cx.px(19.0));
        cx.set_cursor(tmin);
        let trash = ui.invisible_button("##discard_all", tmax - tmin);
        let trash_hovered = ui.is_item_hovered();
        draw.add_rect_filled(
            tmin,
            tmax,
            p.red.with_alpha(if trash_hovered { 0.3 } else { 0.2 }),
            cx.px(2.0),
        );
        draw.add_rect(tmin, tmax, p.red, cx.px(2.0), 1.0);
        cx.draw_fa_icon(
            draw,
            "far:trash-alt",
            (tmin + tmax) / 2.0,
            cx.px(12.0),
            p.text_normal,
        );
        cx.tooltip("Discard all changes");
        if trash {
            app.dialog.open(DialogKind::DiscardAll);
        }

        let count = state.status.count();
        let text = format!(
            "{count} {} on",
            if count == 1 {
                "file change"
            } else {
                "file changes"
            }
        );
        let branch = state.history.head_branch.as_deref().unwrap_or("HEAD");
        let text_w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, &text).x;
        let pill_w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, branch).x + cx.px(10.0);
        let group = text_w + cx.px(5.0) + pill_w;
        let x = origin.x + cx.px(32.5) + (width - cx.px(42.5) - group) / 2.0;
        let y = origin.y + cx.px(9.5);
        css_text(cx, draw, FONT_SIZE_ROW, vec2(x, y), p.text_selected, &text);
        let pmin = vec2(x + text_w + cx.px(5.0), y);
        draw.add_rect_filled(
            pmin,
            vec2(pmin.x + pill_w, pmin.y + cx.px(17.0)),
            p.graph_color(0).mix(p.app_bg0, 0.45),
            cx.px(2.0),
        );
        css_text(
            cx,
            draw,
            FONT_SIZE_ROW,
            vec2(pmin.x + cx.px(5.0), y),
            p.text_selected,
            branch,
        );
        draw.add_rect_filled(
            vec2(origin.x, origin.y + cx.px(35.0)),
            vec2(origin.x + width, origin.y + cx.px(36.0)),
            p.section_border,
            0.0,
        );
    }

    view_toggle_row(
        cx,
        ViewToggle {
            file_sort: app.settings.file_sort,
            tree_view: &mut state.tree_view,
            origin_x: origin.x,
            width,
            top: origin.y + cx.px(46.9),
            sort_x: origin.x + cx.px(10.0),
            semibold: true,
        },
    );

    // The file sections share the room between the toolbar and the composer (310px from the bottom, more while
    // the commit options are open: their three rows sit above the commit button).
    let options_extra = if !state.composer_stash && state.commit_options_open {
        cx.px(COMMIT_OPTION_ROW * 3.0)
    } else {
        0.0
    };
    let composer_top = origin.y + size.y - cx.px(310.0) - options_extra;
    let sections_top = origin.y + cx.px(71.0);
    let sections_bottom = composer_top - cx.px(5.0);

    struct SectionSpec<'a> {
        label: &'static str,
        files: &'a [FileChange],
        kind: ListKind,
        button: Option<&'static str>,
        danger: bool,
        action: RowAction,
    }
    let mut results: Vec<SectionEvents> = Vec::new();
    let mut stage_all = false;
    let mut unstage_all = false;
    {
        let status = &state.status;
        let mut specs = Vec::new();
        if !status.conflicted.is_empty() {
            specs.push(SectionSpec {
                label: "Conflicted Files",
                files: &status.conflicted,
                kind: ListKind::Unstaged,
                button: None,
                danger: false,
                action: RowAction::MarkResolved,
            });
        }
        specs.push(SectionSpec {
            label: "Unstaged Files",
            files: &status.unstaged,
            kind: ListKind::Unstaged,
            button: Some("Stage All Changes"),
            danger: false,
            action: RowAction::Stage,
        });
        specs.push(SectionSpec {
            label: "Staged Files",
            files: &status.staged,
            kind: ListKind::Staged,
            button: Some("Unstage All Changes"),
            danger: true,
            action: RowAction::Unstage,
        });
        let mut headers = 0.0;
        let mut open_sections = 0;
        for (i, spec) in specs.iter().enumerate() {
            headers += cx.px(if i == 0 { 40.0 } else { 44.0 });
            if !state
                .collapsed_sections
                .contains(&format!("files:{}", spec.label))
            {
                open_sections += 1;
            }
        }
        let list_h = if open_sections > 0 {
            ((sections_bottom - sections_top - headers) / open_sections as f32).max(0.0)
        } else {
            0.0
        };

        let tree_view = state.tree_view;
        let mut y = sections_top;
        for (i, spec) in specs.into_iter().enumerate() {
            let margin = cx.px(if i == 0 { 0.0 } else { 4.0 });
            let (open, clicked) = SectionHeader {
                label: spec.label,
                count: spec.files.len(),
                pos: vec2(origin.x, y),
                width,
                top_margin: margin,
                button_label: spec.button,
                danger: spec.danger,
            }
            .draw(cx, &mut state.collapsed_sections);
            if clicked {
                if spec.danger {
                    unstage_all = true;
                } else {
                    stage_all = true;
                }
            }
            y += margin + cx.px(40.0);
            if !open {
                continue;
            }
            // .file-node-list-wrapper: a 1px top border, 10px side margins.
            draw.add_rect_filled(
                vec2(origin.x + cx.px(10.0), y),
                vec2(origin.x + width - cx.px(10.0), y + 1.0),
                p.section_border,
                0.0,
            );
            cx.set_cursor(vec2(origin.x + cx.px(10.0), y + 1.0));
            let events = ui
                .child_window(spec.label)
                .size([width - cx.px(20.0), (list_h - 1.0).max(1.0)])
                .build(ui, || {
                    let _spacing =
                        ui.push_style_var_y(dear_imgui_rs::StyleVarVec2::ItemSpacing, 0.0);
                    let root = build_tree(spec.files, tree_view);
                    let mut list = FileList {
                        width: width - cx.px(20.0),
                        kind: spec.kind.clone(),
                        action: Some(spec.action),
                        file_sort: app.settings.file_sort,
                        diff_target: state.diff_target.as_ref(),
                        expanded: &mut state.expanded_folders,
                        events: Vec::new(),
                    };
                    list.draw(cx, &root, 1, "wip:");
                    list.events
                })
                .unwrap_or_default();
            results.push(SectionEvents {
                kind: spec.kind,
                events,
            });
            y += list_h;
        }
    }
    let mut stage_requests: Vec<String> = Vec::new();
    let mut unstage_requests: Vec<String> = Vec::new();
    for section in results {
        let actions = apply_events(cx, app, state, &section.kind, section.events);
        for change in actions {
            if section.kind == ListKind::Staged {
                unstage_requests.push(change.path);
                unstage_requests.extend(change.old_path);
            } else {
                // Stage, or Mark resolved (which stages the file as it is).
                stage_requests.push(change.path);
            }
        }
    }
    let staging =
        stage_all || unstage_all || !stage_requests.is_empty() || !unstage_requests.is_empty();
    if staging && app.refuse_while_busy(state) {
        // A git command is running on the repository: nothing is staged meanwhile.
    } else if stage_all {
        let result = state.repo.stage_all();
        app.run(state, "Stage all", result, None);
    } else if unstage_all {
        let result = state.repo.unstage_all();
        app.run(state, "Unstage all", result, None);
    } else if !stage_requests.is_empty() {
        let result = state.repo.stage(&stage_requests);
        app.run(state, "Stage", result, None);
    } else if !unstage_requests.is_empty() {
        let result = state.repo.unstage(&unstage_requests);
        app.run(state, "Unstage", result, None);
    }

    draw_composer(
        cx,
        app,
        state,
        panel,
        origin,
        width,
        composer_top,
        options_extra,
    );
}

/// The composer, anchored to the bottom: the Commit | Stash tabs and the form of the active one.
#[allow(clippy::too_many_arguments)]
fn draw_composer(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    panel: &mut State,
    origin: Vec2,
    width: f32,
    c: f32,
    options_extra: f32,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    {
        let cx_mid = origin.x + width / 2.0;
        draw.add_rect_filled(
            vec2(cx_mid - cx.px(50.0), c),
            vec2(cx_mid + cx.px(50.0), c + cx.px(4.0)),
            p.section_border,
            cx.px(4.0),
        );
    }
    // Tabs (Commit | Stash): the active tab shows its label and is boxed on three sides, the other is its icon
    // alone; the rest of the row is underlined.
    {
        let top = c + cx.px(10.0);
        let h = cx.px(31.0);
        let row_l = origin.x + cx.px(10.0);
        let row_r = origin.x + width - cx.px(10.0);
        let border = p.section_border;
        let tabs = [
            ("##tab_commit", "far:code-commit", "Commit", false),
            ("##tab_stash", "far:inbox-in", "Stash", true),
        ];
        let mut x = row_l;
        let mut active_l = row_l;
        let mut active_r = row_l;
        for (id, icon, label, stash) in tabs {
            let active = stash == state.composer_stash;
            let label_w = if active {
                cx.text_size(cx.fonts.regular, 14.0, label).x
            } else {
                0.0
            };
            let w = if active {
                cx.px(33.5) + label_w + cx.px(13.0)
            } else {
                cx.px(31.0)
            };
            cx.set_cursor(vec2(x, top));
            if ui.invisible_button(id, vec2(w, h - 1.0)) && !active {
                state.composer_stash = stash;
            }
            let hovered = ui.is_item_hovered();
            if hovered && !active {
                cx.tooltip(label);
            }
            let color = if active || hovered {
                p.text_selected
            } else {
                p.text_secondary
            };
            if active {
                active_l = x;
                active_r = x + w;
                draw.add_rect_filled(vec2(x, top), vec2(x + w, top + 1.0), border, 0.0);
                draw.add_rect_filled(vec2(x, top), vec2(x + 1.0, top + h), border, 0.0);
                draw.add_rect_filled(vec2(x + w - 1.0, top), vec2(x + w, top + h), border, 0.0);
                cx.draw_fa_icon(
                    draw,
                    icon,
                    vec2(x + cx.px(19.75), top + cx.px(16.0)),
                    cx.px(14.0),
                    color,
                );
                css_text(
                    cx,
                    draw,
                    14.0,
                    vec2(x + cx.px(33.5), top + cx.px(6.0)),
                    color,
                    label,
                );
            } else {
                cx.draw_fa_icon(
                    draw,
                    icon,
                    vec2(x + cx.px(15.5), top + cx.px(15.0)),
                    cx.px(14.0),
                    color,
                );
            }
            x += w + cx.px(6.7);
        }
        draw.add_rect_filled(
            vec2(row_l, top + h - 1.0),
            vec2(active_l, top + h),
            border,
            0.0,
        );
        draw.add_rect_filled(
            vec2(active_r, top + h - 1.0),
            vec2(row_r, top + h),
            border,
            0.0,
        );
    }
    if state.composer_stash {
        draw_stash_form(cx, app, state, panel, origin, width, c);
        return;
    }
    // Amend previous commit
    if small_checkbox(
        cx,
        "##amend",
        "Amend previous commit",
        &mut state.amend,
        vec2(origin.x + cx.px(11.0), c + cx.px(55.0)),
        vec2(origin.x + cx.px(31.0), c + cx.px(51.0)),
        14.0,
    ) && state.amend
        && let Ok(message) = state.repo.head_commit_message()
        && !message.is_empty()
    {
        let (summary, description) = split_amend_message(&message);
        state.commit_summary = summary;
        state.commit_description = description;
    }
    // .commit-message-input: summary (16px, 72-character counter) over the description.
    let focus = if ui
        .is_key_chord_pressed(KeyChord::new(Key::M).with_mods(KeyMods::CTRL | KeyMods::SHIFT))
    {
        SummaryFocus::SelectAll
    } else {
        SummaryFocus::None
    };
    MessageBox {
        summary: &mut state.commit_summary,
        description: &mut state.commit_description,
        min: vec2(origin.x + cx.px(11.0), c + cx.px(76.0)),
        max: vec2(origin.x + width - cx.px(11.0), c + cx.px(76.0 + 137.9)),
        summary_id: "##summary",
        description_id: "##description",
        focus,
        summary_hint: "Commit summary",
        counter_shown: true,
        caret_to_end: &mut panel.caret_to_end,
    }
    .draw(cx);
    // Commit options
    {
        let omin = vec2(origin.x + cx.px(11.0), c + cx.px(221.0));
        let options = "Commit options";
        let options_w = cx.text_size(cx.fonts.regular, 14.0, options).x;
        cx.set_cursor(omin);
        if ui.invisible_button(
            "##commit_options",
            vec2(cx.px(20.0) + options_w, cx.px(22.0)),
        ) {
            state.commit_options_open = !state.commit_options_open;
        }
        let color = if ui.is_item_hovered() {
            p.text_normal
        } else {
            p.text_disabled
        };
        cx.draw_fa_icon(
            draw,
            if state.commit_options_open {
                "far:chevron-down"
            } else {
                "far:chevron-right"
            },
            vec2(origin.x + cx.px(18.5), c + cx.px(231.9)),
            cx.px(12.0),
            color,
        );
        css_text(
            cx,
            draw,
            14.0,
            vec2(origin.x + cx.px(31.0), c + cx.px(221.92)),
            color,
            options,
        );
    }
    if state.commit_options_open {
        let option = |cx: &mut Cx<'_>, id: &str, label: &str, value: &mut bool, row: f32| {
            let y = COMMIT_OPTION_ROW * row;
            small_checkbox(
                cx,
                id,
                label,
                value,
                vec2(origin.x + cx.px(31.0), c + cx.px(250.0 + y)),
                vec2(origin.x + cx.px(51.0), c + cx.px(246.0 + y)),
                12.0,
            );
        };
        option(
            cx,
            "##push_after",
            "Push after commit",
            &mut state.push_after_commit,
            0.0,
        );
        option(
            cx,
            "##gpg_sign",
            "Sign commit with GPG (-S)",
            &mut state.commit_sign,
            1.0,
        );
        option(
            cx,
            "##skip_hooks",
            "Skip commit hooks (--no-verify)",
            &mut state.commit_skip_hooks,
            2.0,
        );
    }

    // The commit button (.btn-success .normal-button, 40px, 50% opacity while disabled).
    let (label, enabled) = commit_button_state(state);
    let bmin = vec2(origin.x + cx.px(11.0), c + cx.px(260.0) + options_extra);
    let pressed = composer_button(
        cx,
        "##commit",
        "far:code-commit",
        &label,
        bmin,
        width - cx.px(22.0),
        enabled,
    );
    // Ctrl+Enter is the shell's Enter too: with the terminal panel focused it is typed there, not a commit
    // (and a dialog or prompt that is open has the keyboard).
    let chord = enabled
        && !commit_chord_blocked(
            app.show_terminal_panel && app.terminal.focused,
            app.dialog.is_open() || app.credential_prompt.is_some(),
        )
        && ui.is_key_chord_pressed(KeyChord::new(Key::Enter).with_mods(KeyMods::CTRL));
    if pressed || chord {
        commit(cx, app, state);
    }
}

/// Whether the Ctrl+Enter commit shortcut must not fire: the terminal panel has the keyboard (the chord is also
/// sent to the shell there) or a dialog / prompt is open.
fn commit_chord_blocked(terminal_has_keyboard: bool, modal_open: bool) -> bool {
    terminal_has_keyboard || modal_open
}

/// Commits the staged changes (or amends HEAD) with the form's message.
fn commit(_cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState) {
    if app.refuse_while_busy(state) {
        return;
    }
    let message = join_message(&state.commit_summary, &state.commit_description);
    let amend = state.amend;
    let previous_head = state.history.head_sha.clone();
    // libgit2 neither runs hooks nor signs: `git commit` does whenever that matters.
    let options = CommitOptions {
        amend,
        sign: state.commit_sign,
        skip_hooks: state.commit_skip_hooks,
        strip_comments: state.repo.config_bool("imgit.stripComments", false),
    };
    let with_git = options.sign || (!options.skip_hooks && state.repo.has_commit_hooks());
    let result = if with_git {
        state.repo.commit_with_git(&message, &options)
    } else {
        state.repo.commit(&message, amend)
    };
    let committed = app.run(
        state,
        if amend { "Amend" } else { "Commit" },
        result,
        Some(if amend {
            "Commit amended"
        } else {
            "Changes committed"
        }),
    );
    if committed.is_none() {
        return;
    }
    if !amend && let Some(previous_head) = previous_head {
        state.push_undo(
            UndoEntry::new("commit", move |repo| {
                repo.reset(&previous_head, ResetMode::Soft)
            })
            .with_redo_mode(ResetMode::Soft),
        );
    }
    state.commit_summary.clear();
    state.commit_description.clear();
    apply_commit_template(state);
    state.amend = false;
    if state.push_after_commit {
        app.run_git_cli(state, "Push", ["push"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_commit_chord_stays_out_of_the_terminal_and_dialogs() {
        assert!(!commit_chord_blocked(false, false));
        assert!(commit_chord_blocked(true, false));
        assert!(commit_chord_blocked(false, true));
        assert!(commit_chord_blocked(true, true));
    }

    #[test]
    fn templates_split_into_summary_and_description() {
        assert_eq!(split_template("one"), ("one", ""));
        assert_eq!(split_template("one\n\n\nbody\nmore"), ("one", "body\nmore"));
        assert_eq!(split_template("one\n"), ("one", ""));
        assert_eq!(split_template(""), ("", ""));
    }

    #[test]
    fn amend_messages_are_trimmed() {
        assert_eq!(
            split_amend_message("Fix it\n\nbecause\n"),
            ("Fix it".to_owned(), "because".to_owned())
        );
        assert_eq!(
            split_amend_message("Only\n"),
            ("Only".to_owned(), String::new())
        );
        assert_eq!(
            split_amend_message("No newline"),
            ("No newline".to_owned(), String::new())
        );
        assert_eq!(
            split_amend_message("A\r\n\r\nB\r\n"),
            ("A".to_owned(), "B".to_owned())
        );
    }

    #[test]
    fn messages_join_with_a_blank_line() {
        assert_eq!(join_message("s", ""), "s");
        assert_eq!(join_message("s", "d"), "s\n\nd");
    }
}
