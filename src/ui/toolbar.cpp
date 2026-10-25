#include "app/application.hpp"
#include "app/theme.hpp"
#include "platform/shell.hpp"
#include "ui/popover.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <cmath>
#include <filesystem>
#include <fstream>
#include <imgui.h>
#include <string>

namespace imgit::ui
{
    namespace
    {
        // The toolbar (48px, toolbar__bg1 with a 1px toolbar__bg0 bottom border). Buttons are 52px wide:
        // an Open Sans 11px caption (baseline 15px down) over a 16px Font Awesome icon centred 32px down.
        constexpr float BUTTON_WIDTH = 52.f;
        constexpr float CARET_WIDTH = 15.f;
        constexpr float SEPARATOR_WIDTH = 6.f;
        constexpr float LABEL_FONT = 11.f;
        constexpr float LABEL_BASELINE = 15.f;
        constexpr float ICON_EM = 16.f;
        constexpr float ICON_CENTER = 32.f;

        struct ToolbarItemResult
        {
            bool clicked = false;
            bool dropdown = false;
        };

        struct ToolbarIcon
        {
            Icon icon;
            const char* fa = nullptr; // a Font Awesome name instead of `icon`
            float rotation = 0.f;
        };

        // An icon under a small caption. `dropdown` adds the split caret that opens a menu.
        ToolbarItemResult toolbar_item(app::Application& app, const char* id, ToolbarIcon icon, const char* label,
                                       bool enabled, bool dropdown, const char* tip = nullptr, bool open = false)
        {
            const theme::Palette& p = theme::palette();
            const float height = px(theme::TOOLBAR_HEIGHT) - 1.f;
            const bool labels = app.settings.show_toolbar_labels;
            const float width = px(BUTTON_WIDTH + (dropdown ? CARET_WIDTH : 0.f));
            const ImVec2 pos = ImGui::GetCursorScreenPos();
            ToolbarItemResult result;

            if (!enabled)
            {
                ImGui::BeginDisabled();
            }
            ImGui::InvisibleButton(id, ImVec2(width, height));
            const bool hovered = ImGui::IsItemHovered();
            const bool clicked = ImGui::IsItemClicked(ImGuiMouseButton_Left);
            if (!enabled)
            {
                ImGui::EndDisabled();
            }
            if (tip != nullptr && enabled)
            {
                tooltip(tip);
            }

            ImDrawList* draw = ImGui::GetWindowDrawList();
            // The glyph brightens from 90% white to text-selected on hover; disabled buttons fade to 20% / 30%.
            const float hover_t = smooth(id, hovered && enabled, 0.12f);
            const ImVec4 icon_color =
                    enabled ? theme::with_alpha(p.white, 0.9f + 0.1f * hover_t) : theme::with_alpha(p.white, 0.2f);
            const ImVec4 label_color = enabled ? p.text_secondary : theme::with_alpha(p.white, 0.3f);
            const float center_x = pos.x + px(BUTTON_WIDTH) / 2.f;
            const float icon_y = labels ? pos.y + px(ICON_CENTER) : pos.y + height / 2.f;
            if (icon.fa != nullptr)
            {
                draw_fa_icon(draw, icon.fa, ImVec2(center_x, icon_y), px(ICON_EM), theme::u32(icon_color),
                             icon.rotation);
            }
            else
            {
                draw_icon(draw, icon.icon, ImVec2(center_x, icon_y), px(ICON_EM), theme::u32(icon_color));
            }
            if (labels)
            {
                const ImVec2 label_size = text_size(fonts().regular, LABEL_FONT, label);
                const float ascent = font_px(fonts().regular, LABEL_FONT) * 2189.f / 2789.f;
                draw_text(draw, fonts().regular, LABEL_FONT,
                          ImVec2(center_x - label_size.x / 2.f, pos.y + px(LABEL_BASELINE) - ascent),
                          theme::u32(label_color), label);
            }

            if (dropdown)
            {
                const float split_x = pos.x + px(BUTTON_WIDTH);
                const bool on_dropdown = hovered && ImGui::GetIO().MousePos.x >= split_x;
                draw_fa_icon(draw, open ? "fas:caret-up" : "fas:caret-down",
                             ImVec2(split_x + px(5.5f), pos.y + px(24.f)), px(14.f),
                             theme::u32(enabled ? p.text_secondary : theme::with_alpha(p.white, 0.3f)));
                if (clicked)
                {
                    (on_dropdown ? result.dropdown : result.clicked) = true;
                }
            }
            else
            {
                result.clicked = clicked;
            }
            ImGui::SameLine(0.f, 0.f);
            return result;
        }

        // The LFS DropdownButton (.mr1): "LFS" in 16px (--fs-4) 10px from its left edge and bootstrap's 8x4 caret,
        // on toolbar-bg0 while hovered or open; a spinning circle-notch replaces the text while an LFS command runs.
        constexpr float LFS_WIDTH = 52.f;
        constexpr float LFS_MARGIN = 5.f;

        bool lfs_button(bool busy, bool open)
        {
            const theme::Palette& p = theme::palette();
            const float height = px(theme::TOOLBAR_HEIGHT) - 1.f;
            // Whole pixels, like the dropdown under it.
            const ImVec2 pos(std::round(ImGui::GetCursorScreenPos().x), ImGui::GetCursorScreenPos().y);
            ImGui::SetCursorScreenPos(pos);
            if (busy)
            {
                ImGui::BeginDisabled();
            }
            const bool clicked = ImGui::InvisibleButton("##lfs", ImVec2(px(LFS_WIDTH), height));
            const bool hovered = ImGui::IsItemHovered();
            if (busy)
            {
                ImGui::EndDisabled();
            }
            else
            {
                tooltip("LFS Commands");
            }
            ImDrawList* draw = ImGui::GetWindowDrawList();
            if ((hovered && !busy) || open)
            {
                draw->AddRectFilled(ImVec2(pos.x, pos.y + 1.f), ImVec2(pos.x + px(LFS_WIDTH), pos.y + height + 1.f),
                                    theme::u32(p.toolbar_bg0));
            }
            const float hover_t = smooth("##lfs", hovered && !busy, 0.12f);
            const ImU32 color = theme::u32(busy ? theme::with_alpha(p.white, 0.2f)
                                                : theme::with_alpha(p.white, 0.9f + 0.1f * hover_t));
            const float baseline = pos.y + px(29.f);
            if (busy)
            {
                draw_fa_icon(draw, "far:circle-notch", ImVec2(pos.x + px(23.5f), baseline - px(5.5f)), px(16.f), color,
                             static_cast<float>(std::fmod(ImGui::GetTime(), 2.0) * 180.0));
            }
            else
            {
                const float ascent = font_px(fonts().regular, 16.f) * 2189.f / 2789.f;
                draw_text(draw, fonts().regular, 16.f, ImVec2(pos.x + px(10.f), baseline - ascent), color, "LFS");
            }
            const float caret_x = pos.x + px(40.f);
            const float caret_y = pos.y + px(22.f);
            draw->AddTriangleFilled(ImVec2(caret_x, caret_y), ImVec2(caret_x + px(8.f), caret_y),
                                    ImVec2(caret_x + px(4.f), caret_y + px(4.f)), color);
            ImGui::SameLine(0.f, px(LFS_MARGIN));
            return clicked && !busy;
        }

        void toolbar_separator(ImDrawList* draw, float height)
        {
            const ImVec2 pos = ImGui::GetCursorScreenPos();
            draw->AddLine(ImVec2(pos.x + 0.5f, pos.y + 1.f), ImVec2(pos.x + 0.5f, pos.y + height - 1.f),
                          theme::u32(theme::with_alpha(theme::palette().white, 0.2f)), 1.f);
            ImGui::Dummy(ImVec2(px(SEPARATOR_WIDTH), height));
            ImGui::SameLine(0.f, 0.f);
        }

        // .bread-crumb .option: min-width 110px, padding-left 20px; a 10px caption ("repository") over the 15px
        // value (bold for the repository), with a caret 11.5px from the right edge.
        // The pull dropdown: the explanation (12px secondary, 10px padding) and four radio rows, the default one in
        // green with a dot.
        void pull_mode_list(app::Application& app)
        {
            static constexpr const char* MODES[] = {"Fetch All", "Pull (fast-forward if possible)",
                                                    "Pull (fast-forward only)", "Pull (rebase)"};
            popover_note("Select a default pull/fetch operation to execute when clicking this button");
            for (int i = 0; i < 4; ++i)
            {
                ImGui::PushID(i);
                if (popover_radio_row(MODES[i], app.settings.pull_mode == i))
                {
                    app.settings.pull_mode = i;
                    app.settings.save(app.config_path("settings.txt"));
                    close_popover();
                }
                ImGui::PopID();
            }
        }

        // `open`: its dropdown is showing (ImGit then flips the caret up).
        bool breadcrumb(ImDrawList* draw, const char* id, const char* caption, const std::string& value, float height,
                        bool bold, bool open = false)
        {
            const theme::Palette& p = theme::palette();
            const ImVec2 pos = ImGui::GetCursorScreenPos();
            ImFont* value_font = bold ? fonts().bold : fonts().regular;
            constexpr float VALUE_FONT = 15.f;
            const float value_w = std::min(text_size(value_font, VALUE_FONT, value.c_str()).x, px(320.f));
            const float width = std::max(px(110.f), px(20.f) + value_w + px(28.f));
            ImGui::InvisibleButton(id, ImVec2(width, height));
            const bool hovered = ImGui::IsItemHovered();
            if (hovered)
            {
                draw->AddRectFilled(pos, ImVec2(pos.x + width, pos.y + height), theme::u32(p.toolbar_bg0));
            }
            const float caption_ascent = font_px(fonts().regular, 10.f) * 2189.f / 2789.f;
            draw_text(draw, fonts().regular, 10.f, ImVec2(pos.x + px(20.f), pos.y + px(12.7f) - caption_ascent),
                      theme::u32(p.text_secondary), caption);
            const float value_ascent = font_px(value_font, VALUE_FONT) * 2189.f / 2789.f;
            draw_text_ellipsis(draw, value_font, VALUE_FONT, ImVec2(pos.x + px(20.f), pos.y + px(34.4f) - value_ascent),
                               value_w + 1.f, theme::u32(p.text_selected), value.c_str());
            draw_fa_icon(draw, open ? "fas:caret-up" : "fas:caret-down",
                         ImVec2(pos.x + width - px(11.5f), pos.y + px(30.f)), px(12.f),
                         theme::u32(hovered || open ? p.text_selected : p.text_secondary));
            ImGui::SameLine(0.f, 0.f);
            return ImGui::IsItemClicked(ImGuiMouseButton_Left);
        }

        // ImGit shows its LFS button only for repositories that use Git LFS.
        bool uses_lfs(const app::RepoState& state)
        {
            std::error_code error;
            const std::filesystem::path root = state.repo.workdir();
            if (std::filesystem::exists(root / ".git" / "lfs", error))
            {
                return true;
            }
            std::ifstream attributes(root / ".gitattributes");
            std::string line;
            while (std::getline(attributes, line))
            {
                if (line.find("filter=lfs") != std::string::npos)
                {
                    return true;
                }
            }
            return false;
        }

        // The remote `git lfs push --all` sends to: the checked-out branch's upstream remote, else origin.
        std::string lfs_remote(const app::RepoState& state)
        {
            for (const git::Ref& ref : state.history.refs)
            {
                if (ref.is_head && !ref.upstream.empty())
                {
                    return ref.upstream.substr(0, ref.upstream.find('/'));
                }
            }
            return "origin";
        }

        const char* state_label(git::RepoState state)
        {
            switch (state)
            {
            case git::RepoState::MERGE:
                return "Merge in progress";
            case git::RepoState::REVERT:
                return "Revert in progress";
            case git::RepoState::CHERRYPICK:
                return "Cherry pick in progress";
            case git::RepoState::REBASE:
                return "Rebase in progress";
            case git::RepoState::OTHER:
                return "Operation in progress";
            default:
                return "";
            }
        }
    } // namespace

    void draw_toolbar(app::Application& app, app::RepoState& state)
    {
        using namespace std::string_literals;
        const theme::Palette& p = theme::palette();
        const float height = px(theme::TOOLBAR_HEIGHT);
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        const float width = ImGui::GetContentRegionAvail().x;

        ImGui::BeginChild("##toolbar", ImVec2(width, height), ImGuiChildFlags_None,
                          ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoScrollWithMouse);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        draw->AddRectFilled(origin, ImVec2(origin.x + width, origin.y + height), theme::u32(p.toolbar_bg1));
        draw->AddRectFilled(ImVec2(origin.x, origin.y + height - 1.f), ImVec2(origin.x + width, origin.y + height),
                            theme::u32(p.toolbar_bg0));
        const float item_h = height - 1.f;

        const git::Ref* head = state.head_ref();
        const bool has_head = !state.history.head_sha.empty();
        const bool idle = !state.busy();
        bool has_stash = false;
        for (const git::Ref& ref : state.history.refs)
        {
            if (ref.type == git::RefType::STASH)
            {
                has_stash = true;
                break;
            }
        }

        // Breadcrumb: repository > branch. Their dropdowns open under the bar (1px over its border), 250px wide:
        // a search box, then the recent repositories / local branches.
        static std::string crumb_search;
        const float dropdown_y = origin.y + height - 1.f;
        ImGui::SetCursorScreenPos(origin);
        const float repo_x = ImGui::GetCursorScreenPos().x;
        if (breadcrumb(draw, "##repo_crumb", "repository", state.repo.name(), item_h, true,
                       ImGui::IsPopupOpen("##repo_crumb_menu")))
        {
            crumb_search.clear();
            open_popover("##repo_crumb_menu");
        }
        if (begin_popover("##repo_crumb_menu", ImVec2(repo_x, dropdown_y), 250.f))
        {
            popover_search(crumb_search);
            popover_header("Recently opened", "far:clock");
            for (const std::string& path : app.recent_repos)
            {
                if (path == state.repo.workdir())
                {
                    continue;
                }
                std::string name = path;
                while (!name.empty() && (name.back() == '/' || name.back() == '\\'))
                {
                    name.pop_back();
                }
                name = name.substr(name.find_last_of("/\\") + 1);
                if (!crumb_search.empty() && !contains_ci(name, crumb_search))
                {
                    continue;
                }
                ImGui::PushID(path.c_str());
                if (popover_row(name.c_str()))
                {
                    const std::string open = path;
                    close_popover();
                    app.open_repository(open);
                }
                ImGui::PopID();
            }
            if (popover_link_row("View all repositories"))
            {
                close_popover();
                app.repo_management_open = true;
            }
            end_popover();
        }
        {
            const ImVec2 pos = ImGui::GetCursorScreenPos();
            draw_icon(draw, Icon::CHEVRON_RIGHT, ImVec2(pos.x + px(8.5f), pos.y + px(28.5f)), px(16.f),
                      theme::u32(p.text_disabled));
            ImGui::Dummy(ImVec2(px(13.f), item_h));
            ImGui::SameLine(0.f, 0.f);
        }
        std::string branch_text = state.history.detached ? "detached HEAD" : state.history.head_branch;
        if (state.history.unborn)
        {
            branch_text = "no commits yet";
        }
        const float branch_x = ImGui::GetCursorScreenPos().x;
        if (breadcrumb(draw, "##branch_crumb", "branch", branch_text, item_h, false,
                       ImGui::IsPopupOpen("##branch_crumb_menu")))
        {
            crumb_search.clear();
            open_popover("##branch_crumb_menu");
        }
        if (begin_popover("##branch_crumb_menu", ImVec2(branch_x, dropdown_y), 250.f))
        {
            popover_search(crumb_search);
            std::optional<git::Ref> checkout;
            for (const git::Ref& ref : state.history.refs)
            {
                if (ref.type != git::RefType::LOCAL_BRANCH
                    || (!crumb_search.empty() && !contains_ci(ref.name, crumb_search)))
                {
                    continue;
                }
                ImGui::PushID(ref.full_name.c_str());
                if (popover_row(ref.name.c_str(), "far:code-branch", ref.is_head) && !ref.is_head)
                {
                    checkout = ref;
                }
                ImGui::PopID();
            }
            if (checkout)
            {
                close_popover();
            }
            end_popover();
            if (checkout)
            {
                checkout_ref(app, state, *checkout);
            }
        }

        // Centre: Undo Redo Pull▾ Push Branch Stash Pop | Terminal [LFS▾], centred in the space left of the
        // Actions / Search group.
        static bool lfs = false;
        static std::string lfs_checked_for;
        if (lfs_checked_for != state.repo.workdir())
        {
            lfs_checked_for = state.repo.workdir();
            lfs = uses_lfs(state);
        }
        const float right_group = px(2.f * BUTTON_WIDTH + 1.f);
        const float group_width =
                px(8.f * BUTTON_WIDTH + CARET_WIDTH + SEPARATOR_WIDTH) + (lfs ? px(LFS_WIDTH + LFS_MARGIN) : 0.f);
        ImGui::SetCursorScreenPos(ImVec2(origin.x
                                                 + std::max(ImGui::GetCursorScreenPos().x - origin.x + px(20.f),
                                                            (width - right_group - group_width) / 2.f),
                                         origin.y));

        if (toolbar_item(app, "##undo", {Icon::UNDO}, "Undo", !state.undo_stack.empty(), false,
                         state.undo_stack.empty() ? nullptr : ("Undo " + state.undo_stack.back().label).c_str())
                    .clicked)
        {
            app.undo(state);
        }
        if (toolbar_item(app, "##redo", {Icon::REDO}, "Redo", !state.redo_stack.empty(), false,
                         state.redo_stack.empty() ? nullptr : ("Redo " + state.redo_stack.back().label).c_str())
                    .clicked)
        {
            app.redo(state);
        }

        // The pull button runs the default operation chosen in its dropdown.
        static constexpr const char* PULL_MODES[] = {"Fetch All", "Pull (fast-forward if possible)",
                                                     "Pull (fast-forward only)", "Pull (rebase)"};
        const int pull_mode = std::clamp(app.settings.pull_mode, 0, 3);
        const float pull_x = ImGui::GetCursorScreenPos().x;
        const ToolbarItemResult pull =
                toolbar_item(app, "##pull", {Icon::PULL}, "Pull", idle && head != nullptr, true,
                             head == nullptr ? nullptr : PULL_MODES[pull_mode], ImGui::IsPopupOpen("##pull_menu"));
        const auto run_pull = [&](int mode)
        {
            switch (mode)
            {
            case 0:
                app.run_git_cli(state, "Fetch", {"fetch", "--all", "--prune"});
                break;
            case 2:
                app.run_git_cli(state, "Pull", {"pull", "--ff-only"});
                break;
            case 3:
                app.run_git_cli(state, "Pull", {"pull", "--rebase"});
                break;
            default:
                app.run_git_cli(state, "Pull", {"pull", "--ff"});
                break;
            }
        };
        if (pull.clicked)
        {
            run_pull(pull_mode);
        }
        if (pull.dropdown)
        {
            open_popover("##pull_menu");
        }
        if (begin_popover("##pull_menu", ImVec2(pull_x, dropdown_y), 250.f))
        {
            pull_mode_list(app);
            end_popover();
        }

        if (toolbar_item(app, "##push", {Icon::PUSH}, "Push", idle && head != nullptr, false,
                         head == nullptr ? nullptr : "Push the current branch")
                    .clicked)
        {
            if (head->upstream.empty())
            {
                app.dialog.open(app::Dialog::Kind::PUSH_NEW_BRANCH);
                app.dialog.ref_name = head->name;
                app.dialog.text_a = state.history.remotes.empty() ? "origin" : state.history.remotes.front().name;
            }
            else
            {
                app.run_git_cli(state, "Push", {"push"});
            }
        }
        if (toolbar_item(app, "##branch", {Icon::BRANCH}, "Branch", has_head, false, "Create a branch at HEAD (Ctrl+B)")
                    .clicked)
        {
            app.dialog.open(app::Dialog::Kind::CREATE_BRANCH);
            app.dialog.flag = true;
            app.dialog.sha = state.history.head_sha;
        }
        if (toolbar_item(app, "##stash", {Icon::STASH}, "Stash", has_head && !state.status.empty(), false,
                         "Stash all changes")
                    .clicked)
        {
            if (app.run(
                        state, "Stash",
                        [&]
                        {
                            return state.repo.stash_push("");
                        },
                        "Changes stashed"))
            {
                git::Repository* repo = &state.repo;
                state.push_undo({"stash",
                                 [repo]
                                 {
                                     return repo->stash_pop(0);
                                 },
                                 [repo]
                                 {
                                     return repo->stash_push("");
                                 }});
            }
        }
        if (toolbar_item(app, "##pop", {Icon::POP}, "Pop", has_stash, false, "Pop the most recent stash").clicked)
        {
            app.run(
                    state, "Pop stash",
                    [&]
                    {
                        return state.repo.stash_pop(0);
                    },
                    "Stash popped");
        }
        toolbar_separator(draw, item_h);
        if (toolbar_item(app, "##terminal", {Icon::TERMINAL}, "Terminal", true, false,
                         "Toggle the terminal panel (Ctrl+`)")
                    .clicked)
        {
            app.show_terminal_panel = !app.show_terminal_panel;
            app.terminal.focus_requested = app.show_terminal_panel;
        }
        if (lfs)
        {
            // Lfs-Fetch / Checkout / Pull / Push / Prune, the generateLfsMenuItems order.
            const float lfs_x = std::round(ImGui::GetCursorScreenPos().x);
            const bool lfs_busy = state.async && state.async->title.starts_with("LFS ");
            if (lfs_button(lfs_busy, ImGui::IsPopupOpen("##lfs_menu")))
            {
                open_popover("##lfs_menu");
            }
            if (begin_popover("##lfs_menu", ImVec2(lfs_x, dropdown_y), 250.f))
            {
                popover_padding(5.f);
                if (popover_menu_item("Fetch all LFS files"))
                {
                    close_popover();
                    app.run_git_cli(state, "LFS Fetch", {"lfs", "fetch"});
                }
                if (popover_menu_item("Checkout all LFS files"))
                {
                    close_popover();
                    app.run_git_cli(state, "LFS Checkout", {"lfs", "checkout"});
                }
                if (popover_menu_item("Pull all LFS files"))
                {
                    close_popover();
                    app.run_git_cli(state, "LFS Pull", {"lfs", "pull"});
                }
                if (popover_menu_item("Push all LFS files"))
                {
                    close_popover();
                    app.run_git_cli(state, "LFS Push", {"lfs", "push", "--all", lfs_remote(state)});
                }
                if (popover_menu_item("Prune local LFS repository"))
                {
                    close_popover();
                    app.run_git_cli(state, "LFS Prune", {"lfs", "prune"});
                }
                popover_padding(5.f);
                end_popover();
            }
        }

        // Right side: in-progress operation, busy indicator, Actions, Search.
        float right = origin.x + width - 1.f;
        right -= px(BUTTON_WIDTH);
        ImGui::SetCursorScreenPos(ImVec2(right, origin.y));
        if (toolbar_item(app, "##search", {Icon::SEARCH}, "Search", true, false, "Search Commits (Ctrl + F)").clicked)
        {
            state.search_open = !state.search_open;
            state.search_focus_requested = state.search_open;
            state.run_search();
        }
        right -= px(BUTTON_WIDTH);
        ImGui::SetCursorScreenPos(ImVec2(right, origin.y));
        // Actions opens the command palette.
        if (toolbar_item(app, "##actions", {Icon::MAGIC, "fal:magic"}, "Actions", true, false, "Actions (Ctrl+P)")
                    .clicked)
        {
            app.fuzzy.open = true;
            app.fuzzy.text.clear();
            app.fuzzy.selected = 0;
            app.fuzzy.focus_requested = true;
        }
        right -= px(8.f);
        if (state.busy())
        {
            const std::string text = state.async->title + "...";
            const ImVec2 text_dims = text_size(fonts().regular, theme::FONT_SIZE_SMALL, text.c_str());
            draw_icon(draw, Icon::SPINNER, ImVec2(right - text_dims.x - px(14.f), origin.y + height / 2.f), px(12.f),
                      theme::u32(p.text_accent));
            draw_text(draw, fonts().regular, theme::FONT_SIZE_SMALL,
                      ImVec2(right - text_dims.x, origin.y + (height - text_dims.y) / 2.f), theme::u32(p.text_accent),
                      text.c_str());
            right -= text_dims.x + px(30.f);
        }
        if (state.operation_state != git::RepoState::NONE)
        {
            const char* label = state_label(state.operation_state);
            const ImVec2 label_size = text_size(fonts().semibold, theme::FONT_SIZE_SMALL, label);
            const float badge_width = label_size.x + px(90.f);
            const ImVec2 badge_min(right - badge_width, origin.y + px(10.f));
            const ImVec2 badge_max(right, origin.y + height - px(10.f));
            draw->AddRectFilled(badge_min, badge_max, theme::u32(theme::with_alpha(p.orange, 0.25f)), px(3.f));
            draw_icon(draw, Icon::WARNING, ImVec2(badge_min.x + px(14.f), (badge_min.y + badge_max.y) / 2.f), px(14.f),
                      theme::u32(p.orange));
            draw_text(draw, fonts().semibold, theme::FONT_SIZE_SMALL,
                      ImVec2(badge_min.x + px(26.f), (badge_min.y + badge_max.y) / 2.f - label_size.y / 2.f),
                      theme::u32(p.text_selected), label);
            ImGui::SetCursorScreenPos(ImVec2(badge_max.x - px(56.f), badge_min.y + px(2.f)));
            if (danger_button("Abort", ImVec2(px(52.f), badge_max.y - badge_min.y - px(4.f))))
            {
                app.run(
                        state, "Abort",
                        [&]
                        {
                            return state.repo.abort_operation();
                        },
                        "Operation aborted");
            }
        }
        ImGui::EndChild();
    }
} // namespace imgit::ui
