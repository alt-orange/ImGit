#include "app/application.hpp"
#include "app/theme.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <functional>
#include <imgui.h>
#include <imgui_internal.h>
#include <imgui_stdlib.h>
#include <limits>
#include <optional>
#include <set>
#include <string>
#include <vector>

namespace imgit::ui
{
    namespace
    {
        // The left panel constants: rows, headers, section sizes.
        constexpr float REF_HEIGHT = 24.f;
        constexpr float HEADER_HEIGHT = 30.f;
        constexpr float SECTION_DEFAULT_HEIGHT = 132.f;
        constexpr float SECTION_MINIMUM_HEIGHT = 108.f;
        constexpr float DEPTH_INDENT = 15.f; // one .width-15 spacer per level

        // A ref, folder or remote in a section's tree.
        struct TreeNode
        {
            std::string name; // the path segment shown
            std::string key; // section-qualified folder path, for the collapsed set
            const git::Ref* ref = nullptr; // leaves only
            std::vector<TreeNode> children;
        };

        // Tag names compare as semver ranges (minVersion of semver.validRange): "v1.0" is ">=1.0.0 <1.1.0-0".
        struct Version
        {
            bool valid = false;
            bool exact = false; // a full x.y.z
            int part[3] = {0, 0, 0};
            std::string pre;
        };

        Version parse_version(const std::string& name)
        {
            Version v;
            std::size_t i = 0;
            while (i < name.size() && (name[i] == 'v' || name[i] == '=' || name[i] == ' '))
            {
                ++i;
            }
            int parts = 0;
            while (parts < 3)
            {
                if (i >= name.size())
                {
                    break;
                }
                if (name[i] == 'x' || name[i] == 'X' || name[i] == '*')
                {
                    ++i;
                }
                else if (std::isdigit(static_cast<unsigned char>(name[i])) != 0)
                {
                    int value = 0;
                    while (i < name.size() && std::isdigit(static_cast<unsigned char>(name[i])) != 0)
                    {
                        value = value * 10 + (name[i] - '0');
                        ++i;
                    }
                    v.part[parts] = value;
                }
                else
                {
                    return {};
                }
                ++parts;
                if (i < name.size() && name[i] == '.' && parts < 3)
                {
                    ++i;
                    continue;
                }
                break;
            }
            if (parts == 0)
            {
                return {};
            }
            if (i < name.size() && name[i] == '-')
            {
                const std::size_t end = name.find('+', i);
                v.pre = name.substr(i + 1, end == std::string::npos ? std::string::npos : end - i - 1);
                i = end == std::string::npos ? name.size() : end;
            }
            if (i < name.size() && name[i] == '+')
            {
                i = name.size();
            }
            if (i != name.size())
            {
                return {};
            }
            v.valid = true;
            v.exact = parts == 3;
            return v;
        }

        std::string lower(std::string text)
        {
            std::transform(text.begin(), text.end(), text.begin(),
                           [](unsigned char c)
                           {
                               return static_cast<char>(std::tolower(c));
                           });
            return text;
        }

        // defaultCompareFn: case-insensitive by the name shown at this depth (folders and refs mixed);
        // compareSemverOrAlpha for tags: plain names first (alphabetically), then versions newest first.
        bool node_less(const TreeNode& a, const TreeNode& b, bool tags)
        {
            if (tags)
            {
                const Version va = parse_version(a.name);
                const Version vb = parse_version(b.name);
                if (va.valid != vb.valid)
                {
                    return vb.valid;
                }
                if (va.valid)
                {
                    for (int i = 0; i < 3; ++i)
                    {
                        if (va.part[i] != vb.part[i])
                        {
                            return va.part[i] > vb.part[i];
                        }
                    }
                    if (va.pre != vb.pre)
                    {
                        return va.pre.empty() || (!vb.pre.empty() && va.pre > vb.pre);
                    }
                    return va.exact && !vb.exact;
                }
            }
            return lower(a.name) < lower(b.name);
        }

        void insert_ref(TreeNode& root, const std::string& name, const git::Ref* ref, const std::string& prefix)
        {
            TreeNode* node = &root;
            std::size_t start = 0;
            while (true)
            {
                const std::size_t slash = name.find('/', start);
                if (slash == std::string::npos)
                {
                    break;
                }
                const std::string segment = name.substr(start, slash - start);
                const auto found = std::find_if(node->children.begin(), node->children.end(),
                                                [&](const TreeNode& child)
                                                {
                                                    return child.ref == nullptr && child.name == segment;
                                                });
                if (found == node->children.end())
                {
                    node->children.push_back(TreeNode{segment, prefix + name.substr(0, slash), nullptr, {}});
                    node = &node->children.back();
                }
                else
                {
                    node = &*found;
                }
                start = slash + 1;
            }
            node->children.push_back(TreeNode{name.substr(start), prefix + name, ref, {}});
        }

        void sort_tree(TreeNode& node, bool tags)
        {
            std::stable_sort(node.children.begin(), node.children.end(),
                             [&](const TreeNode& a, const TreeNode& b)
                             {
                                 return node_less(a, b, tags);
                             });
            for (TreeNode& child : node.children)
            {
                sort_tree(child, tags);
            }
        }

        void collect_refs(const TreeNode& node, std::vector<const git::Ref*>& out)
        {
            if (node.ref != nullptr)
            {
                out.push_back(node.ref);
            }
            for (const TreeNode& child : node.children)
            {
                collect_refs(child, out);
            }
        }

        const char* ref_icon(const git::Ref& ref)
        {
            switch (ref.type)
            {
            case git::RefType::TAG:
                return "far:tag";
            case git::RefType::STASH:
                return "far:inbox";
            default:
                return "far:code-branch";
            }
        }

        struct Context
        {
            app::Application* app;
            app::RepoState* state;
            float row_width;
            bool refresh_needed = false;
            // A double-clicked ref, checked out once the tree walk is over: the checkout refreshes the state,
            // which frees the refs the walk reads.
            std::optional<git::Ref> checkout_request{};
        };

        void set_hidden(Context& ctx, const std::vector<const git::Ref*>& refs, bool hidden)
        {
            app::RepoState& state = *ctx.state;
            state.solo_ref.clear();
            for (const git::Ref* ref : refs)
            {
                if (ref->is_head)
                {
                    continue;
                }
                if (hidden)
                {
                    state.hidden_refs.insert(ref->full_name);
                }
                else
                {
                    state.hidden_refs.erase(ref->full_name);
                }
            }
            ctx.refresh_needed = true;
        }

        bool all_hidden(const std::vector<const git::Ref*>& refs)
        {
            bool any = false;
            for (const git::Ref* ref : refs)
            {
                if (ref->is_head)
                {
                    continue;
                }
                if (!ref->hidden)
                {
                    return false;
                }
                any = true;
            }
            return any;
        }

        // The text of a row: the name, or a stash's "message on: branch" with the branch in text-disabled.
        struct RowLabel
        {
            std::string text;
            std::string stash_branch;
        };

        // CSS text-overflow: ellipsis over inline runs (each with its left margin): glyphs are kept while they
        // and the ellipsis fit, and the ellipsis follows the last kept glyph.
        struct TextRun
        {
            std::string text;
            ImU32 color;
            float margin;
        };

        void draw_runs_ellipsis(ImDrawList* draw, ImVec2 pos, float room, ImU32 ellipsis_color,
                                const std::vector<TextRun>& runs)
        {
            ImFont* font = fonts().regular;
            constexpr float SIZE = theme::FONT_SIZE_ROW;
            float total = 0.f;
            for (const TextRun& run : runs)
            {
                total += run.margin + text_size(font, SIZE, run.text.c_str()).x;
            }
            if (total <= room)
            {
                float x = pos.x;
                for (const TextRun& run : runs)
                {
                    x += run.margin;
                    draw_text(draw, font, SIZE, ImVec2(x, pos.y), run.color, run.text.c_str());
                    x += text_size(font, SIZE, run.text.c_str()).x;
                }
                return;
            }
            const char* ellipsis = "\xE2\x80\xA6"; // U+2026
            const float budget = room - text_size(font, SIZE, ellipsis).x;
            float x = pos.x;
            float last_glyph_end = pos.x;
            for (const TextRun& run : runs)
            {
                x += run.margin;
                std::size_t keep = 0;
                const char* text = run.text.c_str();
                const char* end = text + run.text.size();
                const char* cursor = text;
                while (cursor < end)
                {
                    unsigned int code_point = 0;
                    const char* next = cursor + std::max(ImTextCharFromUtf8(&code_point, cursor, end), 1);
                    if (x - pos.x + text_size(font, SIZE, std::string(text, next).c_str()).x > budget)
                    {
                        break;
                    }
                    keep = static_cast<std::size_t>(next - text);
                    cursor = next;
                }
                const std::string kept = run.text.substr(0, keep);
                if (!kept.empty())
                {
                    draw_text(draw, font, SIZE, ImVec2(x, pos.y), run.color, kept.c_str());
                    last_glyph_end = x + text_size(font, SIZE, kept.c_str()).x;
                }
                if (keep < run.text.size())
                {
                    draw_text(draw, font, SIZE, ImVec2(last_glyph_end, pos.y), ellipsis_color, ellipsis);
                    return;
                }
                x = last_glyph_end;
            }
        }

        struct RowResult
        {
            bool clicked = false;
            bool double_clicked = false;
            bool context = false; // right-click or the ellipsis button
            bool eye_clicked = false;
        };

        struct RowOptions
        {
            int depth = 0;
            const char* icon = nullptr;
            bool checked_out = false;
            bool synced = false; // a submodule at the commit its repository records: a green check
            bool hidden = false;
            bool selected = false;
            bool can_hide = true;
            int ahead = 0;
            int behind = 0;
            std::string tooltip;
        };

        // A .hover-row (24px, 5px left padding): the 15px eye / checked-out slot, 15px per depth level, the 15px
        // icon slot in text-disabled, the label 5px on, then the upstream status and the 13.5px ellipsis button
        // (hover only, though it keeps its space). Hidden refs show the eye-slash and draw at half opacity.
        RowResult panel_row(Context& ctx, const char* id, const RowLabel& label, const RowOptions& options)
        {
            const theme::Palette& p = theme::palette();
            const float h = px(REF_HEIGHT);
            const float w = ctx.row_width;
            const ImVec2 pos = ImGui::GetCursorScreenPos();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            RowResult result;
            ImGui::PushID(id);

            const ImVec2 mouse = ImGui::GetIO().MousePos;
            const bool hovered = ImGui::IsWindowHovered(ImGuiHoveredFlags_AllowWhenBlockedByActiveItem)
                                 && mouse.x >= pos.x && mouse.x < pos.x + w && mouse.y >= pos.y && mouse.y < pos.y + h;
            // Slots submitted before the row so they win the hover test; the test still passes while a slot is held
            // down (blocked by its own active item), so its click lands on release.
            const ImVec2 eye_min(pos.x + px(5.f), pos.y);
            const float menu_x = pos.x + w + 1.f - px(13.5f);
            if (hovered && options.can_hide && !options.checked_out)
            {
                ImGui::SetCursorScreenPos(eye_min);
                if (ImGui::InvisibleButton("##eye", ImVec2(px(15.f), h)))
                {
                    result.eye_clicked = true;
                }
                if (ImGui::IsItemHovered())
                {
                    styled_tooltip(ImGui::GetItemRectMin(), ImGui::GetItemRectMax(),
                               options.hidden ? "Show in graph" : "Hide in graph");
                }
            }
            if (hovered)
            {
                ImGui::SetCursorScreenPos(ImVec2(menu_x, pos.y));
                if (ImGui::InvisibleButton("##menu", ImVec2(px(13.5f), h)))
                {
                    result.context = true;
                }
            }
            ImGui::SetCursorScreenPos(pos);
            ImGui::InvisibleButton("##row", ImVec2(w, h));
            result.clicked = ImGui::IsItemClicked(ImGuiMouseButton_Left);
            result.double_clicked = ImGui::IsItemHovered() && ImGui::IsMouseDoubleClicked(ImGuiMouseButton_Left);
            result.context =
                    result.context || (ImGui::IsItemHovered() && ImGui::IsMouseReleased(ImGuiMouseButton_Right));
            ImGui::PopID();

            // .hover-row:hover:not(.selected-row) wins over .checked-out.
            if (options.selected)
            {
                draw->AddRectFilled(pos, ImVec2(pos.x + w, pos.y + h), theme::u32(p.selected_row));
            }
            else if (hovered)
            {
                draw->AddRectFilled(pos, ImVec2(pos.x + w, pos.y + h), theme::u32(p.hover_row));
            }
            else if (options.checked_out)
            {
                draw->AddRectFilled(pos, ImVec2(pos.x + w, pos.y + h), theme::u32(p.checked_out));
            }
            const float alpha = options.hidden ? 0.5f : 1.f;
            const auto color = [&](ImVec4 c)
            {
                return theme::u32(theme::with_alpha(c, c.w * alpha));
            };
            const float mid = pos.y + h / 2.f;
            const float slot = eye_min.x + px(7.5f);
            if (options.checked_out)
            {
                draw_fa_icon(draw, "fas:check-square", ImVec2(slot, mid), px(14.f), color(p.green));
            }
            else if (options.synced)
            {
                draw_fa_icon(draw, "fas:check", ImVec2(slot, mid), px(11.f), color(p.green));
            }
            else if (options.hidden)
            {
                draw_fa_icon(draw, "far:eye-slash", ImVec2(slot, mid), px(12.f), color(p.text_normal));
            }
            else if (hovered && options.can_hide)
            {
                draw_fa_icon(draw, "far:eye", ImVec2(slot, mid), px(12.f), color(p.green));
            }
            const float depth_x = px(DEPTH_INDENT) * static_cast<float>(options.depth);
            if (options.icon != nullptr)
            {
                draw_fa_icon(draw, options.icon, ImVec2(pos.x + px(32.5f) + depth_x, mid), px(12.f),
                             color(p.text_disabled));
            }

            // Right side: the upstream status (10px) before the ellipsis slot.
            float right = menu_x;
            const ImVec4 text = options.checked_out ? p.text_selected : p.text_normal;
            const auto upstream = [&](int count, const char* arrow)
            {
                if (count <= 0)
                {
                    return;
                }
                const float icon_w = fa_icon_width(arrow, px(10.f));
                right -= icon_w;
                draw_fa_icon(draw, arrow, ImVec2(right + icon_w / 2.f, mid), px(10.f), color(text));
                const std::string number = std::to_string(count);
                const ImVec2 size = text_size(fonts().regular, 10.f, number.c_str());
                right -= text_size(fonts().regular, 10.f, " ").x + size.x;
                draw_text(draw, fonts().regular, 10.f, ImVec2(right, mid - size.y / 2.f), color(text), number.c_str());
                right -= px(5.f); // .ml1
            };
            upstream(options.behind, "far:long-arrow-down");
            upstream(options.ahead, "far:long-arrow-up");
            if (hovered)
            {
                draw_fa_icon(draw, "far:ellipsis-v", ImVec2(menu_x + px(6.75f), mid), px(14.f),
                             theme::u32(p.text_secondary));
            }

            const float label_x = pos.x + px(45.f) + depth_x;
            const float label_room = right - px(5.f) - label_x;
            const float line_h = text_size(fonts().regular, theme::FONT_SIZE_ROW, "A").y;
            const float ty = mid - line_h / 2.f;
            if (label.stash_branch.empty())
            {
                draw_text_ellipsis(draw, fonts().regular, theme::FONT_SIZE_ROW, ImVec2(label_x, ty), label_room,
                                   color(text), label.text.c_str());
            }
            else
            {
                draw_runs_ellipsis(draw, ImVec2(label_x, ty), label_room, color(text),
                                   {{label.text, color(text), 0.f},
                                    {"on:", color(text), px(5.f)},
                                    {label.stash_branch, color(p.text_disabled), px(5.f)}});
            }
            if (hovered && !options.tooltip.empty() && mouse.x >= label_x && mouse.x < label_x + label_room)
            {
                styled_tooltip(ImVec2(label_x, pos.y), ImVec2(label_x + label_room, pos.y + h), options.tooltip.c_str());
            }
            ImGui::SetCursorScreenPos(ImVec2(pos.x, pos.y + h));
            return result;
        }

        // The rows of one tree level: folders (collapsible, hide all their refs) and refs.
        void draw_tree(Context& ctx, const TreeNode& node, int depth, bool remote_level)
        {
            app::Application& app = *ctx.app;
            app::RepoState& state = *ctx.state;
            for (const TreeNode& child : node.children)
            {
                if (child.ref != nullptr)
                {
                    const git::Ref& ref = *child.ref;
                    RowOptions options;
                    options.depth = depth;
                    options.icon = ref_icon(ref);
                    options.checked_out = ref.is_head;
                    options.hidden = ref.hidden;
                    options.selected = state.left_selected == ref.full_name && state.is_selected(ref.target)
                                       && state.selected.size() == 1;
                    options.ahead = ref.type == git::RefType::LOCAL_BRANCH ? ref.ahead : 0;
                    options.behind = ref.type == git::RefType::LOCAL_BRANCH ? ref.behind : 0;
                    options.tooltip = ref.name;
                    const RowResult row = panel_row(ctx, ref.full_name.c_str(), RowLabel{child.name, {}}, options);
                    if (row.eye_clicked)
                    {
                        set_hidden(ctx, {&ref}, !ref.hidden);
                    }
                    else if (row.double_clicked)
                    {
                        ctx.checkout_request = ref;
                    }
                    else if (row.clicked)
                    {
                        state.left_selected = ref.full_name;
                        jump_to_commit(state, ref.target.c_str());
                    }
                    if (row.context)
                    {
                        popup_menu(ref_menu(app, state, ref, true));
                    }
                    continue;
                }
                std::vector<const git::Ref*> refs;
                collect_refs(child, refs);
                const bool collapsed = state.collapsed_nodes.contains(child.key);
                RowOptions options;
                options.depth = depth;
                options.icon = remote_level ? "far:globe" : "far:folder";
                options.hidden = all_hidden(refs);
                const RowResult row = panel_row(ctx, child.key.c_str(), RowLabel{child.name, {}}, options);
                if (row.eye_clicked)
                {
                    set_hidden(ctx, refs, !options.hidden);
                }
                else if (row.clicked)
                {
                    if (collapsed)
                    {
                        state.collapsed_nodes.erase(child.key);
                    }
                    else
                    {
                        state.collapsed_nodes.insert(child.key);
                    }
                }
                if (row.context)
                {
                    if (remote_level)
                    {
                        popup_menu(remote_menu(app, state, child.name, true));
                    }
                    else if (child.key.rfind("local:", 0) == 0)
                    {
                        popup_menu(local_folder_menu(app, state, child.key.substr(6)));
                    }
                }
                if (!collapsed)
                {
                    draw_tree(ctx, child, depth + 1, false);
                }
            }
        }

        void simple_row(Context& ctx, const char* id, const char* icon, const std::string& label,
                        const std::string& detail)
        {
            RowOptions options;
            options.icon = icon;
            options.can_hide = false;
            options.tooltip = detail;
            panel_row(ctx, id, RowLabel{label, {}}, options);
        }

        // The left panel sections, in order.
        const std::vector<std::string> SECTIONS = {"Local", "Remote", "Worktrees", "Stashes", "Tags", "Submodules"};

        // getLeftPanelHandleBounds + getBoundedSectionYsByKey: each section ends at a Y; collapsed ones are
        // exactly the header, expanded ones at least header + SECTION_MINIMUM_HEIGHT; a section's preferred Y is
        // its stored one or the previous preferred Y + SECTION_DEFAULT_HEIGHT + HEADER_HEIGHT, then bounded so
        // the sections fill the panel. Returns the bounded Ys.
        std::vector<float> section_ys(const std::vector<bool>& expanded,
                                      const std::vector<std::optional<float>>& stored, float panel)
        {
            const std::size_t n = expanded.size();
            std::vector<float> ys(n, 0.f);
            if (n == 0)
            {
                return ys;
            }
            constexpr float INF = std::numeric_limits<float>::infinity();
            const float header = px(HEADER_HEIGHT);
            std::vector<float> mins(n), maxs(n);
            for (std::size_t i = 0; i < n; ++i)
            {
                mins[i] = expanded[i] ? header + px(SECTION_MINIMUM_HEIGHT) : header;
                maxs[i] = expanded[i] ? INF : header;
            }
            // Bounds (ba).
            std::vector<float> lo(n), hi(n);
            {
                float sum_min = 0.f;
                float sum_max = 0.f;
                bool unbounded = false;
                for (std::size_t i = 0; i < n; ++i)
                {
                    unbounded = unbounded || maxs[i] == INF;
                    sum_min += mins[i];
                    lo[i] = sum_min;
                    hi[i] = std::max(sum_min, panel);
                    if (!unbounded)
                    {
                        sum_max += maxs[i];
                        hi[i] = sum_max;
                    }
                }
                if (!unbounded && sum_max < panel)
                {
                    lo = hi;
                }
                else if (sum_min >= panel)
                {
                    hi = lo;
                }
                else
                {
                    sum_min = 0.f;
                    sum_max = 0.f;
                    for (std::size_t i = 0; i < n; ++i)
                    {
                        sum_min += mins[i];
                        sum_max = std::min(sum_max + maxs[i], panel);
                        lo[i] = sum_min;
                        hi[i] = sum_max;
                    }
                    std::optional<float> from_end = panel;
                    float room = panel;
                    lo[n - 1] = hi[n - 1] = panel;
                    for (std::size_t k = n - 1; k-- > 0;)
                    {
                        if (maxs[k + 1] != INF && from_end)
                        {
                            *from_end -= maxs[k + 1];
                        }
                        else
                        {
                            from_end.reset();
                        }
                        room -= mins[k + 1];
                        if (from_end)
                        {
                            lo[k] = std::max(lo[k], *from_end);
                        }
                        hi[k] = std::min(hi[k], room);
                    }
                }
            }
            float previous = 0.f;
            for (std::size_t i = 0; i < n; ++i)
            {
                const float preferred = stored[i] ? px(*stored[i]) : previous + px(SECTION_DEFAULT_HEIGHT) + header;
                previous = preferred;
                ys[i] = std::clamp(preferred, lo[i], std::max(lo[i], hi[i]));
            }
            for (std::size_t i = 1; i < n; ++i)
            {
                const float low = ys[i - 1] + mins[i];
                const float high = maxs[i] != INF ? ys[i - 1] + maxs[i] : ys[i];
                ys[i] = std::clamp(ys[i], low, std::max(low, high));
            }
            return ys;
        }

        // The 30px sticky header: chevron (fa-fw), the section icon in text-disabled, the caps label and the count
        // (both 12px semibold); text-secondary turning white on hover. With `add`, hovering swaps the count for a
        // 24px green "+" button 10px from the edge, tooltip `add_tip` (`add` is set when it is clicked). Returns
        // true when the header itself is clicked.
        bool section_header(Context& ctx, ImVec2 pos, float width, const char* label, const char* icon,
                            std::size_t count, bool expanded, bool* add = nullptr, const char* add_tip = nullptr)
        {
            const theme::Palette& p = theme::palette();
            const float h = px(HEADER_HEIGHT);
            ImDrawList* draw = ImGui::GetWindowDrawList();
            ImGui::SetCursorScreenPos(pos);
            ImGui::PushID(label);
            ImGui::SetNextItemAllowOverlap();
            const bool clicked = ImGui::InvisibleButton("##section", ImVec2(width, h));
            // While the "+" is held it is the active item; the header must stay hovered for it to finish the click.
            const bool hovered = ImGui::IsWindowHovered(ImGuiHoveredFlags_AllowWhenBlockedByActiveItem)
                                 && ImGui::IsMouseHoveringRect(pos, ImVec2(pos.x + width, pos.y + h));
            const ImVec2 add_min(pos.x + width - px(10.f) - px(24.f), pos.y + px(3.2f));
            const ImVec2 add_max(add_min.x + px(24.f), add_min.y + px(24.f));
            bool add_hovered = false;
            if (add != nullptr && hovered)
            {
                ImGui::SetCursorScreenPos(add_min);
                *add = ImGui::InvisibleButton("##add", ImVec2(add_max.x - add_min.x, add_max.y - add_min.y));
                add_hovered = ImGui::IsItemHovered();
                if (add_hovered && add_tip != nullptr)
                {
                    tooltip(add_tip);
                }
            }
            if (hovered && ImGui::IsMouseReleased(ImGuiMouseButton_Right))
            {
                popup_menu(left_section_menu(*ctx.app, *ctx.state, label, SECTIONS));
            }
            ImGui::PopID();
            draw->AddRectFilled(pos, ImVec2(pos.x + width, pos.y + h), theme::u32(p.panel_bg0));
            draw->AddRectFilled(pos, ImVec2(pos.x + width, pos.y + 1.f), theme::u32(p.section_border));
            const ImU32 color = theme::u32(hovered ? p.text_selected : p.text_secondary);
            const float mid = pos.y + 1.f + (h - 1.f) / 2.f;
            draw_fa_icon(draw, expanded ? "far:chevron-down" : "far:chevron-right", ImVec2(pos.x + px(12.5f), mid),
                         px(12.f), color);
            draw_fa_icon(draw, icon, ImVec2(pos.x + px(32.5f), mid), px(12.f), theme::u32(p.text_disabled));
            const std::string upper = to_upper(label);
            const float ascent = font_px(fonts().semibold, theme::FONT_SIZE_ROW) * 2189.f / 2789.f;
            const float baseline = pos.y + px(19.6f);
            draw_text(draw, fonts().semibold, theme::FONT_SIZE_ROW, ImVec2(pos.x + px(45.f), baseline - ascent), color,
                      upper.c_str());
            if (add != nullptr && hovered)
            {
                draw->AddRectFilled(add_min, add_max,
                                    theme::u32(theme::with_alpha(p.green, add_hovered ? 0.3f : 0.2f)), px(2.f));
                draw->AddRect(add_min, add_max, theme::u32(p.green), px(2.f), 0, 1.f);
                draw_svg_path(draw, "plus", svg_path("plus"), 24.f, 24.f,
                              ImVec2(add_min.x + px(4.f), add_min.y + px(4.f)),
                              ImVec2(add_max.x - px(4.f), add_max.y - px(4.f)), theme::u32(p.text_normal));
            }
            else if (count != SIZE_MAX)
            {
                const std::string count_text = std::to_string(count);
                const ImVec2 count_size = text_size(fonts().semibold, theme::FONT_SIZE_ROW, count_text.c_str());
                draw_text(draw, fonts().semibold, theme::FONT_SIZE_ROW,
                          ImVec2(pos.x + width - px(10.f) - count_size.x, baseline - ascent), theme::u32(p.text_accent),
                          count_text.c_str());
            }
            return clicked;
        }

        struct Section
        {
            std::string name; // "Local"
            const char* icon;
            std::size_t count;
            std::function<void(Context&)> body;
        };
    } // namespace

    void draw_left_panel(app::Application& app, app::RepoState& state, ImVec2 size, bool open)
    {
        const theme::Palette& p = theme::palette();
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        ImGui::BeginChild("##left_panel", size, ImGuiChildFlags_None, ImGuiWindowFlags_NoScrollbar);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        // panel__bg0 with a 1px panel border (composited over the app background) on the right.
        draw->AddRectFilled(origin, ImVec2(origin.x + size.x - 1.f, origin.y + size.y), theme::u32(p.panel_bg0));
        draw->AddRectFilled(ImVec2(origin.x + size.x - 1.f, origin.y), ImVec2(origin.x + size.x, origin.y + size.y),
                            theme::u32(theme::mix(p.panel_border, p.app_bg0, p.panel_border.w)));
        const float inner_w = size.x - 1.f;

        if (!open)
        {
            // The collapsed panel (LEFT_PANEL_COLLAPSED_WIDTH 36): the expand chevron in a 30px row, then
            // each section's 14px icon (text-secondary) over its 10px count (text-accent) in 36px rows; a row
            // opens the panel on that section.
            const float strip_w = px(35.f);
            draw->AddRectFilled(origin, ImVec2(origin.x + strip_w, origin.y + size.y), theme::u32(p.panel_bg0));
            const auto open_panel = [&](const char* section)
            {
                app.show_left_panel = true;
                state.left_open_in_diff = true;
                if (section != nullptr)
                {
                    state.expanded_left_sections.insert(section);
                }
            };
            ImGui::SetCursorScreenPos(origin);
            if (ImGui::InvisibleButton("##expand_left", ImVec2(strip_w, px(30.f))))
            {
                open_panel(nullptr);
            }
            if (ImGui::IsItemHovered())
            {
                draw->AddRectFilled(origin, ImVec2(origin.x + strip_w, origin.y + px(30.f)), theme::u32(p.hover_row));
                styled_tooltip(origin, ImVec2(origin.x + strip_w, origin.y + px(30.f)), "Expand Panel (Ctrl+J)");
            }
            draw_fa_icon(draw, "fas:chevron-circle-right", ImVec2(origin.x + px(17.5f), origin.y + px(15.f)), px(14.f),
                         theme::u32(p.text_normal));
            std::size_t local_n = 0, remote_n = 0, stash_n = 0, tag_n = 0;
            for (const git::Ref& ref : state.history.refs)
            {
                switch (ref.type)
                {
                case git::RefType::LOCAL_BRANCH:
                    ++local_n;
                    break;
                case git::RefType::REMOTE_BRANCH:
                    ++remote_n;
                    break;
                case git::RefType::STASH:
                    ++stash_n;
                    break;
                case git::RefType::TAG:
                    ++tag_n;
                    break;
                }
            }
            struct StripItem
            {
                const char* section;
                const char* icon;
                std::size_t count;
            };
            std::vector<StripItem> items = {{"Local", "far:laptop", local_n}};
            const std::set<std::string>& hidden = app.settings.hidden_left_sections;
            if (!hidden.contains("Remote"))
            {
                items.push_back({"Remote", "far:cloud", remote_n});
            }
            if (!hidden.contains("Worktrees"))
            {
                items.push_back({"Worktrees", "far:sitemap", state.history.worktrees.size()});
            }
            if (stash_n > 0 && !hidden.contains("Stashes"))
            {
                items.push_back({"Stashes", "far:inbox", stash_n});
            }
            if (!hidden.contains("Tags"))
            {
                items.push_back({"Tags", "far:tag", tag_n});
            }
            if (!hidden.contains("Submodules"))
            {
                items.push_back({"Submodules", "far:layer-group", state.history.submodules.size()});
            }
            float y = origin.y + px(30.f);
            for (const StripItem& item : items)
            {
                ImGui::SetCursorScreenPos(ImVec2(origin.x, y));
                ImGui::PushID(item.section);
                if (ImGui::InvisibleButton("##strip_section", ImVec2(strip_w, px(36.f))))
                {
                    open_panel(item.section);
                }
                const bool hovered = ImGui::IsItemHovered();
                ImGui::PopID();
                if (hovered)
                {
                    draw->AddRectFilled(ImVec2(origin.x, y), ImVec2(origin.x + strip_w, y + px(36.f)),
                                        theme::u32(p.hover_row));
                    styled_tooltip(ImVec2(origin.x, y), ImVec2(origin.x + strip_w, y + px(36.f)), item.section);
                }
                draw_fa_icon(draw, item.icon, ImVec2(origin.x + px(17.5f), y + px(13.4f)), px(14.f),
                             theme::u32(hovered ? p.text_selected : p.text_secondary));
                if (item.count != SIZE_MAX)
                {
                    const std::string count = std::to_string(item.count);
                    const float w = text_size(fonts().regular, 10.f, count.c_str()).x;
                    const float ascent = 10.f * 2189.f / 2048.f * dpi_scale();
                    draw_text(draw, fonts().regular, 10.f,
                              ImVec2(origin.x + px(17.5f) - w / 2.f,
                                     std::round(y + px(20.4f) + std::round(ascent)) - std::ceil(ascent)),
                              theme::u32(p.text_accent), count.c_str());
                }
                y += px(36.f);
            }
            ImGui::EndChild();
            return;
        }

        // Header row: the collapse button, then "Viewing N"; the filter box under it.
        {
            const ImVec2 center(origin.x + px(18.5f), origin.y + px(17.f));
            const float box = px(22.f);
            ImGui::SetCursorScreenPos(ImVec2(center.x - box / 2.f, center.y - box / 2.f));
            ImGui::InvisibleButton("##collapse_left", ImVec2(box, box));
            const bool hovered = ImGui::IsItemHovered();
            draw_icon(draw, Icon::BACK, center, px(16.f),
                      theme::u32(hovered ? p.text_selected : theme::with_alpha(p.white, 0.7f)));
            if (hovered)
            {
                tooltip("Collapse Panel (Ctrl+J)");
            }
            if (ImGui::IsItemClicked(ImGuiMouseButton_Left))
            {
                if (state.diff_target && state.left_open_in_diff)
                {
                    state.left_open_in_diff = false;
                }
                else
                {
                    app.show_left_panel = false;
                }
            }
        }

        // "Viewing N": the branches and tags walked for the graph (not hidden; stashes do not count).
        std::size_t visible_refs = 0;
        for (const git::Ref& ref : state.history.refs)
        {
            if (!ref.hidden && ref.type != git::RefType::STASH)
            {
                ++visible_refs;
            }
        }
        {
            const float ascent = font_px(fonts().regular, theme::FONT_SIZE_ROW) * 2189.f / 2789.f;
            const float y = origin.y + px(21.6f) - ascent;
            const float x = origin.x + px(36.f);
            const float w = text_size(fonts().regular, theme::FONT_SIZE_ROW, "Viewing").x;
            draw_text(draw, fonts().regular, theme::FONT_SIZE_ROW, ImVec2(x, y), theme::u32(p.text_normal), "Viewing");
            draw_text(draw, fonts().semibold, theme::FONT_SIZE_ROW, ImVec2(x + w + px(5.f), y),
                      theme::u32(p.text_accent), std::to_string(visible_refs).c_str());
        }
        {
            const ImVec2 min(origin.x + px(10.f), origin.y + px(33.f));
            const ImVec2 max(origin.x + inner_w - px(10.f), min.y + px(21.f));
            draw->AddRectFilled(min, max, theme::u32(p.input_bg), px(3.f));
            draw->AddRect(min, max, theme::u32(p.subtle_border), px(3.f), 0, 1.f);
            const float text_h = font_px(fonts().regular, theme::FONT_SIZE_ROW);
            ImGui::SetCursorScreenPos(ImVec2(min.x, min.y));
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(6.f), (max.y - min.y - text_h) / 2.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgActive, ImVec4(0.f, 0.f, 0.f, 0.f));
            push_font(fonts().regular, theme::FONT_SIZE_ROW);
            input_text("##left_filter", state.left_filter, shortcut_label("Filter (Ctrl + Alt + f)").c_str(), 0,
                       max.x - min.x - px(24.f));
            ImGui::PopFont();
            ImGui::PopStyleColor(3);
            ImGui::PopStyleVar();
            draw_icon(draw, Icon::SEARCH, ImVec2(origin.x + inner_w - px(21.5f), (min.y + max.y) / 2.f), px(12.f),
                      theme::u32(p.text_secondary));
            if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Alt | ImGuiKey_F))
            {
                ImGui::SetKeyboardFocusHere(-1);
            }
        }

        // The trees.
        const bool filtering = !state.left_filter.empty();
        TreeNode local, remote, tags;
        std::vector<const git::Ref*> stashes;
        std::size_t local_count = 0, remote_count = 0, tag_count = 0;
        // Every configured remote gets its row, also one with nothing fetched yet (its branches fill it in).
        for (const git::Remote& r : state.history.remotes)
        {
            // (A name with '/' is left to its branches: the tree splits ref names there.)
            if (r.name.find('/') == std::string::npos && (!filtering || contains_ci(r.name, state.left_filter)))
            {
                remote.children.push_back(TreeNode{r.name, "remote:" + r.name, nullptr, {}});
            }
        }
        for (const git::Ref& ref : state.history.refs)
        {
            if (filtering && !contains_ci(ref.name, state.left_filter) && !contains_ci(ref.message, state.left_filter))
            {
                continue;
            }
            switch (ref.type)
            {
            case git::RefType::LOCAL_BRANCH:
                ++local_count;
                insert_ref(local, ref.name, &ref, "local:");
                break;
            case git::RefType::REMOTE_BRANCH:
                ++remote_count;
                insert_ref(remote, ref.name, &ref, "remote:");
                break;
            case git::RefType::TAG:
                ++tag_count;
                insert_ref(tags, ref.name, &ref, "tag:");
                break;
            case git::RefType::STASH:
                stashes.push_back(&ref);
                break;
            }
        }
        sort_tree(local, false);
        sort_tree(remote, false);
        sort_tree(tags, true);

        const std::set<std::string>& hidden_sections = app.settings.hidden_left_sections;
        std::vector<Section> sections;
        sections.push_back({"Local", "far:laptop", local_count, [&](Context& ctx)
                            {
                                draw_tree(ctx, local, 0, false);
                            }});
        if (!hidden_sections.contains("Remote"))
        {
            sections.push_back({"Remote", "far:cloud", remote_count, [&](Context& ctx)
                                {
                                    draw_tree(ctx, remote, 0, true);
                                }});
        }
        if (!hidden_sections.contains("Worktrees"))
        {
            sections.push_back({"Worktrees", "far:sitemap", state.history.worktrees.size(), [&](Context& ctx)
                                {
                                    for (const git::Worktree& worktree : state.history.worktrees)
                                    {
                                        simple_row(ctx, worktree.path.c_str(), "far:sitemap", worktree.name,
                                                   worktree.path);
                                    }
                                }});
        }
        if (!stashes.empty() && !hidden_sections.contains("Stashes"))
        {
            sections.push_back({"Stashes", "far:inbox", stashes.size(), [&](Context& ctx)
                                {
                                    for (const git::Ref* stash : stashes)
                                    {
                                        // "On <branch>: message" / "WIP on <branch>: ..." -> message on: branch
                                        std::string branch;
                                        for (const char* prefix : {"WIP on ", "On "})
                                        {
                                            const std::size_t length = std::strlen(prefix);
                                            if (stash->message.compare(0, length, prefix) == 0)
                                            {
                                                const std::size_t colon = stash->message.find(": ", length);
                                                if (colon != std::string::npos)
                                                {
                                                    branch = stash->message.substr(length, colon - length);
                                                }
                                                break;
                                            }
                                        }
                                        RowOptions options;
                                        options.icon = "far:inbox";
                                        options.hidden = stash->hidden;
                                        options.selected = state.left_selected == stash->full_name
                                                           && state.is_selected(stash->target);
                                        const RowResult row = panel_row(
                                                ctx, stash->full_name.c_str(),
                                                RowLabel{git::stash_summary(stash->message), branch}, options);
                                        if (row.eye_clicked)
                                        {
                                            set_hidden(ctx, {stash}, !stash->hidden);
                                        }
                                        else if (row.clicked)
                                        {
                                            state.left_selected = stash->full_name;
                                            jump_to_commit(state, stash->target.c_str());
                                        }
                                        if (row.context)
                                        {
                                            popup_menu(ref_menu(app, state, *stash, true));
                                        }
                                    }
                                }});
        }
        if (!hidden_sections.contains("Tags"))
        {
            sections.push_back({"Tags", "far:tag", tag_count, [&](Context& ctx)
                                {
                                    draw_tree(ctx, tags, 0, false);
                                }});
        }
        if (!hidden_sections.contains("Submodules"))
        {
            sections.push_back({"Submodules", "far:layer-group", state.history.submodules.size(), [&](Context& ctx)
                                {
                                    for (const git::Submodule& submodule : state.history.submodules)
                                    {
                                        // Double-click: the Edit Submodule panel sliding out over the graph.
                                        const std::string key = "submodule:" + submodule.path;
                                        RowOptions options;
                                        options.icon = "far:layer-group";
                                        options.can_hide = false;
                                        options.synced = submodule.in_sync();
                                        options.selected = state.left_selected == key;
                                        options.tooltip = "Path: /" + submodule.path + "\nRemote URL: " + submodule.url;
                                        const RowResult row =
                                                panel_row(ctx, key.c_str(), RowLabel{submodule.name, {}}, options);
                                        if (row.clicked)
                                        {
                                            state.left_selected = key;
                                        }
                                        if (row.double_clicked)
                                        {
                                            app.dialog.open(app::Dialog::Kind::EDIT_SUBMODULE);
                                            app.dialog.text_a = submodule.url;
                                            app.dialog.text_b = submodule.path;
                                        }
                                    }
                                }});
        }

        // Sections fill the panel under the filter box.
        const float top = origin.y + px(63.f);
        const float panel_h = origin.y + size.y - top;
        std::vector<bool> expanded;
        std::vector<std::optional<float>> stored;
        for (const Section& section : sections)
        {
            expanded.push_back(filtering || state.expanded_left_sections.contains(section.name));
            const auto found = state.left_section_ys.find(section.name);
            stored.push_back(found != state.left_section_ys.end() ? std::optional<float>(found->second) : std::nullopt);
        }
        const std::vector<float> ys = section_ys(expanded, stored, panel_h);
        Context ctx{&app, &state, inner_w - px(10.f), false};
        ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing, ImVec2(ImGui::GetStyle().ItemSpacing.x, 0.f));
        for (std::size_t i = 0; i < sections.size(); ++i)
        {
            const Section& section = sections[i];
            const float y0 = top + (i > 0 ? ys[i - 1] : 0.f);
            const float y1 = top + ys[i];
            if (expanded[i] && y1 - y0 > px(HEADER_HEIGHT))
            {
                ImGui::SetCursorScreenPos(ImVec2(origin.x, y0 + px(HEADER_HEIGHT)));
                ImGui::BeginChild(("##section_" + section.name).c_str(), ImVec2(inner_w, y1 - y0 - px(HEADER_HEIGHT)));
                section.body(ctx);
                ImGui::EndChild();
            }
            // The top edge of each later header drags the previous section's end (submitted first to win the
            // hover over the header).
            if (i > 0 && (expanded[i - 1] || expanded[i]))
            {
                ImGui::SetCursorScreenPos(ImVec2(origin.x, y0));
                ImGui::PushID(section.name.c_str());
                ImGui::InvisibleButton("##resize", ImVec2(inner_w, px(4.f)));
                if (ImGui::IsItemHovered() || ImGui::IsItemActive())
                {
                    ImGui::SetMouseCursor(ImGuiMouseCursor_ResizeNS);
                }
                if (ImGui::IsItemActive())
                {
                    const float y = ImGui::GetIO().MousePos.y - top;
                    state.left_section_ys[sections[i - 1].name] = y / px(1.f);
                }
                ImGui::PopID();
            }
            // REMOTE and SUBMODULES have a "+": it adds a remote / a submodule.
            const bool remotes = section.name == "Remote";
            const bool submodules = section.name == "Submodules";
            bool add = false;
            if (section_header(ctx, ImVec2(origin.x, y0), inner_w, section.name.c_str(), section.icon, section.count,
                               expanded[i], remotes || submodules ? &add : nullptr,
                               remotes ? "Add Remote" : "Add Submodule"))
            {
                if (!state.expanded_left_sections.erase(section.name))
                {
                    state.expanded_left_sections.insert(section.name);
                }
            }
            if (add)
            {
                app.dialog.open(remotes ? app::Dialog::Kind::ADD_REMOTE : app::Dialog::Kind::ADD_SUBMODULE);
            }
        }
        ImGui::PopStyleVar();
        ImGui::EndChild();

        if (ctx.refresh_needed)
        {
            state.refresh();
        }
        if (ctx.checkout_request)
        {
            checkout_ref(app, state, *ctx.checkout_request);
        }
    }
} // namespace imgit::ui
