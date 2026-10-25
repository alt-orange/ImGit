#include "app/application.hpp"
#include "app/theme.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cmath>
#include <imgui.h>
#include <string>
#include <vector>

namespace imgit::ui
{
    namespace
    {
        // The file view: a 36px header, a 53px toolbar row, then Monaco's diff editor
        // with Consolas 12px on 16px lines.
        constexpr float HEADER_H = 36.f;
        constexpr float TOOLBAR_H = 53.f;
        constexpr float LINE_H = 16.f;
        constexpr float OLD_NUMBERS_RIGHT = 33.f; // the original editor's 33px line-number margin
        constexpr float NEW_NUMBERS_RIGHT = 66.f;
        constexpr float TEXT_X = 76.f; // after the 10px +/- sign column
        constexpr float DIFF_RULER_W = 30.f; // .diffViewport: the original / modified overview rulers, 15px each
        constexpr float DECORATIONS_RULER_W = 14.f;

        const ImVec4& INSERT_BG = theme::palette().added_line; // the current theme's, by reference
        const ImVec4& DELETE_BG = theme::palette().deleted_line;
        const ImU32 LINE_NUMBER = IM_COL32(133, 133, 133, 255);
        const ImU32 EDITOR_TEXT = IM_COL32(255, 255, 255, 255);

        // One line of the inline / split / file layouts: indices into the diff's lines, -1 for none.
        struct Row
        {
            const git::DiffLine* left = nullptr; // inline: the line itself
            const git::DiffLine* right = nullptr; // split: the new side
            bool hunk_header = false;
            std::size_t hunk = 0;
        };

        float css_text_y(float size, float box_top, bool mono)
        {
            const float ascent = size * (mono ? 1884.f : 2189.f) / 2048.f * dpi_scale();
            return std::round(box_top + std::round(ascent)) - std::ceil(ascent);
        }

        // Monaco text: Consolas 12px on the 16px line; Chromium puts its baseline 12px under the 14px glyph box
        // that starts 1px into the line (measured).
        void mono_text(ImDrawList* draw, float x, float line_top, ImU32 color, const char* text)
        {
            draw_text(draw, fonts().mono, 12.f, ImVec2(x, css_text_y(12.f, line_top + px(2.f), true)), color, text);
        }

        // Monaco's line numbers: rgb(133,133,133), rgb(198,198,198) on the editor's cursor line.
        void mono_number(ImDrawList* draw, float right, float line_top, int number, bool active = false)
        {
            if (number <= 0)
            {
                return;
            }
            const std::string text = std::to_string(number);
            mono_text(draw, right - text_size(fonts().mono, 12.f, text.c_str()).x, line_top,
                      active ? IM_COL32(198, 198, 198, 255) : LINE_NUMBER, text.c_str());
        }

        // A toggle button (12px / 16px, 2.5px 10px padding, 23px): text or a 12px icon.
        bool toggle_button(const char* id, const char* label, const char* icon, ImVec2 pos, bool checked,
                           const char* tip = nullptr)
        {
            const theme::Palette& p = theme::palette();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const float content = icon != nullptr ? fa_icon_width(icon, px(12.f))
                                                  : text_size(fonts().regular, theme::FONT_SIZE_ROW, label).x;
            const float w = content + px(22.f);
            const float h = px(23.f);
            ImGui::SetCursorScreenPos(pos);
            const bool clicked = ImGui::InvisibleButton(id, ImVec2(w, h));
            const bool hovered = ImGui::IsItemHovered();
            const ImVec4 accent = theme::with_alpha(p.accent, 0.2f);
            const ImVec2 max(pos.x + w, pos.y + h);
            if (checked)
            {
                draw->AddRectFilled(pos, max, theme::u32(accent));
            }
            else if (hovered)
            {
                draw->AddRectFilled(pos, max, theme::u32(p.hover_row));
            }
            draw->AddRect(pos, max, theme::u32(checked ? accent : theme::with_alpha(p.white, 0.2f)), 0.f, 0, 1.f);
            const ImU32 color = theme::u32(checked || hovered ? p.text_selected : p.text_normal);
            if (icon != nullptr)
            {
                draw_fa_icon(draw, icon, ImVec2(pos.x + w / 2.f, pos.y + px(11.f)), px(12.f), color);
            }
            else
            {
                draw_text(draw, fonts().regular, theme::FONT_SIZE_ROW,
                          ImVec2(pos.x + px(11.f), css_text_y(12.f, pos.y + px(2.5f), false)), color, label);
            }
            if (hovered && tip != nullptr)
            {
                styled_tooltip(pos, max, tip);
            }
            return clicked;
        }

        float toggle_width(const char* label, const char* icon)
        {
            return (icon != nullptr ? fa_icon_width(icon, px(12.f))
                                    : text_size(fonts().regular, theme::FONT_SIZE_ROW, label).x)
                   + px(22.f);
        }

        const char* status_icon(git::FileStatus status, ImVec4& color)
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
            default:
                color = p.orange;
                return "fas:pencil";
            }
        }

        // Rows for the current mode. Split pairs the deletions and additions of each change block and pads the
        // shorter side; the file view keeps the new side only.
        std::vector<Row> build_rows(const git::FileDiff& diff, app::RepoState::DiffMode mode, bool file_view)
        {
            std::vector<Row> rows;
            for (std::size_t h = 0; h < diff.hunks.size(); ++h)
            {
                const git::DiffHunk& hunk = diff.hunks[h];
                if (mode == app::RepoState::DiffMode::HUNK && !file_view)
                {
                    rows.push_back({nullptr, nullptr, true, h});
                }
                if (file_view)
                {
                    for (const git::DiffLine& line : hunk.lines)
                    {
                        if (line.origin != '-')
                        {
                            rows.push_back({&line, nullptr, false, h});
                        }
                    }
                    continue;
                }
                if (mode != app::RepoState::DiffMode::SPLIT)
                {
                    for (const git::DiffLine& line : hunk.lines)
                    {
                        rows.push_back({&line, nullptr, false, h});
                    }
                    continue;
                }
                std::size_t i = 0;
                while (i < hunk.lines.size())
                {
                    if (hunk.lines[i].origin == ' ')
                    {
                        rows.push_back({&hunk.lines[i], &hunk.lines[i], false, h});
                        ++i;
                        continue;
                    }
                    std::vector<const git::DiffLine*> deleted, added;
                    while (i < hunk.lines.size() && hunk.lines[i].origin != ' ')
                    {
                        (hunk.lines[i].origin == '-' ? deleted : added).push_back(&hunk.lines[i]);
                        ++i;
                    }
                    for (std::size_t k = 0; k < std::max(deleted.size(), added.size()); ++k)
                    {
                        rows.push_back({k < deleted.size() ? deleted[k] : nullptr,
                                        k < added.size() ? added[k] : nullptr, false, h});
                    }
                }
            }
            if (mode != app::RepoState::DiffMode::HUNK || file_view)
            {
                // Monaco shows the empty line after the file's final newline.
                static thread_local git::DiffLine last;
                last = {};
                last.origin = ' ';
                for (const git::DiffHunk& hunk : diff.hunks)
                {
                    for (const git::DiffLine& line : hunk.lines)
                    {
                        if (line.old_lineno > 0)
                        {
                            last.old_lineno = line.old_lineno + 1;
                        }
                        if (line.new_lineno > 0)
                        {
                            last.new_lineno = line.new_lineno + 1;
                        }
                    }
                }
                if (last.old_lineno > 0 || last.new_lineno > 0)
                {
                    rows.push_back(
                            {&last, mode == app::RepoState::DiffMode::SPLIT && !file_view ? &last : nullptr, false, 0});
                }
            }
            return rows;
        }

        // Monaco's diagonal-fill: rgba(204,204,204,.2) stripes at -45 degrees, 8px apart (a 12.5% band).
        void diagonal_fill(ImDrawList* draw, ImVec2 min, ImVec2 max)
        {
            draw->PushClipRect(min, max, true);
            const ImU32 color = IM_COL32(204, 204, 204, 51);
            const float h = max.y - min.y;
            for (float x = min.x - h; x < max.x; x += px(8.f))
            {
                draw->AddLine(ImVec2(x, max.y), ImVec2(x + h, min.y), color, px(1.4f));
            }
            draw->PopClipRect();
        }
    } // namespace

    void draw_diff_view(app::Application& app, app::RepoState& state, ImVec2 size)
    {
        const theme::Palette& p = theme::palette();
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        ImGui::BeginChild("##diff_view", size, ImGuiChildFlags_None, ImGuiWindowFlags_NoScrollbar);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const app::DiffTarget target = *state.diff_target;
        const bool commit_diff = target.source == app::DiffTarget::Source::COMMIT;
        draw->AddRectFilled(origin, ImVec2(origin.x + size.x, origin.y + size.y), theme::u32(p.app_bg0));

        // .file-view-header: status icon, the 14px white name; the encoding button and the close cross.
        {
            const float h = px(HEADER_H);
            draw->AddRectFilled(origin, ImVec2(origin.x + size.x, origin.y + h), theme::u32(p.panel_bg0));
            draw->AddRectFilled(ImVec2(origin.x, origin.y + h - 1.f), ImVec2(origin.x + size.x, origin.y + h),
                                theme::u32(p.section_border));
            ImVec4 tone;
            const char* icon = status_icon(target.status, tone);
            draw_fa_icon(draw, icon, ImVec2(origin.x + px(16.25f), origin.y + px(17.5f)), px(10.f), theme::u32(tone));
            const std::string name = file_name(target.path);
            draw_text_ellipsis(draw, fonts().regular, 14.f,
                               ImVec2(origin.x + px(27.5f), css_text_y(14.f, origin.y + px(7.5f), false)),
                               size.x * 0.6f, theme::u32(p.text_selected), name.c_str());
            if (ImGui::IsMouseHoveringRect(
                        ImVec2(origin.x + px(27.5f), origin.y),
                        ImVec2(origin.x + px(27.5f) + text_size(fonts().regular, 14.f, name.c_str()).x, origin.y + h)))
            {
                styled_tooltip(ImVec2(origin.x + px(27.5f), origin.y + px(7.f)),
                           ImVec2(origin.x + px(27.5f) + text_size(fonts().regular, 14.f, name.c_str()).x,
                                  origin.y + px(28.f)),
                           target.path.c_str());
            }
            // The close cross (zmdi-close, 20px) 10px from the right.
            const ImVec2 cmin(origin.x + size.x - px(30.f), origin.y + px(7.5f));
            ImGui::SetCursorScreenPos(cmin);
            if (ImGui::InvisibleButton("##close_diff", ImVec2(px(20.f), px(20.f)))
                || ImGui::IsKeyPressed(ImGuiKey_Escape))
            {
                state.close_diff();
                ImGui::EndChild();
                return;
            }
            const ImU32 cross = theme::u32(ImGui::IsItemHovered() ? p.text_selected : p.text_normal);
            const float a = px(4.6f);
            const float b = px(15.4f);
            draw->AddLine(ImVec2(cmin.x + a, cmin.y + a), ImVec2(cmin.x + b, cmin.y + b), cross, px(1.6f));
            draw->AddLine(ImVec2(cmin.x + b, cmin.y + a), ImVec2(cmin.x + a, cmin.y + b), cross, px(1.6f));
            float encoding_right = cmin.x - px(10.f);
            if (!commit_diff)
            {
                // .btn-xs (26px here): Stage File / Unstage File, 12px regular.
                const bool staged = target.source == app::DiffTarget::Source::STAGED;
                const char* label = staged ? "Unstage File" : "Stage File";
                const ImVec4 tone = staged ? p.red : p.green;
                const float w = text_size(fonts().regular, theme::FONT_SIZE_ROW, label).x + px(12.f);
                const ImVec2 bmin(encoding_right - w, origin.y + px(4.5f));
                const ImVec2 bmax(bmin.x + w, bmin.y + px(26.f));
                ImGui::SetCursorScreenPos(bmin);
                const bool clicked = ImGui::InvisibleButton("##stage_file", ImVec2(w, px(26.f)));
                const bool hovered = ImGui::IsItemHovered();
                draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(tone, hovered ? 0.3f : 0.2f)), px(2.f));
                draw->AddRect(bmin, bmax, theme::u32(tone), px(2.f), 0, 1.f);
                draw_text(draw, fonts().regular, theme::FONT_SIZE_ROW,
                          ImVec2(bmin.x + px(6.f), css_text_y(12.f, origin.y + px(8.5f), false)),
                          theme::u32(p.text_normal), label);
                if (clicked)
                {
                    const std::string path = target.path;
                    if (staged)
                    {
                        app.run(state, "Unstage",
                                [&]
                                {
                                    return state.repo.unstage({path});
                                });
                        state.open_diff({app::DiffTarget::Source::UNSTAGED, {}, path, target.status});
                    }
                    else
                    {
                        app.run(state, "Stage",
                                [&]
                                {
                                    return state.repo.stage({path});
                                });
                        state.open_diff({app::DiffTarget::Source::STAGED, {}, path, target.status});
                    }
                    ImGui::EndChild();
                    return;
                }
                encoding_right = bmin.x - px(20.f);
            }
            // UTF-8: a 10px text-disabled dropdown button, 5px padding.
            const char* encoding = "UTF-8";
            const float ew = text_size(fonts().regular, 10.f, encoding).x + px(10.f);
            const ImVec2 emin(encoding_right - ew, origin.y + px(4.5f));
            ImGui::SetCursorScreenPos(emin);
            if (ImGui::InvisibleButton("##encoding", ImVec2(ew, px(26.f))))
            {
                app.toasts.push(app::ToastKind::INFO, "Encoding", "Files are shown as UTF-8.");
            }
            draw->AddRectFilled(emin, ImVec2(emin.x + ew, emin.y + px(26.f)), theme::u32(p.panel_bg0), px(2.f));
            draw_text(draw, fonts().regular, 10.f,
                      ImVec2(emin.x + px(5.f), css_text_y(10.f, origin.y + px(10.5f), false)),
                      theme::u32(ImGui::IsItemHovered() ? p.text_normal : p.text_disabled), encoding);
        }

        // Toolbar row
        const float bar_y = origin.y + px(HEADER_H + 10.f);
        bool scroll_prev = false, scroll_next = false;
        {
            {
                // .btn-xs.btn-primary with far:pencil: "Edit in Working Directory" for commits, "Edit This File"
                // for the working directory.
                const char* label = commit_diff ? "Edit in Working Directory" : "Edit This File";
                const float w = text_size(fonts().regular, theme::FONT_SIZE_ROW, label).x + px(29.f);
                const ImVec2 bmin(origin.x + px(10.f), bar_y + px(1.5f));
                const ImVec2 bmax(bmin.x + w, bmin.y + px(20.f));
                ImGui::SetCursorScreenPos(bmin);
                if (ImGui::InvisibleButton("##edit_file", ImVec2(w, px(20.f))))
                {
                    app.toasts.push(app::ToastKind::INFO, label,
                                    "The built-in editor is not part of ImGit yet.");
                }
                const bool hovered = ImGui::IsItemHovered();
                draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.accent, hovered ? 0.3f : 0.2f)), px(2.f));
                draw->AddRect(bmin, bmax, theme::u32(p.accent), px(2.f), 0, 1.f);
                draw_fa_icon(draw, "far:pencil", ImVec2(bmin.x + px(12.f), bmin.y + px(10.f)), px(12.f),
                             theme::u32(p.text_normal));
                draw_text(draw, fonts().regular, theme::FONT_SIZE_ROW,
                          ImVec2(bmin.x + px(23.02f), css_text_y(12.f, bar_y + px(2.5f), false)),
                          theme::u32(p.text_normal), label);
            }
            // Centred: [Unstaged / Staged] (working directory) then File View | Diff View.
            const char* side = target.source == app::DiffTarget::Source::STAGED ? "Staged" : "Unstaged";
            const float sw = commit_diff ? 0.f : toggle_width(side, nullptr) + px(10.f);
            const float fw = toggle_width("File View", nullptr);
            const float dw = toggle_width("Diff View", nullptr);
            const float group = sw + fw + dw + (commit_diff ? 0.f : px(10.f));
            float x = std::round(origin.x + (size.x - group) / 2.f);
            if (!commit_diff)
            {
                toggle_button("##side", side, nullptr, ImVec2(x, bar_y), true);
                x += sw;
            }
            if (toggle_button("##file_view", "File View", nullptr, ImVec2(x, bar_y), state.file_view))
            {
                state.file_view = true;
                state.reload_diff();
            }
            if (toggle_button("##diff_mode", "Diff View", nullptr, ImVec2(x + fw, bar_y), !state.file_view))
            {
                state.file_view = false;
                state.reload_diff();
            }
            // Right: Blame, History | previous, next change | hunk, inline, split | whitespace, wrap.
            struct Button
            {
                const char* id;
                const char* label;
                const char* icon;
                const char* tip;
                float gap; // before the button
            };
            const Button buttons[] = {
                    {"##blame", "Blame", nullptr, nullptr, 0.f},
                    {"##history", "History", nullptr, nullptr, 0.f},
                    {"##prev", nullptr, "fas:arrow-up", "Previous change", 10.f},
                    {"##next", nullptr, "fas:arrow-down", "Next change", 0.f},
                    {"##hunk", nullptr, "far:list-alt", "Hunk View", 10.f},
                    {"##inline", nullptr, "far:list", "Inline View", 0.f},
                    {"##split", nullptr, "far:columns", "Split View", 0.f},
                    {"##whitespace", nullptr, "far:paragraph", "Show whitespace", 10.f},
                    {"##wrap", nullptr, "fas:align-left", "Word wrap", 0.f},
            };
            float total = 0.f;
            for (const Button& button : buttons)
            {
                total += px(button.gap) + toggle_width(button.label, button.icon);
            }
            x = origin.x + size.x - px(10.f) - total;
            for (const Button& button : buttons)
            {
                x += px(button.gap);
                const std::string id = button.id;
                bool checked = false;
                if (id == "##hunk")
                {
                    checked = state.diff_mode == app::RepoState::DiffMode::HUNK;
                }
                else if (id == "##inline")
                {
                    checked = state.diff_mode == app::RepoState::DiffMode::INLINE;
                }
                else if (id == "##split")
                {
                    checked = state.diff_mode == app::RepoState::DiffMode::SPLIT;
                }
                else if (id == "##whitespace")
                {
                    checked = state.diff_whitespace;
                }
                else if (id == "##wrap")
                {
                    checked = state.diff_wrap;
                }
                if (toggle_button(button.id, button.label, button.icon, ImVec2(x, bar_y), checked, button.tip))
                {
                    if (id == "##hunk" || id == "##inline" || id == "##split")
                    {
                        state.diff_mode = id == "##hunk" ? app::RepoState::DiffMode::HUNK
                                                         : (id == "##inline" ? app::RepoState::DiffMode::INLINE
                                                                             : app::RepoState::DiffMode::SPLIT);
                        state.file_view = false;
                        state.reload_diff();
                    }
                    else if (id == "##whitespace")
                    {
                        state.diff_whitespace = !state.diff_whitespace;
                    }
                    else if (id == "##wrap")
                    {
                        state.diff_wrap = !state.diff_wrap;
                    }
                    else if (id == "##prev")
                    {
                        scroll_prev = true;
                    }
                    else if (id == "##next")
                    {
                        scroll_next = true;
                    }
                    else
                    {
                        app.toasts.push(app::ToastKind::INFO, button.label,
                                        std::string(button.label) + " for a file is not available in ImGit yet.");
                    }
                }
                x += toggle_width(button.label, button.icon);
            }
        }

        // The editor
        const ImVec2 editor_min(origin.x, origin.y + px(HEADER_H + TOOLBAR_H));
        const ImVec2 editor_max(origin.x + size.x - 1.f, origin.y + size.y);
        const bool split = state.diff_mode == app::RepoState::DiffMode::SPLIT && !state.file_view;
        if (!state.diff_error.empty() || !state.diff || state.diff->binary || state.diff->hunks.empty())
        {
            const char* message =
                    !state.diff_error.empty()
                            ? state.diff_error.c_str()
                            : (!state.diff ? "Loading..." : (state.diff->binary ? "Binary file" : "No Changes"));
            draw_text(draw, fonts().regular, 14.f, ImVec2(editor_min.x, css_text_y(14.f, editor_min.y, false)),
                      theme::u32(state.diff_error.empty() ? p.text_normal : p.red), message);
            ImGui::EndChild();
            return;
        }
        const git::FileDiff& diff = *state.diff;
        const std::vector<Row> rows = build_rows(diff, state.diff_mode, state.file_view);
        const float line_h = px(LINE_H);
        const float rulers_w = px(DIFF_RULER_W);
        const float content_right = editor_max.x - rulers_w;
        ImGui::SetCursorScreenPos(editor_min);
        ImGui::BeginChild("##diff_lines", ImVec2(content_right - editor_min.x, editor_max.y - editor_min.y),
                          ImGuiChildFlags_None, ImGuiWindowFlags_HorizontalScrollbar);
        ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing, ImVec2(ImGui::GetStyle().ItemSpacing.x, 0.f));
        ImDrawList* body = ImGui::GetWindowDrawList();
        const float view_h = ImGui::GetWindowHeight();
        const float scroll_y = ImGui::GetScrollY();
        // Change navigation: the first changed row above / below the top of the view.
        if (scroll_prev || scroll_next)
        {
            const int top_row = static_cast<int>(scroll_y / line_h);
            int target_row = -1;
            for (int i = 0; i < static_cast<int>(rows.size()); ++i)
            {
                const Row& row = rows[static_cast<std::size_t>(i)];
                const bool changed = (row.left != nullptr && row.left->origin != ' ')
                                     || (row.right != nullptr && row.right->origin != ' ');
                const bool starts = changed
                                    && (i == 0
                                        || !((rows[static_cast<std::size_t>(i - 1)].left != nullptr
                                              && rows[static_cast<std::size_t>(i - 1)].left->origin != ' ')
                                             || (rows[static_cast<std::size_t>(i - 1)].right != nullptr
                                                 && rows[static_cast<std::size_t>(i - 1)].right->origin != ' ')));
                if (!starts)
                {
                    continue;
                }
                if (scroll_next && i > top_row)
                {
                    target_row = i;
                    break;
                }
                if (scroll_prev && i < top_row)
                {
                    target_row = i;
                }
            }
            if (target_row >= 0)
            {
                ImGui::SetScrollY(static_cast<float>(target_row) * line_h);
            }
        }
        int new_cursor = 1;
        for (const Row& row : rows)
        {
            if (row.left != nullptr && row.left->origin == '+')
            {
                new_cursor = row.left->new_lineno;
                break;
            }
            if (row.right != nullptr && row.right->origin == '+')
            {
                new_cursor = row.right->new_lineno;
                break;
            }
        }
        const float half = (content_right - editor_min.x) / 2.f;
        const float base_x = ImGui::GetCursorScreenPos().x;
        float max_text = 0.f;
        ImGuiListClipper clipper;
        clipper.Begin(static_cast<int>(rows.size()), line_h);
        while (clipper.Step())
        {
            for (int i = clipper.DisplayStart; i < clipper.DisplayEnd; ++i)
            {
                const Row& row = rows[static_cast<std::size_t>(i)];
                const ImVec2 pos = ImGui::GetCursorScreenPos();
                const float right = std::max(content_right, base_x + max_text + px(TEXT_X + 40.f));
                if (row.hunk_header)
                {
                    body->AddRectFilled(ImVec2(base_x, pos.y), ImVec2(right, pos.y + line_h),
                                        theme::u32(p.hunk_header));
                    mono_text(body, base_x + px(TEXT_X), pos.y, theme::u32(p.text_secondary),
                              diff.hunks[row.hunk].header.c_str());
                }
                else if (state.file_view)
                {
                    mono_number(body, base_x + px(OLD_NUMBERS_RIGHT), pos.y, row.left->new_lineno);
                    mono_text(body, base_x + px(43.f), pos.y, EDITOR_TEXT, row.left->text.c_str());
                    max_text = std::max(max_text, text_size(fonts().mono, 12.f, row.left->text.c_str()).x);
                }
                else if (split)
                {
                    // Each side: 33px numbers, the sign column, the text; the missing side is hatched.
                    for (int side = 0; side < 2; ++side)
                    {
                        const git::DiffLine* line = side == 0 ? row.left : row.right;
                        const float x0 = base_x + (side == 0 ? 0.f : half);
                        const float x1 = side == 0 ? base_x + half : right;
                        if (line == nullptr)
                        {
                            diagonal_fill(body, ImVec2(x0 + px(43.f), pos.y), ImVec2(x1, pos.y + line_h));
                            continue;
                        }
                        if (line->origin != ' ')
                        {
                            body->AddRectFilled(ImVec2(x0 + px(43.f), pos.y), ImVec2(x1, pos.y + line_h),
                                                theme::u32(line->origin == '+' ? INSERT_BG : DELETE_BG));
                            mono_text(body, x0 + px(34.f), pos.y, theme::u32(p.text_normal),
                                      line->origin == '+' ? "+" : "-");
                        }
                        mono_number(body, x0 + px(OLD_NUMBERS_RIGHT), pos.y,
                                    side == 0 ? line->old_lineno : line->new_lineno);
                        body->PushClipRect(ImVec2(x0 + px(43.f), pos.y), ImVec2(x1, pos.y + line_h), true);
                        mono_text(body, x0 + px(43.f), pos.y, EDITOR_TEXT, line->text.c_str());
                        body->PopClipRect();
                    }
                }
                else
                {
                    const git::DiffLine& line = *row.left;
                    if (line.origin != ' ')
                    {
                        body->AddRectFilled(ImVec2(base_x + px(TEXT_X), pos.y), ImVec2(right, pos.y + line_h),
                                            theme::u32(line.origin == '+' ? INSERT_BG : DELETE_BG));
                        // codicon diff-insert / diff-remove in the 10px sign column.
                        const float cx = std::floor(base_x + px(NEW_NUMBERS_RIGHT + 5.f));
                        const float cy = std::floor(pos.y + line_h / 2.f);
                        const ImU32 sign = theme::u32(p.text_normal);
                        body->AddRectFilled(ImVec2(cx - px(5.f), cy), ImVec2(cx + px(5.f), cy + 1.f), sign);
                        if (line.origin == '+')
                        {
                            body->AddRectFilled(ImVec2(cx, cy - px(5.f)), ImVec2(cx + 1.f, cy + px(5.f) + 1.f), sign);
                        }
                    }
                    mono_number(body, base_x + px(OLD_NUMBERS_RIGHT), pos.y, line.old_lineno, line.old_lineno == 1);
                    mono_number(body, base_x + px(NEW_NUMBERS_RIGHT), pos.y, line.new_lineno,
                                line.new_lineno == new_cursor);
                    mono_text(body, base_x + px(TEXT_X), pos.y, EDITOR_TEXT, line.text.c_str());
                    max_text = std::max(max_text, text_size(fonts().mono, 12.f, line.text.c_str()).x);
                }
                ImGui::Dummy(ImVec2(max_text + px(TEXT_X + 40.f), line_h));
            }
        }
        clipper.End();
        ImGui::PopStyleVar();
        const float content_h = static_cast<float>(rows.size()) * line_h;
        ImGui::EndChild();

        // The modified editor's decorations ruler (14px, a 1px left border) and the diff overview: the visible
        // range in rgba(121,121,121,.4), deletions in the original (left) half, insertions in the modified half.
        {
            const float ruler_x = content_right - px(DECORATIONS_RULER_W);
            draw->AddRectFilled(ImVec2(ruler_x, editor_min.y), ImVec2(ruler_x + 1.f, editor_max.y),
                                IM_COL32(127, 127, 127, 77));
            const float area_h = editor_max.y - editor_min.y;
            const float scale = content_h > area_h ? area_h / content_h : 1.f;
            const float viewport_top = editor_min.y + scroll_y * scale;
            const float viewport_h = std::min(area_h, view_h * scale);
            draw->AddRectFilled(ImVec2(content_right, viewport_top), ImVec2(editor_max.x, viewport_top + viewport_h),
                                IM_COL32(121, 121, 121, 102));
            for (std::size_t i = 0; i < rows.size(); ++i)
            {
                const Row& row = rows[i];
                const float y0 = editor_min.y + static_cast<float>(i) * line_h * scale;
                const float y1 = y0 + std::max(1.f, line_h * scale);
                const auto mark = [&](const git::DiffLine* line)
                {
                    if (line == nullptr || line->origin == ' ')
                    {
                        return;
                    }
                    const bool added = line->origin == '+';
                    const float x0 = content_right + (added ? rulers_w / 2.f : 0.f);
                    draw->AddRectFilled(
                            ImVec2(x0, y0), ImVec2(x0 + rulers_w / 2.f, y1),
                            theme::u32(added ? theme::with_alpha(p.green, 0.5f) : theme::with_alpha(p.red, 0.5f)));
                };
                mark(row.left);
                mark(row.right);
            }
        }
        ImGui::EndChild();
    }
} // namespace imgit::ui
