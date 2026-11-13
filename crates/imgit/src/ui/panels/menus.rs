//! The context menus (shown with `ui::menu::popup_menu`) and the shared actions with undo entries.
//
// Every menu is a list of `MenuItem`s whose clicks are `Action`s capturing owned data (shas, names, the tab's
// working directory) and finding their repository again when they run. The code is split by topic:
//
// * `shared`: the repository-bound click, undo entries, the commit chain, branch groups, common items;
// * `commits`: commits, branches, tags, stashes, dropped refs, checkout and reword;
// * `files`: working directory / commit files, several files, folders, the File History's commits;
// * `misc`: tabs, the graph header, left panel sections, remotes, worktrees, local folders, terminal, text;
// * `text_field`: the editable context menu of text fields.

mod commits;
mod files;
mod misc;
mod shared;
mod text_field;

pub use commits::{
    can_drop_ref, can_reword, checkout_ref, commit_menu, jump_to_commit, ref_drop_menu, ref_menu,
    reword_commit,
};
pub use files::{
    commit_file_menu, file_history_commit_menu, folder_menu, multiple_files_menu, workdir_file_menu,
};
pub use misc::{
    graph_header_menu, left_section_menu, local_folder_menu, remote_menu, tab_menu, terminal_menu,
    text_input_menu, worktree_menu,
};
pub use shared::{fetch_args, push_delete_args, repo_view_item};
pub use text_field::{PendingTextEdit, text_field_menu};

/// The menus keep their state in `UiState` (a text field's pending command must outlive the menu).
#[derive(Default)]
pub struct State {}
