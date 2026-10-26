#include "app/application.hpp"
#include "app/theme.hpp"
#include "platform/shell.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <array>
#include <cmath>
#include <filesystem>
#include <imgui.h>
#include <imgui_internal.h>
#include <imgui_stdlib.h>
#include <optional>
#include <string>
#include <vector>

namespace imgit::ui
{
    namespace
    {

        const char* dialog_title(app::Dialog::Kind kind)
        {
            switch (kind)
            {
            case app::Dialog::Kind::CLONE_REPO:
                return "Clone a Repository";
            case app::Dialog::Kind::INIT_REPO:
                return "Init a Repository";
            case app::Dialog::Kind::CREATE_BRANCH:
                return "Create Branch";
            case app::Dialog::Kind::CREATE_TAG:
                return "Create Tag";
            case app::Dialog::Kind::DELETE_BRANCH:
                return "Delete Branch";
            case app::Dialog::Kind::DELETE_TAG:
                return "Delete Tag";
            case app::Dialog::Kind::DELETE_STASH:
                return "Delete Stash";
            case app::Dialog::Kind::DISCARD_ALL:
                return "Discard All Changes";
            case app::Dialog::Kind::RESET:
                return "Reset Branch";
            case app::Dialog::Kind::ADD_REMOTE:
                return "Add Remote";
            case app::Dialog::Kind::EDIT_REMOTE:
                return "Edit Remote";
            case app::Dialog::Kind::REWORD_COMMIT:
                return "Edit Commit Message";
            case app::Dialog::Kind::SQUASH_COMMITS:
                return "Squash Commits";
            case app::Dialog::Kind::SET_UPSTREAM:
                return "Set Upstream";
            case app::Dialog::Kind::PREFERENCES:
                return "Preferences";
            case app::Dialog::Kind::KEYBOARD_SHORTCUTS:
                return "Keyboard Shortcuts";
            case app::Dialog::Kind::ABOUT:
                return "About ImGit Desktop";
            default:
                return "";
            }
        }

        float dialog_width(app::Dialog::Kind kind)
        {
            switch (kind)
            {
            case app::Dialog::Kind::PREFERENCES:
                return 720.f;
            case app::Dialog::Kind::KEYBOARD_SHORTCUTS:
                return 560.f;
            case app::Dialog::Kind::SQUASH_COMMITS:
            case app::Dialog::Kind::REWORD_COMMIT:
                return 540.f;
            default:
                return 480.f;
            }
        }

        // Path field with a native folder picker.
        void path_field(const char* id, std::string& value, const char* hint)
        {
            const float browse_w = px(80.f);
            input_text(id, value, hint, 0,
                       ImGui::GetContentRegionAvail().x - browse_w - ImGui::GetStyle().ItemSpacing.x);
            ImGui::SameLine();
            if (default_button("Browse...", ImVec2(browse_w, 0.f)))
            {
                if (auto picked = platform::shell::pick_folder_dialog())
                {
                    value = *picked;
                }
            }
        }

        // Cancel / confirm row; returns true when confirmed (button or Enter).
        bool buttons(const char* confirm_label, bool enabled, bool danger, bool enter_submits, bool show_cancel = true)
        {
            ImGui::Dummy(ImVec2(0.f, px(8.f)));
            const float width = px(std::max(110.f, static_cast<float>(std::string(confirm_label).size()) * 7.f + 30.f));
            ImGui::SetCursorPosX(ImGui::GetContentRegionMax().x - width
                                 - (show_cancel ? px(110.f) + ImGui::GetStyle().ItemSpacing.x : 0.f));
            bool cancel = false;
            if (show_cancel)
            {
                cancel = default_button("Cancel", ImVec2(px(110.f), px(32.f)));
                ImGui::SameLine();
            }
            bool confirm = danger ? danger_button(confirm_label, ImVec2(width, px(32.f)), enabled)
                                  : primary_button(confirm_label, ImVec2(width, px(32.f)), enabled);
            if (enter_submits && enabled && ImGui::IsKeyPressed(ImGuiKey_Enter) && !ImGui::IsAnyItemActive())
            {
                confirm = true;
            }
            if (cancel || ImGui::IsKeyPressed(ImGuiKey_Escape))
            {
                ImGui::CloseCurrentPopup();
            }
            return confirm;
        }

        // A thin rounded bar filled to `fraction` (0..1) across the content width.
        void progress_bar(float fraction)
        {
            const theme::Palette& p = theme::palette();
            const ImVec2 min = ImGui::GetCursorScreenPos();
            const float width = ImGui::GetContentRegionAvail().x;
            const float height = px(6.f);
            const float fill = smooth_value("##clone_progress_fill", std::clamp(fraction, 0.f, 1.f), 0.25f);
            ImDrawList* draw = ImGui::GetWindowDrawList();
            draw->AddRectFilled(min, ImVec2(min.x + width, min.y + height), theme::u32(p.input_bg), height / 2.f);
            if (fill > 0.f)
            {
                draw->AddRectFilled(min, ImVec2(min.x + std::max(height, width * fill), min.y + height),
                                    theme::u32(p.accent), height / 2.f);
            }
            ImGui::Dummy(ImVec2(width, height));
        }

        void error_text(const std::string& error)
        {
            if (!error.empty())
            {
                ImGui::PushStyleColor(ImGuiCol_Text, theme::palette().red);
                ImGui::TextWrapped("%s", error.c_str());
                ImGui::PopStyleColor();
            }
        }

        void shortcut_row(const char* keys, const char* what)
        {
            const theme::Palette& p = theme::palette();
            ImGui::TableNextRow();
            ImGui::TableNextColumn();
            ImGui::PushStyleColor(ImGuiCol_Text, p.text_selected);
            ImGui::TextUnformatted(shortcut_label(keys).c_str());
            ImGui::PopStyleColor();
            ImGui::TableNextColumn();
            ImGui::TextUnformatted(what);
        }

        void preferences_body(app::Application& app)
        {
            const theme::Palette& p = theme::palette();
            app::Settings& s = app.settings;
            const char* pages[] = {"General", "UI Customization", "Commit"};
            ImGui::BeginChild("##pref_nav", ImVec2(px(170.f), px(360.f)));
            for (int i = 0; i < 3; ++i)
            {
                const bool active = app.dialog.page == i;
                ImGui::PushStyleColor(ImGuiCol_Header, p.selected_row);
                if (ImGui::Selectable(pages[i], active, 0, ImVec2(0.f, px(26.f))))
                {
                    app.dialog.page = i;
                }
                ImGui::PopStyleColor();
            }
            ImGui::EndChild();
            ImGui::SameLine();
            ImGui::BeginChild("##pref_page", ImVec2(0.f, px(360.f)));
            push_font(fonts().semibold, theme::FONT_SIZE_LARGE);
            ImGui::TextUnformatted(pages[app.dialog.page]);
            ImGui::PopFont();
            ImGui::Dummy(ImVec2(0.f, px(6.f)));
            bool changed = false;
            switch (app.dialog.page)
            {
            case 0:
                secondary_text("Auto-Fetch Interval");
                ImGui::SetNextItemWidth(px(120.f));
                changed |= ImGui::InputInt("##autofetch", &s.auto_fetch_minutes);
                s.auto_fetch_minutes = std::clamp(s.auto_fetch_minutes, 0, 60);
                secondary_text("In minutes, between 0 and 60 (0 disables auto-fetch). Fetches all visible remotes.");
                ImGui::Dummy(ImVec2(0.f, px(8.f)));
                secondary_text("Initial Commits in Graph");
                ImGui::SetNextItemWidth(px(120.f));
                changed |= ImGui::InputInt("##commits", &s.initial_commits, 500, 1000);
                s.initial_commits = std::max(500, s.initial_commits);
                secondary_text("Minimum 500. Increasing this value may adversely affect performance.");
                ImGui::Dummy(ImVec2(0.f, px(8.f)));
                secondary_text("Default Branch Name");
                changed |= input_text("##default_branch", s.default_branch, "main", 0, px(200.f));
                ImGui::Dummy(ImVec2(0.f, px(8.f)));
                changed |= checkbox("Remember tabs", &s.remember_tabs);
                break;
            case 1:
                changed |= checkbox("Show toolbar icon labels", &s.show_toolbar_labels);
                changed |= checkbox("Display author initials and generic remote icons instead of avatars",
                                    &s.author_initials);
                changed |= checkbox("Show commit description in graph", &s.show_description);
                changed |= checkbox("Show commit author in graph", &s.show_author);
                changed |= checkbox("Show commit date/time in graph", &s.show_date);
                changed |= checkbox("Show commit sha in graph", &s.show_sha);
                changed |= checkbox("Show commit changes in graph", &s.show_changes);
                changed |=
                        checkbox("Show ghost branch/tag when hovering over or selecting a commit", &s.show_ghost_refs);
                changed |= checkbox("Highlight associated rows when hovering over a branch",
                                    &s.highlight_rows_on_ref_hover);
                ImGui::Dummy(ImVec2(0.f, px(8.f)));
                secondary_text("Theme");
                {
                    const auto themes = theme::theme_names();
                    int theme_index = 0;
                    for (std::size_t i = 0; i < themes.size(); ++i)
                    {
                        if (s.theme == themes[i])
                        {
                            theme_index = static_cast<int>(i);
                        }
                    }
                    ImGui::SetNextItemWidth(px(200.f));
                    if (ImGui::Combo("##theme", &theme_index, themes.data(), static_cast<int>(themes.size())))
                    {
                        s.theme = themes[static_cast<std::size_t>(theme_index)];
                        app.apply_settings();
                        changed = true;
                    }
                }
                ImGui::Dummy(ImVec2(0.f, px(8.f)));
                secondary_text("Zoom");
                ImGui::SetNextItemWidth(px(200.f));
                {
                    float zoom = s.zoom;
                    if (ImGui::SliderFloat("##zoom", &zoom, 0.5f, 2.f, "%.0f%%", ImGuiSliderFlags_None)
                        && !ImGui::IsItemActive())
                    {
                        app.set_zoom(zoom);
                    }
                    if (ImGui::IsItemDeactivatedAfterEdit())
                    {
                        app.set_zoom(zoom);
                    }
                }
                break;
            case 2:
                changed |= checkbox("Push after each commit", &s.push_after_commit);
                if (changed)
                {
                    for (auto& tab : app.tabs)
                    {
                        tab->push_after_commit = s.push_after_commit;
                    }
                }
                ImGui::Dummy(ImVec2(0.f, px(8.f)));
                secondary_text("Commit summaries no longer than 72 characters are fully displayed in most websites and "
                               "CLI tools.");
                break;
            default:
                break;
            }
            if (changed)
            {
                s.save(app.config_path("settings.txt"));
            }
            ImGui::EndChild();
        }
    } // namespace

    void create_branch_at(app::Application& app, app::RepoState& state, const std::string& name, const git::Sha& sha,
                          bool checkout)
    {
        git::Repository* repo = &state.repo;
        const std::string previous_branch = state.history.detached ? "" : state.history.head_branch;
        const git::Sha previous_sha = state.history.head_sha;
        if (app.run(
                    state, "Create branch",
                    [&]
                    {
                        return state.repo.create_branch(name, sha, checkout);
                    },
                    "Created branch " + name))
        {
            state.push_undo({"create branch " + name,
                             [repo, name, checkout, previous_branch, previous_sha]() -> git::Result<void>
                             {
                                 if (checkout)
                                 {
                                     auto back = previous_branch.empty() ? repo->checkout_commit(previous_sha)
                                                                         : repo->checkout_branch(previous_branch);
                                     if (!back)
                                     {
                                         return back;
                                     }
                                 }
                                 return repo->delete_branch(name);
                             },
                             [repo, name, sha, checkout]
                             {
                                 return repo->create_branch(name, sha, checkout);
                             }});
        }
    }

    void create_tag_at(app::Application& app, app::RepoState& state, const std::string& name, const git::Sha& sha,
                       const std::string& message)
    {
        git::Repository* repo = &state.repo;
        if (app.run(
                    state, "Create tag",
                    [&]
                    {
                        return state.repo.create_tag(name, sha, message);
                    },
                    "Created tag " + name))
        {
            state.push_undo({"create tag " + name,
                             [repo, name]
                             {
                                 return repo->delete_tag(name);
                             },
                             [repo, name, sha, message]
                             {
                                 return repo->create_tag(name, sha, message);
                             }});
        }
    }

    namespace
    {
        // The confirm prompt (.user-input over the toolbar): the toolbar's 48px strip tinted
        // rgba(77,136,255,.2), the 14px message 90% white and, 10px on, the xs buttons (12px bold, 12px apart,
        // 8px from the end), all centred. Returns 1 for confirm, -1 for cancel, 0 while open.
        int draw_prompt_bar(const std::string& message, const std::string& confirm_label, bool danger)
        {
            const theme::Palette& p = theme::palette();
            const ImGuiViewport* viewport = ImGui::GetMainViewport();
            const ImVec2 min(viewport->WorkPos.x,
                             viewport->WorkPos.y + px(theme::TABS_BAR_HEIGHT)); // under the menu bar, if any
            const ImVec2 size(viewport->Size.x, px(theme::TOOLBAR_HEIGHT));
            ImGui::SetNextWindowPos(min);
            ImGui::SetNextWindowSize(size);
            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
            ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding, 0.f);
            ImGui::Begin("##confirm_prompt", nullptr,
                         ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoSavedSettings
                                 | ImGuiWindowFlags_NoBackground | ImGuiWindowFlags_NoFocusOnAppearing);
            ImGui::PopStyleVar(3);
            ImGui::BringWindowToDisplayFront(ImGui::GetCurrentWindow());
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const ImVec2 max(min.x + size.x, min.y + size.y);
            draw->AddRectFilled(min, max, theme::u32(p.toolbar_bg1));
            draw->AddRectFilled(min, max, theme::u32(theme::with_alpha(p.accent, 0.2f)));

            const ImU32 text = IM_COL32(255, 255, 255, 230);
            const char* cancel = "Cancel";
            const float message_w = text_size(fonts().regular, 14.f, message.c_str()).x;
            const float confirm_w = text_size(fonts().bold, 12.f, confirm_label.c_str()).x + px(12.f);
            const float cancel_w = text_size(fonts().bold, 12.f, cancel).x + px(12.f);
            const float total = message_w + px(10.f) + px(12.f) + confirm_w + px(12.f) + cancel_w + px(8.f);
            float x = std::round(min.x + std::max(px(10.f), (size.x - total) / 2.f));
            const float ascent14 = 14.f * 2189.f / 2048.f * dpi_scale();
            draw_text(draw, fonts().regular, 14.f,
                      ImVec2(x, std::round(min.y + px(14.f) + std::round(ascent14)) - std::ceil(ascent14)), text,
                      message.c_str());
            x += message_w + px(10.f) + px(12.f);
            int result = 0;
            const auto button = [&](const char* id, const char* label, float w, bool primary)
            {
                const ImVec2 bmin(x, min.y + px(13.f));
                const ImVec2 bmax(x + w, bmin.y + px(22.f));
                ImGui::SetCursorScreenPos(bmin);
                const bool clicked = ImGui::InvisibleButton(id, ImVec2(w, bmax.y - bmin.y));
                const bool hovered = ImGui::IsItemHovered();
                if (primary)
                {
                    const ImVec4 tone = danger ? p.red : p.green;
                    draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(tone, hovered ? 0.3f : 0.2f)),
                                        px(2.f));
                    draw->AddRect(bmin, bmax, theme::u32(tone), px(2.f), 0, 1.f);
                }
                else
                {
                    if (hovered)
                    {
                        draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.white, 0.1f)), px(2.f));
                    }
                    draw->AddRect(bmin, bmax, IM_COL32(255, 255, 255, 255), px(2.f), 0, 1.f);
                }
                const float ascent12 = 12.f * 2189.f / 2048.f * dpi_scale();
                draw_text(draw, fonts().bold, 12.f,
                          ImVec2(bmin.x + px(6.f),
                                 std::round(bmin.y + px(2.f) + std::round(ascent12)) - std::ceil(ascent12)),
                          text, label);
                x += w + px(12.f);
                return clicked;
            };
            if (button("##confirm", confirm_label.c_str(), confirm_w, true))
            {
                result = 1;
            }
            // Cancel has the focus when the prompt opens, so Enter answers it too.
            const bool keyed = ImGui::IsKeyPressed(ImGuiKey_Escape)
                               || (!ImGui::GetIO().WantTextInput
                                   && (ImGui::IsKeyPressed(ImGuiKey_Enter) || ImGui::IsKeyPressed(ImGuiKey_KeypadEnter)
                                       || ImGui::IsKeyPressed(ImGuiKey_Space)));
            if (button("##cancel", cancel, cancel_w, false) || keyed)
            {
                result = -1;
            }
            ImGui::End();
            return result;
        }

        std::string in_quotes(const std::string& text)
        {
            return "\"" + text + "\"";
        }

        // The set-upstream prompt for a branch without one, in the prompt bar:
        // the question, the remote react-select (150x34, focused: #2684FF ring), an 18px "/", the branch name
        // field (191x34, the branch's own name as placeholder), Submit and Cancel. Returns 1 to push, -1 to cancel.
        int draw_push_prompt(app::Dialog& d, const std::vector<git::Remote>& remotes)
        {
            const theme::Palette& p = theme::palette();
            const ImGuiViewport* viewport = ImGui::GetMainViewport();
            const ImVec2 min(viewport->WorkPos.x,
                             viewport->WorkPos.y + px(theme::TABS_BAR_HEIGHT)); // under the menu bar, if any
            const ImVec2 size(viewport->Size.x, px(theme::TOOLBAR_HEIGHT));
            ImGui::SetNextWindowPos(min);
            ImGui::SetNextWindowSize(size);
            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
            ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding, 0.f);
            ImGui::Begin("##push_prompt", nullptr,
                         ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoSavedSettings
                                 | ImGuiWindowFlags_NoBackground);
            ImGui::PopStyleVar(3);
            ImGui::BringWindowToDisplayFront(ImGui::GetCurrentWindow());
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const ImVec2 max(min.x + size.x, min.y + size.y);
            draw->AddRectFilled(min, max, theme::u32(p.toolbar_bg1));
            draw->AddRectFilled(min, max, theme::u32(theme::with_alpha(p.accent, 0.2f)));
            const ImU32 text = IM_COL32(255, 255, 255, 230);
            const ImU32 field_bg = IM_COL32(0, 0, 0, 51);
            const ImU32 field_border = IM_COL32(255, 255, 255, 51);
            const auto text_at =
                    [&](ImFont* font, float font_size, float x, float box_top, ImU32 color, const char* label)
            {
                const float ascent = font_size * 2189.f / 2048.f * dpi_scale();
                draw_text(draw, font, font_size,
                          ImVec2(x, std::round(box_top + std::round(ascent)) - std::ceil(ascent)), color, label);
            };

            const std::string message =
                    "What remote/branch should " + in_quotes(d.ref_name) + " push to and pull from?";
            const float message_w = text_size(fonts().regular, 14.f, message.c_str()).x;
            const float slash_w = text_size(fonts().regular, 18.f, "/").x;
            const float submit_w = text_size(fonts().bold, 12.f, "Submit").x + px(12.f);
            const float cancel_w = text_size(fonts().bold, 12.f, "Cancel").x + px(12.f);
            const float actions_w = px(150.f) + px(7.5f) + slash_w + px(7.5f) + px(191.f) + px(12.f) + submit_w
                                    + px(12.f) + cancel_w + px(8.f);
            const float total = message_w + px(10.f) + actions_w;
            float x = std::round(min.x + std::max(px(10.f), (size.x - total) / 2.f));
            text_at(fonts().regular, 14.f, x, min.y + px(14.f), text, message.c_str());
            x += message_w + px(10.f);

            // The remote select: value, a 1px separator, the chevron; a menu of the remotes under it.
            static bool select_focused = true;
            const ImVec2 smin(std::round(x), min.y + px(7.f));
            const ImVec2 smax(smin.x + px(150.f), smin.y + px(34.f));
            ImGui::SetCursorScreenPos(smin);
            if (ImGui::InvisibleButton("##push_remote", ImVec2(smax.x - smin.x, smax.y - smin.y)))
            {
                d.flag = !d.flag;
                select_focused = true;
            }
            if (d.just_opened)
            {
                select_focused = true;
                d.just_opened = false;
            }
            draw->AddRectFilled(smin, smax, field_bg, px(4.f));
            draw->AddRect(smin, smax, field_border, px(4.f), 0, 1.f);
            if (select_focused)
            {
                draw->AddRect(ImVec2(smin.x - 1.f, smin.y - 1.f), ImVec2(smax.x + 1.f, smax.y + 1.f),
                              theme::u32(p.accent), px(5.f), 0, 1.f);
            }
            text_at(fonts().regular, 14.f, smin.x + px(11.f), smin.y + px(7.f), text, d.text_a.c_str());
            // The select's search input has the caret, in front of the value (Chromium blinks it every 500ms).
            if (select_focused && std::fmod(ImGui::GetTime(), 1.0) < 0.5)
            {
                const float caret_x = std::round(smin.x + px(11.f));
                draw->AddRectFilled(ImVec2(caret_x, smin.y + px(8.f)), ImVec2(caret_x + 1.f, smin.y + px(26.f)), text);
            }
            draw->AddRectFilled(ImVec2(smin.x + px(120.f), smin.y + px(7.f)),
                                ImVec2(smin.x + px(121.f), smin.y + px(27.f)), field_border);
            draw_svg_path(draw, "chevron-down", svg_path("chevron-down"), 20.f, 20.f,
                          ImVec2(smin.x + px(125.f), smin.y + px(7.f)), ImVec2(smin.x + px(145.f), smin.y + px(27.f)),
                          text);
            x = smax.x + px(7.5f);
            text_at(fonts().regular, 18.f, x, min.y + px(11.14f), text, "/");
            x += slash_w + px(7.5f);

            // The branch name: empty pushes to a branch of the same name.
            const ImVec2 imin(std::round(x), min.y + px(7.f));
            const ImVec2 imax(imin.x + px(191.f), imin.y + px(34.f));
            draw->AddRectFilled(imin, imax, field_bg, px(4.f));
            // The field covers the whole box; its 12px padding is the frame padding.
            ImGui::SetCursorScreenPos(imin);
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding,
                                ImVec2(px(12.f), (px(34.f) - text_size(fonts().regular, 14.f, "A").y) / 2.f));
            ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
            ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgActive, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_TextDisabled, p.text_disabled);
            push_font(fonts().regular, 14.f);
            const bool entered = input_text("##push_branch", d.text_b, d.ref_name.c_str(),
                                            ImGuiInputTextFlags_EnterReturnsTrue, imax.x - imin.x);
            const bool name_active = ImGui::IsItemActive();
            ImGui::PopFont();
            ImGui::PopStyleColor(4);
            ImGui::PopStyleVar(2);
            if (name_active)
            {
                select_focused = false;
                d.flag = false;
            }
            draw->AddRect(imin, imax, name_active ? theme::u32(p.accent) : field_border, px(4.f), 0, 1.f);
            x = imax.x + px(12.f);

            int result = 0;
            const auto button = [&](const char* id, const char* label, float w, bool primary)
            {
                const ImVec2 bmin(x, min.y + px(13.f));
                const ImVec2 bmax(x + w, bmin.y + px(22.f));
                ImGui::SetCursorScreenPos(bmin);
                const bool clicked = ImGui::InvisibleButton(id, ImVec2(w, bmax.y - bmin.y));
                const bool hovered = ImGui::IsItemHovered();
                if (primary)
                {
                    draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.green, hovered ? 0.3f : 0.2f)),
                                        px(2.f));
                    draw->AddRect(bmin, bmax, theme::u32(p.green), px(2.f), 0, 1.f);
                }
                else
                {
                    if (hovered)
                    {
                        draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.white, 0.1f)), px(2.f));
                    }
                    draw->AddRect(bmin, bmax, IM_COL32(255, 255, 255, 191), px(2.f), 0, 1.f);
                }
                text_at(fonts().bold, 12.f, bmin.x + px(6.f), bmin.y + px(2.f), text, label);
                x += w + px(12.f);
                return clicked;
            };
            if (button("##push_submit", "Submit", submit_w, true) || entered
                || (select_focused && !d.flag && ImGui::IsKeyPressed(ImGuiKey_Enter)))
            {
                result = 1;
            }
            if (button("##push_cancel", "Cancel", cancel_w, false) || (!d.flag && ImGui::IsKeyPressed(ImGuiKey_Escape)))
            {
                result = -1;
            }

            ImGui::End();

            // react-select's menu: 8px under the control, toolbar-bg0, 4px padding, 37px options.
            if (d.flag)
            {
                const ImVec2 mmin(smin.x, smax.y + px(8.f));
                const float option_h = px(37.f);
                const ImVec2 msize(smax.x - smin.x, px(8.f) + option_h * static_cast<float>(remotes.size()));
                ImGui::SetNextWindowPos(mmin);
                ImGui::SetNextWindowSize(msize);
                ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
                ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
                ImGui::Begin("##push_remote_menu", nullptr,
                             ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoSavedSettings
                                     | ImGuiWindowFlags_NoBackground);
                ImGui::PopStyleVar(2);
                ImGui::BringWindowToDisplayFront(ImGui::GetCurrentWindow());
                ImDrawList* menu = ImGui::GetWindowDrawList();
                const ImVec2 mmax(mmin.x + msize.x, mmin.y + msize.y);
                menu->AddRectFilled(mmin, mmax, theme::u32(p.toolbar_bg0), px(4.f));
                menu->AddRect(mmin, mmax, field_border, px(4.f), 0, 1.f);
                for (std::size_t i = 0; i < remotes.size(); ++i)
                {
                    const ImVec2 omin(mmin.x + 1.f, mmin.y + px(5.f) + option_h * static_cast<float>(i));
                    ImGui::SetCursorScreenPos(omin);
                    ImGui::PushID(static_cast<int>(i));
                    const bool picked = ImGui::InvisibleButton("##remote_option", ImVec2(msize.x - 2.f, option_h));
                    ImGui::PopID();
                    if (ImGui::IsItemHovered() || remotes[i].name == d.text_a)
                    {
                        menu->AddRectFilled(omin, ImVec2(omin.x + msize.x - 2.f, omin.y + option_h),
                                            theme::u32(p.hover_row));
                    }
                    const float ascent = 14.f * 2189.f / 2048.f * dpi_scale();
                    draw_text(menu, fonts().regular, 14.f,
                              ImVec2(omin.x + px(11.f),
                                     std::round(omin.y + px(8.f) + std::round(ascent)) - std::ceil(ascent)),
                              text, remotes[i].name.c_str());
                    if (picked)
                    {
                        d.text_a = remotes[i].name;
                        d.flag = false;
                    }
                }
                const bool inside =
                        ImGui::IsMouseHoveringRect(mmin, mmax, false) || ImGui::IsMouseHoveringRect(smin, smax, false);
                if (ImGui::IsKeyPressed(ImGuiKey_Escape) || (ImGui::IsMouseClicked(ImGuiMouseButton_Left) && !inside))
                {
                    d.flag = false;
                }
                ImGui::End();
            }
            return result;
        }

        // The hosting services by host: the friendly name and what their password field asks for.
        struct HostingService
        {
            const char* name = nullptr;
            const char* secret = "Password";
        };

        HostingService hosting_service(const std::string& url)
        {
            std::string host = url.substr(url.find("://") + 3);
            host = host.substr(0, host.find(':'));
            const auto is = [&](const char* domain)
            {
                const std::string suffix = std::string(".") + domain;
                return host == domain || host.ends_with(suffix);
            };
            if (is("github.com"))
            {
                return {"GitHub", "Personal Access Token"};
            }
            if (is("gitlab.com"))
            {
                return {"GitLab", "Personal Access Token"};
            }
            if (is("bitbucket.org"))
            {
                return {"Bitbucket", "App Password"};
            }
            if (is("dev.azure.com") || is("visualstudio.com"))
            {
                return {"Azure DevOps", "Personal Access Token"};
            }
            return {};
        }

        // The credentials form in the prompt bar: the message, Username, Password (or token),
        // Log In, Cancel and Remember me. Returns 1 to log in, -1 to cancel, 0 while open.
        int draw_credentials_prompt(app::Application::CredentialPrompt& prompt)
        {
            const theme::Palette& p = theme::palette();
            const ImGuiViewport* viewport = ImGui::GetMainViewport();
            const ImVec2 min(viewport->WorkPos.x,
                             viewport->WorkPos.y + px(theme::TABS_BAR_HEIGHT)); // under the menu bar, if any
            const ImVec2 size(viewport->Size.x, px(theme::TOOLBAR_HEIGHT));
            ImGui::SetNextWindowPos(min);
            ImGui::SetNextWindowSize(size);
            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
            ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding, 0.f);
            ImGui::Begin("##credentials_prompt", nullptr,
                         ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoSavedSettings
                                 | ImGuiWindowFlags_NoBackground);
            ImGui::PopStyleVar(3);
            ImGui::BringWindowToDisplayFront(ImGui::GetCurrentWindow());
            if (prompt.just_opened)
            {
                ImGui::SetWindowFocus();
            }
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const ImVec2 max(min.x + size.x, min.y + size.y);
            draw->AddRectFilled(min, max, theme::u32(p.toolbar_bg1));
            draw->AddRectFilled(min, max, theme::u32(theme::with_alpha(p.accent, 0.2f)));
            const ImU32 text = IM_COL32(255, 255, 255, 230);
            const ImU32 field_bg = IM_COL32(0, 0, 0, 51);
            const ImU32 field_border = IM_COL32(255, 255, 255, 51);
            const auto text_at =
                    [&](ImFont* font, float font_size, float x, float box_top, ImU32 color, const char* label)
            {
                const float ascent = font_size * 2189.f / 2048.f * dpi_scale();
                draw_text(draw, font, font_size,
                          ImVec2(x, std::round(box_top + std::round(ascent)) - std::ceil(ascent)), color, label);
            };

            const HostingService service = hosting_service(prompt.url);
            const std::string host = prompt.url.substr(prompt.url.find("://") + 3);
            std::string message;
            if (service.name != nullptr)
            {
                message = std::string(prompt.rejected ? "Authentication failed. " : "") + "Please enter your "
                          + service.name + " credentials:";
            }
            else
            {
                message = prompt.rejected ? "Invalid credentials for " + host + ". Please try again:"
                                          : "Please enter your credentials for " + host + ":";
            }
            const char* remember_label = "Remember me";
            const float field_w = px(191.f);
            const float message_w = text_size(fonts().regular, 14.f, message.c_str()).x;
            const float login_w = text_size(fonts().bold, 12.f, "Log In").x + px(12.f);
            const float cancel_w = text_size(fonts().bold, 12.f, "Cancel").x + px(12.f);
            const float box = px(13.f);
            const float remember_w = box + px(6.f) + text_size(fonts().regular, 14.f, remember_label).x;
            const float total = message_w + px(10.f) + field_w + px(12.f) + field_w + px(12.f) + login_w + px(12.f)
                                + cancel_w + px(12.f) + remember_w + px(8.f);
            float x = std::round(min.x + std::max(px(10.f), (size.x - total) / 2.f));
            text_at(fonts().regular, 14.f, x, min.y + px(14.f), text, message.c_str());
            x += message_w + px(10.f);

            bool submitted = false;
            const auto field = [&](const char* id, std::string& value, const char* hint, ImGuiInputTextFlags flags,
                                   bool focus)
            {
                const ImVec2 imin(std::round(x), min.y + px(7.f));
                const ImVec2 imax(imin.x + field_w, imin.y + px(34.f));
                draw->AddRectFilled(imin, imax, field_bg, px(4.f));
                ImGui::SetCursorScreenPos(imin);
                ImGui::PushStyleVar(ImGuiStyleVar_FramePadding,
                                    ImVec2(px(12.f), (px(34.f) - text_size(fonts().regular, 14.f, "A").y) / 2.f));
                ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
                ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
                ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, ImVec4(0.f, 0.f, 0.f, 0.f));
                ImGui::PushStyleColor(ImGuiCol_FrameBgActive, ImVec4(0.f, 0.f, 0.f, 0.f));
                ImGui::PushStyleColor(ImGuiCol_TextDisabled, p.text_disabled);
                push_font(fonts().regular, 14.f);
                if (focus)
                {
                    ImGui::SetKeyboardFocusHere();
                }
                submitted |= input_text(id, value, hint, flags | ImGuiInputTextFlags_EnterReturnsTrue, field_w);
                const bool active = ImGui::IsItemActive();
                ImGui::PopFont();
                ImGui::PopStyleColor(4);
                ImGui::PopStyleVar(2);
                draw->AddRect(imin, imax, active ? theme::u32(p.accent) : field_border, px(4.f), 0, 1.f);
                x = imax.x + px(12.f);
            };
            const bool focus_username = prompt.just_opened && prompt.username.empty();
            const bool focus_password = prompt.just_opened && !prompt.username.empty();
            prompt.just_opened = false;
            field("##credentials_username", prompt.username, "Username", 0, focus_username);
            field("##credentials_password", prompt.password, service.secret, ImGuiInputTextFlags_Password,
                  focus_password);

            const bool complete = !prompt.username.empty() && !prompt.password.empty();
            int result = 0;
            const auto button = [&](const char* id, const char* label, float w, bool primary)
            {
                const ImVec2 bmin(x, min.y + px(13.f));
                const ImVec2 bmax(x + w, bmin.y + px(22.f));
                ImGui::SetCursorScreenPos(bmin);
                const bool clicked = ImGui::InvisibleButton(id, ImVec2(w, bmax.y - bmin.y));
                const bool hovered = ImGui::IsItemHovered();
                if (primary)
                {
                    const float alpha = !complete ? 0.1f : (hovered ? 0.3f : 0.2f);
                    draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.green, alpha)), px(2.f));
                    draw->AddRect(bmin, bmax, theme::u32(theme::with_alpha(p.green, complete ? 1.f : 0.5f)), px(2.f),
                                  0, 1.f);
                }
                else
                {
                    if (hovered)
                    {
                        draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.white, 0.1f)), px(2.f));
                    }
                    draw->AddRect(bmin, bmax, IM_COL32(255, 255, 255, 191), px(2.f), 0, 1.f);
                }
                const ImU32 label_color = primary && !complete ? IM_COL32(255, 255, 255, 115) : text;
                text_at(fonts().bold, 12.f, bmin.x + px(6.f), bmin.y + px(2.f), label_color, label);
                x += w + px(12.f);
                return clicked;
            };
            // Enter submits the form from anywhere in it, not only from the fields.
            submitted |= ImGui::IsWindowFocused()
                         && (ImGui::IsKeyPressed(ImGuiKey_Enter) || ImGui::IsKeyPressed(ImGuiKey_KeypadEnter));
            if ((button("##credentials_login", "Log In", login_w, true) || submitted) && complete)
            {
                result = 1;
            }
            if (button("##credentials_cancel", "Cancel", cancel_w, false) || ImGui::IsKeyPressed(ImGuiKey_Escape))
            {
                result = -1;
            }

            // Remember me: a 13px checkbox and its label, both clickable.
            const ImVec2 cmin(x, std::round(min.y + (size.y - box) / 2.f));
            ImGui::SetCursorScreenPos(ImVec2(cmin.x, min.y + px(13.f)));
            if (ImGui::InvisibleButton("##credentials_remember", ImVec2(remember_w, px(22.f))))
            {
                prompt.remember = !prompt.remember;
            }
            const ImVec2 cmax(cmin.x + box, cmin.y + box);
            if (prompt.remember)
            {
                draw->AddRectFilled(cmin, cmax, theme::u32(p.accent), px(2.f));
                draw->AddPolyline(std::array{ImVec2(cmin.x + box * 0.22f, cmin.y + box * 0.52f),
                                             ImVec2(cmin.x + box * 0.42f, cmin.y + box * 0.72f),
                                             ImVec2(cmin.x + box * 0.8f, cmin.y + box * 0.3f)}
                                          .data(),
                                  3, IM_COL32(255, 255, 255, 255), 0, px(1.6f));
            }
            else
            {
                draw->AddRectFilled(cmin, cmax, field_bg, px(2.f));
                draw->AddRect(cmin, cmax, IM_COL32(255, 255, 255, 140), px(2.f), 0, 1.f);
            }
            text_at(fonts().regular, 14.f, cmax.x + px(6.f), min.y + px(14.f), text, remember_label);

            ImGui::End();
            return result;
        }

        // The rename prompt for a local branch, in the prompt bar: "Rename 'name' to:", the name field (230x34, the
        // current name in it, focused with the caret at the end), Submit and Cancel. Returns 1 to rename, -1 to
        // cancel, 0 while open.
        int draw_rename_prompt(app::Dialog& d)
        {
            const theme::Palette& p = theme::palette();
            const ImGuiViewport* viewport = ImGui::GetMainViewport();
            const ImVec2 min(viewport->WorkPos.x,
                             viewport->WorkPos.y + px(theme::TABS_BAR_HEIGHT)); // under the menu bar, if any
            const ImVec2 size(viewport->Size.x, px(theme::TOOLBAR_HEIGHT));
            ImGui::SetNextWindowPos(min);
            ImGui::SetNextWindowSize(size);
            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
            ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding, 0.f);
            ImGui::Begin("##rename_prompt", nullptr,
                         ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoSavedSettings
                                 | ImGuiWindowFlags_NoBackground);
            ImGui::PopStyleVar(3);
            ImGui::BringWindowToDisplayFront(ImGui::GetCurrentWindow());
            if (d.just_opened)
            {
                ImGui::SetWindowFocus();
            }
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const ImVec2 max(min.x + size.x, min.y + size.y);
            draw->AddRectFilled(min, max, theme::u32(p.toolbar_bg1));
            draw->AddRectFilled(min, max, theme::u32(theme::with_alpha(p.accent, 0.2f)));
            const ImU32 text = IM_COL32(255, 255, 255, 230);
            const ImU32 field_bg = IM_COL32(0, 0, 0, 51);
            const ImU32 field_border = IM_COL32(255, 255, 255, 51);
            const auto text_at =
                    [&](ImFont* font, float font_size, float x, float box_top, ImU32 color, const char* label)
            {
                const float ascent = font_size * 2189.f / 2048.f * dpi_scale();
                draw_text(draw, font, font_size,
                          ImVec2(x, std::round(box_top + std::round(ascent)) - std::ceil(ascent)), color, label);
            };

            const std::string message = "Rename '" + d.ref_name + "' to:";
            const float field_w = px(230.f);
            const float message_w = text_size(fonts().regular, 14.f, message.c_str()).x;
            const float submit_w = text_size(fonts().bold, 12.f, "Submit").x + px(12.f);
            const float cancel_w = text_size(fonts().bold, 12.f, "Cancel").x + px(12.f);
            const float total =
                    message_w + px(10.f) + field_w + px(12.f) + submit_w + px(12.f) + cancel_w + px(8.f);
            float x = std::round(min.x + std::max(px(10.f), (size.x - total) / 2.f));
            text_at(fonts().regular, 14.f, x, min.y + px(14.f), text, message.c_str());
            x += message_w + px(10.f);

            // The name field; keyboard focus would select the name, so the caret goes to its end instead.
            const ImVec2 imin(std::round(x), min.y + px(7.f));
            const ImVec2 imax(imin.x + field_w, imin.y + px(34.f));
            draw->AddRectFilled(imin, imax, field_bg, px(4.f));
            ImGui::SetCursorScreenPos(imin);
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding,
                                ImVec2(px(12.f), (px(34.f) - text_size(fonts().regular, 14.f, "A").y) / 2.f));
            ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
            ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgActive, ImVec4(0.f, 0.f, 0.f, 0.f));
            push_font(fonts().regular, 14.f);
            static bool caret_to_end = false;
            if (d.just_opened)
            {
                ImGui::SetKeyboardFocusHere();
                caret_to_end = true;
                d.just_opened = false;
            }
            ImGui::SetNextItemWidth(field_w);
            const ImGuiInputTextFlags flags = ImGuiInputTextFlags_EnterReturnsTrue | ImGuiInputTextFlags_CallbackAlways;
            const bool entered = ImGui::InputText("##rename_branch", &d.text_a, flags,
                                                  [](ImGuiInputTextCallbackData* data)
                                                  {
                                                      if (caret_to_end)
                                                      {
                                                          data->CursorPos = data->SelectionStart = data->SelectionEnd =
                                                                  data->BufTextLen;
                                                          caret_to_end = false;
                                                      }
                                                      return 0;
                                                  });
            text_field_menu(d.text_a);
            const bool active = ImGui::IsItemActive();
            ImGui::PopFont();
            ImGui::PopStyleColor(3);
            ImGui::PopStyleVar(2);
            draw->AddRect(imin, imax, active ? theme::u32(p.accent) : field_border, px(4.f), 0, 1.f);
            x = imax.x + px(12.f);

            const bool complete = !d.text_a.empty();
            int result = 0;
            const auto button = [&](const char* id, const char* label, float w, bool primary)
            {
                const ImVec2 bmin(x, min.y + px(13.f));
                const ImVec2 bmax(x + w, bmin.y + px(22.f));
                ImGui::SetCursorScreenPos(bmin);
                const bool clicked = ImGui::InvisibleButton(id, ImVec2(w, bmax.y - bmin.y));
                const bool hovered = ImGui::IsItemHovered();
                if (primary)
                {
                    const float alpha = !complete ? 0.1f : (hovered ? 0.3f : 0.2f);
                    draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.green, alpha)), px(2.f));
                    draw->AddRect(bmin, bmax, theme::u32(theme::with_alpha(p.green, complete ? 1.f : 0.5f)), px(2.f),
                                  0, 1.f);
                }
                else
                {
                    if (hovered)
                    {
                        draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.white, 0.1f)), px(2.f));
                    }
                    draw->AddRect(bmin, bmax, IM_COL32(255, 255, 255, 191), px(2.f), 0, 1.f);
                }
                const ImU32 label_color = primary && !complete ? IM_COL32(255, 255, 255, 115) : text;
                text_at(fonts().bold, 12.f, bmin.x + px(6.f), bmin.y + px(2.f), label_color, label);
                x += w + px(12.f);
                return clicked;
            };
            if ((button("##rename_submit", "Submit", submit_w, true) || entered) && complete)
            {
                result = 1;
            }
            if (button("##rename_cancel", "Cancel", cancel_w, false) || ImGui::IsKeyPressed(ImGuiKey_Escape))
            {
                result = -1;
            }
            ImGui::End();
            return result;
        }

        // The tab the dialog was opened for: the one with its working directory (the active tab until it is
        // recorded), nullptr once that tab is closed.
        app::RepoState* dialog_state(app::Application& app, const app::Dialog& d)
        {
            if (d.workdir.empty())
            {
                return app.active();
            }
            for (const std::unique_ptr<app::RepoState>& tab : app.tabs)
            {
                if (tab->repo.workdir() == d.workdir)
                {
                    return tab.get();
                }
            }
            return nullptr;
        }

        // Undo for a history rewrite: a hard reset to the previous tip, with the uncommitted changes present at
        // undo time stashed around it so they survive (as menus.cpp's tip_restore_entry does). Built before the
        // rewrite, while the history still holds the tip.
        app::UndoEntry tip_restore_entry(app::RepoState& state, const std::string& label)
        {
            git::Repository* repo = &state.repo;
            const git::Sha sha = state.history.head_sha;
            return {label,
                    [repo, sha]() -> git::Result<void>
                    {
                        if (sha.empty())
                        {
                            return std::unexpected(git::Error{"nothing to restore"});
                        }
                        bool stashed = false;
                        if (repo->load_status().value_or(git::WorkDirStatus{}).count() > 0)
                        {
                            if (auto pushed = repo->stash_push("ImGit: auto stash before undo"); pushed)
                            {
                                stashed = true;
                            }
                        }
                        auto result = repo->reset(sha, git::ResetMode::HARD);
                        if (stashed)
                        {
                            (void)repo->stash_pop(0);
                        }
                        return result;
                    },
                    {},
                    git::ResetMode::HARD};
        }
    } // namespace

    namespace
    {
        // A button of the button families with a Font Awesome icon before its label, the two centred.
        bool icon_label_button(bool (*button)(const char*, ImVec2, bool), const char* id, const char* icon,
                               const char* label, ImVec2 size, bool enabled = true)
        {
            const theme::Palette& p = theme::palette();
            const bool pressed = button(id, size, enabled);
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const ImVec2 min = ImGui::GetItemRectMin();
            const ImVec2 max = ImGui::GetItemRectMax();
            const float icon_w = fa_icon_width(icon, px(12.f));
            const ImVec2 text = text_size(fonts().regular, 13.f, label);
            const float x = std::round((min.x + max.x - icon_w - px(5.f) - text.x) / 2.f);
            const float mid = (min.y + max.y) / 2.f;
            ImVec4 color = enabled && ImGui::IsItemHovered() ? p.text_selected : p.text_normal;
            if (!enabled)
            {
                color = theme::with_alpha(color, color.w * 0.5f);
            }
            draw_fa_icon(draw, icon, ImVec2(x + icon_w / 2.f, mid), px(12.f), theme::u32(color));
            draw_text(draw, fonts().regular, 13.f, ImVec2(x + icon_w + px(5.f), std::round(mid - text.y / 2.f)),
                      theme::u32(color), label);
            return pressed;
        }

        // `text` broken into lines `width` device px wide (at its newlines, then between words).
        std::vector<std::string> wrap_lines(ImFont* font, float size, const std::string& text, float width)
        {
            const float glyph_h = font_px(font, size);
            std::vector<std::string> lines;
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
                    lines.emplace_back();
                }
                while (line < line_end)
                {
                    const char* wrap = font->CalcWordWrapPosition(glyph_h, line, line_end, width);
                    if (wrap == line)
                    {
                        wrap = line + 1;
                    }
                    lines.emplace_back(line, wrap);
                    line = wrap;
                    while (line < line_end && *line == ' ')
                    {
                        ++line;
                    }
                }
                start = end + 1;
            }
            return lines;
        }

        // The commit a submodule has checked out, read from the submodule itself (cached by folder and sha).
        const git::Commit* submodule_commit(const std::string& folder, const git::Sha& sha)
        {
            static std::string cached_key;
            static std::optional<git::Commit> cached;
            const std::string key = folder + '\n' + sha;
            if (key != cached_key)
            {
                cached_key = key;
                cached.reset();
                if (auto repo = git::Repository::open(folder); repo)
                {
                    if (auto commit = repo->load_commit(sha); commit)
                    {
                        cached = std::move(*commit);
                    }
                }
            }
            return cached ? &*cached : nullptr;
        }
    } // namespace

    // The panel sliding out of the left panel over the graph (.panel-slide, 0.25s): the section icon and the 18px
    // title with a close cross in a 50px header, then the form, 20px in.
    // Add Submodule (400px): the Remote URL and Name/Path fields under their labels and the Add Submodule button.
    // Edit Submodule (600px): the Remote URL (editable) and the Path, the Edit this submodule button, whether the
    // submodule is checked out at the commit the repository records, the commit it has checked out, and the
    // Open / Delete this submodule buttons.
    void draw_slide_panel(app::Application& app, app::RepoState& state, ImVec2 origin, float height)
    {
        app::Dialog& d = app.dialog;
        const bool open = (d.kind == app::Dialog::Kind::ADD_SUBMODULE || d.kind == app::Dialog::Kind::EDIT_SUBMODULE)
                          && (d.workdir.empty() ? app.active() == &state : d.workdir == state.repo.workdir());
        const float t = smooth("##slide_panel", open, 0.25f);
        if (t <= 0.f)
        {
            return;
        }
        // The form keeps its last values for the way out, once the dialog is gone.
        static bool editing = false;
        static std::string url;
        static std::string path;
        static std::string original_url;
        if (open)
        {
            editing = d.kind == app::Dialog::Kind::EDIT_SUBMODULE;
            url = d.text_a;
            path = d.text_b;
            if (d.just_opened)
            {
                original_url = d.text_a;
            }
        }
        const git::Submodule* submodule = nullptr;
        if (editing)
        {
            for (const git::Submodule& entry : state.history.submodules)
            {
                if (entry.path == path)
                {
                    submodule = &entry;
                }
            }
        }
        const theme::Palette& p = theme::palette();
        const float width = px(editing ? 600.f : 400.f);
        const float shadow = px(10.f);
        const float visible = std::round(width * t);
        // The child holds the panel's shown part and its shadow over the graph.
        ImGui::SetCursorScreenPos(origin);
        ImGui::BeginChild("##slide_panel", ImVec2(visible + shadow, height), ImGuiChildFlags_None,
                          ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoScrollWithMouse);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const float x = origin.x + visible - width; // slides in from under the left panel
        const ImVec2 min(x, origin.y);
        const ImVec2 max(x + width, origin.y + height);
        draw->AddRectFilled(min, max, theme::u32(p.panel_bg1));
        // The shadow on its right edge (box-shadow 0 0 10px rgba(0,0,0,.5)), over the graph.
        for (float i = 0.f; i < shadow; i += 1.f)
        {
            const float alpha = 0.15f * (1.f - i / shadow);
            draw->AddRectFilled(ImVec2(max.x + i, min.y), ImVec2(max.x + i + 1.f, max.y),
                                theme::u32(ImVec4(0.f, 0.f, 0.f, alpha)));
        }
        draw->AddRectFilled(ImVec2(max.x - 1.f, min.y), max, theme::u32(p.panel_border));

        const float pad = px(20.f);
        // Header
        draw_fa_icon(draw, "far:layer-group", ImVec2(x + pad + px(8.f), origin.y + px(25.f)), px(16.f),
                     theme::u32(p.text_secondary));
        {
            const float text_h = text_size(fonts().regular, 18.f, "A").y;
            draw_text(draw, fonts().regular, 18.f, ImVec2(x + pad + px(27.f), origin.y + px(25.f) - text_h / 2.f),
                      theme::u32(p.text_selected), editing ? "Edit Submodule" : "Add Submodule");
        }
        bool close = false;
        {
            const ImVec2 cmin(max.x - pad - px(16.f), origin.y + px(17.f));
            ImGui::SetCursorScreenPos(cmin);
            if (ImGui::InvisibleButton("##slide_close", ImVec2(px(16.f), px(16.f))))
            {
                close = true;
            }
            draw_fa_icon(draw, "fas:times", ImVec2(cmin.x + px(8.f), cmin.y + px(8.f)), px(14.f),
                         theme::u32(ImGui::IsItemHovered() ? p.text_selected : p.text_normal));
        }

        // The path defaults to the repository's name, as `git submodule add` picks it.
        std::string name = url;
        while (!name.empty() && (name.back() == '/' || name.back() == '\\'))
        {
            name.pop_back();
        }
        if (name.ends_with(".git"))
        {
            name.resize(name.size() - 4);
        }
        if (const auto slash = name.find_last_of("/\\:"); slash != std::string::npos)
        {
            name = name.substr(slash + 1);
        }

        const float field_w = width - 2.f * pad;
        const auto label = [&](const char* text, float y)
        {
            draw_text(draw, fonts().regular, 14.f, ImVec2(x + pad, origin.y + y), theme::u32(p.text_normal), text);
        };
        const auto field = [&](const char* id, std::string& value, const char* hint, float y, bool focus,
                               ImGuiInputTextFlags flags = 0)
        {
            ImGui::SetCursorScreenPos(ImVec2(x + pad, origin.y + y));
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding,
                                ImVec2(px(8.f), (px(32.f) - text_size(fonts().regular, 14.f, "A").y) / 2.f));
            push_font(fonts().regular, 14.f);
            if (focus)
            {
                ImGui::SetKeyboardFocusHere();
            }
            const bool entered = input_text(id, value, hint, ImGuiInputTextFlags_EnterReturnsTrue | flags, field_w);
            ImGui::PopFont();
            ImGui::PopStyleVar();
            return entered;
        };
        ImGui::PushStyleColor(ImGuiCol_FrameBg, p.input_bg);
        ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, p.input_bg);
        ImGui::PushStyleColor(ImGuiCol_FrameBgActive, p.input_bg);
        ImGui::PushStyleColor(ImGuiCol_TextDisabled, p.text_disabled);
        const bool focus = open && d.just_opened;
        if (open)
        {
            d.just_opened = false;
        }
        bool submit = false;
        label("Remote URL", 58.f);
        if (editing)
        {
            submit |= field("##submodule_url", url, "https://github.com/owner/repo.git", 82.f, false);
            label("Path", 132.f);
            // The path is the submodule's identity here: shown, not edited.
            ImGui::PushStyleColor(ImGuiCol_Text, p.text_disabled);
            (void)field("##submodule_path", path, "", 156.f, false, ImGuiInputTextFlags_ReadOnly);
            ImGui::PopStyleColor();
        }
        else
        {
            submit |= field("##submodule_url", url, "https://github.com/owner/repo.git", 82.f, focus);
            label("Name/Path", 132.f);
            submit |= field("##submodule_path", path, name.empty() ? "path/in/this/repository" : name.c_str(), 156.f,
                            false);
        }
        ImGui::PopStyleColor(4);
        ImGui::SetCursorScreenPos(ImVec2(x + pad, origin.y + px(206.f)));
        const bool url_changed = !url.empty() && url != original_url;
        if (editing)
        {
            submit |= success_button("Edit this submodule", ImVec2(px(132.f), px(28.f)), url_changed);
            submit = submit && url_changed;
        }
        else
        {
            submit |= success_button("Add Submodule", ImVec2(px(112.f), px(28.f)), !url.empty());
            submit = submit && !url.empty();
        }

        // Edit Submodule: its state and revision, and what can be done with it.
        bool open_submodule = false;
        bool delete_submodule = false;
        std::vector<std::string> sync_args; // initialize / update it to the recorded commit
        if (editing && submodule != nullptr)
        {
            const auto divider = [&](float y)
            {
                draw->AddRectFilled(ImVec2(x + pad, y), ImVec2(x + pad + field_w, y + 1.f), theme::u32(p.panel_border));
            };
            float y = origin.y + px(254.f);
            divider(y);
            y += px(24.f);

            // The status line: an icon and a sentence centred under the divider.
            const bool initialized = !submodule->checked_out.empty();
            const bool in_sync = submodule->in_sync();
            const std::string sentence =
                    !initialized ? "This submodule is not initialized."
                    : in_sync    ? "This submodule is in-sync with your repository (" + state.repo.name() + ")."
                                 : "This submodule is out of sync with your repository (" + state.repo.name() + ").";
            const char* status_icon = in_sync ? "fas:check" : "fas:exclamation-triangle";
            const ImVec4 status_color = in_sync ? p.green : p.yellow;
            {
                const float icon_w = fa_icon_width(status_icon, px(13.f));
                const ImVec2 text = text_size(fonts().regular, 14.f, sentence.c_str());
                const float sx = std::round(x + pad + (field_w - icon_w - px(5.f) - text.x) / 2.f);
                draw_fa_icon(draw, status_icon, ImVec2(sx + icon_w / 2.f, y), px(13.f), theme::u32(status_color));
                draw_text(draw, fonts().regular, 14.f, ImVec2(sx + icon_w + px(5.f), std::round(y - text.y / 2.f)),
                          theme::u32(status_color), sentence.c_str());
            }
            y += px(18.f);
            if (!in_sync)
            {
                // Checks out the commit the repository records (cloning the submodule first when needed).
                const char* action = initialized ? "Update this submodule" : "Initialize this submodule";
                const float button_w = text_size(fonts().regular, 13.f, action).x + px(24.f);
                ImGui::SetCursorScreenPos(ImVec2(std::round(x + pad + (field_w - button_w) / 2.f), y + px(4.f)));
                if (success_button(action, ImVec2(button_w, px(28.f)), !state.busy()))
                {
                    sync_args = initialized ? std::vector<std::string>{"submodule", "update", "--", path}
                                            : std::vector<std::string>{"submodule", "update", "--init", "--", path};
                }
                y += px(44.f);
            }

            // Current Revision: the commit checked out in the submodule.
            const std::filesystem::path folder = std::filesystem::path(state.repo.workdir()) / path;
            const git::Commit* commit =
                    initialized ? submodule_commit(folder.string(), submodule->checked_out) : nullptr;
            if (initialized)
            {
                y += px(6.f);
                const ImVec2 caption = text_size(fonts().regular, 14.f, "Current Revision");
                draw_text(draw, fonts().regular, 14.f, ImVec2(x + pad, y), theme::u32(p.text_normal),
                          "Current Revision");
                if (commit != nullptr)
                {
                    const std::string date = "(" + format_date_time(commit->time) + ")";
                    draw_text(draw, fonts().regular, 14.f, ImVec2(x + pad + caption.x + px(10.f), y),
                              theme::u32(p.text_secondary), date.c_str());
                }
                y += caption.y + px(6.f);

                // The box: "Commit" and "Commit Message" in bold monospace, their values beside them.
                constexpr float MONO = 12.f;
                const float line_h = px(19.f);
                const float value_x = x + pad + px(125.f);
                const float value_w = x + pad + field_w - px(10.f) - value_x;
                std::string message = commit != nullptr ? commit->summary : std::string{};
                if (commit != nullptr && !commit->body.empty())
                {
                    message += "\n\n" + commit->body;
                }
                while (!message.empty() && (message.back() == '\n' || message.back() == ' '))
                {
                    message.pop_back();
                }
                std::vector<std::string> lines = wrap_lines(fonts().mono, MONO, message, value_w);
                constexpr std::size_t MAX_LINES = 12;
                if (lines.size() > MAX_LINES)
                {
                    lines.resize(MAX_LINES);
                    lines.back() += "...";
                }
                const float rows = 1.f + static_cast<float>(std::max<std::size_t>(lines.size(), 1));
                const ImVec2 box_min(x + pad, y);
                const ImVec2 box_max(x + pad + field_w, y + px(16.f) + line_h * rows);
                draw->AddRectFilled(box_min, box_max, theme::u32(p.panel_bg0));
                const float mono_h = text_size(fonts().mono, MONO, "A").y;
                float row_y = y + px(8.f) + (line_h - mono_h) / 2.f;
                const auto row_label = [&](const char* text)
                {
                    draw_text(draw, fonts().mono, MONO, ImVec2(box_min.x + px(5.f), row_y), theme::u32(p.text_normal),
                              text);
                };
                row_label("Commit");
                draw_text(draw, fonts().mono, MONO, ImVec2(value_x, row_y), theme::u32(p.text_selected),
                          submodule->checked_out.c_str());
                row_y += line_h;
                row_label("Commit Message");
                for (const std::string& line : lines)
                {
                    draw_text(draw, fonts().mono, MONO, ImVec2(value_x, row_y), theme::u32(p.text_selected),
                              line.c_str());
                    row_y += line_h;
                }
                y = box_max.y + px(11.f);
                divider(y);
                y += px(21.f);
            }

            // Open / Delete this submodule, side by side.
            const float button_w = (field_w - px(10.f)) / 2.f;
            ImGui::SetCursorScreenPos(ImVec2(x + pad, y));
            open_submodule = icon_label_button(primary_button, "##open_submodule", "far:folder-open",
                                               "Open this submodule", ImVec2(button_w, px(32.f)), initialized);
            ImGui::SetCursorScreenPos(ImVec2(x + pad + button_w + px(10.f), y));
            delete_submodule = icon_label_button(danger_button, "##delete_submodule", "far:trash-alt",
                                                 "Delete this submodule", ImVec2(button_w, px(32.f)), !state.busy());
        }

        if (open)
        {
            d.text_a = url;
            d.text_b = path;
            if (ImGui::IsWindowFocused(ImGuiFocusedFlags_ChildWindows) && ImGui::IsKeyPressed(ImGuiKey_Escape))
            {
                close = true;
            }
            // The submodule is gone (deleted, or a checkout without it): nothing left to edit.
            if (editing && submodule == nullptr)
            {
                close = true;
            }
            if (submit && editing)
            {
                app.run_git_cli(state, "Edit submodule", {"submodule", "set-url", "--", path, url});
                close = true;
            }
            else if (submit)
            {
                app.run_git_cli(state, "Add submodule", {"submodule", "add", "--", url, path.empty() ? name : path});
                close = true;
            }
            if (!sync_args.empty())
            {
                app.run_git_cli(state, sync_args[2] == "--init" ? "Initialize submodule" : "Update submodule",
                                std::move(sync_args));
            }
            if (close)
            {
                d.close();
            }
            else if (delete_submodule)
            {
                // `git rm` drops the folder and its .gitmodules entry once deinit has emptied it.
                const std::string target = path;
                d.open(app::Dialog::Kind::CONFIRM);
                d.workdir = state.repo.workdir();
                d.title = "Delete submodule";
                d.message = "Delete the submodule '" + target
                            + "'? Its folder and its entry in .gitmodules are removed and the removal is staged.";
                d.confirm_label = "Delete";
                d.danger = true;
                d.on_confirm = [&app, workdir = state.repo.workdir(), target]
                {
                    for (const std::unique_ptr<app::RepoState>& tab : app.tabs)
                    {
                        if (tab->repo.workdir() == workdir)
                        {
                            app.run_git_cli_sequence(
                                    *tab, "Delete submodule",
                                    {{"submodule", "deinit", "-f", "--", target}, {"rm", "-f", "--", target}});
                            break;
                        }
                    }
                };
            }
            else if (open_submodule)
            {
                const std::string target = path;
                d.close();
                // Last: the tab shows the submodule from the next frame on (this state stays alive behind it).
                app.open_submodule(state, target);
            }
        }
        ImGui::EndChild();
    }

    void draw_dialogs(app::Application& app)
    {
        if (app.credential_prompt)
        {
            const int answer = draw_credentials_prompt(*app.credential_prompt);
            if (answer != 0)
            {
                app::Application::CredentialPrompt prompt = std::move(*app.credential_prompt);
                app.credential_prompt.reset();
                if (answer > 0)
                {
                    prompt.retry(git::cli::Credential{prompt.url, prompt.username, prompt.password, prompt.remember});
                }
            }
        }
        app::Dialog& d = app.dialog;
        if (d.kind == app::Dialog::Kind::NONE)
        {
            return;
        }
        // The dialog acts on the tab it was opened for (the active one on its first frame), whichever tab is
        // active when it is answered, and closes when that tab is gone.
        if (d.workdir.empty())
        {
            if (const app::RepoState* active = app.active(); active != nullptr)
            {
                d.workdir = active->repo.workdir();
            }
        }
        app::RepoState* state = dialog_state(app, d);
        const bool needs_repo = d.kind != app::Dialog::Kind::CLONE_REPO && d.kind != app::Dialog::Kind::INIT_REPO
                                && d.kind != app::Dialog::Kind::PREFERENCES
                                && d.kind != app::Dialog::Kind::KEYBOARD_SHORTCUTS && d.kind != app::Dialog::Kind::ABOUT
                                && d.kind != app::Dialog::Kind::CONFIRM;
        // A confirmation's action may be repository-free, but one opened from a tab must not outlive it (its
        // action may refer to that tab).
        if (state == nullptr && (needs_repo || (d.kind == app::Dialog::Kind::CONFIRM && !d.workdir.empty())))
        {
            d.close();
            return;
        }
        // Adding or editing a submodule is the panel sliding out of the left panel (draw_slide_panel).
        if (d.kind == app::Dialog::Kind::ADD_SUBMODULE || d.kind == app::Dialog::Kind::EDIT_SUBMODULE)
        {
            return;
        }
        // Pushing a branch without an upstream asks for the remote branch in the prompt bar.
        if (d.kind == app::Dialog::Kind::PUSH_NEW_BRANCH)
        {
            const int answer = draw_push_prompt(d, state->history.remotes);
            if (answer == 0)
            {
                return;
            }
            const app::Dialog request = d;
            d.close();
            if (answer > 0 && !request.text_a.empty())
            {
                const std::string target = request.text_b.empty() ? request.ref_name : request.text_b;
                app.run_git_cli(*state, "Push",
                                {"push", "--set-upstream", request.text_a, request.ref_name + ":" + target});
            }
            return;
        }
        // Renaming a branch asks for the new name in the prompt bar.
        if (d.kind == app::Dialog::Kind::RENAME_BRANCH)
        {
            const int answer = draw_rename_prompt(d);
            if (answer == 0)
            {
                return;
            }
            const app::Dialog request = d;
            d.close();
            if (answer < 0 || request.text_a == request.ref_name)
            {
                return;
            }
            git::Repository* repo = &state->repo;
            const std::string from = request.ref_name;
            const std::string to = request.text_a;
            if (app.run(
                        *state, "Rename branch",
                        [&]
                        {
                            return state->repo.rename_branch(from, to);
                        },
                        "Renamed " + from + " to " + to))
            {
                state->push_undo({"rename " + from,
                                  [repo, from, to]
                                  {
                                      return repo->rename_branch(to, from);
                                  },
                                  [repo, from, to]
                                  {
                                      return repo->rename_branch(from, to);
                                  }});
            }
            return;
        }
        // Destructive confirmations are the prompt bar over the toolbar, not a modal.
        if (d.kind == app::Dialog::Kind::CONFIRM || d.kind == app::Dialog::Kind::DISCARD_ALL
            || d.kind == app::Dialog::Kind::DELETE_BRANCH || d.kind == app::Dialog::Kind::DELETE_TAG
            || d.kind == app::Dialog::Kind::DELETE_STASH)
        {
            std::string message;
            std::string label = "Delete";
            bool danger = true;
            switch (d.kind)
            {
            case app::Dialog::Kind::CONFIRM:
                message = d.message;
                label = d.confirm_label.empty() ? "OK" : d.confirm_label;
                danger = d.danger;
                break;
            case app::Dialog::Kind::DISCARD_ALL:
                message = "This will discard all staged and unstaged changes, including new untracked files. Are you "
                          "sure you want to discard all changes?";
                label = "Reset All";
                break;
            case app::Dialog::Kind::DELETE_BRANCH:
                if (!d.ref_remote.empty())
                {
                    message = "This is a destructive operation, are you sure you want to delete "
                              + in_quotes(d.ref_remote + "/" + d.ref_name) + "?";
                }
                else if (d.flag)
                {
                    message = "This is a destructive operation, are you sure you want to delete "
                              + in_quotes(d.ref_name) + " at this node, including remotes?";
                }
                else
                {
                    message = "This is a destructive operation, are you sure you want to delete "
                              + in_quotes(d.ref_name) + "?";
                }
                break;
            case app::Dialog::Kind::DELETE_TAG:
                message = d.flag ? "This is a destructive operation, are you sure you want to delete "
                                           + in_quotes(d.ref_name) + " from your remote repos?"
                                 : "This is a destructive operation, are you sure you want to delete "
                                           + in_quotes(d.ref_name) + "?";
                break;
            default:
                message = "This is a destructive operation, are you sure you want to delete stash with message "
                          + in_quotes(d.text_b.empty() ? d.ref_name : d.text_b) + "?";
                break;
            }
            const int answer = draw_prompt_bar(message, label, danger);
            if (answer == 0)
            {
                return;
            }
            const app::Dialog request = d;
            d.close();
            if (answer < 0)
            {
                return;
            }
            git::Repository* repo = state != nullptr ? &state->repo : nullptr;
            switch (request.kind)
            {
            case app::Dialog::Kind::CONFIRM:
                if (request.on_confirm)
                {
                    request.on_confirm();
                }
                break;
            case app::Dialog::Kind::DISCARD_ALL:
                app.run(
                        *state, "Discard all",
                        [&]
                        {
                            return state->repo.discard_all();
                        },
                        "Changes discarded");
                break;
            case app::Dialog::Kind::DELETE_BRANCH:
                if (request.ref_remote.empty())
                {
                    const std::string name = request.ref_name;
                    const std::string upstream = request.text_a;
                    const git::Sha sha = request.sha;
                    if (app.run(
                                *state, "Delete branch",
                                [&]
                                {
                                    return state->repo.delete_branch(name);
                                },
                                "Deleted " + name))
                    {
                        state->push_undo({"delete branch " + name,
                                          [repo, name, sha]
                                          {
                                              return repo->set_branch_target(name, sha);
                                          },
                                          [repo, name]
                                          {
                                              return repo->delete_branch(name);
                                          }});
                        if (request.flag && !upstream.empty())
                        {
                            const auto slash = upstream.find('/');
                            if (slash != std::string::npos)
                            {
                                app.run_git_cli(
                                        *state, "Delete remote branch",
                                        {"push", upstream.substr(0, slash), "--delete", upstream.substr(slash + 1)});
                            }
                        }
                    }
                }
                else
                {
                    app.run_git_cli(*state, "Delete remote branch",
                                    {"push", request.ref_remote, "--delete", request.ref_name});
                }
                break;
            case app::Dialog::Kind::DELETE_TAG:
            {
                const std::string name = request.ref_name;
                const git::Sha sha = request.sha;
                const std::string remote = state->history.remotes.empty() ? "" : state->history.remotes.front().name;
                if (app.run(
                            *state, "Delete tag",
                            [&]
                            {
                                return state->repo.delete_tag(name);
                            },
                            "Deleted tag " + name))
                {
                    state->push_undo({"delete tag " + name,
                                      [repo, name, sha]
                                      {
                                          return repo->create_tag(name, sha, "");
                                      },
                                      [repo, name]
                                      {
                                          return repo->delete_tag(name);
                                      }});
                    if (request.flag && !remote.empty())
                    {
                        app.run_git_cli(*state, "Delete remote tag", {"push", remote, "--delete", "refs/tags/" + name});
                    }
                }
                break;
            }
            default:
            {
                // The stash is found again by its commit: one pushed since the prompt opened shifts the indexes.
                const int index = state->repo.stash_index_of(request.sha);
                if (index < 0)
                {
                    app.toasts.push(app::ToastKind::ERROR, "Delete stash failed", "The stash is gone.");
                    break;
                }
                app.run(
                        *state, "Delete stash",
                        [&]
                        {
                            return state->repo.stash_drop(index);
                        },
                        "Stash deleted");
                break;
            }
            }
            return;
        }
        // Resets run straight away.
        if (d.kind == app::Dialog::Kind::RESET)
        {
            const app::Dialog request = d;
            d.close();
            git::Repository* repo = &state->repo;
            const char* mode = request.reset_mode == git::ResetMode::SOFT
                                       ? "Soft"
                                       : (request.reset_mode == git::ResetMode::MIXED ? "Mixed" : "Hard");
            const git::Sha sha = request.sha;
            const git::ResetMode reset_mode = request.reset_mode;
            const git::Sha previous = state->history.head_sha;
            if (app.run(
                        *state, "Reset",
                        [&]
                        {
                            return state->repo.reset(sha, reset_mode);
                        },
                        std::string(mode) + " reset to " + git::short_sha(sha)))
            {
                const git::ResetMode undo_mode =
                        reset_mode == git::ResetMode::HARD ? git::ResetMode::MIXED : reset_mode;
                state->push_undo({"reset",
                                  [repo, previous, undo_mode]
                                  {
                                      return repo->reset(previous, undo_mode);
                                  },
                                  [repo, sha, reset_mode]
                                  {
                                      return repo->reset(sha, reset_mode);
                                  }});
            }
            return;
        }
        // Branches and lightweight tags are named inline in the graph, like the ref zone input.
        if (state != nullptr
            && (d.kind == app::Dialog::Kind::CREATE_BRANCH || (d.kind == app::Dialog::Kind::CREATE_TAG && !d.flag)))
        {
            const git::Sha sha = d.sha.empty() ? state->history.head_sha : d.sha;
            if (const auto row = state->history.row_by_sha.find(sha); row != state->history.row_by_sha.end())
            {
                app::RepoState::InlineRef inline_ref;
                inline_ref.sha = sha;
                inline_ref.tag = d.kind == app::Dialog::Kind::CREATE_TAG;
                inline_ref.checkout = d.kind == app::Dialog::Kind::CREATE_BRANCH && d.flag;
                state->inline_ref = inline_ref;
                state->select_only(sha);
                state->scroll_to_row = row->second;
                d.close();
                return;
            }
        }
        const theme::Palette& p = theme::palette();
        const std::string title_text = d.kind == app::Dialog::Kind::CONFIRM ? d.title + "###confirm_dialog"
                                                                            : std::string(dialog_title(d.kind));
        const char* title = title_text.c_str();
        if (d.just_opened)
        {
            ImGui::OpenPopup(title);
            d.just_opened = false;
        }
        const float fade = smooth("##dialog_fade", true, 0.15f);
        const ImGuiViewport* viewport = ImGui::GetMainViewport();
        ImGui::SetNextWindowPos(
                ImVec2(viewport->Pos.x + viewport->Size.x / 2.f, viewport->Pos.y + viewport->Size.y / 2.f),
                ImGuiCond_Appearing, ImVec2(0.5f, 0.5f));
        ImGui::SetNextWindowSize(ImVec2(px(dialog_width(d.kind)), 0.f));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(px(20.f), px(16.f)));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding, px(6.f));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 1.f);
        ImGui::PushStyleVar(ImGuiStyleVar_Alpha, fade);
        ImGui::PushStyleColor(ImGuiCol_PopupBg, p.panel_bg1);
        ImGui::PushStyleColor(ImGuiCol_TitleBgActive, p.panel_bg1);
        bool open = true;
        const bool visible = ImGui::BeginPopupModal(title, &open, ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoMove);
        ImGui::PopStyleColor(2);
        ImGui::PopStyleVar(4);
        if (!visible)
        {
            d.close();
            return;
        }
        ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(8.f), px(6.f)));
        git::Repository* repo = state != nullptr ? &state->repo : nullptr;

        switch (d.kind)
        {
        case app::Dialog::Kind::CLONE_REPO:
        {
            secondary_text("URL");
            input_text("##clone_url", d.text_a, "https://github.com/owner/repo.git");
            ImGui::Dummy(ImVec2(0.f, px(4.f)));
            secondary_text("Where to clone to");
            path_field("##clone_dest", d.text_b, "C:\\projects");
            error_text(d.error);
            // While the clone runs the dialog shows its progress (d.flag: it was seen running here, so the dialog
            // closes when it is done); Hide leaves it running and the toasts report the outcome.
            const bool busy = app.pending_clone.has_value();
            if (busy)
            {
                d.flag = true;
                const git::cli::CloneProgress progress = app.clone_progress();
                ImGui::Dummy(ImVec2(0.f, px(8.f)));
                secondary_text(progress.line.empty() ? "Cloning..." : progress.line.c_str());
                ImGui::Dummy(ImVec2(0.f, px(4.f)));
                progress_bar(progress.fraction);
                keep_animating();
                if (buttons("Hide", true, false, false, false))
                {
                    ImGui::CloseCurrentPopup();
                }
                break;
            }
            if (d.flag)
            {
                ImGui::CloseCurrentPopup();
                break;
            }
            if (buttons("Clone", !d.text_a.empty() && !d.text_b.empty(), false, true))
            {
                std::string name = d.text_a;
                while (!name.empty() && (name.back() == '/' || name.back() == '\\'))
                {
                    name.pop_back();
                }
                if (name.ends_with(".git"))
                {
                    name.resize(name.size() - 4);
                }
                const auto slash = name.find_last_of("/\\:");
                if (slash != std::string::npos)
                {
                    name = name.substr(slash + 1);
                }
                const std::string destination = (std::filesystem::path(d.text_b) / name).string();
                const std::string url = d.text_a;
                const std::string parent = d.text_b;
                // git creates the repository folder, not the folder it runs in.
                std::error_code created;
                std::filesystem::create_directories(parent, created);
                app.start_clone(parent, url, destination);
                app.toasts.push(app::ToastKind::INFO, "Cloning " + name, "This may take a while.");
            }
            break;
        }
        case app::Dialog::Kind::INIT_REPO:
        {
            secondary_text("New repository folder");
            path_field("##init_path", d.text_a, "C:\\projects\\new-repo");
            error_text(d.error);
            if (buttons("Create Repository", !d.text_a.empty(), false, true))
            {
                std::error_code ec;
                std::filesystem::create_directories(d.text_a, ec);
                if (auto created = git::Repository::init(d.text_a))
                {
                    app.open_repository(d.text_a);
                    ImGui::CloseCurrentPopup();
                }
                else
                {
                    d.error = created.error().message;
                }
            }
            break;
        }
        case app::Dialog::Kind::CREATE_BRANCH:
        {
            secondary_text(("Branch at " + git::short_sha(d.sha)).c_str());
            if (ImGui::IsWindowAppearing())
            {
                ImGui::SetKeyboardFocusHere();
            }
            const bool submitted =
                    input_text("##branch_name", d.text_a, "Branch name", ImGuiInputTextFlags_EnterReturnsTrue);
            checkbox("Checkout after create", &d.flag);
            error_text(d.error);
            if (buttons("Create Branch", !d.text_a.empty(), false, false) || submitted)
            {
                create_branch_at(app, *state, d.text_a, d.sha, d.flag);
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::CREATE_TAG:
        {
            secondary_text(("Tag at " + git::short_sha(d.sha)).c_str());
            if (ImGui::IsWindowAppearing())
            {
                ImGui::SetKeyboardFocusHere();
            }
            const bool submitted = input_text("##tag_name", d.text_a, "Tag name", ImGuiInputTextFlags_EnterReturnsTrue);
            checkbox("Annotated tag", &d.flag);
            if (d.flag)
            {
                input_multiline("##tag_message", d.text_b, "Tag message", ImVec2(-1.f, px(60.f)));
            }
            if (buttons("Create Tag", !d.text_a.empty() && (!d.flag || !d.text_b.empty()), false, false) || submitted)
            {
                create_tag_at(app, *state, d.text_a, d.sha, d.flag ? d.text_b : "");
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::DELETE_BRANCH:
        {
            if (d.ref_remote.empty())
            {
                ImGui::TextWrapped("Delete local branch %s?", d.ref_name.c_str());
                if (!d.text_a.empty())
                {
                    checkbox(("Also delete " + d.text_a + " on the remote").c_str(), &d.flag);
                }
            }
            else
            {
                ImGui::TextWrapped("Delete %s/%s on the remote? This cannot be undone from here.", d.ref_remote.c_str(),
                                   d.ref_name.c_str());
            }
            if (buttons("Delete", true, true, true))
            {
                if (d.ref_remote.empty())
                {
                    const std::string name = d.ref_name;
                    const std::string upstream = d.text_a;
                    const git::Sha sha = d.sha;
                    const bool also_remote = d.flag;
                    if (app.run(
                                *state, "Delete branch",
                                [&]
                                {
                                    return state->repo.delete_branch(name);
                                },
                                "Deleted " + name))
                    {
                        state->push_undo({"delete branch " + name,
                                          [repo, name, sha]
                                          {
                                              return repo->set_branch_target(name, sha);
                                          },
                                          [repo, name]
                                          {
                                              return repo->delete_branch(name);
                                          }});
                        if (also_remote && !upstream.empty())
                        {
                            const auto slash = upstream.find('/');
                            if (slash != std::string::npos)
                            {
                                app.run_git_cli(
                                        *state, "Delete remote branch",
                                        {"push", upstream.substr(0, slash), "--delete", upstream.substr(slash + 1)});
                            }
                        }
                    }
                }
                else
                {
                    app.run_git_cli(*state, "Delete remote branch", {"push", d.ref_remote, "--delete", d.ref_name});
                }
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::DELETE_TAG:
        {
            ImGui::TextWrapped("Delete tag %s locally?", d.ref_name.c_str());
            if (!state->history.remotes.empty())
            {
                checkbox(("Also delete from " + state->history.remotes.front().name).c_str(), &d.flag);
            }
            if (buttons("Delete", true, true, true))
            {
                const std::string name = d.ref_name;
                const git::Sha sha = d.sha;
                const bool also_remote = d.flag;
                const std::string remote = state->history.remotes.empty() ? "" : state->history.remotes.front().name;
                if (app.run(
                            *state, "Delete tag",
                            [&]
                            {
                                return state->repo.delete_tag(name);
                            },
                            "Deleted tag " + name))
                {
                    state->push_undo({"delete tag " + name,
                                      [repo, name, sha]
                                      {
                                          return repo->create_tag(name, sha, "");
                                      },
                                      [repo, name]
                                      {
                                          return repo->delete_tag(name);
                                      }});
                    if (also_remote && !remote.empty())
                    {
                        app.run_git_cli(*state, "Delete remote tag", {"push", remote, "--delete", "refs/tags/" + name});
                    }
                }
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::DELETE_STASH:
        {
            ImGui::TextWrapped("Delete %s? Its changes will be lost.", d.ref_name.c_str());
            if (buttons("Delete", true, true, true))
            {
                // The stash is found again by its commit: one pushed since the dialog opened shifts the indexes.
                const int index = state->repo.stash_index_of(d.sha);
                if (index < 0)
                {
                    app.toasts.push(app::ToastKind::ERROR, "Delete stash failed", "The stash is gone.");
                }
                else
                {
                    app.run(
                            *state, "Delete stash",
                            [&]
                            {
                                return state->repo.stash_drop(index);
                            },
                            "Stash deleted");
                }
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::DISCARD_ALL:
        {
            ImGui::TextWrapped("Discard all unstaged changes in the working directory? Untracked files are deleted. "
                               "This cannot be undone.");
            if (buttons("Discard", true, true, true))
            {
                app.run(
                        *state, "Discard all",
                        [&]
                        {
                            return state->repo.discard_all();
                        },
                        "Changes discarded");
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::RESET:
        {
            const char* mode = d.reset_mode == git::ResetMode::SOFT
                                       ? "Soft"
                                       : (d.reset_mode == git::ResetMode::MIXED ? "Mixed" : "Hard");
            ImGui::TextWrapped("%s reset %s to %s?", mode, d.ref_name.c_str(), git::short_sha(d.sha).c_str());
            if (d.reset_mode == git::ResetMode::HARD)
            {
                ImGui::PushStyleColor(ImGuiCol_Text, p.orange);
                ImGui::TextWrapped("All uncommitted changes will be discarded.");
                ImGui::PopStyleColor();
            }
            if (buttons("Reset", true, d.reset_mode == git::ResetMode::HARD, true))
            {
                const git::Sha sha = d.sha;
                const git::ResetMode reset_mode = d.reset_mode;
                const git::Sha previous = state->history.head_sha;
                if (app.run(
                            *state, "Reset",
                            [&]
                            {
                                return state->repo.reset(sha, reset_mode);
                            },
                            std::string(mode) + " reset to " + git::short_sha(sha)))
                {
                    const git::ResetMode undo_mode =
                            reset_mode == git::ResetMode::HARD ? git::ResetMode::MIXED : reset_mode;
                    state->push_undo({"reset",
                                      [repo, previous, undo_mode]
                                      {
                                          return repo->reset(previous, undo_mode);
                                      },
                                      [repo, sha, reset_mode]
                                      {
                                          return repo->reset(sha, reset_mode);
                                      }});
                }
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::ADD_REMOTE:
        {
            secondary_text("Name");
            if (ImGui::IsWindowAppearing())
            {
                ImGui::SetKeyboardFocusHere();
            }
            input_text("##remote_name", d.text_a, "origin");
            secondary_text("URL");
            input_text("##remote_url", d.text_b, "https://github.com/owner/repo.git");
            error_text(d.error);
            if (buttons("Add Remote", !d.text_a.empty() && !d.text_b.empty(), false, true))
            {
                const std::string name = d.text_a;
                const std::string url = d.text_b;
                if (app.run(
                            *state, "Add remote",
                            [&]
                            {
                                return state->repo.add_remote(name, url);
                            },
                            "Added remote " + name))
                {
                    app.run_git_cli(*state, "Fetch", {"fetch", name});
                    ImGui::CloseCurrentPopup();
                }
            }
            break;
        }
        case app::Dialog::Kind::EDIT_REMOTE:
        {
            secondary_text("Name");
            ImGui::BeginDisabled();
            input_text("##remote_name", d.text_a);
            ImGui::EndDisabled();
            secondary_text("Pull URL");
            input_text("##remote_url", d.text_b, "https://github.com/owner/repo.git");
            ImGui::Dummy(ImVec2(0.f, px(6.f)));
            if (danger_button("Remove remote", ImVec2(px(140.f), px(28.f))))
            {
                const std::string name = d.ref_name;
                app.run(
                        *state, "Remove remote",
                        [&]
                        {
                            return state->repo.remove_remote(name);
                        },
                        "Removed remote " + name);
                ImGui::CloseCurrentPopup();
            }
            if (buttons("Save", !d.text_b.empty(), false, true))
            {
                const std::string name = d.ref_name;
                const std::string url = d.text_b;
                app.run(
                        *state, "Edit remote",
                        [&]
                        {
                            return state->repo.set_remote_url(name, url);
                        },
                        "Updated remote " + name);
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::REWORD_COMMIT:
        {
            secondary_text(("Commit " + git::short_sha(d.sha)).c_str());
            if (ImGui::IsWindowAppearing())
            {
                ImGui::SetKeyboardFocusHere();
            }
            input_multiline("##message", d.text_a, "Commit message", ImVec2(-1.f, px(140.f)));
            if (buttons("Update Message", !d.text_a.empty(), false, false))
            {
                const git::Sha sha = d.sha;
                const std::string message = d.text_a;
                app::UndoEntry undo = tip_restore_entry(*state, "edit commit message");
                bool ok;
                if (sha == state->history.head_sha)
                {
                    ok = app.run(
                            *state, "Edit commit message",
                            [&]
                            {
                                return state->repo.reword_head(message);
                            },
                            "Commit message updated");
                }
                else
                {
                    const git::Commit* commit = state->commit_for(sha);
                    std::vector<git::RebaseStep> steps;
                    const git::Sha base = commit != nullptr && !commit->parents.empty() ? commit->parents.front() : "";
                    for (const git::Commit* c : state->commits_above(base))
                    {
                        steps.push_back({c->sha, c->sha == sha ? git::RebaseAction::REWORD : git::RebaseAction::PICK,
                                         c->sha == sha ? message : ""});
                    }
                    ok = app.run(
                            *state, "Edit commit message",
                            [&]
                            {
                                return state->repo.rewrite_history(base, steps);
                            },
                            "Commit message updated");
                }
                if (ok)
                {
                    state->push_undo(std::move(undo));
                }
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::SQUASH_COMMITS:
        {
            ImGui::TextWrapped("Squash %zu commits into one. Commit message:", d.shas.size());
            input_multiline("##squash_message", d.text_a, "Commit message", ImVec2(-1.f, px(160.f)));
            if (buttons("Squash", !d.text_a.empty() && !d.shas.empty(), false, false))
            {
                const git::Commit* oldest = state->commit_for(d.shas.front());
                if (oldest != nullptr && !oldest->parents.empty())
                {
                    const git::Sha base = oldest->parents.front();
                    std::vector<git::RebaseStep> steps;
                    bool first = true;
                    for (const git::Commit* c : state->commits_above(base))
                    {
                        const bool selected = std::find(d.shas.begin(), d.shas.end(), c->sha) != d.shas.end();
                        if (selected && first)
                        {
                            steps.push_back({c->sha, git::RebaseAction::REWORD, d.text_a});
                            first = false;
                        }
                        else if (selected)
                        {
                            steps.push_back({c->sha, git::RebaseAction::SQUASH, d.text_a});
                        }
                        else
                        {
                            steps.push_back({c->sha, git::RebaseAction::PICK, ""});
                        }
                    }
                    app::UndoEntry undo = tip_restore_entry(*state, "squash");
                    if (app.run(
                                *state, "Squash",
                                [&]
                                {
                                    return state->repo.rewrite_history(base, steps);
                                },
                                "Squashed " + std::to_string(d.shas.size()) + " commits"))
                    {
                        state->push_undo(std::move(undo));
                    }
                }
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::SET_UPSTREAM:
        {
            secondary_text(("Upstream of " + d.ref_name).c_str());
            if (ImGui::BeginCombo("##upstream", d.text_a.empty() ? "(none)" : d.text_a.c_str()))
            {
                if (ImGui::Selectable("(none)", d.text_a.empty()))
                {
                    d.text_a.clear();
                }
                for (const git::Ref& ref : state->history.refs)
                {
                    if (ref.type == git::RefType::REMOTE_BRANCH
                        && ImGui::Selectable(ref.name.c_str(), ref.name == d.text_a))
                    {
                        d.text_a = ref.name;
                    }
                }
                ImGui::EndCombo();
            }
            if (buttons("Set Upstream", true, false, true))
            {
                const std::string name = d.ref_name;
                const std::string upstream = d.text_a;
                app.run(
                        *state, "Set upstream",
                        [&]
                        {
                            return state->repo.set_upstream(name, upstream);
                        },
                        upstream.empty() ? "Upstream cleared" : "Upstream set to " + upstream);
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::PREFERENCES:
        {
            preferences_body(app);
            if (buttons("Done", true, false, false, false))
            {
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::KEYBOARD_SHORTCUTS:
        {
            push_font(fonts().regular, theme::FONT_SIZE_ROW);
            if (ImGui::BeginTable("##shortcuts", 2, ImGuiTableFlags_RowBg | ImGuiTableFlags_SizingStretchProp))
            {
                shortcut_row("Ctrl + P", "Toggle Command Palette");
                shortcut_row("Ctrl + O", "Open Repo");
                shortcut_row("Ctrl + T", "Open a new tab");
                shortcut_row("Ctrl + W", "Close File/Tab");
                shortcut_row("Ctrl + Tab", "Jump to the next open tab");
                shortcut_row("Ctrl + 1..9", "Jump to a specific tab");
                shortcut_row("Ctrl + Shift + A", "Open tabs list");
                shortcut_row("Ctrl + F", "Search commits");
                shortcut_row("Ctrl + L", "Fetch All");
                shortcut_row("Ctrl + B", "Create Branch");
                shortcut_row("Ctrl + Z / Ctrl + Y", "Undo / Redo");
                shortcut_row("Ctrl + Shift + S", "Stage All Files");
                shortcut_row("Ctrl + Shift + U", "Unstage All Files");
                shortcut_row("Ctrl + Shift + M", "Focus commit message");
                shortcut_row("Ctrl + Enter", "Commit (when in message input)");
                shortcut_row("Ctrl + J", "Toggle Left Panel");
                shortcut_row("Ctrl + K", "Toggle Commit Details Panel");
                shortcut_row("Ctrl + `", "Toggle Terminal Panel");
                shortcut_row("Ctrl + = / Ctrl + -", "Increase / Decrease Zoom");
                shortcut_row("Ctrl + 0", "Reset Zoom");
                shortcut_row("Up / Down, k / j", "Select Previous / Next Item");
                shortcut_row("Shift + Up / Down", "Extend the selection");
                shortcut_row("Home / End", "Select First / Last Item");
                shortcut_row("p / r / s / d", "Interactive rebase: pick / reword / squash / drop");
                shortcut_row("Esc", "Close View");
                shortcut_row("F5", "Reload Window");
                ImGui::EndTable();
            }
            ImGui::PopFont();
            if (buttons("Close", true, false, true, false))
            {
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::CONFIRM:
        {
            ImGui::TextWrapped("%s", d.message.c_str());
            if (buttons(d.confirm_label.empty() ? "OK" : d.confirm_label.c_str(), true, d.danger, true))
            {
                std::function<void()> action = std::move(d.on_confirm);
                ImGui::CloseCurrentPopup();
                if (action)
                {
                    action();
                }
            }
            break;
        }
        case app::Dialog::Kind::ABOUT:
        {
            push_font(fonts().semibold, 20.f);
            ImGui::TextUnformatted("ImGit Desktop");
            ImGui::PopFont();
            secondary_text("A Git client built on Dear ImGui and libgit2.");
            ImGui::Dummy(ImVec2(0.f, px(6.f)));
            ImGui::TextUnformatted("ImGit uses some fine open source projects:");
            ImGui::BulletText("Dear ImGui (MIT)");
            ImGui::BulletText("libgit2 (GPLv2 with linking exception)");
            ImGui::BulletText("GLFW (zlib)");
            ImGui::BulletText("Open Sans (SIL Open Font License)");
            ImGui::BulletText("Font Awesome Free icons (CC BY 4.0)");
            if (buttons("Close", true, false, true, false))
            {
                ImGui::CloseCurrentPopup();
            }
            break;
        }
        case app::Dialog::Kind::NONE:
        case app::Dialog::Kind::RENAME_BRANCH: // the prompt bar
        case app::Dialog::Kind::PUSH_NEW_BRANCH:
        case app::Dialog::Kind::ADD_SUBMODULE: // the slide panel
        case app::Dialog::Kind::EDIT_SUBMODULE:
            break;
        }
        ImGui::PopStyleVar();
        ImGui::EndPopup();
        if (!ImGui::IsPopupOpen(title))
        {
            d.close();
        }
    }
} // namespace imgit::ui
