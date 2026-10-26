#include "app/application.hpp"
#include "app/theme.hpp"
#include "git/repository.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cmath>
#include <imgui.h>
#include <string>

namespace imgit::ui
{
    namespace
    {
        // The File History view: the file view's 36px header, the commit list down the left, and the file view's
        // toolbar row over the editor on the right.
        constexpr float HEADER_H = 36.f;
        constexpr float TOOLBAR_H = 53.f;
        constexpr float LIST_W = 432.f;
        constexpr float ROW_H = 44.f;
        constexpr float NODE_X = 32.f; // the node's centre
        constexpr float ROW_TEXT_X = 54.f;
        constexpr float LIST_TOP_PADDING = 4.f;

        float css_text_y(float size, float box_top)
        {
            const float ascent = size * 2189.f / 2048.f * dpi_scale();
            return std::round(box_top + std::round(ascent)) - std::ceil(ascent);
        }

        const char* status_word(git::FileStatus status)
        {
            switch (status)
            {
            case git::FileStatus::ADDED:
                return "ADDED";
            case git::FileStatus::DELETED:
                return "DELETED";
            case git::FileStatus::RENAMED:
                return "RENAMED";
            default:
                return "MODIFIED";
            }
        }

        // The lane colour of a commit: its graph lane's when it is loaded in the graph, else one from its sha.
        ImVec4 commit_color(const app::RepoState& state, const git::Sha& sha)
        {
            const auto row = state.history.row_by_sha.find(sha);
            if (row != state.history.row_by_sha.end() && row->second < state.layout.rows.size())
            {
                return theme::graph_color(state.layout.rows[row->second].column);
            }
            return theme::graph_color(sha_color_index(sha));
        }

        // The commit node of a list row: the 22px lane-coloured ring around the 18px avatar (a 12px disc for a
        // merge), as in the graph.
        void draw_list_node(ImDrawList* draw, ImVec2 center, const git::Commit& commit, ImVec4 color)
        {
            if (commit.type == git::NodeType::MERGE)
            {
                draw->AddCircleFilled(center, px(theme::COMMIT_MERGE_NODE_DIAMETER) / 2.f, theme::u32(color), 16);
                return;
            }
            draw->AddCircleFilled(center, px(theme::COMMIT_NODE_DIAMETER) / 2.f, theme::u32(color), 32);
            draw_avatar(draw, center, px(theme::COMMIT_ZONE_AVATAR_DIAMETER), commit.author);
        }

        // The commit list: one 44px row per entry (node, summary, short sha; "<age> by <author>" under it), the
        // lane line through the nodes, then the file's status at its oldest entry and "End of History".
        void draw_list(app::Application& app, app::RepoState& state, ImVec2 min, ImVec2 max)
        {
            const theme::Palette& p = theme::palette();
            app::FileHistoryView& view = *state.file_history;
            ImGui::SetCursorScreenPos(min);
            ImGui::BeginChild("##file_history_list", ImVec2(max.x - min.x, max.y - min.y));
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const float width = max.x - min.x;
            const float row_h = px(ROW_H);
            const ImVec2 top = ImGui::GetCursorScreenPos();
            if (view.history_loading || !view.history_error.empty() || view.history.entries.empty())
            {
                const char* message = view.history_loading          ? "Loading..."
                                      : !view.history_error.empty() ? view.history_error.c_str()
                                                                    : "No history for this file";
                const float w = text_size(fonts().regular, 12.f, message).x;
                draw_text(draw, fonts().regular, 12.f,
                          ImVec2(top.x + std::round((width - w) / 2.f), css_text_y(12.f, top.y + px(20.f))),
                          theme::u32(view.history_error.empty() ? p.text_secondary : p.red), message);
                ImGui::EndChild();
                return;
            }
            const std::vector<git::FileHistoryEntry>& entries = view.history.entries;
            const int count = static_cast<int>(entries.size());
            if (view.scroll_to)
            {
                const float y = px(LIST_TOP_PADDING) + static_cast<float>(*view.scroll_to) * row_h;
                const float view_h = ImGui::GetWindowHeight();
                if (y < ImGui::GetScrollY() || y + row_h > ImGui::GetScrollY() + view_h)
                {
                    ImGui::SetScrollY(std::max(0.f, y - (view_h - row_h) / 2.f));
                }
                view.scroll_to.reset();
            }
            // Keyboard: the arrows move the selection while the list has the focus.
            if (ImGui::IsWindowFocused() && view.selected >= 0)
            {
                if (ImGui::IsKeyPressed(ImGuiKey_DownArrow) && view.selected + 1 < count)
                {
                    state.select_file_history_entry(view.selected + 1);
                    view.scroll_to = static_cast<std::size_t>(view.selected);
                }
                else if (ImGui::IsKeyPressed(ImGuiKey_UpArrow) && view.selected > 0)
                {
                    state.select_file_history_entry(view.selected - 1);
                    view.scroll_to = static_cast<std::size_t>(view.selected);
                }
            }
            ImGui::Dummy(ImVec2(width, px(LIST_TOP_PADDING)));
            const float first_y = top.y + px(LIST_TOP_PADDING);
            const float lane_x = std::round(top.x + px(NODE_X));
            const ImVec4 lane = commit_color(state, entries.front().commit.sha);
            // The lane line: from the newest node down to the status label under the oldest.
            const float line_bottom = first_y + static_cast<float>(count) * row_h + px(6.f);
            draw->AddRectFilled(ImVec2(lane_x - 1.f, first_y + row_h / 2.f), ImVec2(lane_x + 1.f, line_bottom),
                                theme::u32(lane));
            ImGuiListClipper clipper;
            clipper.Begin(count, row_h);
            while (clipper.Step())
            {
                for (int i = clipper.DisplayStart; i < clipper.DisplayEnd; ++i)
                {
                    const git::FileHistoryEntry& entry = entries[static_cast<std::size_t>(i)];
                    const ImVec2 pos = ImGui::GetCursorScreenPos();
                    ImGui::PushID(i);
                    ImGui::InvisibleButton("##row", ImVec2(width, row_h));
                    const bool hovered = ImGui::IsItemHovered();
                    const bool selected = i == view.selected;
                    if (selected || hovered)
                    {
                        draw->AddRectFilled(pos, ImVec2(pos.x + width, pos.y + row_h),
                                            theme::u32(selected ? p.selected_row : p.hover_row));
                    }
                    if (ImGui::IsItemClicked(ImGuiMouseButton_Left) && !selected)
                    {
                        state.select_file_history_entry(i);
                    }
                    if (ImGui::IsItemClicked(ImGuiMouseButton_Right))
                    {
                        state.select_file_history_entry(i);
                        popup_menu(file_history_commit_menu(app, state, entry.commit.sha, entry.path));
                    }
                    if (hovered && ImGui::IsMouseDoubleClicked(ImGuiMouseButton_Left))
                    {
                        // The commit in the graph.
                        const git::Sha sha = entry.commit.sha;
                        state.close_file_history();
                        jump_to_commit(state, sha.c_str());
                        ImGui::PopID();
                        ImGui::EndChild();
                        return;
                    }
                    ImGui::PopID();
                    draw_list_node(draw, ImVec2(lane_x, pos.y + row_h / 2.f), entry.commit, lane);
                    // The short sha, right-aligned; the summary fills up to it.
                    const std::string sha = git::short_sha(entry.commit.sha).substr(0, 6);
                    const float sha_w = text_size(fonts().regular, 12.f, sha.c_str()).x;
                    const float sha_x = pos.x + width - px(16.f) - sha_w;
                    draw_text(draw, fonts().regular, 12.f, ImVec2(sha_x, css_text_y(12.f, pos.y + px(7.f))),
                              theme::u32(p.text_normal), sha.c_str());
                    const float text_x = pos.x + px(ROW_TEXT_X);
                    const float summary_w = sha_x - px(10.f) - text_x;
                    draw_text_ellipsis(draw, fonts().semibold, 12.f, ImVec2(text_x, css_text_y(12.f, pos.y + px(7.f))),
                                       summary_w, theme::u32(p.text_selected), entry.commit.summary.c_str());
                    const std::string by = format_relative(entry.commit.time) + " by " + entry.commit.author.name;
                    draw_text_ellipsis(draw, fonts().regular, 11.f, ImVec2(text_x, css_text_y(11.f, pos.y + px(24.f))),
                                       pos.x + width - px(16.f) - text_x, theme::u32(p.text_secondary), by.c_str());
                    // A clipped summary shows whole in a tooltip.
                    if (hovered && text_size(fonts().semibold, 12.f, entry.commit.summary.c_str()).x > summary_w)
                    {
                        styled_tooltip(pos, ImVec2(pos.x + width, pos.y + row_h), entry.commit.summary.c_str());
                    }
                }
            }
            clipper.End();
            // Under the oldest entry: what it did to the file, then the end of the history.
            {
                const ImVec2 pos = ImGui::GetCursorScreenPos();
                const git::FileHistoryEntry& oldest = entries.back();
                const ImVec4 tone = status_color(oldest.status);
                draw->AddRectFilled(ImVec2(pos.x + px(10.f), pos.y + px(6.f)), ImVec2(pos.x + width, pos.y + px(7.f)),
                                    theme::u32(theme::with_alpha(tone, 0.6f)));
                const char* word = status_word(oldest.status);
                draw_text(draw, fonts().semibold, 11.f, ImVec2(pos.x + px(10.f), css_text_y(11.f, pos.y + px(10.f))),
                          theme::u32(tone), word);
                const float word_w = text_size(fonts().semibold, 11.f, word).x;
                const std::string name = file_name(oldest.path);
                draw_text_ellipsis(draw, fonts().regular, 11.f,
                                   ImVec2(pos.x + px(10.f) + word_w + px(8.f), css_text_y(11.f, pos.y + px(10.f))),
                                   width - px(36.f) - word_w, theme::u32(p.text_normal), name.c_str());
                const char* end = view.history.has_more ? "History cut short at the walk limit" : "End of History";
                const float end_w = text_size(fonts().regular, 12.f, end).x;
                draw_text(draw, fonts().regular, 12.f,
                          ImVec2(pos.x + std::round((width - end_w) / 2.f), css_text_y(12.f, pos.y + px(30.f))),
                          theme::u32(p.text_secondary), end);
                ImGui::Dummy(ImVec2(width, px(60.f)));
            }
            ImGui::EndChild();
        }
    } // namespace

    void draw_file_history(app::Application& app, app::RepoState& state, ImVec2 size)
    {
        state.poll_file_history();
        app::FileHistoryView& view = *state.file_history;
        if (view.history_loading || view.blame_loading)
        {
            keep_animating(); // to collect the background jobs
        }
        const theme::Palette& p = theme::palette();
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        ImGui::BeginChild("##file_history", size, ImGuiChildFlags_None,
                          ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoScrollWithMouse);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        draw->AddRectFilled(origin, ImVec2(origin.x + size.x, origin.y + size.y), theme::u32(p.app_bg0));

        // Header: "File History: <name>", the encoding and the close cross.
        {
            const float h = px(HEADER_H);
            draw->AddRectFilled(origin, ImVec2(origin.x + size.x, origin.y + h), theme::u32(p.panel_bg0));
            draw->AddRectFilled(ImVec2(origin.x, origin.y + h - 1.f), ImVec2(origin.x + size.x, origin.y + h),
                                theme::u32(p.section_border));
            const std::string title = "File History: " + file_name(view.path);
            draw_text_ellipsis(draw, fonts().regular, 14.f,
                               ImVec2(origin.x + px(10.f), css_text_y(14.f, origin.y + px(7.5f))), size.x * 0.6f,
                               theme::u32(p.text_selected), title.c_str());
            const float title_w = text_size(fonts().regular, 14.f, title.c_str()).x;
            if (ImGui::IsMouseHoveringRect(ImVec2(origin.x + px(10.f), origin.y),
                                           ImVec2(origin.x + px(10.f) + title_w, origin.y + h)))
            {
                styled_tooltip(ImVec2(origin.x + px(10.f), origin.y + px(7.f)),
                               ImVec2(origin.x + px(10.f) + title_w, origin.y + px(28.f)), view.path.c_str());
            }
            const ImVec2 cmin(origin.x + size.x - px(30.f), origin.y + px(7.5f));
            ImGui::SetCursorScreenPos(cmin);
            if (ImGui::InvisibleButton("##close_file_history", ImVec2(px(20.f), px(20.f))))
            {
                state.close_file_history();
                ImGui::EndChild();
                return;
            }
            const ImU32 cross = theme::u32(ImGui::IsItemHovered() ? p.text_selected : p.text_normal);
            const float a = px(4.6f);
            const float b = px(15.4f);
            draw->AddLine(ImVec2(cmin.x + a, cmin.y + a), ImVec2(cmin.x + b, cmin.y + b), cross, px(1.6f));
            draw->AddLine(ImVec2(cmin.x + b, cmin.y + a), ImVec2(cmin.x + a, cmin.y + b), cross, px(1.6f));
            const char* encoding = "UTF-8";
            const float ew = text_size(fonts().regular, 10.f, encoding).x + px(10.f);
            const ImVec2 emin(cmin.x - px(10.f) - ew, origin.y + px(4.5f));
            ImGui::SetCursorScreenPos(emin);
            if (ImGui::InvisibleButton("##encoding", ImVec2(ew, px(26.f))))
            {
                app.toasts.push(app::ToastKind::INFO, "Encoding", "Files are shown as UTF-8.");
            }
            draw_text(draw, fonts().regular, 10.f, ImVec2(emin.x + px(5.f), css_text_y(10.f, origin.y + px(10.5f))),
                      theme::u32(ImGui::IsItemHovered() ? p.text_normal : p.text_disabled), encoding);
        }

        // The commit list down the left, a border after it.
        const float body_top = origin.y + px(HEADER_H);
        const float list_w = std::min(px(LIST_W), std::round(size.x * 0.45f));
        draw_list(app, state, ImVec2(origin.x, body_top), ImVec2(origin.x + list_w, origin.y + size.y));
        if (!state.file_history)
        {
            ImGui::EndChild();
            return;
        }
        draw->AddRectFilled(ImVec2(origin.x + list_w, body_top), ImVec2(origin.x + list_w + 1.f, origin.y + size.y),
                            theme::u32(p.section_border));
        const float pane_x = origin.x + list_w + 1.f;
        const float pane_w = origin.x + size.x - pane_x;

        // Toolbar: File View | Diff View centred; the diff modes and Blame on the right.
        const float bar_y = body_top + px(10.f);
        {
            const float fw = view_toggle_width("File View", nullptr);
            const float dw = view_toggle_width("Diff View", nullptr);
            float x = std::round(pane_x + (pane_w - fw - dw) / 2.f);
            if (view_toggle_button("##history_file_view", "File View", nullptr, ImVec2(x, bar_y), view.file_view)
                && !view.file_view)
            {
                view.file_view = true;
                state.reload_file_history_diff();
            }
            if (view_toggle_button("##history_diff_view", "Diff View", nullptr, ImVec2(x + fw, bar_y), !view.file_view)
                && view.file_view)
            {
                view.file_view = false;
                view.blame = false;
                state.reload_file_history_diff();
            }
            struct Button
            {
                const char* id;
                const char* label;
                const char* icon;
                const char* tip;
                float gap;
                app::RepoState::DiffMode mode;
            };
            const Button modes[] = {
                    {"##history_hunk", nullptr, "far:list-alt", "Hunk View", 0.f, app::RepoState::DiffMode::HUNK},
                    {"##history_inline", nullptr, "far:list", "Inline View", 0.f, app::RepoState::DiffMode::INLINE},
                    {"##history_split", nullptr, "far:columns", "Split View", 0.f, app::RepoState::DiffMode::SPLIT},
            };
            const float bw = view_toggle_width("Blame", nullptr);
            float total = bw;
            if (!view.file_view)
            {
                total += px(10.f);
                for (const Button& button : modes)
                {
                    total += view_toggle_width(button.label, button.icon);
                }
            }
            x = pane_x + pane_w - px(10.f) - total;
            if (!view.file_view)
            {
                for (const Button& button : modes)
                {
                    if (view_toggle_button(button.id, button.label, button.icon, ImVec2(x, bar_y),
                                           state.diff_mode == button.mode, button.tip)
                        && state.diff_mode != button.mode)
                    {
                        state.diff_mode = button.mode;
                        state.reload_file_history_diff();
                    }
                    x += view_toggle_width(button.label, button.icon);
                }
                x += px(10.f);
            }
            if (view_toggle_button("##history_blame", "Blame", nullptr, ImVec2(x, bar_y), view.blame,
                                   "Who last changed each line"))
            {
                view.blame = !view.blame;
                if (view.blame && !view.file_view)
                {
                    view.file_view = true;
                    state.reload_file_history_diff();
                }
            }
        }

        // The editor: the file as the selected commit left it (its blame in the gutter), or its diff.
        const ImVec2 editor_min(pane_x, origin.y + px(HEADER_H + TOOLBAR_H));
        const ImVec2 editor_max(origin.x + size.x - 1.f, origin.y + size.y);
        const git::FileHistoryEntry* entry =
                view.selected >= 0 && view.selected < static_cast<int>(view.history.entries.size())
                        ? &view.history.entries[static_cast<std::size_t>(view.selected)]
                        : nullptr;
        if (entry == nullptr)
        {
            if (!view.history_loading && view.history_error.empty())
            {
                draw_text(draw, fonts().regular, 14.f, ImVec2(editor_min.x, css_text_y(14.f, editor_min.y)),
                          theme::u32(p.text_normal), "No Changes");
            }
            ImGui::EndChild();
            return;
        }
        if (view.file_view && entry->status == git::FileStatus::DELETED)
        {
            draw_text(draw, fonts().regular, 14.f, ImVec2(editor_min.x, css_text_y(14.f, editor_min.y)),
                      theme::u32(p.text_normal), "The file was deleted in this commit.");
            ImGui::EndChild();
            return;
        }
        DiffEditorOptions options;
        options.mode = state.diff_mode;
        options.file_view = view.file_view;
        options.blame_gutter = true;
        options.blame = view.blame_data ? &*view.blame_data : nullptr;
        options.blame_columns = view.blame;
        options.blame_color = [&state](const git::Sha& sha)
        {
            return commit_color(state, sha);
        };
        options.id = "##file_history_lines";
        const git::BlameHunk* clicked =
                draw_diff_editor(view.diff ? &*view.diff : nullptr, view.diff_error, editor_min, editor_max, options);
        if (clicked != nullptr)
        {
            // The blamed commit in the list (selecting it replaces the blame `clicked` points into).
            const git::Sha sha = clicked->sha;
            for (std::size_t i = 0; i < view.history.entries.size(); ++i)
            {
                if (view.history.entries[i].commit.sha == sha)
                {
                    if (static_cast<int>(i) != view.selected)
                    {
                        state.select_file_history_entry(static_cast<int>(i));
                    }
                    view.scroll_to = i;
                    break;
                }
            }
        }
        if (!view.blame_error.empty() && view.file_view)
        {
            // The blame failed (a file libgit2 cannot blame): say so under the toolbar, on the right.
            const float w = text_size(fonts().regular, 11.f, view.blame_error.c_str()).x;
            draw_text(draw, fonts().regular, 11.f,
                      ImVec2(editor_max.x - px(40.f) - w, css_text_y(11.f, bar_y + px(29.f))), theme::u32(p.red),
                      view.blame_error.c_str());
        }
        ImGui::EndChild();
    }
} // namespace imgit::ui
