#include "app/application.hpp"
#include "app/theme.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cmath>
#include <imgui.h>
#include <imgui_stdlib.h>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace imgit::ui
{
    namespace
    {

        // Files grouped into folders for the "Tree" view, with the per-folder counts shown next to
        // the folder name (modified / added / deleted files).
        struct FileNode
        {
            std::string name;
            std::string path;
            std::map<std::string, FileNode> children;
            const git::FileChange* file = nullptr;
            int modified = 0, added = 0, deleted = 0;
            int additions = 0, deletions = 0;
        };

        void count_file(FileNode& node, const git::FileChange& change)
        {
            switch (change.status)
            {
            case git::FileStatus::ADDED:
            case git::FileStatus::UNTRACKED:
                ++node.added;
                break;
            case git::FileStatus::DELETED:
                ++node.deleted;
                break;
            default:
                ++node.modified;
                break;
            }
            node.additions += change.additions;
            node.deletions += change.deletions;
        }

        FileNode build_tree(const std::vector<git::FileChange>& changes, bool tree_view)
        {
            FileNode root;
            for (const git::FileChange& change : changes)
            {
                count_file(root, change);
                FileNode* node = &root;
                if (tree_view)
                {
                    std::size_t start = 0;
                    while (true)
                    {
                        const std::size_t slash = change.path.find('/', start);
                        if (slash == std::string::npos)
                        {
                            break;
                        }
                        const std::string segment = change.path.substr(start, slash - start);
                        FileNode& child = node->children[segment];
                        child.name = segment;
                        child.path = change.path.substr(0, slash);
                        count_file(child, change);
                        node = &child;
                        start = slash + 1;
                    }
                }
                FileNode& leaf = node->children[(tree_view ? file_name(change.path) : change.path) + "\x01"];
                leaf.name = tree_view ? file_name(change.path) : change.path;
                leaf.path = change.path;
                leaf.file = &change;
                leaf.additions = change.additions;
                leaf.deletions = change.deletions;
            }
            return root;
        }

        // "✎ 63  + 34  − 3" after a folder, or "+12 −3" after a file.
        float draw_counts(ImDrawList* draw, ImVec2 pos, float right_limit, int modified, int added, int deleted,
                          int additions, int deletions, bool folder)
        {
            const theme::Palette& p = theme::palette();
            float x = right_limit;
            const auto item_right = [&](const std::string& text, ImVec4 color, std::optional<Icon> icon)
            {
                const ImVec2 size = text_size(fonts().regular, theme::FONT_SIZE_TINY, text.c_str());
                x -= size.x;
                draw_text(draw, fonts().regular, theme::FONT_SIZE_TINY, ImVec2(x, pos.y), theme::u32(color),
                          text.c_str());
                if (icon)
                {
                    x -= px(12.f);
                    draw_icon(draw, *icon, ImVec2(x + px(5.f), pos.y + size.y / 2.f), px(9.f), theme::u32(color));
                }
                x -= px(8.f);
            };
            if (folder)
            {
                if (deleted > 0)
                {
                    item_right(std::to_string(deleted), p.red, Icon::MINUS);
                }
                if (added > 0)
                {
                    item_right(std::to_string(added), p.green, Icon::PLUS);
                }
                if (modified > 0)
                {
                    item_right(std::to_string(modified), p.stats_files, Icon::PENCIL);
                }
            }
            else
            {
                if (deletions > 0)
                {
                    item_right("\xE2\x88\x92" + std::to_string(deletions), p.red, std::nullopt);
                }
                if (additions > 0)
                {
                    item_right("+" + std::to_string(additions), p.green, std::nullopt);
                }
            }
            return right_limit - x;
        }

        struct FileRowResult
        {
            bool clicked = false;
            bool action = false;
            bool secondary_action = false;
            bool right_clicked = false;
        };

        // Chromium places Open Sans text whose CSS box starts at `box_top` on the pixel nearest box top + the
        // rounded ascent; ImGui puts the baseline ceil(ascent) under its y.
        float css_text_y(float size, float box_top)
        {
            const float ascent = size * 2189.f / 2048.f * dpi_scale();
            return std::round(box_top + std::round(ascent)) - std::ceil(ascent);
        }

        void css_text(ImDrawList* draw, float size, ImVec2 box_pos, ImU32 color, const char* text,
                      ImFont* font = nullptr)
        {
            draw_text(draw, font != nullptr ? font : fonts().regular, size,
                      ImVec2(box_pos.x, css_text_y(size, box_pos.y)), color, text);
        }

        // The coloured buttons (.btn-success / .btn-danger / .btn-default): the
        // tone at 20% (30% hovered) inside a 1px border of the tone, radius 2, 12px semibold text 5px in.
        // `height` 22 is .btn-xs, 24.1 .btn-small. Returns true when clicked.
        bool tone_button(const char* id, const char* label, ImVec2 pos, float height, ImVec4 tone, bool enabled = true)
        {
            const theme::Palette& p = theme::palette();
            const float w = text_size(fonts().semibold, theme::FONT_SIZE_ROW, label).x + px(12.f);
            ImGui::SetCursorScreenPos(pos);
            const bool clicked = ImGui::InvisibleButton(id, ImVec2(w, height)) && enabled;
            const bool hovered = ImGui::IsItemHovered() && enabled;
            const float alpha = enabled ? 1.f : 0.5f;
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const ImVec2 max(pos.x + w, pos.y + height);
            draw->AddRectFilled(pos, max, theme::u32(theme::with_alpha(tone, (hovered ? 0.3f : 0.2f) * alpha)),
                                px(2.f));
            draw->AddRect(pos, max, theme::u32(theme::with_alpha(tone, alpha)), px(2.f), 0, 1.f);
            css_text(draw, theme::FONT_SIZE_ROW, ImVec2(pos.x + px(6.f), pos.y + (height - px(17.f)) / 2.f),
                     theme::u32(theme::with_alpha(p.text_normal, p.text_normal.w * alpha)), label, fonts().semibold);
            return clicked;
        }

        float tone_button_width(const char* label)
        {
            return text_size(fonts().semibold, theme::FONT_SIZE_ROW, label).x + px(12.f);
        }

        const char* status_fa_icon(git::FileStatus status, ImVec4& color)
        {
            const theme::Palette& p = theme::palette();
            switch (status)
            {
            case git::FileStatus::ADDED:
            case git::FileStatus::UNTRACKED:
                color = p.green;
                return "fas:plus";
            case git::FileStatus::DELETED:
                color = p.red;
                return "fas:minus";
            case git::FileStatus::CONFLICTED:
                color = p.red;
                return "fas:exclamation-triangle";
            default:
                color = p.orange;
                return "fas:pencil";
            }
        }

        // .file-node (32px): the 10px solid status icon in a 12.5px fa-fw box 10px in, the 14px name 5px after;
        // hovered, the "Stage File" / "Unstage File" xs button 10px from the right edge.
        FileRowResult file_row(const char* id, const git::FileChange& change, const std::string& label, float width,
                               int depth, const char* action_tip, Icon action_icon, const char* secondary_tip,
                               Icon secondary_icon, bool active, bool show_line_stats)
        {
            (void)action_icon;
            (void)secondary_icon;
            (void)show_line_stats;
            const theme::Palette& p = theme::palette();
            const float height = px(32.f);
            const ImVec2 pos = ImGui::GetCursorScreenPos();
            ImGui::PushID(id);
            FileRowResult result;
            const ImVec2 mouse = ImGui::GetIO().MousePos;
            const bool hovered = ImGui::IsWindowHovered(ImGuiHoveredFlags_AllowWhenBlockedByActiveItem)
                                 && mouse.x >= pos.x && mouse.x < pos.x + width && mouse.y >= pos.y
                                 && mouse.y < pos.y + height;
            ImDrawList* draw = ImGui::GetWindowDrawList();
            if (active)
            {
                draw->AddRectFilled(pos, ImVec2(pos.x + width, pos.y + height), theme::u32(p.selected_row));
            }
            else if (hovered)
            {
                draw->AddRectFilled(pos, ImVec2(pos.x + width, pos.y + height), theme::u32(p.hover_row));
            }
            float right = pos.x + width;
            if (hovered && action_tip != nullptr)
            {
                const bool unstage = std::string_view(action_tip) == "Unstage file";
                const char* button = unstage ? "Unstage File" : "Stage File";
                const float w = tone_button_width(button);
                right = pos.x + width - px(10.f) - w;
                if (tone_button("##action", button, ImVec2(right, pos.y + px(6.f)), px(22.f),
                                unstage ? p.red : p.green))
                {
                    result.action = true;
                }
            }
            (void)secondary_tip;
            const float indent = px(14.f) * static_cast<float>(std::max(0, depth - 1));
            ImVec4 tone;
            const char* icon = status_fa_icon(change.status, tone);
            draw_fa_icon(draw, icon, ImVec2(pos.x + indent + px(16.25f), pos.y + px(16.f)), px(10.f), theme::u32(tone));
            const float name_x = pos.x + indent + px(27.5f);
            draw_text_ellipsis(draw, fonts().regular, 14.f, ImVec2(name_x, css_text_y(14.f, pos.y + px(6.f))),
                               right - px(10.f) - name_x, theme::u32(p.text_normal), label.c_str());

            ImGui::SetCursorScreenPos(pos);
            ImGui::InvisibleButton("##file", ImVec2(width, height));
            result.clicked = ImGui::IsItemClicked(ImGuiMouseButton_Left);
            result.right_clicked = ImGui::IsItemHovered() && ImGui::IsMouseReleased(ImGuiMouseButton_Right);
            if (ImGui::IsItemHovered() && !change.old_path.empty())
            {
                tooltip((change.old_path + " \xE2\x86\x92 " + change.path).c_str());
            }
            ImGui::PopID();
            return result;
        }

        bool folder_row(app::RepoState& state, const FileNode& node, float width, int depth, const std::string& key)
        {
            const theme::Palette& p = theme::palette();
            const float height = px(24.f);
            const ImVec2 pos = ImGui::GetCursorScreenPos();
            const bool collapsed = !state.expanded_folders.contains(key);
            ImGui::PushID(key.c_str());
            ImGui::InvisibleButton("##folder", ImVec2(width, height));
            ImDrawList* draw = ImGui::GetWindowDrawList();
            if (ImGui::IsItemHovered())
            {
                draw->AddRectFilled(pos, ImVec2(pos.x + width, pos.y + height), theme::u32(p.hover_row));
            }
            if (ImGui::IsItemClicked(ImGuiMouseButton_Left))
            {
                if (collapsed)
                {
                    state.expanded_folders.insert(key);
                }
                else
                {
                    state.expanded_folders.erase(key);
                }
            }
            const float indent = px(10.f) + px(14.f) * static_cast<float>(depth);
            draw_icon(draw, collapsed ? Icon::CHEVRON_RIGHT : Icon::CHEVRON_DOWN,
                      ImVec2(pos.x + indent - px(2.f), pos.y + height / 2.f), px(9.f), theme::u32(p.text_secondary));
            const float text_h = text_size(fonts().regular, theme::FONT_SIZE_ROW, "A").y;
            const float counts_w =
                    draw_counts(draw, ImVec2(0.f, pos.y + (height - text_h) / 2.f), pos.x + width - px(10.f),
                                node.modified, node.added, node.deleted, 0, 0, true);
            draw_text_ellipsis(draw, fonts().regular, theme::FONT_SIZE_ROW,
                               ImVec2(pos.x + indent + px(10.f), pos.y + (height - text_h) / 2.f),
                               width - indent - px(24.f) - counts_w, theme::u32(p.text_normal), node.name.c_str());
            ImGui::PopID();
            return !collapsed;
        }

        struct FileListContext
        {
            app::Application* app;
            app::RepoState* state;
            float width;
            app::DiffTarget::Source source;
            git::Sha sha;
            const char* action_tip;
            Icon action_icon;
            const char* secondary_tip;
            Icon secondary_icon;
            std::vector<const git::FileChange*> action_requests;
            std::vector<const git::FileChange*> secondary_requests;
        };

        void collect_paths(const FileNode& node, std::vector<std::string>& out)
        {
            for (const auto& [key, child] : node.children)
            {
                if (child.file != nullptr)
                {
                    out.push_back(child.file->path);
                }
                else
                {
                    collect_paths(child, out);
                }
            }
        }

        void draw_file_tree(FileListContext& ctx, const FileNode& node, int depth, const std::string& prefix)
        {
            for (const auto& [key, child] : node.children)
            {
                if (child.file != nullptr)
                {
                    const git::FileChange& change = *child.file;
                    const bool active = ctx.state->diff_target && ctx.state->diff_target->source == ctx.source
                                        && ctx.state->diff_target->path == change.path;
                    const FileRowResult row =
                            file_row(change.path.c_str(), change, child.name, ctx.width, depth, ctx.action_tip,
                                     ctx.action_icon, ctx.secondary_tip, ctx.secondary_icon, active,
                                     ctx.source != app::DiffTarget::Source::COMMIT);
                    if (row.clicked)
                    {
                        // A conflicted file opens in the merge tool (its plain diff when its sides cannot be read).
                        std::string error;
                        if (change.status != git::FileStatus::CONFLICTED
                            || !ctx.state->open_merge_tool(change.path, error))
                        {
                            ctx.state->open_diff({ctx.source, ctx.sha, change.path, change.status});
                        }
                    }
                    if (row.action)
                    {
                        ctx.action_requests.push_back(&change);
                    }
                    if (row.secondary_action)
                    {
                        ctx.secondary_requests.push_back(&change);
                    }
                    if (row.right_clicked)
                    {
                        if (ctx.source == app::DiffTarget::Source::COMMIT)
                        {
                            popup_menu(commit_file_menu(*ctx.app, *ctx.state, ctx.sha, change));
                        }
                        else
                        {
                            popup_menu(workdir_file_menu(*ctx.app, *ctx.state, change,
                                                         ctx.source == app::DiffTarget::Source::STAGED));
                        }
                    }
                    continue;
                }
                const std::string folder_key = prefix + child.path;
                const bool open = folder_row(*ctx.state, child, ctx.width, depth, folder_key);
                if (ImGui::IsItemHovered() && ImGui::IsMouseReleased(ImGuiMouseButton_Right))
                {
                    std::vector<std::string> files;
                    collect_paths(child, files);
                    popup_menu(folder_menu(*ctx.app, *ctx.state, child.path, std::move(files),
                                           ctx.source == app::DiffTarget::Source::STAGED,
                                           ctx.source != app::DiffTarget::Source::COMMIT));
                }
                if (open)
                {
                    draw_file_tree(ctx, child, depth + 1, prefix);
                }
            }
        }

        // "Unstaged Files (2)": the 14px caret 15px in, the 14px label 5px after it and the xs button 15px from
        // the right; `top_margin` is 4px for every section after the first.
        bool files_section(app::RepoState& state, const char* label, std::size_t count, ImVec2 pos, float width,
                           float top_margin, const char* button_label, bool* button_clicked, bool danger)
        {
            const theme::Palette& p = theme::palette();
            const std::string key = std::string("files:") + label;
            const bool collapsed = state.collapsed_sections.contains(key);
            ImGui::PushID(label);
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const float y = pos.y + top_margin;
            if (button_label != nullptr && count > 0)
            {
                const float w = tone_button_width(button_label);
                if (tone_button("##all", button_label, ImVec2(pos.x + width - px(15.f) - w, y + px(12.f)), px(22.f),
                                danger ? p.red : p.green))
                {
                    *button_clicked = true;
                }
            }
            const std::string text = std::string(label) + " (" + std::to_string(count) + ")";
            const float text_w = text_size(fonts().regular, 14.f, text.c_str()).x;
            ImGui::SetCursorScreenPos(ImVec2(pos.x + px(15.f), y + px(10.f)));
            if (ImGui::InvisibleButton("##section", ImVec2(px(13.75f) + text_w, px(26.f))))
            {
                if (collapsed)
                {
                    state.collapsed_sections.erase(key);
                }
                else
                {
                    state.collapsed_sections.insert(key);
                }
            }
            draw_fa_icon(draw, collapsed ? "fas:caret-right" : "fas:caret-down",
                         ImVec2(pos.x + px(19.4f), y + px(22.8f)), px(14.f), theme::u32(p.text_normal));
            css_text(draw, 14.f, ImVec2(pos.x + px(28.75f), y + px(13.f)), theme::u32(p.text_normal), text.c_str());
            ImGui::PopID();
            return !collapsed;
        }

        // The sort icon and the centred Path | Tree toggle (12px semibold here).
        void view_toggle_row(app::RepoState& state, ImVec2 origin, float width, float top, bool semibold)
        {
            const theme::Palette& p = theme::palette();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const float row_h = px(24.1f);
            ImGui::SetCursorScreenPos(ImVec2(origin.x + px(10.f), top));
            ImGui::InvisibleButton("##sort", ImVec2(px(20.f), row_h));
            draw_fa_icon(draw, "far:sort-alpha-up", ImVec2(origin.x + px(20.f), top + px(12.1f)), px(16.f),
                         theme::u32(ImGui::IsItemHovered() ? p.text_selected : p.text_normal));
            if (ImGui::IsItemHovered())
            {
                tooltip("Sort");
            }
            ImFont* font = semibold ? fonts().semibold : fonts().regular;
            const char* icons[] = {"far:bars", "fas:folder-tree"};
            const char* labels[] = {"Path", "Tree"};
            float widths[2];
            for (int i = 0; i < 2; ++i)
            {
                widths[i] = px(11.f) + fa_icon_width(icons[i], px(12.f)) + px(5.f)
                            + text_size(font, theme::FONT_SIZE_ROW, labels[i]).x + px(11.f);
            }
            float bx = origin.x + (width - widths[0] - widths[1]) / 2.f;
            for (int i = 0; i < 2; ++i)
            {
                const bool checked = state.tree_view == (i == 1);
                const ImVec2 bmin(bx, top);
                const ImVec2 bmax(bx + widths[i], top + row_h);
                ImGui::SetCursorScreenPos(bmin);
                ImGui::PushID(i);
                if (ImGui::InvisibleButton("##view_mode", ImVec2(widths[i], row_h)))
                {
                    state.tree_view = i == 1;
                }
                const bool hovered = ImGui::IsItemHovered();
                ImGui::PopID();
                const ImVec4 accent = theme::with_alpha(p.accent, 0.2f);
                if (checked)
                {
                    draw->AddRectFilled(bmin, bmax, theme::u32(accent));
                }
                else if (hovered)
                {
                    draw->AddRectFilled(bmin, bmax, theme::u32(p.hover_row));
                }
                draw->AddRect(bmin, bmax, theme::u32(checked ? accent : theme::with_alpha(p.white, 0.2f)), 0.f, 0, 1.f);
                const ImU32 color = theme::u32(checked || hovered ? p.text_selected : p.text_normal);
                const float icon_w = fa_icon_width(icons[i], px(12.f));
                draw_fa_icon(draw, icons[i], ImVec2(bmin.x + px(11.f) + icon_w / 2.f, top + px(12.05f)), px(12.f),
                             color);
                css_text(draw, theme::FONT_SIZE_ROW, ImVec2(bmin.x + px(16.f) + icon_w, top + px(3.57f)), color,
                         labels[i], font);
                bx += widths[i];
            }
        }

        // A 12px checkbox (rgba(0,0,0,.2), 1px 40% white border, radius 1) with its label; true when toggled.
        bool small_checkbox(const char* id, const char* label, bool* value, ImVec2 box_pos, ImVec2 label_pos, float size)
        {
            const theme::Palette& p = theme::palette();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const float label_w = text_size(fonts().regular, size, label).x;
            ImGui::SetCursorScreenPos(box_pos);
            const bool clicked =
                    ImGui::InvisibleButton(id, ImVec2(label_pos.x + label_w - box_pos.x, std::max(px(12.f), px(20.f))));
            if (clicked)
            {
                *value = !*value;
            }
            const ImVec2 bmax(box_pos.x + px(12.f), box_pos.y + px(12.f));
            if (*value)
            {
                draw->AddRectFilled(box_pos, bmax, theme::u32(p.accent), px(1.f));
                draw_fa_icon(draw, "fas:check", ImVec2(box_pos.x + px(6.f), box_pos.y + px(6.f)), px(9.f),
                             theme::u32(p.white));
            }
            else
            {
                draw->AddRectFilled(box_pos, bmax, theme::u32(p.input_bg), px(1.f));
                draw->AddRect(box_pos, bmax, theme::u32(p.text_disabled), px(1.f), 0, 1.f);
            }
            css_text(draw, size, label_pos, theme::u32(p.text_normal), label);
            return clicked;
        }

        // .commit-message-input: a 1px subtle-border box, radius 4, holding the summary row
        // (padding-top 10px: the 72-character counter 10px from the right edge, the 16px input filling the
        // rest) over the 14px description textarea (5px 10px padding), which fills
        // the box below the 38px summary row.
        enum class SummaryFocus
        {
            NONE,
            SELECT_ALL,
            CARET_END,
        };

        void message_input(std::string& summary, std::string& description, ImVec2 bmin, ImVec2 bmax,
                           const char* summary_id, const char* description_id, SummaryFocus focus,
                           const char* summary_hint = "Commit summary", bool counter_shown = true)
        {
            const theme::Palette& p = theme::palette();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            draw->AddRectFilled(ImVec2(bmin.x + 1.f, bmin.y + 1.f), ImVec2(bmax.x - 1.f, bmin.y + px(38.f)),
                                theme::u32(p.input_bg), px(4.f), ImDrawFlags_RoundCornersTop);
            draw->AddRectFilled(ImVec2(bmin.x + 1.f, bmin.y + px(38.f)), ImVec2(bmax.x - 1.f, bmax.y - 1.f),
                                theme::u32(p.input_bg), px(4.f), ImDrawFlags_RoundCornersBottom);
            draw->AddRect(bmin, bmax, theme::u32(p.subtle_border), px(4.f), 0, 1.f);

            ImGui::PushID(summary_id);
            ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
            ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgActive, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_Text, p.text_normal);
            const int remaining = 72 - static_cast<int>(summary.size());
            const std::string counter = std::to_string(remaining);
            const float counter_w = counter_shown ? text_size(fonts().regular, 16.f, counter.c_str()).x : 0.f;
            const float counter_x = bmax.x - px(10.f) - counter_w;

            // Summary: an input with 10px padding inside a 1px transparent border.
            const ImVec2 smin(bmin.x + 1.f, bmin.y + px(12.1f));
            const float text_box = smin.y + px(1.43f);
            ImGui::SetCursorScreenPos(smin);
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(11.f), css_text_y(16.f, text_box) - smin.y));
            push_font(fonts().regular, 16.f);
            ImGui::SetNextItemWidth(counter_x - smin.x);
            // Keyboard focus selects the text; an autofocused input (the amend editor) puts the caret at the end.
            static bool caret_to_end = false;
            if (focus != SummaryFocus::NONE)
            {
                ImGui::SetKeyboardFocusHere();
                caret_to_end = focus == SummaryFocus::CARET_END;
            }
            ImGui::InputText(summary_id, &summary, ImGuiInputTextFlags_CallbackAlways,
                             [](ImGuiInputTextCallbackData* data)
                             {
                                 if (caret_to_end)
                                 {
                                     data->CursorPos = data->SelectionStart = data->SelectionEnd = data->BufTextLen;
                                     caret_to_end = false;
                                 }
                                 return 0;
                             });
            text_field_menu(summary);
            ImGui::PopFont();
            ImGui::PopStyleVar();
            if (summary.empty())
            {
                css_text(draw, 16.f, ImVec2(smin.x + px(11.f), text_box), theme::u32(p.text_secondary),
                         summary_hint);
            }
            if (counter_shown)
            {
                css_text(draw, 16.f, ImVec2(counter_x, bmin.y + px(13.08f)),
                         theme::u32(remaining < 0 ? p.orange : p.text_disabled), counter.c_str());
            }
            // Description: a textarea with 5px 10px padding.
            const ImVec2 dmin(bmin.x + 1.f, bmin.y + px(38.f));
            ImGui::SetCursorScreenPos(dmin);
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding,
                                ImVec2(px(10.f), css_text_y(14.f, dmin.y + px(5.5f)) - dmin.y));
            push_font(fonts().regular, 14.f);
            ImGui::InputTextMultiline(description_id, &description,
                                      ImVec2(bmax.x - bmin.x - 2.f, std::max(1.f, bmax.y - 1.f - dmin.y)));
            text_field_menu(description);
            ImGui::PopFont();
            ImGui::PopStyleVar();
            if (description.empty())
            {
                css_text(draw, 14.f, ImVec2(dmin.x + px(10.f), dmin.y + px(5.5f)), theme::u32(p.text_secondary),
                         "Description");
            }
            ImGui::PopStyleColor(4);
            ImGui::PopStyleVar();
            ImGui::PopID();
        }

        // .btn-success / .btn-danger (12px, 6px 12px padding, 31.14px tall): a 1px border around 20% of the colour,
        // 30% while hovered.
        bool tone_action_button(const char* id, const char* label, ImVec2 min, ImVec2 max, ImVec4 tone)
        {
            const theme::Palette& p = theme::palette();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            ImGui::SetCursorScreenPos(min);
            const bool clicked = ImGui::InvisibleButton(id, ImVec2(max.x - min.x, max.y - min.y));
            const bool hovered = ImGui::IsItemHovered();
            draw->AddRectFilled(min, max, theme::u32(theme::with_alpha(tone, hovered ? 0.3f : 0.2f)), px(2.f));
            draw->AddRect(min, max, theme::u32(tone), px(2.f), 0, 1.f);
            const float text_w = text_size(fonts().regular, 12.f, label).x;
            css_text(draw, 12.f, ImVec2(std::round((min.x + max.x - text_w) / 2.f), min.y + px(7.f)),
                     theme::u32(p.text_normal), label);
            return clicked;
        }

        // The composer's Stash tab: a title and description over the stash button, which takes every change
        // (the .btn-success of the commit button, 50% opacity while there is nothing to stash).
        void draw_stash_form(app::Application& app, app::RepoState& state, ImVec2 origin, float width, float c)
        {
            const theme::Palette& p = theme::palette();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            message_input(state.stash_summary, state.stash_description, ImVec2(origin.x + px(11.f), c + px(55.f)),
                          ImVec2(origin.x + width - px(11.f), c + px(55.f + 185.f)), "##stash_summary",
                          "##stash_description", SummaryFocus::NONE, "Stash title", false);

            const bool enabled = !state.status.empty() && !state.history.head_sha.empty();
            const char* label = enabled ? "Stash all changes" : "Stage Changes to Stash";
            const ImVec2 bmin(origin.x + px(11.f), c + px(260.f));
            const ImVec2 bmax(origin.x + width - px(11.f), bmin.y + px(40.f));
            ImGui::SetCursorScreenPos(bmin);
            const bool pressed = ImGui::InvisibleButton("##stash", ImVec2(bmax.x - bmin.x, bmax.y - bmin.y)) && enabled;
            const bool hovered = ImGui::IsItemHovered() && enabled;
            if (ImGui::IsItemHovered() && !enabled)
            {
                ImGui::SetMouseCursor(ImGuiMouseCursor_NotAllowed);
            }
            const float alpha = enabled ? 1.f : 0.5f;
            draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.green, (hovered ? 0.3f : 0.2f) * alpha)),
                                px(2.f));
            draw->AddRect(bmin, bmax, theme::u32(theme::with_alpha(p.green, alpha)), px(2.f), 0, 1.f);
            const float text_w = text_size(fonts().semibold, 12.f, label).x;
            const float content = px(15.f + 5.f) + text_w;
            const float x = bmin.x + (bmax.x - bmin.x - content) / 2.f;
            const ImU32 color = theme::u32(theme::with_alpha(p.text_normal, p.text_normal.w * alpha));
            draw_fa_icon(draw, "far:inbox-in", ImVec2(x + px(7.5f), bmin.y + px(20.f)), px(12.f), color);
            css_text(draw, 12.f, ImVec2(x + px(20.f), bmin.y + px(11.42f)), color, label, fonts().semibold);

            if (pressed || (enabled && ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_Enter)))
            {
                std::string message = state.stash_summary;
                if (!state.stash_description.empty())
                {
                    message += "\n\n" + state.stash_description;
                }
                if (stash_changes(app, state, message))
                {
                    state.stash_summary.clear();
                    state.stash_description.clear();
                }
            }
        }

        void draw_wip_panel(app::Application& app, app::RepoState& state, ImVec2 size)
        {
            const theme::Palette& p = theme::palette();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const git::WorkDirStatus& status = state.status;
            const ImVec2 origin = ImGui::GetCursorScreenPos();
            const float width = size.x;

            // Header (36px, border-bottom): the danger trash button and "N file changes on [branch]" centred in
            // the rest.
            {
                const ImVec2 tmin(origin.x + px(10.f), origin.y + px(8.f));
                const ImVec2 tmax(tmin.x + px(22.5f), tmin.y + px(19.f));
                ImGui::SetCursorScreenPos(tmin);
                const bool trash = ImGui::InvisibleButton("##discard_all", ImVec2(tmax.x - tmin.x, tmax.y - tmin.y));
                const bool trash_hovered = ImGui::IsItemHovered();
                draw->AddRectFilled(tmin, tmax, theme::u32(theme::with_alpha(p.red, trash_hovered ? 0.3f : 0.2f)),
                                    px(2.f));
                draw->AddRect(tmin, tmax, theme::u32(p.red), px(2.f), 0, 1.f);
                draw_fa_icon(draw, "far:trash-alt", ImVec2((tmin.x + tmax.x) / 2.f, (tmin.y + tmax.y) / 2.f), px(12.f),
                             theme::u32(p.text_normal));
                if (trash_hovered)
                {
                    tooltip("Discard all changes");
                }
                if (trash)
                {
                    app.dialog.open(app::Dialog::Kind::DISCARD_ALL);
                }

                const std::size_t count = status.count();
                const std::string text = std::to_string(count) + (count == 1 ? " file change on" : " file changes on");
                const std::string branch = state.history.head_branch.empty() ? "HEAD" : state.history.head_branch;
                const float text_w = text_size(fonts().regular, theme::FONT_SIZE_ROW, text.c_str()).x;
                const float pill_w = text_size(fonts().regular, theme::FONT_SIZE_ROW, branch.c_str()).x + px(10.f);
                const float group = text_w + px(5.f) + pill_w;
                const float x = origin.x + px(32.5f) + (width - px(42.5f) - group) / 2.f;
                const float y = origin.y + px(9.5f);
                css_text(draw, theme::FONT_SIZE_ROW, ImVec2(x, y), theme::u32(p.text_selected), text.c_str());
                const ImVec2 pmin(x + text_w + px(5.f), y);
                draw->AddRectFilled(pmin, ImVec2(pmin.x + pill_w, pmin.y + px(17.f)),
                                    theme::u32(theme::mix(theme::graph_color(0), p.app_bg0, 0.45f)), px(2.f));
                css_text(draw, theme::FONT_SIZE_ROW, ImVec2(pmin.x + px(5.f), y), theme::u32(p.text_selected),
                         branch.c_str());
                draw->AddRectFilled(ImVec2(origin.x, origin.y + px(35.f)),
                                    ImVec2(origin.x + width, origin.y + px(36.f)), theme::u32(p.section_border));
            }

            view_toggle_row(state, origin, width, origin.y + px(46.9f), true);

            // The file sections share the room between the toolbar and the composer (310px from the bottom).
            const float composer_top = origin.y + size.y - px(310.f);
            const float sections_top = origin.y + px(71.f);
            const float sections_bottom = composer_top - px(5.f);
            struct SectionSpec
            {
                const char* label;
                const std::vector<git::FileChange>* files;
                app::DiffTarget::Source source;
                const char* button;
                bool danger;
                const char* action_tip;
            };
            std::vector<SectionSpec> specs;
            if (!status.conflicted.empty())
            {
                specs.push_back({"Conflicted Files", &status.conflicted, app::DiffTarget::Source::UNSTAGED, nullptr,
                                 false, "Mark resolved"});
            }
            specs.push_back({"Unstaged Files", &status.unstaged, app::DiffTarget::Source::UNSTAGED, "Stage All Changes",
                             false, "Stage file"});
            specs.push_back({"Staged Files", &status.staged, app::DiffTarget::Source::STAGED, "Unstage All Changes",
                             true, "Unstage file"});
            float headers = 0.f;
            int open_sections = 0;
            for (std::size_t i = 0; i < specs.size(); ++i)
            {
                headers += px(i == 0 ? 40.f : 44.f);
                if (!state.collapsed_sections.contains(std::string("files:") + specs[i].label))
                {
                    ++open_sections;
                }
            }
            const float list_h =
                    open_sections > 0 ? std::max(0.f, (sections_bottom - sections_top - headers) / open_sections) : 0.f;

            std::vector<const git::FileChange*> stage_requests, unstage_requests, resolve_requests;
            bool stage_all = false, unstage_all = false;
            float y = sections_top;
            for (std::size_t i = 0; i < specs.size(); ++i)
            {
                const SectionSpec& spec = specs[i];
                const float margin = px(i == 0 ? 0.f : 4.f);
                bool clicked = false;
                const bool open = files_section(state, spec.label, spec.files->size(), ImVec2(origin.x, y), width,
                                                margin, spec.button, &clicked, spec.danger);
                if (clicked)
                {
                    (spec.danger ? unstage_all : stage_all) = true;
                }
                y += margin + px(40.f);
                if (!open)
                {
                    continue;
                }
                // .file-node-list-wrapper: a 1px top border, 10px side margins.
                draw->AddRectFilled(ImVec2(origin.x + px(10.f), y), ImVec2(origin.x + width - px(10.f), y + 1.f),
                                    theme::u32(p.section_border));
                ImGui::SetCursorScreenPos(ImVec2(origin.x + px(10.f), y + 1.f));
                ImGui::BeginChild(spec.label, ImVec2(width - px(20.f), std::max(1.f, list_h - 1.f)));
                ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing, ImVec2(ImGui::GetStyle().ItemSpacing.x, 0.f));
                FileNode root = build_tree(*spec.files, state.tree_view);
                FileListContext ctx{&app,       &state,  width - px(20.f), spec.source, {}, spec.action_tip,
                                    Icon::PLUS, nullptr, Icon::PLUS,       {},          {}};
                draw_file_tree(ctx, root, 1, "wip:");
                ImGui::PopStyleVar();
                ImGui::EndChild();
                for (const git::FileChange* change : ctx.action_requests)
                {
                    if (spec.source == app::DiffTarget::Source::STAGED)
                    {
                        unstage_requests.push_back(change);
                    }
                    else if (spec.button == nullptr)
                    {
                        resolve_requests.push_back(change);
                    }
                    else
                    {
                        stage_requests.push_back(change);
                    }
                }
                y += list_h;
            }
            if (stage_all)
            {
                app.run(state, "Stage all",
                        [&]
                        {
                            return state.repo.stage_all();
                        });
            }
            else if (unstage_all)
            {
                app.run(state, "Unstage all",
                        [&]
                        {
                            return state.repo.unstage_all();
                        });
            }
            else if (!stage_requests.empty() || !resolve_requests.empty())
            {
                std::vector<std::string> paths;
                for (const git::FileChange* change : stage_requests)
                {
                    paths.push_back(change->path);
                }
                for (const git::FileChange* change : resolve_requests)
                {
                    paths.push_back(change->path);
                }
                app.run(state, "Stage",
                        [&]
                        {
                            return state.repo.stage(paths);
                        });
            }
            else if (!unstage_requests.empty())
            {
                std::vector<std::string> paths;
                for (const git::FileChange* change : unstage_requests)
                {
                    paths.push_back(change->path);
                    if (!change->old_path.empty())
                    {
                        paths.push_back(change->old_path);
                    }
                }
                app.run(state, "Unstage",
                        [&]
                        {
                            return state.repo.unstage(paths);
                        });
            }

            // The composer, anchored to the bottom.
            const float c = composer_top;
            {
                const float cx = origin.x + width / 2.f;
                draw->AddRectFilled(ImVec2(cx - px(50.f), c), ImVec2(cx + px(50.f), c + px(4.f)),
                                    theme::u32(p.section_border), px(4.f));
            }
            // Tabs (Commit | Stash): the active tab shows its label and is boxed on three sides, the other is
            // its icon alone; the rest of the row is underlined.
            {
                const float top = c + px(10.f);
                const float h = px(31.f);
                const float row_l = origin.x + px(10.f);
                const float row_r = origin.x + width - px(10.f);
                const ImU32 border = theme::u32(p.section_border);
                struct Tab
                {
                    const char* id;
                    const char* icon;
                    const char* label;
                    bool stash;
                };
                const Tab tabs[] = {{"##tab_commit", "far:code-commit", "Commit", false},
                                    {"##tab_stash", "far:inbox-in", "Stash", true}};
                float x = row_l;
                float active_l = row_l;
                float active_r = row_l;
                for (const Tab& tab : tabs)
                {
                    const bool active = tab.stash == state.composer_stash;
                    const float label_w = active ? text_size(fonts().regular, 14.f, tab.label).x : 0.f;
                    const float w = active ? px(33.5f) + label_w + px(13.f) : px(31.f);
                    ImGui::SetCursorScreenPos(ImVec2(x, top));
                    if (ImGui::InvisibleButton(tab.id, ImVec2(w, h - 1.f)) && !active)
                    {
                        state.composer_stash = tab.stash;
                    }
                    const bool hovered = ImGui::IsItemHovered();
                    if (hovered && !active)
                    {
                        tooltip(tab.label);
                    }
                    const ImU32 color = theme::u32(active || hovered ? p.text_selected : p.text_secondary);
                    if (active)
                    {
                        active_l = x;
                        active_r = x + w;
                        draw->AddRectFilled(ImVec2(x, top), ImVec2(x + w, top + 1.f), border);
                        draw->AddRectFilled(ImVec2(x, top), ImVec2(x + 1.f, top + h), border);
                        draw->AddRectFilled(ImVec2(x + w - 1.f, top), ImVec2(x + w, top + h), border);
                        draw_fa_icon(draw, tab.icon, ImVec2(x + px(19.75f), top + px(16.f)), px(14.f), color);
                        css_text(draw, 14.f, ImVec2(x + px(33.5f), top + px(6.f)), color, tab.label);
                    }
                    else
                    {
                        draw_fa_icon(draw, tab.icon, ImVec2(x + px(15.5f), top + px(15.f)), px(14.f), color);
                    }
                    x += w + px(6.7f);
                }
                draw->AddRectFilled(ImVec2(row_l, top + h - 1.f), ImVec2(active_l, top + h), border);
                draw->AddRectFilled(ImVec2(active_r, top + h - 1.f), ImVec2(row_r, top + h), border);
            }
            if (state.composer_stash)
            {
                draw_stash_form(app, state, origin, width, c);
                return;
            }
            // Amend previous commit
            if (small_checkbox("##amend", "Amend previous commit", &state.amend, ImVec2(origin.x + px(11.f), c + px(55.f)),
                            ImVec2(origin.x + px(31.f), c + px(51.f)), 14.f)
                && state.amend)
            {
                if (auto message = state.repo.head_commit_message())
                {
                    const auto newline = message->find('\n');
                    state.commit_summary = message->substr(0, newline);
                    state.commit_description.clear();
                    if (newline != std::string::npos)
                    {
                        std::string body = message->substr(newline + 1);
                        while (!body.empty() && (body.front() == '\n' || body.front() == '\r'))
                        {
                            body.erase(body.begin());
                        }
                        while (!body.empty() && (body.back() == '\n' || body.back() == '\r'))
                        {
                            body.pop_back();
                        }
                        state.commit_description = body;
                    }
                }
            }
            // .commit-message-input: summary (16px, 72-character counter) over the description.
            message_input(
                    state.commit_summary, state.commit_description, ImVec2(origin.x + px(11.f), c + px(76.f)),
                    ImVec2(origin.x + width - px(11.f), c + px(76.f + 137.9f)), "##summary", "##description",
                    ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | ImGuiKey_M) ? SummaryFocus::SELECT_ALL
                                                                                          : SummaryFocus::NONE);
            // Commit options
            {
                const ImVec2 omin(origin.x + px(11.f), c + px(221.f));
                const char* options = "Commit options";
                const float options_w = text_size(fonts().regular, 14.f, options).x;
                ImGui::SetCursorScreenPos(omin);
                if (ImGui::InvisibleButton("##commit_options", ImVec2(px(20.f) + options_w, px(22.f))))
                {
                    state.commit_options_open = !state.commit_options_open;
                }
                const ImU32 color = theme::u32(ImGui::IsItemHovered() ? p.text_normal : p.text_disabled);
                draw_fa_icon(draw, state.commit_options_open ? "far:chevron-down" : "far:chevron-right",
                             ImVec2(origin.x + px(18.5f), c + px(231.9f)), px(12.f), color);
                css_text(draw, 14.f, ImVec2(origin.x + px(31.f), c + px(221.92f)), color, options);
            }
            if (state.commit_options_open)
            {
                // The one commit option ImGit supports, inline.
                small_checkbox("##push_after", "Push after commit", &state.push_after_commit,
                            ImVec2(origin.x + px(31.f), c + px(250.f)), ImVec2(origin.x + px(51.f), c + px(246.f)),
                            12.f);
            }

            // The commit button (.btn-success .normal-button, 40px, 50% opacity while disabled).
            const std::size_t staged_count = status.staged.size();
            std::string label;
            bool enabled;
            if (state.amend)
            {
                label = "Amend Previous Commit";
                enabled = !state.commit_summary.empty() && !state.history.head_sha.empty();
            }
            else if (staged_count == 0)
            {
                label = "Stage files/changes to commit";
                enabled = false;
            }
            else if (state.commit_summary.empty())
            {
                label = "Type a Message to Commit";
                enabled = false;
            }
            else
            {
                label = "Commit changes to " + std::to_string(staged_count) + (staged_count == 1 ? " file" : " files");
                enabled = true;
            }
            bool pressed = false;
            {
                const ImVec2 bmin(origin.x + px(11.f), c + px(260.f));
                const ImVec2 bmax(origin.x + width - px(11.f), bmin.y + px(40.f));
                ImGui::SetCursorScreenPos(bmin);
                pressed = ImGui::InvisibleButton("##commit", ImVec2(bmax.x - bmin.x, bmax.y - bmin.y)) && enabled;
                const bool hovered = ImGui::IsItemHovered() && enabled;
                if (ImGui::IsItemHovered() && !enabled)
                {
                    ImGui::SetMouseCursor(ImGuiMouseCursor_NotAllowed);
                }
                const float alpha = enabled ? 1.f : 0.5f;
                draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.green, (hovered ? 0.3f : 0.2f) * alpha)),
                                    px(2.f));
                draw->AddRect(bmin, bmax, theme::u32(theme::with_alpha(p.green, alpha)), px(2.f), 0, 1.f);
                const float text_w = text_size(fonts().semibold, 12.f, label.c_str()).x;
                const float content = px(15.f + 5.f) + text_w;
                const float x = bmin.x + (bmax.x - bmin.x - content) / 2.f;
                const ImU32 color = theme::u32(theme::with_alpha(p.text_normal, p.text_normal.w * alpha));
                draw_fa_icon(draw, "far:code-commit", ImVec2(x + px(7.5f), bmin.y + px(20.f)), px(12.f), color);
                css_text(draw, 12.f, ImVec2(x + px(20.f), bmin.y + px(11.42f)), color, label.c_str(), fonts().semibold);
            }
            if (pressed || (enabled && ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_Enter)))
            {
                std::string message = state.commit_summary;
                if (!state.commit_description.empty())
                {
                    message += "\n\n" + state.commit_description;
                }
                const bool amend = state.amend;
                git::Repository* repo = &state.repo;
                const git::Sha previous_head = state.history.head_sha;
                if (app.run(
                            state, amend ? "Amend" : "Commit",
                            [&]
                            {
                                return state.repo.commit(message, amend);
                            },
                            amend ? "Commit amended" : "Changes committed"))
                {
                    if (!amend && !previous_head.empty())
                    {
                        state.push_undo({"commit",
                                         [repo, previous_head]
                                         {
                                             return repo->reset(previous_head, git::ResetMode::SOFT);
                                         },
                                         {},
                                         git::ResetMode::SOFT});
                    }
                    state.commit_summary.clear();
                    state.commit_description.clear();
                    state.amend = false;
                    if (state.push_after_commit)
                    {
                        app.run_git_cli(state, "Push", {"push"});
                    }
                }
            }
        }

        // "Co-Authored-By: Name <email>" trailers of a commit message.
        std::vector<git::Author> co_authors(const std::string& body)
        {
            std::vector<git::Author> result;
            std::size_t start = 0;
            while (start < body.size())
            {
                std::size_t end = body.find('\n', start);
                if (end == std::string::npos)
                {
                    end = body.size();
                }
                const std::string line = body.substr(start, end - start);
                start = end + 1;
                constexpr std::string_view KEY = "co-authored-by:";
                if (line.size() <= KEY.size() || !contains_ci(line.substr(0, KEY.size()), std::string(KEY)))
                {
                    continue;
                }
                git::Author author;
                const std::size_t open = line.find('<');
                const std::size_t close = line.find('>', open);
                author.name =
                        line.substr(KEY.size(), open == std::string::npos ? std::string::npos : open - KEY.size());
                while (!author.name.empty() && author.name.front() == ' ')
                {
                    author.name.erase(author.name.begin());
                }
                while (!author.name.empty() && author.name.back() == ' ')
                {
                    author.name.pop_back();
                }
                if (open != std::string::npos && close != std::string::npos)
                {
                    author.email = line.substr(open + 1, close - open - 1);
                }
                result.push_back(author);
            }
            return result;
        }

        // Text laid out top-down with CSS line boxes (line-height 1.42857, Bootstrap's body default).
        float css_line_height(float font_size)
        {
            return px(font_size) * 1.42857f;
        }

        // Draws `text` wrapped to `width` with the given CSS font size; returns the height used.
        float draw_wrapped(ImDrawList* draw, ImFont* font, float size, ImVec2 pos, float width, ImU32 color,
                           const std::string& text)
        {
            const float line_h = css_line_height(size);
            const float glyph_h = font_px(font, size);
            float y = pos.y;
            std::size_t start = 0;
            while (start <= text.size())
            {
                std::size_t end = text.find('\n', start);
                if (end == std::string::npos)
                {
                    end = text.size();
                }
                const char* line = text.c_str() + start;
                const char* line_end = text.c_str() + end;
                if (line == line_end)
                {
                    y += line_h;
                }
                while (line < line_end)
                {
                    const char* wrap = font->CalcWordWrapPosition(glyph_h, line, line_end, width);
                    if (wrap == line)
                    {
                        wrap = line + 1;
                    }
                    draw->AddText(font, glyph_h, ImVec2(pos.x, y + (line_h - glyph_h) / 2.f), color, line, wrap);
                    y += line_h;
                    line = wrap;
                    while (line < line_end && *line == ' ')
                    {
                        ++line;
                    }
                }
                start = end + 1;
            }
            return y - pos.y;
        }

        void draw_commit_panel(app::Application& app, app::RepoState& state, const git::Commit& commit, ImVec2 size)
        {
            const theme::Palette& p = theme::palette();
            state.load_commit_files(commit.sha);

            ImGui::BeginChild("##commit_detail", size, ImGuiChildFlags_None, ImGuiWindowFlags_NoScrollbar);
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const ImVec2 origin = ImGui::GetCursorScreenPos();
            const float right = origin.x + size.x - px(10.f);
            const float left = origin.x + px(10.f);
            const bool is_stash = commit.type == git::NodeType::STASH;

            // The commit view: an optional 36px "file changes in working directory"
            // banner, the 36px header, then the message view and the file list sharing the rest equally.
            const float width = size.x;
            int wip_files = 0;
            git::Sha wip_sha;
            for (const git::Commit& row : state.history.commits)
            {
                if (row.type == git::NodeType::WORK_DIR)
                {
                    wip_sha = row.sha;
                    wip_files = static_cast<int>(state.status.staged.size() + state.status.unstaged.size()
                                                 + state.status.conflicted.size());
                    break;
                }
            }
            const float banner_h = wip_files > 0 ? px(36.f) : 0.f;
            if (banner_h > 0.f)
            {
                draw->AddRectFilled(origin, ImVec2(origin.x + width, origin.y + banner_h),
                                    theme::u32(theme::with_alpha(p.accent, 0.4f)));
                const std::string text = std::to_string(wip_files) + (wip_files == 1 ? " file change" : " file changes")
                                         + " in working directory";
                const char* button = "View Changes";
                const float text_w = text_size(fonts().regular, theme::FONT_SIZE_ROW, text.c_str()).x;
                const float button_w = text_size(fonts().regular, theme::FONT_SIZE_ROW, button).x + px(12.f);
                const float x = std::round(origin.x + (width - (text_w + px(10.f) + button_w)) / 2.f);
                css_text(draw, theme::FONT_SIZE_ROW, ImVec2(x, origin.y + px(9.4f)), theme::u32(p.text_selected),
                         text.c_str());
                const ImVec2 bmin(x + text_w + px(10.f), origin.y + px(7.f));
                const ImVec2 bmax(bmin.x + button_w, bmin.y + px(22.f));
                ImGui::SetCursorScreenPos(bmin);
                if (ImGui::InvisibleButton("##view_changes", ImVec2(bmax.x - bmin.x, bmax.y - bmin.y)))
                {
                    state.select_only(wip_sha);
                    state.close_diff();
                }
                if (ImGui::IsItemHovered())
                {
                    draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.white, 0.1f)), px(2.f));
                }
                draw->AddRect(bmin, bmax, theme::u32(p.text_normal), px(2.f), 0, 1.f);
                css_text(draw, theme::FONT_SIZE_ROW, ImVec2(bmin.x + px(6.f), bmin.y + px(2.5f)),
                         theme::u32(p.text_normal), button);
            }

            // Header: "commit: <sha6>", centred.
            const float header_top = origin.y + banner_h;
            {
                const std::string label = is_stash ? "stash:" : "commit:";
                const std::string sha = git::short_sha(commit.sha).substr(0, 6);
                const float label_w = text_size(fonts().regular, theme::FONT_SIZE_ROW, label.c_str()).x;
                const float sha_w = text_size(fonts().regular, theme::FONT_SIZE_ROW, sha.c_str()).x;
                const float total = label_w + px(5.f) + sha_w;
                const float x = origin.x + px(10.f) + (width - px(20.f) - total) / 2.f;
                const float y = header_top + px(9.5f);
                css_text(draw, theme::FONT_SIZE_ROW, ImVec2(x, y), theme::u32(p.text_normal), label.c_str());
                ImGui::SetCursorScreenPos(ImVec2(x + label_w + px(5.f), y));
                ImGui::InvisibleButton("##sha", ImVec2(sha_w, px(17.f)));
                css_text(draw, theme::FONT_SIZE_ROW, ImVec2(x + label_w + px(5.f), y), theme::u32(p.text_selected),
                         sha.c_str());
                if (ImGui::IsItemHovered())
                {
                    tooltip("Copy commit sha");
                }
                if (ImGui::IsItemClicked(ImGuiMouseButton_Left))
                {
                    app.copy_to_clipboard(commit.sha);
                }
                draw->AddRectFilled(ImVec2(origin.x, header_top + px(35.f)),
                                    ImVec2(origin.x + width, header_top + px(36.f)), theme::u32(p.section_border));
            }

            // The message: the re-resizable box, layout.CommitMessage.rightPanelHeight tall (150 by default)
            // within [120, panelHeight - 213] (52 more while editing), panelHeight being what the header leaves. The
            // view fills it but its 17px bottom margin; the editor fills it but 50px. Its 100 x 4 handle, 9px above
            // its bottom edge, drags it with the row-resize cursor. HEAD's message is editable: hovering the view
            // turns its border blue, clicking it opens the editor with Update Message / Cancel Amend under it.
            const float box_top = header_top + px(46.f);
            const bool amendable = commit.sha == state.history.head_sha && !is_stash && !state.rebase.active;
            if (!amendable)
            {
                state.amending_message = false;
            }
            const bool editing = state.amending_message;
            // Update Message rewrites HEAD and reloads the history `commit` points into: done once the panel is drawn.
            bool update_message = false;
            const auto finish = [&]
            {
                if (!update_message)
                {
                    return;
                }
                std::string message = state.amend_summary;
                if (!state.amend_description.empty())
                {
                    message += "\n\n" + state.amend_description;
                }
                if (app.run(state, "Update message",
                            [&]
                            {
                                return state.repo.reword_head(message);
                            }))
                {
                    state.amending_message = false;
                    // The amended commit replaces HEAD: keep it selected.
                    state.select_only(state.history.head_sha);
                }
            };
            const float panel_h = origin.y + size.y - header_top - px(36.f);
            const float max_h = panel_h - px(143.f + 50.f + 20.f) - (editing ? px(52.f) : 0.f);
            const float min_h = px(120.f);
            // A short window may leave less than the minimum: the box keeps the minimum rather than going negative.
            float message_h = std::clamp(px(app.settings.commit_message_height), min_h, std::max(min_h, max_h));
            {
                const float cx = std::round(origin.x + width / 2.f);
                const ImVec2 hmin(cx - px(50.f), box_top + message_h - px(9.f));
                const ImVec2 hmax(cx + px(50.f), hmin.y + px(4.f));
                const float before = message_h;
                if (resize_drag("##message_resize", hmin, hmax, false, 1.f, &message_h, min_h, std::max(min_h, max_h)))
                {
                    app.settings.save(app.config_path("settings.txt"));
                }
                if (message_h != before)
                {
                    app.settings.commit_message_height = message_h / dpi_scale();
                }
                draw->AddRectFilled(ImVec2(cx - px(50.f), box_top + message_h - px(9.f)),
                                    ImVec2(cx + px(50.f), box_top + message_h - px(5.f)), theme::u32(p.section_border),
                                    px(4.f));
            }
            if (editing)
            {
                message_input(state.amend_summary, state.amend_description, ImVec2(left, box_top),
                              ImVec2(right, box_top + message_h - px(50.f)), "##amend_summary", "##amend_description",
                              state.amend_focus_requested ? SummaryFocus::CARET_END : SummaryFocus::NONE);
                state.amend_focus_requested = false;
                // The button row (10px padding): Update Message, 5px, Cancel Amend, sharing the width.
                const float row_top = box_top + message_h + px(10.f);
                const float button_w = (right - left - px(5.f)) / 2.f;
                const ImVec2 umin(left, row_top);
                const ImVec2 umax(std::round(left + button_w), row_top + px(31.14f));
                const ImVec2 cmin(umax.x + px(5.f), row_top);
                const ImVec2 cmax(right, umax.y);
                update_message = tone_action_button("##update_message", "Update Message", umin, umax, p.green);
                if (tone_action_button("##cancel_amend", "Cancel Amend", cmin, cmax, p.red))
                {
                    state.amending_message = false;
                }
            }
            else
            {
                const ImVec2 bmin(left, box_top);
                const ImVec2 bmax(right, std::max(bmin.y + 1.f, box_top + message_h - px(17.f)));
                bool hovered = false;
                if (amendable)
                {
                    ImGui::SetCursorScreenPos(bmin);
                    if (ImGui::InvisibleButton("##editable_message", ImVec2(bmax.x - bmin.x, bmax.y - bmin.y)))
                    {
                        state.amending_message = true;
                        state.amend_focus_requested = true;
                        state.amend_summary = commit.summary;
                        state.amend_description = commit.body;
                        while (!state.amend_description.empty()
                               && (state.amend_description.back() == '\n' || state.amend_description.back() == '\r'))
                        {
                            state.amend_description.pop_back();
                        }
                    }
                    hovered = ImGui::IsItemHovered();
                    if (hovered)
                    {
                        ImGui::SetMouseCursor(ImGuiMouseCursor_Hand);
                        styled_tooltip_left(bmin, bmax, "Click to amend your commit message");
                    }
                }
                draw->AddRectFilled(bmin, bmax, theme::u32(p.input_bg), px(4.f));
                draw->AddRect(bmin, bmax, theme::u32(hovered ? p.accent : p.subtle_border), px(4.f), 0, 1.f);
                draw->PushClipRect(bmin, bmax, true);
                const float text_x = bmin.x + px(11.f);
                const float text_w = bmax.x - px(11.f) - text_x;
                float y = bmin.y + px(11.f);
                y += draw_wrapped(draw, fonts().regular, 16.f, ImVec2(text_x, y), text_w, theme::u32(p.text_normal),
                                  commit.summary);
                if (!commit.body.empty())
                {
                    y += px(10.f);
                    draw_wrapped(draw, fonts().regular, theme::FONT_SIZE_ROW, ImVec2(text_x, y), text_w,
                                 theme::u32(p.text_normal), commit.body);
                }
                draw->PopClipRect();
            }

            // Author: 40px avatar, name (14px), "authored <date>", "parent: <sha6>" on the right.
            const float author_top = box_top + message_h + (editing ? px(61.14f) : 0.f);
            {
                const float avatar = px(40.f);
                draw_avatar(draw, ImVec2(left + avatar / 2.f, author_top + avatar / 2.f), avatar, commit.author,
                            px(2.f));
                const float name_x = left + px(48.f);
                draw_text_ellipsis(draw, fonts().regular, 14.f, ImVec2(name_x, css_text_y(14.f, author_top + px(0.7f))),
                                   right - name_x - px(100.f), theme::u32(p.text_normal), commit.author.name.c_str());
                const char* verb = "authored";
                const float verb_w = text_size(fonts().regular, theme::FONT_SIZE_ROW, verb).x;
                css_text(draw, theme::FONT_SIZE_ROW, ImVec2(name_x, author_top + px(22.1f)),
                         theme::u32(p.text_secondary), verb);
                css_text(draw, theme::FONT_SIZE_ROW, ImVec2(name_x + verb_w + px(5.f), author_top + px(22.1f)),
                         theme::u32(p.text_normal), format_date_time(commit.author_time).c_str());
                tooltip_on_rect(ImVec2(left, author_top), ImVec2(left + avatar, author_top + avatar),
                                (commit.author.name + " <" + commit.author.email + ">").c_str());

                float px_right = right;
                for (std::size_t i = commit.parents.size(); i-- > 0;)
                {
                    const std::string parent = git::short_sha(commit.parents[i]).substr(0, 6);
                    const float w = text_size(fonts().regular, theme::FONT_SIZE_ROW, parent.c_str()).x;
                    px_right -= w;
                    ImGui::SetCursorScreenPos(ImVec2(px_right, author_top));
                    ImGui::PushID(static_cast<int>(i));
                    ImGui::InvisibleButton("##parent", ImVec2(w, px(17.f)));
                    const bool hovered = ImGui::IsItemHovered();
                    if (ImGui::IsItemClicked(ImGuiMouseButton_Left))
                    {
                        jump_to_commit(state, commit.parents[i].c_str());
                    }
                    if (hovered)
                    {
                        tooltip("Jump to parent");
                    }
                    ImGui::PopID();
                    css_text(draw, theme::FONT_SIZE_ROW, ImVec2(px_right, author_top),
                             theme::u32(hovered ? p.text_selected : p.text_normal), parent.c_str());
                    px_right -= px(5.f);
                }
                if (!commit.parents.empty())
                {
                    const char* label = commit.parents.size() == 1 ? "parent:" : "parents:";
                    const float w = text_size(fonts().regular, theme::FONT_SIZE_ROW, label).x;
                    css_text(draw, theme::FONT_SIZE_ROW, ImVec2(px_right - w, author_top), theme::u32(p.text_secondary),
                             label);
                }
            }

            // Co-authors from the message trailers.
            const std::vector<git::Author> co = co_authors(commit.body);
            if (!co.empty())
            {
                const float co_y = author_top + px(44.f);
                css_text(draw, theme::FONT_SIZE_ROW, ImVec2(left, co_y + px(2.f)), theme::u32(p.text_secondary),
                         "Co-authors:");
                float x = left + text_size(fonts().regular, theme::FONT_SIZE_ROW, "Co-authors:").x + px(5.f);
                for (const git::Author& author : co)
                {
                    const float avatar = px(20.f);
                    draw_avatar(draw, ImVec2(x + avatar / 2.f, co_y + avatar / 2.f), avatar, author, px(2.f));
                    tooltip_on_rect(ImVec2(x, co_y), ImVec2(x + avatar, co_y + avatar),
                                    (author.name + " <" + author.email + ">").c_str());
                    x += avatar + px(4.f);
                }
            }

            if (!state.commit_files_error.empty())
            {
                ImGui::SetCursorScreenPos(ImVec2(left, author_top + px(70.f)));
                ImGui::PushStyleColor(ImGuiCol_Text, p.red);
                ImGui::PushTextWrapPos(right);
                ImGui::TextWrapped("%s", state.commit_files_error.c_str());
                ImGui::PopTextWrapPos();
                ImGui::PopStyleColor();
                ImGui::EndChild();
                finish();
                return;
            }

            // .tiny-files-readout: "✎ 1 modified" etc., a 10px solid icon (fa-fw), 5px, the text, 8px.
            {
                int added = 0, modified = 0, deleted = 0, renamed = 0;
                for (const git::FileChange& change : state.commit_files)
                {
                    switch (change.status)
                    {
                    case git::FileStatus::ADDED:
                    case git::FileStatus::UNTRACKED:
                        ++added;
                        break;
                    case git::FileStatus::DELETED:
                        ++deleted;
                        break;
                    case git::FileStatus::RENAMED:
                        ++renamed;
                        break;
                    default:
                        ++modified;
                        break;
                    }
                }
                const float line_top = author_top + px(70.4f);
                float x = left;
                const auto item = [&](int count, const char* icon, ImVec4 color, const char* word)
                {
                    if (count == 0)
                    {
                        return;
                    }
                    draw_fa_icon(draw, icon, ImVec2(x + px(6.25f), line_top + px(9.3f)), px(10.f), theme::u32(color));
                    const std::string text = std::to_string(count) + " " + word;
                    css_text(draw, theme::FONT_SIZE_ROW, ImVec2(x + px(17.5f), line_top), theme::u32(p.text_normal),
                             text.c_str());
                    x += px(17.5f) + text_size(fonts().regular, theme::FONT_SIZE_ROW, text.c_str()).x + px(8.f);
                };
                item(modified, "fas:pencil", p.orange, "modified");
                item(added, "fas:plus", p.green, "added");
                item(deleted, "fas:minus", p.red, "deleted");
                item(renamed, "fas:pencil", p.orange, "renamed");
            }

            // Sort, Path | Tree, View all files
            float y = author_top + px(98.9f);
            {
                const float row_top = y;
                const float row_h = px(24.1f);
                ImGui::SetCursorScreenPos(ImVec2(left + px(10.f), row_top));
                ImGui::InvisibleButton("##sort", ImVec2(px(20.f), row_h));
                draw_fa_icon(draw, "far:sort-alpha-up", ImVec2(left + px(20.f), row_top + px(12.1f)), px(16.f),
                             theme::u32(ImGui::IsItemHovered() ? p.text_selected : p.text_normal));
                if (ImGui::IsItemHovered())
                {
                    tooltip("Sort");
                }
                // The toggle button pair: 1px 20% white borders, the checked one rgba(77,136,255,.2) with white text.
                const char* icons[] = {"far:bars", "fas:folder-tree"};
                const char* labels[] = {"Path", "Tree"};
                float widths[2];
                for (int i = 0; i < 2; ++i)
                {
                    widths[i] = px(1.f + 10.f) + fa_icon_width(icons[i], px(12.f)) + px(5.f)
                                + text_size(fonts().regular, theme::FONT_SIZE_ROW, labels[i]).x + px(10.f + 1.f);
                }
                float bx = origin.x + (width - widths[0] - widths[1]) / 2.f;
                for (int i = 0; i < 2; ++i)
                {
                    const bool checked = state.tree_view == (i == 1);
                    const ImVec2 bmin(bx, row_top);
                    const ImVec2 bmax(bx + widths[i], row_top + row_h);
                    ImGui::SetCursorScreenPos(bmin);
                    ImGui::PushID(i);
                    if (ImGui::InvisibleButton("##view_mode", ImVec2(widths[i], row_h)))
                    {
                        state.tree_view = i == 1;
                    }
                    const bool hovered = ImGui::IsItemHovered();
                    ImGui::PopID();
                    const ImVec4 accent = theme::with_alpha(p.accent, 0.2f);
                    if (checked)
                    {
                        draw->AddRectFilled(bmin, bmax, theme::u32(accent));
                    }
                    else if (hovered)
                    {
                        draw->AddRectFilled(bmin, bmax, theme::u32(p.hover_row));
                    }
                    draw->AddRect(bmin, bmax, theme::u32(checked ? accent : theme::with_alpha(p.white, 0.2f)), 0.f, 0,
                                  1.f);
                    const ImU32 color = theme::u32(checked || hovered ? p.text_selected : p.text_normal);
                    const float icon_w = fa_icon_width(icons[i], px(12.f));
                    draw_fa_icon(draw, icons[i], ImVec2(bmin.x + px(11.f) + icon_w / 2.f, row_top + px(12.05f)),
                                 px(12.f), color);
                    css_text(draw, theme::FONT_SIZE_ROW, ImVec2(bmin.x + px(16.f) + icon_w, row_top + px(3.57f)), color,
                             labels[i]);
                    bx += widths[i];
                }
                static bool view_all = false;
                const char* all = "View all files";
                const float all_w = text_size(fonts().regular, theme::FONT_SIZE_ROW, all).x;
                const float label_x = origin.x + width - px(15.f) - all_w;
                const float box_x = label_x - px(20.f);
                ImGui::SetCursorScreenPos(ImVec2(box_x, row_top));
                if (ImGui::InvisibleButton("##view_all", ImVec2(label_x + all_w - box_x, row_h)))
                {
                    view_all = !view_all;
                    app.toasts.push(app::ToastKind::INFO, "View all files",
                                    "Browsing the whole tree of a commit is not available yet.");
                    view_all = false;
                }
                const ImVec2 cmin(box_x, row_top + px(6.1f));
                draw->AddRectFilled(cmin, ImVec2(cmin.x + px(12.f), cmin.y + px(12.f)), theme::u32(p.input_bg),
                                    px(1.f));
                draw->AddRect(cmin, ImVec2(cmin.x + px(12.f), cmin.y + px(12.f)), theme::u32(p.text_disabled), px(1.f),
                              0, 1.f);
                css_text(draw, theme::FONT_SIZE_ROW, ImVec2(label_x, row_top + px(3.5f)), theme::u32(p.text_normal),
                         all);
                y = author_top + px(129.f);
                draw->AddRectFilled(ImVec2(left, y), ImVec2(right, y + 1.f), theme::u32(p.section_border));
                y += 1.f;
            }

            // Files
            ImGui::SetCursorScreenPos(ImVec2(left, y));
            ImGui::BeginChild("##commit_files", ImVec2(right - left, std::max(1.f, origin.y + size.y - px(10.f) - y)));
            FileNode root = build_tree(state.commit_files, state.tree_view);
            FileListContext ctx{&app,
                                &state,
                                right - left,
                                app::DiffTarget::Source::COMMIT,
                                commit.sha,
                                nullptr,
                                Icon::PLUS,
                                nullptr,
                                Icon::PLUS,
                                {},
                                {}};
            ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing, ImVec2(ImGui::GetStyle().ItemSpacing.x, 0.f));
            draw_file_tree(ctx, root, 1, "commit:");
            ImGui::PopStyleVar();
            ImGui::EndChild();
            ImGui::EndChild();
            finish();
        }

        void draw_multi_panel(app::RepoState& state, ImVec2 size)
        {
            const theme::Palette& p = theme::palette();
            ImGui::BeginChild("##multi_detail", size);
            ImGui::Dummy(ImVec2(0.f, px(8.f)));
            ImGui::Indent(px(12.f));
            push_font(fonts().semibold, theme::FONT_SIZE_NORMAL + 1.f);
            ImGui::Text("%zu commits selected", state.selected.size());
            ImGui::PopFont();
            ImGui::PushStyleColor(ImGuiCol_Text, p.text_secondary);
            ImGui::TextWrapped("Right-click the graph to cherry pick, squash or drop them together. Select one commit "
                               "to see its changes.");
            ImGui::PopStyleColor();
            ImGui::Unindent(px(12.f));
            ImGui::Dummy(ImVec2(0.f, px(8.f)));
            push_font(fonts().regular, theme::FONT_SIZE_ROW);
            for (const git::Sha& sha : state.selected)
            {
                if (const git::Commit* commit = state.commit_for(sha); commit != nullptr)
                {
                    ImGui::Indent(px(12.f));
                    ImGui::PushStyleColor(ImGuiCol_Text, p.text_secondary);
                    push_font(fonts().mono, theme::FONT_SIZE_ROW);
                    ImGui::TextUnformatted(sha == app::WIP_SHA ? "WIP    " : git::short_sha(sha).c_str());
                    ImGui::PopFont();
                    ImGui::PopStyleColor();
                    ImGui::SameLine();
                    ImGui::TextUnformatted(commit->summary.c_str());
                    ImGui::Unindent(px(12.f));
                }
            }
            ImGui::PopFont();
            ImGui::EndChild();
        }
    } // namespace

    bool stash_changes(app::Application& app, app::RepoState& state, const std::string& message)
    {
        git::Sha stash_sha;
        if (!app.run(
                    state, "Stash",
                    [&]
                    {
                        git::Result<git::Sha> pushed = state.repo.stash_push(message);
                        if (pushed)
                        {
                            stash_sha = *pushed;
                        }
                        return pushed;
                    },
                    "Changes stashed"))
        {
            return false;
        }
        // The undo finds the stash by its commit: one pushed since shifts the indexes.
        git::Repository* repo = &state.repo;
        state.push_undo({"stash",
                         [repo, stash_sha]() -> git::Result<void>
                         {
                             const int index = repo->stash_index_of(stash_sha);
                             if (index < 0)
                             {
                                 return std::unexpected(git::Error{"the stash is gone"});
                             }
                             return repo->stash_pop(index);
                         },
                         [repo, message]() -> git::Result<void>
                         {
                             const git::Result<git::Sha> pushed = repo->stash_push(message);
                             if (!pushed)
                             {
                                 return std::unexpected(pushed.error());
                             }
                             return {};
                         }});
        return true;
    }

    void draw_detail_panel(app::Application& app, app::RepoState& state, ImVec2 size)
    {
        const theme::Palette& p = theme::palette();
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        ImGui::BeginChild("##detail_panel", size, ImGuiChildFlags_None, ImGuiWindowFlags_NoScrollbar);
        // .commit-detail-wrapper: panel-bg0.
        ImGui::GetWindowDrawList()->AddRectFilled(origin, ImVec2(origin.x + size.x, origin.y + size.y),
                                                  theme::u32(p.panel_bg0));
        ImGui::GetWindowDrawList()->AddLine(origin, ImVec2(origin.x, origin.y + size.y), theme::u32(p.panel_border),
                                            1.f);
        ImGui::SetCursorScreenPos(ImVec2(origin.x + 1.f, origin.y));
        const ImVec2 inner(size.x - 1.f, size.y);

        if (state.selected.size() > 1)
        {
            draw_multi_panel(state, inner);
        }
        else if (const git::Commit* commit = state.single_selected();
                 commit != nullptr && commit->type != git::NodeType::WORK_DIR)
        {
            draw_commit_panel(app, state, *commit, inner);
        }
        else
        {
            draw_wip_panel(app, state, inner);
        }
        ImGui::EndChild();
    }
} // namespace imgit::ui
