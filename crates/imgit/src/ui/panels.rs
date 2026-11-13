//! The regions of the ImGit window, each drawn inside the application's root window (`src/ui/panels.hpp` and
//! the `.cpp` files it declares).
//!
//! One module per C++ source file. **Each is a stub for now**: its functions have their final signature and
//! draw nothing / return empty menus; porting a panel means filling in its file (and only its file). Everything
//! a panel needs from the application (`Application`'s fields and methods, `Dialog`, `Settings`, the toasts,
//! `MenuItems`...) is already public.
//!
//! # Conventions every panel follows
//!
//! * `fn draw_x(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState, ...)`: the repository tab's
//!   [`RepoState`](imgit_app::RepoState) is **detached** from `app.tabs` while the active tab's panels draw
//!   (see `Application::with_tab`): use the `state` you are given, and do not look the same repository up
//!   through `app` (it is not there).
//! * Functions that make menus or run operations take `cx` first too, so a signature never has to change when
//!   a port finds it needs the frame. Pure queries (`can_reword`, `gitflow_branches`...) take only what they
//!   read.
//! * A panel's persistent state (what the C++ kept in function-local statics) is its module's `State`, held in
//!   [`PanelStates`] as `app.panels.<module>`: add fields there, never statics.
//! * Deferred work (menu clicks, dialog confirmations) is an [`Action`](crate::app::action::Action) capturing
//!   owned data only; it finds its repository again with `app.with_repo(&workdir, |app, state| ..)`.

#![allow(unused_imports)] // the re-exports are for the panels

pub mod dialogs;
pub mod fuzzy_finder;
pub mod menus;
pub mod preferences;
pub mod tab_popovers;
pub mod tabs_bar;
pub mod toolbar;
pub mod workflows;

pub mod detail_panel;
pub mod diff_view;
pub mod file_editor;
pub mod file_history;
pub mod graph_view;
pub mod left_panel;
pub mod merge_view;
pub mod rebase_panel;
pub mod repo_management;
pub mod terminal_panel;

/// Every panel's state between frames.
#[derive(Default)]
pub struct PanelStates {
    pub tabs_bar: tabs_bar::State,
    pub toolbar: toolbar::State,
    pub left_panel: left_panel::State,
    pub graph_view: graph_view::State,
    pub detail_panel: detail_panel::State,
    pub diff_view: diff_view::State,
    pub file_editor: file_editor::State,
    pub file_history: file_history::State,
    pub dialogs: dialogs::State,
    pub menus: menus::State,
    pub preferences: preferences::State,
    pub tab_popovers: tab_popovers::State,
    pub repo_management: repo_management::State,
    pub terminal_panel: terminal_panel::State,
    pub fuzzy_finder: fuzzy_finder::State,
    pub rebase_panel: rebase_panel::State,
    pub merge_view: merge_view::State,
    pub workflows: workflows::State,
}

// The functions of `panels.hpp`, importable as `ui::panels::name` whichever file defines them.
pub use detail_panel::{draw_detail_panel, stash_changes};
pub use dialogs::{create_branch_at, create_tag_at, draw_dialogs, draw_slide_panel};
pub use diff_view::{
    DiffEditorOptions, draw_diff_editor, draw_diff_view, sha_color_index, view_toggle_button,
    view_toggle_width,
};
pub use file_editor::{draw_file_editor, open_file_editor};
pub use file_history::draw_file_history;
pub use fuzzy_finder::draw_fuzzy_finder;
pub use graph_view::draw_graph_view;
pub use left_panel::draw_left_panel;
pub use menus::{
    can_drop_ref, can_reword, checkout_ref, commit_file_menu, commit_menu,
    file_history_commit_menu, folder_menu, graph_header_menu, jump_to_commit, left_section_menu,
    local_folder_menu, multiple_files_menu, ref_drop_menu, ref_menu, remote_menu, reword_commit,
    tab_menu, terminal_menu, text_field_menu, text_input_menu, workdir_file_menu, worktree_menu,
};
pub use merge_view::draw_merge_view;
pub use preferences::draw_preferences;
pub use rebase_panel::draw_rebase_panel;
pub use repo_management::{draw_repo_management, favorite_repositories, repo_summary};
pub use tab_popovers::{draw_notifications_popover, draw_profile_popover, draw_tabs_list_popover};
pub use tabs_bar::{draw_new_tab_view, draw_tabs_bar, open_text_prompt, tab_naming_items};
pub use terminal_panel::{
    draw_terminal_panel, terminal_clear, terminal_copy, terminal_has_selection, terminal_paste,
    terminal_select_all, terminal_write,
};
pub use toolbar::draw_toolbar;
pub use workflows::{
    draw_branch_cleanup, gitflow_branches, gitflow_finish_items, gitflow_initialized,
    gitflow_start_items, open_branch_cleanup,
};
