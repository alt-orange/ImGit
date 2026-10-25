#include "app/application.hpp"
#include "app/theme.hpp"
#include "platform/shell.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cctype>
#include <chrono>
#include <cmath>
#include <fstream>
#include <future>
#include <imgui.h>
#include <imgui_stdlib.h>
#include <map>
#include <memory>
#include <set>
#include <string>
#include <vector>

namespace imgit::ui
{
    namespace
    {
        // Branch / ahead-behind / WIP summaries, read in the background once per repository.
        struct SummaryEntry
        {
            std::shared_future<std::optional<git::RepoSummary>> future;
        };
        std::map<std::string, SummaryEntry> g_summaries;

        const git::RepoSummary* summary_for_impl(const std::string& path)
        {
            auto found = g_summaries.find(path);
            if (found == g_summaries.end())
            {
                std::shared_future<std::optional<git::RepoSummary>> future =
                        std::async(std::launch::async,
                                   [path]() -> std::optional<git::RepoSummary>
                                   {
                                       auto repo = git::Repository::open(path);
                                       if (!repo)
                                       {
                                           return std::nullopt;
                                       }
                                       auto summary = repo->summary();
                                       if (!summary)
                                       {
                                           return std::nullopt;
                                       }
                                       return *summary;
                                   })
                                .share();
                found = g_summaries.emplace(path, SummaryEntry{std::move(future)}).first;
            }
            if (found->second.future.wait_for(std::chrono::seconds(0)) != std::future_status::ready)
            {
                return nullptr;
            }
            const std::optional<git::RepoSummary>& value = found->second.future.get();
            return value ? &*value : nullptr;
        }

        std::string normalized(std::string path)
        {
            std::replace(path.begin(), path.end(), '\\', '/');
            while (!path.empty() && path.back() == '/')
            {
                path.pop_back();
            }
            return path;
        }

        std::string repo_name(const std::string& path)
        {
            const std::string clean = normalized(path);
            const auto slash = clean.find_last_of('/');
            return slash == std::string::npos ? clean : clean.substr(slash + 1);
        }

        std::set<std::string> load_favorites(app::Application& app)
        {
            std::set<std::string> result;
            std::ifstream in(app.config_path("favorites.txt"));
            std::string line;
            while (std::getline(in, line))
            {
                if (!line.empty())
                {
                    result.insert(line);
                }
            }
            return result;
        }

        void save_favorites(app::Application& app, const std::set<std::string>& favorites)
        {
            std::ofstream out(app.config_path("favorites.txt"));
            for (const std::string& path : favorites)
            {
                out << path << '\n';
            }
        }

        float css_y(float size, float box_top)
        {
            const float ascent = size * 2189.f / 2048.f * dpi_scale();
            return std::round(box_top + std::round(ascent)) - std::ceil(ascent);
        }
    } // namespace

    const git::RepoSummary* repo_summary(const std::string& path)
    {
        return summary_for_impl(path);
    }

    // Repository Management: the title, the Browse / Clone / Init buttons, Collapse / Expand, the search box
    // and "WIP summary", then the Open /
    // Favorites / Recent / All sections of 34px rows (star, checkbox, name, owner, branch / ahead / WIP pills and
    // the row actions).
    void draw_repo_management(app::Application& app, ImVec2 size)
    {
        const theme::Palette& p = theme::palette();
        ImGui::BeginChild("##repo_management", size);
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        ImDrawList* draw = ImGui::GetWindowDrawList();
        draw->AddRectFilled(origin,
                            ImVec2(origin.x + size.x, origin.y + std::max(size.y, ImGui::GetScrollMaxY() + size.y)),
                            theme::u32(p.app_bg0));
        static std::string search;
        static bool wip_summary = true;
        static std::set<std::string> collapsed;
        static std::set<std::string> favorites;
        static std::string favorites_for; // the profile `favorites` was loaded from
        if (favorites_for != app.profiles.current().guid)
        {
            favorites_for = app.profiles.current().guid;
            favorites = load_favorites(app);
        }
        if (ImGui::IsKeyPressed(ImGuiKey_Escape))
        {
            app.repo_management_open = false;
        }
        const float left = origin.x + px(45.f);
        const float right = origin.x + size.x - px(45.f);

        draw_text(draw, fonts().semibold, 16.f, ImVec2(left, css_y(16.f, origin.y + px(17.f))),
                  theme::u32(p.text_selected), "Repository Management");

        // Toolbar buttons: 12px captions after a 12px fa-fw icon, 6px 12px padding, 5px apart.
        {
            struct Button
            {
                const char* id;
                const char* icon;
                const char* label;
                bool primary;
            };
            const Button buttons[] = {
                    {"##rm_browse", "far:folder-open", "Browse", true},
                    {"##rm_clone", "far:cloud-download", "Clone", true},
                    {"##rm_init", "far:plus", "Init", true},
            };
            float x = left;
            const float y = origin.y + px(47.6f);
            for (const Button& button : buttons)
            {
                const float w = px(31.f) + text_size(fonts().regular, theme::FONT_SIZE_ROW, button.label).x + px(18.f);
                const ImVec2 bmin(x, y);
                const ImVec2 bmax(x + w, y + px(31.1f));
                ImGui::SetCursorScreenPos(bmin);
                const bool clicked = ImGui::InvisibleButton(button.id, ImVec2(w, bmax.y - bmin.y));
                const bool hovered = ImGui::IsItemHovered();
                if (button.primary)
                {
                    draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.accent, hovered ? 0.3f : 0.2f)),
                                        px(2.f));
                    draw->AddRect(bmin, bmax, theme::u32(p.accent), px(2.f), 0, 1.f);
                }
                else
                {
                    if (hovered)
                    {
                        draw->AddRectFilled(bmin, bmax, theme::u32(theme::with_alpha(p.white, 0.1f)), px(2.f));
                    }
                    draw->AddRect(bmin, bmax, theme::u32(p.text_normal), px(2.f), 0, 1.f);
                }
                draw_fa_icon(draw, button.icon, ImVec2(x + px(19.5f), y + px(15.5f)), px(12.f),
                             theme::u32(p.text_normal));
                draw_text(draw, fonts().regular, theme::FONT_SIZE_ROW, ImVec2(x + px(31.f), css_y(12.f, y + px(7.f))),
                          theme::u32(p.text_normal), button.label);
                if (clicked)
                {
                    const std::string id = button.id;
                    if (id == "##rm_browse")
                    {
                        app.browse_for_repository();
                    }
                    else if (id == "##rm_clone")
                    {
                        app.dialog.open(app::Dialog::Kind::CLONE_REPO);
                    }
                    else
                    {
                        app.dialog.open(app::Dialog::Kind::INIT_REPO);
                    }
                }
                x += w + px(5.f);
            }
        }

        // Collapse / Expand, the search box, WIP summary.
        std::vector<std::string> section_names = {"Open repositories", "Favorites", "Recent repositories",
                                                  "All repositories"};
        {
            const float y = origin.y + px(88.7f);
            const auto collapse_button = [&](const char* id, const char* label, float x, bool collapse) -> float
            {
                const float w = px(25.25f) + text_size(fonts().regular, 14.f, label).x + px(8.f);
                ImGui::SetCursorScreenPos(ImVec2(x, y));
                const bool clicked = ImGui::InvisibleButton(id, ImVec2(w, px(36.f)));
                const ImU32 color = theme::u32(ImGui::IsItemHovered() ? p.text_selected : p.text_normal);
                draw_fa_icon(draw, collapse ? "far:chevron-down" : "far:chevron-up",
                             ImVec2(x + px(14.15f), y + px(13.f)), px(14.f) * 10.f / 12.25f, color);
                draw_fa_icon(draw, collapse ? "far:chevron-up" : "far:chevron-down",
                             ImVec2(x + px(14.15f), y + px(23.f)), px(14.f) * 10.f / 12.25f, color);
                draw_text(draw, fonts().regular, 14.f, ImVec2(x + px(25.25f), css_y(14.f, y + px(8.03f))), color,
                          label);
                if (clicked)
                {
                    collapsed.clear();
                    if (collapse)
                    {
                        collapsed.insert(section_names.begin(), section_names.end());
                    }
                }
                return w;
            };
            const float cw = collapse_button("##rm_collapse", "Collapse", left, true);
            collapse_button("##rm_expand", "Expand", left + cw + px(4.f), false);

            const char* wip_label = "WIP summary";
            const float label_w = text_size(fonts().regular, 14.f, wip_label).x;
            const float box_x = right - label_w - px(20.f);
            const float search_x = origin.x + px(231.5f);
            const float search_right = box_x - px(12.f);
            const ImVec2 smin(search_x, y + px(1.f));
            const ImVec2 smax(search_right, smin.y + px(34.f));
            draw->AddRectFilled(smin, smax, theme::u32(p.input_bg), px(4.f));
            ImGui::SetCursorScreenPos(ImVec2(smin.x + px(40.f), smin.y));
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding,
                                ImVec2(px(1.f), (px(34.f) - text_size(fonts().regular, 14.f, "A").y) / 2.f));
            ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
            ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgActive, ImVec4(0.f, 0.f, 0.f, 0.f));
            push_font(fonts().regular, 14.f);
            if (ImGui::IsWindowAppearing())
            {
                ImGui::SetKeyboardFocusHere();
            }
            input_text("##rm_search", search, "Search repositories", 0, smax.x - smin.x - px(50.f));
            const bool focused = ImGui::IsItemActive();
            ImGui::PopFont();
            ImGui::PopStyleColor(3);
            ImGui::PopStyleVar(2);
            draw->AddRect(smin, smax, focused ? theme::u32(p.accent) : theme::u32(p.section_border), px(4.f), 0,
                          1.f);
            draw_fa_icon(draw, "far:search", ImVec2(smin.x + px(17.f), smin.y + px(17.f)), px(14.f),
                         theme::u32(p.text_normal));

            ImGui::SetCursorScreenPos(ImVec2(box_x, y));
            if (ImGui::InvisibleButton("##rm_wip", ImVec2(px(20.f) + label_w, px(36.f))))
            {
                wip_summary = !wip_summary;
            }
            const ImVec2 cmin(box_x, y + px(12.f));
            const ImVec2 cmax(cmin.x + px(12.f), cmin.y + px(12.f));
            if (wip_summary)
            {
                draw->AddRectFilled(cmin, cmax, theme::u32(p.link), px(1.f));
                draw_fa_icon(draw, "fas:check", ImVec2(cmin.x + px(6.f), cmin.y + px(6.f)), px(9.f),
                             IM_COL32(34, 34, 34, 255));
            }
            else
            {
                draw->AddRectFilled(cmin, cmax, theme::u32(p.input_bg), px(1.f));
                draw->AddRect(cmin, cmax, theme::u32(p.text_disabled), px(1.f), 0, 1.f);
            }
            draw_text(draw, fonts().regular, 14.f, ImVec2(box_x + px(20.f), css_y(14.f, y + px(8.03f))),
                      theme::u32(p.text_normal), wip_label);
        }

        // The repositories of each section.
        std::vector<std::string> open_paths;
        for (const auto& tab : app.tabs)
        {
            open_paths.push_back(tab->repo.workdir());
        }
        std::vector<std::string> recent = app.recent_repos;
        std::vector<std::string> favorite_paths(favorites.begin(), favorites.end());
        std::vector<std::string> all;
        {
            std::set<std::string> seen;
            for (const auto* list : {&open_paths, &recent, &favorite_paths})
            {
                for (const std::string& path : *list)
                {
                    if (seen.insert(normalized(path)).second)
                    {
                        all.push_back(path);
                    }
                }
            }
            std::sort(all.begin(), all.end(),
                      [](const std::string& a, const std::string& b)
                      {
                          const auto lower = [](char c)
                          {
                              return static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
                          };
                          std::string la = repo_name(a), lb = repo_name(b);
                          std::transform(la.begin(), la.end(), la.begin(), lower);
                          std::transform(lb.begin(), lb.end(), lb.begin(), lower);
                          return la < lb;
                      });
        }
        const auto filtered = [&](const std::vector<std::string>& paths)
        {
            std::vector<std::string> result;
            for (const std::string& path : paths)
            {
                if (search.empty() || contains_ci(repo_name(path), search) || contains_ci(path, search))
                {
                    result.push_back(path);
                }
            }
            return result;
        };
        struct Section
        {
            std::string name;
            std::vector<std::string> paths;
            bool open_section;
        };
        const std::vector<Section> sections = {
                {section_names[0], filtered(open_paths), true},
                {section_names[1], filtered(favorite_paths), false},
                {section_names[2], filtered(recent), false},
                {section_names[3], filtered(all), false},
        };

        float y = origin.y + px(144.f);
        std::optional<std::string> open_request;
        std::optional<std::string> close_request;
        std::optional<std::string> remove_recent;
        for (const Section& section : sections)
        {
            const bool is_collapsed = collapsed.contains(section.name);
            // .collapsible-panel-header: 5% white, radius 2.5, a 16px chevron, the 14px semibold title and a pill
            // count.
            const ImVec2 hmin(left + px(3.f), y);
            const ImVec2 hmax(right - px(10.f), y + px(44.f));
            draw->AddRectFilled(hmin, hmax, IM_COL32(255, 255, 255, 13), px(2.5f));
            ImGui::SetCursorScreenPos(ImVec2(hmin.x + px(11.f), y + px(11.f)));
            ImGui::PushID(section.name.c_str());
            if (ImGui::InvisibleButton("##toggle", ImVec2(px(28.f), px(22.f))))
            {
                if (!collapsed.erase(section.name))
                {
                    collapsed.insert(section.name);
                }
            }
            draw_fa_icon(draw, "far:chevron-right", ImVec2(hmin.x + px(25.f), y + px(20.8f)), px(16.f),
                         theme::u32(p.text_normal), is_collapsed ? 0.f : 90.f);
            draw_text(draw, fonts().semibold, 14.f, ImVec2(hmin.x + px(47.f), css_y(14.f, y + px(12.f))),
                      theme::u32(p.text_normal), section.name.c_str());
            {
                const std::string count = std::to_string(section.paths.size());
                const float title_w = text_size(fonts().semibold, 14.f, section.name.c_str()).x;
                const float bx = hmin.x + px(47.f) + title_w + px(11.f);
                const float bw = text_size(fonts().regular, 12.f, count.c_str()).x + px(12.f);
                draw->AddRectFilled(ImVec2(bx, y + px(11.4f)), ImVec2(bx + bw, y + px(32.5f)), theme::u32(p.panel_bg1),
                                    px(11.f));
                draw_text(draw, fonts().regular, 12.f, ImVec2(bx + px(6.f), css_y(12.f, y + px(13.4f))),
                          theme::u32(p.text_secondary), count.c_str());
            }
            // Section tools: "Close all tabs" / "Remove all", then the 24px icon buttons.
            {
                float tx = hmax.x - px(13.f) - px(24.f);
                ImGui::SetCursorScreenPos(ImVec2(tx, y + px(10.f)));
                if (ImGui::InvisibleButton("##menu", ImVec2(px(24.f), px(24.f))))
                {
                    app.toasts.push(app::ToastKind::INFO, section.name,
                                    "Section options are not available in ImGit yet.");
                }
                draw_fa_icon(draw, "far:ellipsis-v", ImVec2(tx + px(12.f), y + px(22.f)), px(16.f),
                             theme::u32(ImGui::IsItemHovered() ? p.text_selected : p.text_normal));
                if (section.name != "All repositories")
                {
                    tx -= px(34.f);
                    draw_fa_icon(draw, "far:external-link-alt", ImVec2(tx + px(12.f), y + px(22.f)), px(16.f),
                                 theme::u32(section.paths.empty() ? p.text_disabled : p.text_normal));
                    tx -= px(45.f);
                    draw_fa_icon(draw, "fas:reply", ImVec2(tx + px(8.f), y + px(22.f)), px(16.f),
                                 theme::u32(p.text_secondary), 315.f);
                    draw_fa_icon(draw, "fas:caret-down", ImVec2(tx + px(24.f), y + px(22.f)), px(14.f),
                                 theme::u32(p.text_secondary));
                    tx -= px(39.f);
                    draw_fa_icon(draw, "far:reply", ImVec2(tx + px(12.f), y + px(22.f)), px(16.f),
                                 theme::u32(section.paths.empty() ? p.text_disabled : p.text_normal));
                    const char* bulk = section.open_section
                                               ? "Close all tabs"
                                               : (section.name == "Recent repositories" ? "Remove all" : nullptr);
                    if (bulk != nullptr)
                    {
                        const float bw = px(23.f) + text_size(fonts().regular, 12.f, bulk).x + px(5.f);
                        const float bx = tx - px(10.f) - bw;
                        ImGui::SetCursorScreenPos(ImVec2(bx, y + px(10.f)));
                        const bool clicked = ImGui::InvisibleButton("##bulk", ImVec2(bw, px(24.f)));
                        const ImU32 color = theme::u32(ImGui::IsItemHovered() ? p.text_selected : p.text_normal);
                        draw_fa_icon(draw, "far:times", ImVec2(bx + px(10.f), y + px(22.f)), px(16.f), color);
                        draw_text(draw, fonts().regular, 12.f, ImVec2(bx + px(25.f), css_y(12.f, y + px(13.42f))),
                                  color, bulk);
                        if (clicked)
                        {
                            if (section.open_section)
                            {
                                while (!app.tabs.empty())
                                {
                                    app.close_tab(0);
                                }
                            }
                            else
                            {
                                app.recent_repos.clear();
                                app.save_recent();
                            }
                        }
                    }
                }
            }
            ImGui::PopID();
            y += px(44.f);
            if (is_collapsed)
            {
                y += px(18.f);
                continue;
            }
            if (section.paths.empty())
            {
                draw_text(draw, fonts().italic != nullptr ? fonts().italic : fonts().regular, 14.f,
                          ImVec2(left + px(34.f), css_y(14.f, y + px(25.f))), theme::u32(p.text_normal), "No matches");
                y += px(64.f);
                continue;
            }
            y += px(15.f);
            for (const std::string& path : section.paths)
            {
                const float row_top = y;
                const float mid = row_top + px(17.f);
                ImGui::PushID((section.name + path).c_str());
                const ImVec2 mouse = ImGui::GetIO().MousePos;
                const bool row_hovered = ImGui::IsWindowHovered(ImGuiHoveredFlags_AllowWhenBlockedByActiveItem)
                                         && mouse.y >= row_top && mouse.y < row_top + px(34.f) && mouse.x >= left
                                         && mouse.x < right;
                if (row_hovered)
                {
                    draw->AddRectFilled(ImVec2(left, row_top), ImVec2(right, row_top + px(34.f)),
                                        theme::u32(p.hover_row));
                }
                // star
                const bool favorite = favorites.contains(path);
                ImGui::SetCursorScreenPos(ImVec2(left, row_top));
                if (ImGui::InvisibleButton("##star", ImVec2(px(35.f), px(34.f))))
                {
                    if (favorite)
                    {
                        favorites.erase(path);
                    }
                    else
                    {
                        favorites.insert(path);
                    }
                    save_favorites(app, favorites);
                }
                // .favorite-checkbox: hidden until the row is hovered unless checked.
                if (favorite || row_hovered)
                {
                    draw_fa_icon(draw, favorite ? "fas:star" : "far:star", ImVec2(left + px(18.75f), mid), px(14.f),
                                 theme::u32(favorite ? p.yellow : p.text_disabled));
                }
                // checkbox (multi-select)
                if (section.name != "All repositories")
                {
                    const ImVec2 cmin(left + px(45.f), mid - px(6.f));
                    draw->AddRectFilled(cmin, ImVec2(cmin.x + px(12.f), cmin.y + px(12.f)), theme::u32(p.input_bg),
                                        px(1.f));
                    draw->AddRect(cmin, ImVec2(cmin.x + px(12.f), cmin.y + px(12.f)), theme::u32(p.text_disabled),
                                  px(1.f), 0, 1.f);
                }
                // name, owner
                draw_text_ellipsis(draw, fonts().regular, 14.f, ImVec2(left + px(94.f), css_y(14.f, mid - px(10.f))),
                                   px(230.f), theme::u32(p.text_normal), repo_name(path).c_str());
                const git::RepoSummary* summary = repo_summary(path);
                if (summary != nullptr && !summary->owner.empty())
                {
                    draw_text_ellipsis(draw, fonts().regular, 14.f,
                                       ImVec2(left + px(333.f), css_y(14.f, mid - px(10.f))), px(230.f),
                                       theme::u32(p.text_normal), summary->owner.c_str());
                }
                // branch / ahead-behind / WIP pills (.panel-bg1, radius 2, 0 5px padding, 12px)
                if (summary != nullptr)
                {
                    float x = left + px(580.f);
                    const float top = mid - px(8.55f);
                    const auto pill = [&](float w)
                    {
                        draw->AddRectFilled(ImVec2(x, top), ImVec2(x + w, top + px(17.1f)), theme::u32(p.panel_bg1),
                                            px(2.f));
                    };
                    if (!summary->branch.empty())
                    {
                        const float tw = text_size(fonts().regular, 12.f, summary->branch.c_str()).x;
                        const float w = px(5.f + 7.9f + 5.f) + tw + px(5.f);
                        pill(w);
                        draw_fa_icon(draw, "far:code-branch", ImVec2(x + px(8.95f), mid), px(10.5f),
                                     theme::u32(p.text_normal));
                        draw_text(draw, fonts().regular, 12.f, ImVec2(x + px(17.88f), css_y(12.f, top)),
                                  theme::u32(p.text_normal), summary->branch.c_str());
                        x += w + px(5.f);
                    }
                    const auto counter = [&](int value, const char* icon, ImVec4 color)
                    {
                        if (value <= 0)
                        {
                            return;
                        }
                        const std::string text = std::to_string(value);
                        const float tw = text_size(fonts().semibold, 12.f, text.c_str()).x;
                        const float w = px(5.f + 9.2f + 2.f) + tw + px(5.f);
                        pill(w);
                        draw_fa_icon(draw, icon, ImVec2(x + px(9.6f), mid), px(10.5f), theme::u32(color));
                        draw_text(draw, fonts().semibold, 12.f, ImVec2(x + px(16.2f), css_y(12.f, top)),
                                  theme::u32(color), text.c_str());
                        x += w + px(5.f);
                    };
                    counter(summary->ahead, "far:arrow-up", p.green);
                    counter(summary->behind, "far:arrow-down", p.text_normal);
                    if (wip_summary && (summary->modified + summary->added + summary->deleted) > 0)
                    {
                        float w = px(5.f);
                        const std::pair<int, const char*> items[] = {{summary->modified, "fas:pencil"},
                                                                     {summary->added, "fas:plus"},
                                                                     {summary->deleted, "fas:minus"}};
                        for (const auto& [value, icon] : items)
                        {
                            if (value > 0)
                            {
                                w += px(17.5f) + text_size(fonts().regular, 12.f, std::to_string(value).c_str()).x
                                     + px(6.f);
                            }
                        }
                        w -= px(1.f);
                        pill(w);
                        float ix = x + px(5.f);
                        const ImVec4 colors[] = {p.orange, p.green, p.red};
                        int k = 0;
                        for (const auto& [value, icon] : items)
                        {
                            if (value > 0)
                            {
                                const std::string text = std::to_string(value);
                                draw_fa_icon(draw, icon, ImVec2(ix + px(6.25f), mid + px(0.25f)), px(10.f),
                                             theme::u32(colors[k]));
                                draw_text(draw, fonts().regular, 12.f, ImVec2(ix + px(17.5f), css_y(12.f, top)),
                                          theme::u32(p.text_normal), text.c_str());
                                ix += px(17.5f) + text_size(fonts().regular, 12.f, text.c_str()).x + px(6.f);
                            }
                            ++k;
                        }
                    }
                }
                // Row actions: open in a new window, open (list), close / remove.
                {
                    float ax = right - px(47.f);
                    const auto action = [&](const char* id, const char* icon, const char* tip) -> bool
                    {
                        ImGui::SetCursorScreenPos(ImVec2(ax, row_top + px(5.f)));
                        const bool clicked = ImGui::InvisibleButton(id, ImVec2(px(24.f), px(24.f)));
                        const bool hovered = ImGui::IsItemHovered();
                        draw_fa_icon(draw, icon, ImVec2(ax + px(12.f), mid), px(16.f),
                                     theme::u32(hovered ? p.text_selected : p.text_normal));
                        if (hovered)
                        {
                            styled_tooltip(ImGui::GetItemRectMin(), ImGui::GetItemRectMax(), tip);
                        }
                        ax -= px(34.f);
                        return clicked;
                    };
                    if (section.name == "All repositories")
                    {
                        action("##more", "far:ellipsis-v", "More");
                    }
                    else if (action("##close", "far:times", section.open_section ? "Close tab" : "Remove from list"))
                    {
                        if (section.open_section)
                        {
                            close_request = path;
                        }
                        else if (section.name == "Recent repositories")
                        {
                            remove_recent = path;
                        }
                        else
                        {
                            favorites.erase(path);
                            save_favorites(app, favorites);
                        }
                    }
                    if (action("##open", "far:list", "Open repository"))
                    {
                        open_request = path;
                    }
                    if (action("##window", "far:external-link-alt", "Open in a new window"))
                    {
                        open_request = path;
                    }
                }
                // Clicking the row opens the repository.
                ImGui::SetCursorScreenPos(ImVec2(left + px(70.f), row_top));
                if (ImGui::InvisibleButton("##row", ImVec2(px(500.f), px(34.f))))
                {
                    open_request = path;
                }
                ImGui::PopID();
                y += px(34.f);
            }
            y += px(18.f);
        }
        ImGui::SetCursorScreenPos(ImVec2(origin.x, y));
        ImGui::Dummy(ImVec2(1.f, px(20.f)));
        ImGui::EndChild();

        if (close_request)
        {
            for (std::size_t i = 0; i < app.tabs.size(); ++i)
            {
                if (normalized(app.tabs[i]->repo.workdir()) == normalized(*close_request))
                {
                    app.close_tab(static_cast<int>(i));
                    break;
                }
            }
        }
        if (remove_recent)
        {
            std::erase(app.recent_repos, *remove_recent);
            app.save_recent();
        }
        if (open_request)
        {
            app.repo_management_open = false;
            app.open_repository(*open_request);
        }
    }
} // namespace imgit::ui
