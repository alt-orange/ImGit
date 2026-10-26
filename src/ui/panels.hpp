#pragma once

#include "app/repo_state.hpp"
#include "ui/native_menu.hpp"
#include <functional>
#include <imgui.h>
#include <string>
#include <string_view>
#include <vector>

namespace imgit::app
{
    class Application;
    class RepoState;
} // namespace imgit::app

namespace imgit::git
{
    struct Commit;
    struct Ref;
    struct FileChange;
    struct RepoSummary;
    struct FileDiff;
    struct Blame;
    struct BlameHunk;
} // namespace imgit::git

// The regions of the ImGit window, each drawn inside the application's root window.
namespace imgit::ui
{
    void draw_tabs_bar(app::Application& app);
    void draw_new_tab_view(app::Application& app, ImVec2 size);
    void draw_repo_management(app::Application& app, ImVec2 size);
    // A repository's branch / ahead-behind / WIP summary, read once in the background; nullptr until ready.
    [[nodiscard]] const git::RepoSummary* repo_summary(const std::string& path);
    void draw_toolbar(app::Application& app, app::RepoState& state);
    // `openness` 0..1 animates between the collapsed strip and the full panel.
    void draw_left_panel(app::Application& app, app::RepoState& state, ImVec2 size, bool open);
    void draw_graph_view(app::Application& app, app::RepoState& state, ImVec2 size);
    void draw_detail_panel(app::Application& app, app::RepoState& state, ImVec2 size);
    void draw_diff_view(app::Application& app, app::RepoState& state, ImVec2 size);
    // The diff / file editor of the file view (ui/diff_view.cpp), shared with the File History view.
    struct DiffEditorOptions
    {
        app::RepoState::DiffMode mode = app::RepoState::DiffMode::INLINE;
        bool file_view = false;
        bool scroll_prev = false; // scroll to the change above / below the top of the view
        bool scroll_next = false;
        // File view: a gutter before the numbers for the blame of the shown revision: each hunk's first line
        // carries its author's avatar, every line a bar in its commit's colour (`blame_color`, or one derived from
        // the sha); `blame_columns` adds the commit's summary and date. `blame` may be null while it loads.
        bool blame_gutter = false;
        const git::Blame* blame = nullptr;
        bool blame_columns = false;
        std::function<ImVec4(const git::Sha&)> blame_color;
        const char* id = "##diff_lines";
    };
    // Draws the editor over [min, max): `diff`'s lines, or `error` / "Loading..." (a null diff) / "Binary file" /
    // "No Changes" alone. Returns the blame hunk whose gutter was clicked, nullptr for none.
    const git::BlameHunk* draw_diff_editor(const git::FileDiff* diff, const std::string& error, ImVec2 min, ImVec2 max,
                                           const DiffEditorOptions& options);
    // The file view's toolbar toggles (text, or a 12px icon with a tooltip), 23px tall at `pos`; and their widths.
    bool view_toggle_button(const char* id, const char* label, const char* icon, ImVec2 pos, bool checked,
                            const char* tip = nullptr);
    [[nodiscard]] float view_toggle_width(const char* label, const char* icon);
    // A graph colour index for a commit outside the loaded graph, stable per sha.
    [[nodiscard]] int sha_color_index(const git::Sha& sha);
    // The File History view (ui/file_history.cpp) for state.file_history, over the whole tab.
    void draw_file_history(app::Application& app, app::RepoState& state, ImVec2 size);
    void draw_terminal_panel(app::Application& app, app::RepoState* state, ImVec2 size);
    // The active repository's active terminal: the context menu's commands.
    [[nodiscard]] bool terminal_has_selection(app::Application& app);
    void terminal_copy(app::Application& app);
    void terminal_paste(app::Application& app);
    void terminal_select_all(app::Application& app);
    void terminal_clear(app::Application& app);
    void terminal_write(app::Application& app, std::string_view bytes);
    void draw_rebase_panel(app::Application& app, app::RepoState& state, ImVec2 size);
    // The merge tool for state.merge_tool, over the whole tab.
    void draw_merge_view(app::Application& app, app::RepoState& state, ImVec2 size);
    void draw_dialogs(app::Application& app);
    // The Add Submodule panel sliding out of the left panel at `origin`, over the graph (nothing while closed).
    void draw_slide_panel(app::Application& app, app::RepoState& state, ImVec2 origin, float height);
    void draw_fuzzy_finder(app::Application& app);
    // The tabs bar popovers (ui/tab_popovers.cpp), anchored at their button's bottom-right corner.
    void draw_tabs_list_popover(app::Application& app, ImVec2 anchor_right_bottom);
    void draw_profile_popover(app::Application& app, ImVec2 anchor_right_bottom);
    void draw_notifications_popover(app::Application& app, ImVec2 anchor_right_bottom);
    // The Preferences view, in place of the tab's content.
    void draw_preferences(app::Application& app, ImVec2 size);

    // The context menus (ui/menus.cpp), shown with popup_menu().
    [[nodiscard]] MenuItems commit_menu(app::Application& app, app::RepoState& state, const std::string& sha);
    [[nodiscard]] MenuItems ref_menu(app::Application& app, app::RepoState& state, const git::Ref& ref,
                                     bool in_left_panel);
    [[nodiscard]] MenuItems ref_drop_menu(app::Application& app, app::RepoState& state, const git::Ref& source,
                                          const git::Ref& target);
    [[nodiscard]] MenuItems workdir_file_menu(app::Application& app, app::RepoState& state,
                                              const git::FileChange& change, bool staged);
    [[nodiscard]] MenuItems commit_file_menu(app::Application& app, app::RepoState& state, const std::string& sha,
                                             const git::FileChange& change);
    [[nodiscard]] MenuItems multiple_files_menu(app::Application& app, app::RepoState& state,
                                                std::vector<std::string> paths, bool staged);
    [[nodiscard]] MenuItems folder_menu(app::Application& app, app::RepoState& state, const std::string& folder,
                                        std::vector<std::string> files, bool staged, bool uncommitted);
    // A commit row of the File History view.
    [[nodiscard]] MenuItems file_history_commit_menu(app::Application& app, app::RepoState& state, const git::Sha& sha,
                                                     const std::string& path);
    [[nodiscard]] MenuItems tab_menu(app::Application& app, int index);
    [[nodiscard]] MenuItems graph_header_menu(app::Application& app);
    [[nodiscard]] MenuItems left_section_menu(app::Application& app, app::RepoState& state, const std::string& section,
                                              const std::vector<std::string>& sections);
    [[nodiscard]] MenuItems remote_menu(app::Application& app, app::RepoState& state, const std::string& name,
                                        bool in_left_panel);
    [[nodiscard]] MenuItems local_folder_menu(app::Application& app, app::RepoState& state, const std::string& folder);
    [[nodiscard]] MenuItems terminal_menu(app::Application& app);
    [[nodiscard]] MenuItems text_input_menu(bool read_only);
    // Call right after a text field: its right-click menu (Cut / Copy / Paste / Select All).
    void text_field_menu(const std::string& value);

    // Shared actions with undo entries.
    // Checks the ref out; false when the checkout failed (true when it is HEAD already, or a stash).
    bool checkout_ref(app::Application& app, app::RepoState& state, const git::Ref& ref);
    // Create a branch / lightweight or annotated tag at `sha` with an undo entry.
    void create_branch_at(app::Application& app, app::RepoState& state, const std::string& name, const git::Sha& sha,
                          bool checkout);
    void create_tag_at(app::Application& app, app::RepoState& state, const std::string& name, const git::Sha& sha,
                       const std::string& message);
    // Stashes every change with `message` (empty for git's default); false when it failed.
    bool stash_changes(app::Application& app, app::RepoState& state, const std::string& message);
    void jump_to_commit(app::RepoState& state, const char* sha);

    // Whether a ref dragged in the graph can be dropped on another.
    [[nodiscard]] bool can_drop_ref(const app::RepoState& state, const git::Ref& source, const git::Ref& target);
} // namespace imgit::ui
