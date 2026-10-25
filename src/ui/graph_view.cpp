#include "app/application.hpp"
#include "app/theme.hpp"
#include "gfx/lcd_renderer.hpp"
#include "graph/graph_layout.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <array>
#include <cctype>
#include <cfloat>
#include <chrono>
#include <cmath>
#include <imgui.h>
#include <imgui_internal.h>
#include <imgui_stdlib.h>
#include <numbers>
#include <optional>
#include <string>
#include <unordered_set>
#include <vector>

namespace imgit::ui
{
    namespace
    {
        constexpr float PI = std::numbers::pi_v<float>;

        // The graph zone when it is narrower than its lanes (GraphContainer's node offsets), in CSS px:
        // the lanes scroll sideways between two opaque gutters that cast a shadow over them, the nodes stay
        // pinned inside the zone and fade towards its edges, and below one lane's width (single column mode,
        // `factor` 0..1) every node slides onto one column.
        struct GraphLanes
        {
            float width = 0.f; // the zone
            float content = 0.f; // getCommitZoneContentWidth
            float scroll = 0.f; // scrollLeft
            float factor = 0.f; // getSingleColumnModeFactor
            float slide_out = 0.f; // getRightGutterSlideOutFactor
        };

        // Horizontal layout of the graph: the zones from left to right, in device px.
        struct Zones
        {
            float ref_x = 0.f, ref_w = 0.f;
            float graph_x = 0.f, graph_w = 0.f;
            float message_x = 0.f, message_w = 0.f;
            float author_x = 0.f, author_w = 0.f;
            float changes_x = 0.f, changes_w = 0.f;
            float date_x = 0.f, date_w = 0.f;
            float sha_x = 0.f, sha_w = 0.f;
            GraphLanes lanes;
        };

        // The graph zones, in CSS px. Every shown zone
        // has a current and a preferred width; together they fill the graph, the last one taking what is left.
        // Dragging a zone's right edge moves the zones after it: they shrink from the last one, or grow back
        // towards their preferred widths from the first one.
        enum class ZoneType
        {
            REF,
            GRAPH,
            MESSAGE,
            AUTHOR,
            CHANGES,
            DATETIME,
            SHA,
        };

        struct GraphZone
        {
            ZoneType type = ZoneType::REF;
            float minimum = 0.f;
            float maximum = 0.f; // 0: none
            float current = 0.f;
            float preferred = 0.f;
        };

        struct ZoneLayout
        {
            std::vector<GraphZone> zones; // the shown ones, in order
            float width = 0.f;
            float content = 0.f; // the graph zone's content width, its maximum
        };

        ZoneLayout g_zones;

        // The zone whose right edge is being dragged (re-resizable: its width when pressed plus the mouse's travel).
        struct ZoneDrag
        {
            int index = -1;
            float start_width = 0.f;
            float start_mouse = 0.f;
        };

        ZoneDrag g_zone_drag;

        constexpr float SCROLLBAR_THICKNESS = 8.f; // --scrollable-scrollbar-thickness

        const char* zone_key(ZoneType type)
        {
            switch (type)
            {
            case ZoneType::REF:
                return "ref";
            case ZoneType::GRAPH:
                return "graph";
            case ZoneType::MESSAGE:
                return "message";
            case ZoneType::AUTHOR:
                return "author";
            case ZoneType::CHANGES:
                return "changes";
            case ZoneType::DATETIME:
                return "datetime";
            case ZoneType::SHA:
                return "sha";
            }
            return "";
        }

        constexpr std::array<ZoneType, 7> ALL_ZONES = {ZoneType::REF,     ZoneType::GRAPH,    ZoneType::MESSAGE,
                                                       ZoneType::AUTHOR,  ZoneType::CHANGES,  ZoneType::DATETIME,
                                                       ZoneType::SHA};

        // Every zone in the user's order: the saved keys first, then the rest in the default order.
        std::vector<ZoneType> zone_order(const app::Settings& settings)
        {
            std::vector<ZoneType> order;
            for (const std::string& key : settings.graph_column_order)
            {
                for (const ZoneType type : ALL_ZONES)
                {
                    if (key == zone_key(type) && std::find(order.begin(), order.end(), type) == order.end())
                    {
                        order.push_back(type);
                    }
                }
            }
            for (const ZoneType type : ALL_ZONES)
            {
                if (std::find(order.begin(), order.end(), type) == order.end())
                {
                    order.push_back(type);
                }
            }
            return order;
        }

        bool zone_shown(const app::Settings& settings, ZoneType type)
        {
            switch (type)
            {
            case ZoneType::AUTHOR:
                return settings.show_author;
            case ZoneType::CHANGES:
                return settings.show_changes;
            case ZoneType::DATETIME:
                return settings.show_date;
            case ZoneType::SHA:
                return settings.show_sha;
            default:
                return true;
            }
        }

        const char* zone_label(ZoneType type)
        {
            switch (type)
            {
            case ZoneType::REF:
                return "BRANCH / TAG";
            case ZoneType::GRAPH:
                return "GRAPH";
            case ZoneType::MESSAGE:
                return "COMMIT MESSAGE";
            case ZoneType::AUTHOR:
                return "AUTHOR";
            case ZoneType::CHANGES:
                return "CHANGES";
            case ZoneType::DATETIME:
                return "COMMIT DATE / TIME";
            case ZoneType::SHA:
                return "SHA";
            }
            return "";
        }

        // GraphZoneConstants' minimumWidth (the graph's is COMMIT_ZONE_VIEWPORT_WIDTH_MIN).
        float zone_minimum(ZoneType type)
        {
            switch (type)
            {
            case ZoneType::REF:
            case ZoneType::AUTHOR:
                return 32.f;
            case ZoneType::GRAPH:
                return 56.f;
            default:
                return 50.f;
            }
        }

        // Widths for zones the user never dragged.
        float zone_default(ZoneType type)
        {
            switch (type)
            {
            case ZoneType::REF:
                return theme::GRAPH_REF_ZONE_DEFAULT_WIDTH;
            case ZoneType::GRAPH:
                return 150.f; // COMMIT_ZONE_DEFAULT_WIDTH
            case ZoneType::MESSAGE:
                return 400.f;
            case ZoneType::AUTHOR:
                return 170.f;
            case ZoneType::CHANGES:
                return 110.f;
            case ZoneType::DATETIME:
                return 150.f;
            case ZoneType::SHA:
                return 80.f;
            }
            return 100.f;
        }

        float zones_total(const std::vector<GraphZone>& zones, int skip = -1)
        {
            float total = 0.f;
            for (int i = 0; i < static_cast<int>(zones.size()); ++i)
            {
                if (i != skip)
                {
                    total += zones[static_cast<std::size_t>(i)].current;
                }
            }
            return total;
        }

        // The width a zone may take: its minimum and maximum (the last zone has no maximum).
        float clamp_zone(const std::vector<GraphZone>& zones, int index, float width)
        {
            const GraphZone& zone = zones[static_cast<std::size_t>(index)];
            const bool last = index == static_cast<int>(zones.size()) - 1;
            return std::max(zone.minimum, zone.maximum > 0.f && !last ? std::min(width, zone.maximum) : width);
        }

        void set_zone_width(std::vector<GraphZone>& zones, int index, float width, bool preferred)
        {
            GraphZone& zone = zones[static_cast<std::size_t>(index)];
            zone.current = clamp_zone(zones, index, width);
            if (preferred)
            {
                zone.preferred = zone.current;
            }
        }

        // shrinkZoneWidthsToFitWidth: the last zones above their minimum give way first, down to `from`.
        void shrink_zones(std::vector<GraphZone>& zones, float width, int from = 0)
        {
            for (std::size_t guard = 0; zones_total(zones) > width && guard <= zones.size() * 2; ++guard)
            {
                int index = -1;
                for (int i = static_cast<int>(zones.size()) - 1; i >= std::max(0, from); --i)
                {
                    if (zones[static_cast<std::size_t>(i)].current > zones[static_cast<std::size_t>(i)].minimum)
                    {
                        index = i;
                        break;
                    }
                }
                if (index < 0)
                {
                    break;
                }
                set_zone_width(zones, index, width - zones_total(zones, index), false);
            }
        }

        // expandZoneWidthsToFitWidth: the first zones under their preferred width (capped by their maximum) grow
        // back to it from `from` on; the last zone takes the rest.
        void expand_zones(std::vector<GraphZone>& zones, float width, int from = 0)
        {
            const int last = static_cast<int>(zones.size()) - 1;
            const auto cap = [](const GraphZone& zone)
            {
                return zone.maximum > 0.f ? std::min(zone.maximum, zone.preferred > 0.f ? zone.preferred : FLT_MAX)
                                          : zone.preferred;
            };
            for (std::size_t guard = 0; zones_total(zones) < width && guard <= zones.size() * 2; ++guard)
            {
                int index = last;
                for (int i = from; i <= last; ++i)
                {
                    const GraphZone& zone = zones[static_cast<std::size_t>(i)];
                    if (cap(zone) > 0.f && zone.current < cap(zone))
                    {
                        index = i;
                        break;
                    }
                }
                if (index < 0)
                {
                    break;
                }
                float grown = width - zones_total(zones, index);
                if (index != last)
                {
                    grown = std::min(grown, cap(zones[static_cast<std::size_t>(index)]));
                }
                set_zone_width(zones, index, grown, false);
            }
        }

        // adjustResizedGraphZone: a dragged zone takes its new width and the zones after it make room or fill in.
        void resize_zone(ZoneLayout& layout, int index, float width, bool final)
        {
            const float before = layout.zones[static_cast<std::size_t>(index)].current;
            set_zone_width(layout.zones, index, width, final);
            const float after = layout.zones[static_cast<std::size_t>(index)].current;
            if (before > after)
            {
                expand_zones(layout.zones, layout.width, index + 1);
            }
            if (before < after)
            {
                shrink_zones(layout.zones, layout.width, index - 1);
            }
        }

        // A dragged zone's limits: its minimum, and the room left by the zones before it and the minimums after.
        float max_zone_width(const ZoneLayout& layout, int index)
        {
            float room = layout.width;
            for (int i = 0; i < static_cast<int>(layout.zones.size()); ++i)
            {
                const GraphZone& zone = layout.zones[static_cast<std::size_t>(i)];
                room -= i < index ? zone.current : i > index ? zone.minimum : 0.f;
            }
            const float maximum = layout.zones[static_cast<std::size_t>(index)].maximum;
            return maximum > 0.f ? std::min(maximum, room) : room;
        }

        // Lays the zones out for a graph `width` CSS px wide, keeping the user's widths; `max_columns` is the
        // highest lane in use.
        void update_zones(const app::Settings& settings, float width, int max_columns)
        {
            std::vector<ZoneType> shown;
            for (const ZoneType type : zone_order(settings))
            {
                if (zone_shown(settings, type))
                {
                    shown.push_back(type);
                }
            }
            // getCommitZoneContentWidth: the lanes plus the gutters and paddings on both sides.
            const float content = static_cast<float>(std::max(max_columns, 1)) * theme::COMMIT_COLUMN_WIDTH
                                  + 2.f * theme::COMMIT_ZONE_GUTTER_WIDTH + 2.f * theme::COMMIT_ZONE_PADDING;
            const float graph_maximum = max_columns > 0 ? content : 0.f;

            bool same = shown.size() == g_zones.zones.size();
            for (std::size_t i = 0; same && i < shown.size(); ++i)
            {
                same = g_zones.zones[i].type == shown[i];
            }
            g_zones.width = width;
            if (!same)
            {
                // initializeGraphZones: the saved widths, the last zone also leaving room for the scrollbar.
                g_zone_drag = {};
                g_zones.zones.clear();
                for (const ZoneType type : shown)
                {
                    GraphZone zone;
                    zone.type = type;
                    zone.minimum = zone_minimum(type);
                    zone.maximum = type == ZoneType::GRAPH ? graph_maximum : 0.f;
                    const auto saved = settings.graph_column_widths.find(zone_key(type));
                    zone.preferred = saved != settings.graph_column_widths.end() && saved->second > 0.f
                                             ? saved->second
                                             : zone_default(type);
                    zone.current = zone.preferred;
                    g_zones.zones.push_back(zone);
                }
                g_zones.zones.back().minimum += SCROLLBAR_THICKNESS;
                for (int i = 0; i < static_cast<int>(g_zones.zones.size()); ++i)
                {
                    g_zones.zones[static_cast<std::size_t>(i)].current =
                            clamp_zone(g_zones.zones, i, g_zones.zones[static_cast<std::size_t>(i)].current);
                }
                g_zones.content = content;
            }
            else if (content != g_zones.content)
            {
                // updateCommitZoneContentWidthFromChange: the graph zone never outgrows its lanes.
                g_zones.content = content;
                for (int i = 0; i < static_cast<int>(g_zones.zones.size()); ++i)
                {
                    GraphZone& graph = g_zones.zones[static_cast<std::size_t>(i)];
                    if (graph.type != ZoneType::GRAPH)
                    {
                        continue;
                    }
                    graph.maximum = graph_maximum;
                    if (graph_maximum > 0.f && graph.current > graph_maximum)
                    {
                        graph.current = clamp_zone(g_zones.zones, i, graph_maximum);
                    }
                }
            }
            // ensureZoneWidthsMatchGraphWidth
            if (zones_total(g_zones.zones) > width)
            {
                shrink_zones(g_zones.zones, width);
            }
            if (zones_total(g_zones.zones) < width)
            {
                expand_zones(g_zones.zones, width);
            }
        }

        // The zones in device px from `x` (the first header's left edge); hidden zones stay empty.
        Zones place_zones(float x)
        {
            Zones z;
            float edge = 0.f;
            for (const GraphZone& zone : g_zones.zones)
            {
                const float left = x + std::round(dpi_scale() * edge);
                edge += zone.current;
                const float w = x + std::round(dpi_scale() * edge) - left;
                switch (zone.type)
                {
                case ZoneType::REF:
                    z.ref_x = left, z.ref_w = w;
                    break;
                case ZoneType::GRAPH:
                    z.graph_x = left, z.graph_w = w;
                    break;
                case ZoneType::MESSAGE:
                    z.message_x = left, z.message_w = w;
                    break;
                case ZoneType::AUTHOR:
                    z.author_x = left, z.author_w = w;
                    break;
                case ZoneType::CHANGES:
                    z.changes_x = left, z.changes_w = w;
                    break;
                case ZoneType::DATETIME:
                    z.date_x = left, z.date_w = w;
                    break;
                case ZoneType::SHA:
                    z.sha_x = left, z.sha_w = w;
                    break;
                }
            }
            return z;
        }

        // The right edge of the shown zone `index`, in device px from `x`.
        float zone_right(float x, int index)
        {
            float edge = 0.f;
            for (int i = 0; i <= index; ++i)
            {
                edge += g_zones.zones[static_cast<std::size_t>(i)].current;
            }
            return x + std::round(dpi_scale() * edge);
        }

        // Follows the mouse while a zone's edge is held; on release the zone keeps the width as its preferred one
        // and the column resize saves it.
        void apply_zone_drag(app::Application& app)
        {
            if (g_zone_drag.index < 0 || g_zone_drag.index >= static_cast<int>(g_zones.zones.size()) - 1)
            {
                g_zone_drag = {};
                return;
            }
            GraphZone& zone = g_zones.zones[static_cast<std::size_t>(g_zone_drag.index)];
            if (ImGui::IsMouseDown(ImGuiMouseButton_Left))
            {
                const float travel = (ImGui::GetIO().MousePos.x - g_zone_drag.start_mouse) / dpi_scale();
                const float width = std::max(zone.minimum, std::min(g_zone_drag.start_width + travel,
                                                                    max_zone_width(g_zones, g_zone_drag.index)));
                if (width != zone.current)
                {
                    resize_zone(g_zones, g_zone_drag.index, width, false);
                }
                return;
            }
            if (zone.current != g_zone_drag.start_width)
            {
                resize_zone(g_zones, g_zone_drag.index, zone.current, true);
                app.settings.graph_column_widths[zone_key(zone.type)] = zone.current;
                app.settings.save(app.config_path("settings.txt"));
            }
            g_zone_drag = {};
        }

        // The handles on the right edge of every zone but the last: `header` ones fill the header cell's last
        // 10px; the others, over the rows, are 10px centred 4px left of it (the rows sit 4px left of the header).
        void zone_handles(float x, float top, float bottom, bool header)
        {
            for (int i = 0; i + 1 < static_cast<int>(g_zones.zones.size()); ++i)
            {
                const float right = zone_right(x, i) - (header ? 0.f : px(4.f));
                const float half = header ? px(10.f) : px(5.f);
                const std::string id = std::string(header ? "##zone_header_resize_" : "##zone_resize_")
                                       + zone_key(g_zones.zones[static_cast<std::size_t>(i)].type);
                const ResizeHandle handle = resize_handle(id.c_str(), ImVec2(right - half, top),
                                                          ImVec2(right + (header ? 0.f : half), bottom));
                if (handle.activated)
                {
                    g_zone_drag = {i, g_zones.zones[static_cast<std::size_t>(i)].current, ImGui::GetIO().MousePos.x};
                }
            }
        }

        // A zone's placed edge and width (device px), by type.
        float* zone_x_of(Zones& z, ZoneType type)
        {
            switch (type)
            {
            case ZoneType::REF:
                return &z.ref_x;
            case ZoneType::GRAPH:
                return &z.graph_x;
            case ZoneType::MESSAGE:
                return &z.message_x;
            case ZoneType::AUTHOR:
                return &z.author_x;
            case ZoneType::CHANGES:
                return &z.changes_x;
            case ZoneType::DATETIME:
                return &z.date_x;
            case ZoneType::SHA:
                return &z.sha_x;
            }
            return &z.ref_x;
        }

        float* zone_w_of(Zones& z, ZoneType type)
        {
            switch (type)
            {
            case ZoneType::REF:
                return &z.ref_w;
            case ZoneType::GRAPH:
                return &z.graph_w;
            case ZoneType::MESSAGE:
                return &z.message_w;
            case ZoneType::AUTHOR:
                return &z.author_w;
            case ZoneType::CHANGES:
                return &z.changes_w;
            case ZoneType::DATETIME:
                return &z.date_w;
            case ZoneType::SHA:
                return &z.sha_w;
            }
            return &z.ref_w;
        }

        int zone_index(ZoneType type)
        {
            for (int i = 0; i < static_cast<int>(g_zones.zones.size()); ++i)
            {
                if (g_zones.zones[static_cast<std::size_t>(i)].type == type)
                {
                    return i;
                }
            }
            return -1;
        }

        // Moves the shown zone `from` to `to`, its widths with it; the zone now last gets the scrollbar's room and
        // the one it replaces its limits back, and the zones fit the graph again.
        void move_zone(app::Application& app, int from, int to)
        {
            std::vector<GraphZone>& zones = g_zones.zones;
            if (from == to || from < 0 || to < 0 || from >= static_cast<int>(zones.size())
                || to >= static_cast<int>(zones.size()))
            {
                return;
            }
            const GraphZone moved = zones[static_cast<std::size_t>(from)];
            zones.erase(zones.begin() + from);
            zones.insert(zones.begin() + to, moved);
            for (int i = 0; i < static_cast<int>(zones.size()); ++i)
            {
                GraphZone& zone = zones[static_cast<std::size_t>(i)];
                const bool last = i + 1 == static_cast<int>(zones.size());
                zone.minimum = zone_minimum(zone.type) + (last ? SCROLLBAR_THICKNESS : 0.f);
                zone.current = clamp_zone(zones, i, zone.current);
            }
            if (zones_total(zones) > g_zones.width)
            {
                shrink_zones(zones, g_zones.width);
            }
            if (zones_total(zones) < g_zones.width)
            {
                expand_zones(zones, g_zones.width);
            }
            // The saved order: the shown zones as they are now, the hidden ones after them.
            std::vector<std::string> order;
            for (const GraphZone& zone : zones)
            {
                order.emplace_back(zone_key(zone.type));
            }
            for (const ZoneType type : zone_order(app.settings))
            {
                const std::string key = zone_key(type);
                if (std::find(order.begin(), order.end(), key) == order.end())
                {
                    order.push_back(key);
                }
            }
            app.settings.graph_column_order = order;
        }

        // A header cell held and moved 4px becomes a column drag: the cell follows the mouse as a floating copy
        // and the zone swaps places with a neighbour once the cell's leading edge passes that neighbour's centre
        // (its centre would have to travel half its own width, far for the message column); the order is saved
        // on release.
        struct ZoneMove
        {
            ZoneType type = ZoneType::REF;
            bool pressed = false;
            bool active = false;
            float press_x = 0.f;
            float grab = 0.f; // the press, from the cell's left edge
            float width = 0.f; // the cell
        };

        ZoneMove g_zone_move;

        float g_graph_scroll = 0.f; // the graph zone's scrollLeft, CSS px

        GraphLanes graph_lanes(int max_columns)
        {
            constexpr float GUTTER = theme::COMMIT_ZONE_GUTTER_WIDTH;
            constexpr float PADDING = theme::COMMIT_ZONE_PADDING;
            constexpr float COLUMN = theme::COMMIT_COLUMN_WIDTH;
            GraphLanes lanes;
            for (const GraphZone& zone : g_zones.zones)
            {
                if (zone.type == ZoneType::GRAPH)
                {
                    lanes.width = zone.current;
                }
            }
            lanes.content = static_cast<float>(std::max(max_columns, 1)) * COLUMN + 2.f * GUTTER + 2.f * PADDING;
            // getScrollLeft: no scrolling once the zone shows all its lanes.
            g_graph_scroll = std::clamp(g_graph_scroll, 0.f, std::max(0.f, lanes.content - lanes.width));
            lanes.scroll = lanes.content != lanes.width ? g_graph_scroll : 0.f;
            lanes.factor = max_columns == 1
                                   ? 0.f
                                   : std::clamp(1.f - (lanes.width - 2.f * GUTTER - PADDING) / COLUMN, 0.f, 1.f);
            lanes.slide_out = std::clamp((lanes.width - (lanes.content - (GUTTER + PADDING))) / COLUMN, 0.f, 1.f)
                              + lanes.factor / 2.f;
            return lanes;
        }

        // The zone's body (its lanes' origin, where the gutters and the scrollbar start): a node's left edge sits
        // GUTTER - PADDING_LEFT + 1 into the graph header cell, which puts the body 2px
        // left of the cell.
        float lanes_x(const Zones& z)
        {
            return z.graph_x - px(2.f);
        }

        // A lane's centre, scrolled with the edges (getAbsoluteNodeLeftByColumn).
        float column_x(const Zones& z, int column)
        {
            return lanes_x(z) + px(theme::COMMIT_ZONE_GUTTER_WIDTH)
                   + static_cast<float>(column) * px(theme::COMMIT_COLUMN_WIDTH)
                   - std::round(z.lanes.scroll * dpi_scale()) + px(theme::COMMIT_COLUMN_WIDTH) / 2.f;
        }

        // A node's centre: its lane kept between getMinNodeOffset and getMaxNodeOffset (getNodeOffsetByColumn).
        float node_x(const Zones& z, int column)
        {
            constexpr float GUTTER = theme::COMMIT_ZONE_GUTTER_WIDTH;
            constexpr float PADDING = theme::COMMIT_ZONE_PADDING;
            const GraphLanes& l = z.lanes;
            const float lowest = (1.f - l.factor) * 2.f * PADDING + l.factor * GUTTER;
            const float highest = (1.f - l.factor) * (l.width + PADDING - GUTTER) + l.factor * GUTTER;
            const float left = GUTTER + static_cast<float>(column) * theme::COMMIT_COLUMN_WIDTH - l.scroll;
            return lanes_x(z) + std::round(std::max(lowest, std::min(left, highest)) * dpi_scale())
                   + px(theme::COMMIT_COLUMN_WIDTH) / 2.f;
        }

        // getNodeOpacityByColumn: nodes within half a gutter of the zone's edges fade to 50%.
        float node_opacity(const Zones& z, int column)
        {
            constexpr float GUTTER = theme::COMMIT_ZONE_GUTTER_WIDTH;
            constexpr float PADDING = theme::COMMIT_ZONE_PADDING;
            const GraphLanes& l = z.lanes;
            const float left = GUTTER + static_cast<float>(column) * theme::COMMIT_COLUMN_WIDTH - l.scroll;
            const float from_left = left - PADDING;
            const float from_right = l.width - PADDING - left - GUTTER * (1.f - l.slide_out);
            const float fade = std::max(std::min(from_left, from_right) / (GUTTER / 2.f), l.factor);
            return std::clamp(0.5f + 0.5f * fade, 0.f, 1.f);
        }

        // A gutter's box-shadow (`±5px 5px 10px rgba(0, 0, 0, alpha)`) as it falls beside the gutter: the gutter
        // moved 5px towards `direction` and blurred by 10px (a Gaussian of sigma 5).
        void gutter_shadow(ImDrawList* draw, float edge, float direction, float top, float bottom, float alpha)
        {
            if (alpha <= 0.f)
            {
                return;
            }
            const float sigma = 5.f * dpi_scale();
            const float offset = 5.f * dpi_scale();
            for (float d = 0.f; d < offset + 3.f * sigma; d += 1.f)
            {
                const float a = alpha * 0.5f * std::erfc((d + 0.5f - offset) / (sigma * std::sqrt(2.f)));
                const float x = direction > 0.f ? edge + d : edge - d - 1.f;
                draw->AddRectFilled(ImVec2(x, top), ImVec2(x + 1.f, bottom),
                                    IM_COL32(0, 0, 0, static_cast<int>(a * 255.f + 0.5f)));
            }
        }

        // Fades what was drawn since vertex `start` (CSS opacity on a node and its avatar).
        void fade_vertices(ImDrawList* draw, int start, float opacity)
        {
            if (opacity >= 1.f)
            {
                return;
            }
            for (int k = start; k < draw->VtxBuffer.Size; ++k)
            {
                ImU32& col = draw->VtxBuffer[k].col;
                const auto a =
                        static_cast<ImU32>(static_cast<float>((col >> IM_COL32_A_SHIFT) & 0xFF) * opacity + 0.5f);
                col = (col & ~IM_COL32_A_MASK) | (a << IM_COL32_A_SHIFT);
            }
        }

        // The edges are SVG lines and paths, 2px wide: axis-aligned pieces land on whole pixels and are
        // drawn as filled rectangles (an anti-aliased line fades at its ends, which leaves a seam where two rows
        // meet). Dashed edges use stroke-dasharray "2", phased from each element's first point like SVG.
        void stroke_segment(ImDrawList* draw, ImVec2 a, ImVec2 b, ImU32 color, float thickness)
        {
            const bool vertical = std::fabs(a.x - b.x) < 0.01f;
            const bool horizontal = std::fabs(a.y - b.y) < 0.01f;
            const float half = std::round(thickness) / 2.f;
            if (vertical)
            {
                const float x = std::round(a.x - half);
                draw->AddRectFilled(ImVec2(x, std::round(std::min(a.y, b.y))),
                                    ImVec2(x + 2.f * half, std::round(std::max(a.y, b.y))), color);
            }
            else if (horizontal)
            {
                const float y = std::round(a.y - half);
                draw->AddRectFilled(ImVec2(std::round(std::min(a.x, b.x)), y),
                                    ImVec2(std::round(std::max(a.x, b.x)), y + 2.f * half), color);
            }
            else
            {
                draw->AddLine(a, b, color, thickness);
            }
        }

        // An SVG path through `points`: solid paths draw axis pieces as rectangles and the rest as one
        // anti-aliased polyline; dashed ones walk the 2 on / 2 off pattern from the first point.
        void stroke_path(ImDrawList* draw, const std::vector<ImVec2>& points, ImU32 color, float thickness, bool dashed)
        {
            if (points.size() < 2)
            {
                return;
            }
            if (!dashed)
            {
                std::vector<ImVec2> curve;
                const auto flush = [&]
                {
                    if (curve.size() >= 2)
                    {
                        draw->AddPolyline(curve.data(), static_cast<int>(curve.size()), color, 0, thickness);
                    }
                    curve.clear();
                };
                for (std::size_t i = 0; i + 1 < points.size(); ++i)
                {
                    const ImVec2 a = points[i];
                    const ImVec2 b = points[i + 1];
                    if (std::fabs(a.x - b.x) < 0.01f || std::fabs(a.y - b.y) < 0.01f)
                    {
                        flush();
                        stroke_segment(draw, a, b, color, thickness);
                    }
                    else
                    {
                        if (curve.empty())
                        {
                            curve.push_back(a);
                        }
                        curve.push_back(b);
                    }
                }
                flush();
                return;
            }
            const float dash = px(2.f);
            float travelled = 0.f;
            for (std::size_t i = 0; i + 1 < points.size(); ++i)
            {
                const ImVec2 a = points[i];
                const ImVec2 b = points[i + 1];
                const float length = std::hypot(b.x - a.x, b.y - a.y);
                if (length <= 0.f)
                {
                    continue;
                }
                const ImVec2 dir((b.x - a.x) / length, (b.y - a.y) / length);
                float s = 0.f;
                while (s < length - 1e-4f)
                {
                    const float phase = std::fmod(travelled + s, 2.f * dash);
                    const bool on = phase < dash;
                    const float step = std::min(length - s, on ? dash - phase : 2.f * dash - phase);
                    if (on)
                    {
                        stroke_segment(draw, ImVec2(a.x + dir.x * s, a.y + dir.y * s),
                                       ImVec2(a.x + dir.x * (s + step), a.y + dir.y * (s + step)), color, thickness);
                    }
                    s += step;
                }
                travelled += length;
            }
        }

        void stroke_line(ImDrawList* draw, ImVec2 a, ImVec2 b, ImU32 color, float thickness, bool dashed)
        {
            stroke_path(draw, {a, b}, color, thickness, dashed);
        }

        // Points of a counter-clockwise (SVG sweep-flag 0) arc from angle `from` to `to` around `center`.
        void append_arc(std::vector<ImVec2>& points, ImVec2 center, float radius, float from, float to)
        {
            constexpr int SEGMENTS = 8;
            for (int i = 1; i <= SEGMENTS; ++i)
            {
                const float t = from + (to - from) * static_cast<float>(i) / SEGMENTS;
                points.emplace_back(center.x + radius * std::cos(t), center.y + radius * std::sin(t));
            }
        }

        // The edges of one row: pass-throughs, starting and ending edges per lane.
        // A turn is EDGE_ARC_RADIUS (11) from its corner, drawn as an arc of radius 11 - EDGE_ARC_PADDING (3).
        void draw_edges(ImDrawList* draw, const Zones& z, const graph::RowLayout& row, float y0, float alpha)
        {
            const float h = px(theme::GRAPH_ROW_HEIGHT);
            const float turn = px(theme::COMMIT_ZONE_EDGE_ARC_RADIUS);
            const float radius = turn - px(theme::COMMIT_ZONE_MARGIN);
            const float pad = px(theme::COMMIT_ZONE_MARGIN);
            const float thickness = px(theme::COMMIT_ZONE_LINE_WIDTH);
            const float mid_y = y0 + h / 2.f;
            const int node_column = row.column;
            const float node_x = column_x(z, node_column);

            for (const auto& [column, edges] : row.edges)
            {
                const ImU32 color = theme::u32(theme::with_alpha(theme::graph_color(column), alpha));
                const float x = column_x(z, column);
                // +1: the node is to the right of the lane, -1: to its left.
                const float side = node_column < column ? -1.f : 1.f;
                if (edges.pass_through)
                {
                    stroke_line(draw, ImVec2(x, y0), ImVec2(x, y0 + h), color, thickness,
                                graph::is_dashed(edges.pass_through->type));
                }
                if (edges.starting)
                {
                    const bool dashed = graph::is_dashed(edges.starting->type);
                    if (column == node_column)
                    {
                        stroke_line(draw, ImVec2(x, mid_y), ImVec2(x, y0 + h), color, thickness, dashed);
                    }
                    else
                    {
                        std::vector<ImVec2> path;
                        const ImVec2 center(x + side * radius, mid_y + radius);
                        if (side < 0.f)
                        {
                            path = {ImVec2(x, y0 + h - pad), ImVec2(x, mid_y + radius)};
                            append_arc(path, center, radius, 0.f, -0.5f * PI);
                            path.emplace_back(x - turn, mid_y);
                        }
                        else
                        {
                            path = {ImVec2(x + turn, mid_y), ImVec2(x + radius, mid_y)};
                            append_arc(path, center, radius, 1.5f * PI, PI);
                            path.emplace_back(x, y0 + h - pad);
                        }
                        stroke_path(draw, path, color, thickness, dashed);
                        stroke_line(draw, ImVec2(x, y0 + h - pad), ImVec2(x, y0 + h), color, thickness, dashed);
                        stroke_line(draw, ImVec2(x + side * turn, mid_y), ImVec2(node_x, mid_y), color, thickness,
                                    dashed);
                    }
                }
                if (edges.ending)
                {
                    const bool dashed = graph::is_dashed(edges.ending->type);
                    if (column == node_column)
                    {
                        stroke_line(draw, ImVec2(x, y0), ImVec2(x, mid_y), color, thickness, dashed);
                    }
                    else
                    {
                        stroke_line(draw, ImVec2(x, y0), ImVec2(x, y0 + pad), color, thickness, dashed);
                        std::vector<ImVec2> path;
                        const ImVec2 center(x + side * radius, mid_y - radius);
                        if (side < 0.f)
                        {
                            path = {ImVec2(x - turn, mid_y), ImVec2(x - radius, mid_y)};
                            append_arc(path, center, radius, 0.5f * PI, 0.f);
                            path.emplace_back(x, y0 + pad);
                        }
                        else
                        {
                            path = {ImVec2(x, y0 + pad), ImVec2(x, mid_y - radius)};
                            append_arc(path, center, radius, PI, 0.5f * PI);
                            path.emplace_back(x + turn, mid_y);
                        }
                        stroke_path(draw, path, color, thickness, dashed);
                        stroke_line(draw, ImVec2(x + side * turn, mid_y), ImVec2(node_x, mid_y), color, thickness,
                                    dashed);
                    }
                }
            }
        }

        void draw_node(ImDrawList* draw, const Zones& z, const git::Commit& commit, const graph::RowLayout& row,
                       float y0, bool is_head, float alpha)
        {
            const theme::Palette& p = theme::palette();
            const float h = px(theme::GRAPH_ROW_HEIGHT);
            const ImVec2 center(node_x(z, row.column), y0 + h / 2.f);
            const ImVec4 color = theme::with_alpha(theme::graph_color(row.column), alpha);
            const ImU32 color_u32 = theme::u32(color);
            switch (commit.type)
            {
            case git::NodeType::MERGE:
                draw->AddCircleFilled(center, px(theme::COMMIT_MERGE_NODE_DIAMETER) / 2.f, color_u32, 16);
                break;
            case git::NodeType::STASH:
            {
                // .node.stash-node: the 22px node with a 2px dotted border in the lane colour around far:inbox.
                const float half = px(theme::COMMIT_NODE_DIAMETER) / 2.f;
                const ImVec2 min(center.x - half, center.y - half);
                const ImVec2 max(center.x + half, center.y + half);
                draw->AddRectFilled(min, max, theme::u32(p.app_bg0));
                const float dot = px(2.f);
                for (float t = 0.f; t < max.x - min.x; t += dot * 2.f)
                {
                    const float e = std::min(t + dot, max.x - min.x);
                    draw->AddRectFilled(ImVec2(min.x + t, min.y), ImVec2(min.x + e, min.y + dot), color_u32);
                    draw->AddRectFilled(ImVec2(min.x + t, max.y - dot), ImVec2(min.x + e, max.y), color_u32);
                    draw->AddRectFilled(ImVec2(min.x, min.y + t), ImVec2(min.x + dot, min.y + e), color_u32);
                    draw->AddRectFilled(ImVec2(max.x - dot, min.y + t), ImVec2(max.x, min.y + e), color_u32);
                }
                draw_fa_icon(draw, "far:inbox", ImVec2(center.x - px(0.25f), center.y + px(0.5f)), px(12.f), color_u32);
                break;
            }
            case git::NodeType::WORK_DIR:
            {
                // .node.work-dir-changes: the empty 22px node under an SVG circle r=10, stroke-width 2,
                // stroke-dasharray "2 3" with round caps, starting at 3 o'clock and running clockwise.
                const float radius = px(10.f);
                draw->AddCircleFilled(center, px(theme::COMMIT_NODE_DIAMETER) / 2.f, theme::u32(p.app_bg0), 32);
                const float circumference = 2.f * PI * radius;
                const float dash = px(2.f) / radius;
                const float period = px(5.f) / radius;
                for (float a = 0.f; a < 2.f * PI - 1e-4f; a += period)
                {
                    const float a1 = std::min(a + dash, 2.f * PI);
                    draw->PathArcTo(center, radius, a, a1, 3);
                    draw->PathStroke(color_u32, 0, px(2.f));
                    // Round caps: a 1px disc on each end of the dash.
                    draw->AddCircleFilled(ImVec2(center.x + radius * std::cos(a), center.y + radius * std::sin(a)),
                                          px(1.f), color_u32, 8);
                    draw->AddCircleFilled(ImVec2(center.x + radius * std::cos(a1), center.y + radius * std::sin(a1)),
                                          px(1.f), color_u32, 8);
                }
                (void)circumference;
                break;
            }
            case git::NodeType::COMMIT:
            {
                // .node { border: 2px solid lane } around the 18px avatar.
                const float radius = px(theme::COMMIT_NODE_DIAMETER) / 2.f;
                draw->AddCircleFilled(center, radius, color_u32, 32);
                draw_avatar(draw, center, px(theme::COMMIT_ZONE_AVATAR_DIAMETER), commit.author);
                (void)is_head;
                break;
            }
            }
        }

        // The refs on a commit grouped by short name (a local branch,
        // its remote-tracking branches and a tag of the same name share one node), the checked-out group first,
        // then local over remote over tag groups, bigger groups first, then by name.
        struct RefGroup
        {
            std::string name;
            std::vector<const git::Ref*> refs;
            bool current = false;
        };

        std::string group_name(const git::Ref& ref)
        {
            if (ref.type == git::RefType::REMOTE_BRANCH && ref.name.size() > ref.remote.size())
            {
                return ref.name.substr(ref.remote.size() + 1);
            }
            return ref.name;
        }

        std::vector<RefGroup> ref_groups(const std::vector<const git::Ref*>& refs)
        {
            std::vector<RefGroup> groups;
            const auto add = [&](const git::Ref* ref) -> RefGroup&
            {
                const std::string name = group_name(*ref);
                for (RefGroup& group : groups)
                {
                    if (group.name == name)
                    {
                        group.refs.push_back(ref);
                        return group;
                    }
                }
                groups.push_back(RefGroup{name, {ref}, false});
                return groups.back();
            };
            for (const git::RefType type : {git::RefType::LOCAL_BRANCH, git::RefType::REMOTE_BRANCH, git::RefType::TAG})
            {
                for (const git::Ref* ref : refs)
                {
                    if (ref->type == type)
                    {
                        RefGroup& group = add(ref);
                        group.current = group.current || ref->is_head;
                    }
                }
            }
            const auto priority = [](const RefGroup& group)
            {
                switch (group.refs.front()->type)
                {
                case git::RefType::LOCAL_BRANCH:
                    return 2;
                case git::RefType::REMOTE_BRANCH:
                    return 1;
                default:
                    return 0;
                }
            };
            std::stable_sort(groups.begin(), groups.end(),
                             [&](const RefGroup& a, const RefGroup& b)
                             {
                                 if (a.current != b.current)
                                 {
                                     return a.current;
                                 }
                                 if (priority(a) != priority(b))
                                 {
                                     return priority(a) > priority(b);
                                 }
                                 if (a.refs.size() != b.refs.size())
                                 {
                                     return a.refs.size() > b.refs.size();
                                 }
                                 // localeCompare: case-insensitive first.
                                 const auto lower = [](std::string s)
                                 {
                                     std::transform(s.begin(), s.end(), s.begin(),
                                                    [](unsigned char c)
                                                    {
                                                        return static_cast<char>(std::tolower(c));
                                                    });
                                     return s;
                                 };
                                 const std::string la = lower(a.name);
                                 const std::string lb = lower(b.name);
                                 return la != lb ? la < lb : a.name < b.name;
                             });
            return groups;
        }

        // One .ref-avatar icon per ref of the group: laptop (local), cloud (remote), tag.
        Icon ref_icon(const git::Ref& ref)
        {
            switch (ref.type)
            {
            case git::RefType::REMOTE_BRANCH:
                return Icon::CLOUD;
            case git::RefType::TAG:
                return Icon::TAG;
            default:
                return Icon::LAPTOP;
            }
        }

        std::string ref_icon_tooltip(const git::Ref& ref)
        {
            switch (ref.type)
            {
            case git::RefType::REMOTE_BRANCH:
                return ref.remote + " -> " + group_name(ref) + " (Remote Branch)";
            case git::RefType::TAG:
                return (ref.annotated && !ref.message.empty() ? ref.message : ref.name) + " (Tag)";
            default:
                return ref.name + " (local)";
            }
        }

        constexpr float REF_ICON_EM = 12.f;
        constexpr float REF_PAD = 5.f; // .ref-node.p1

        float ref_icons_width(const RefGroup& group)
        {
            float w = 0.f;
            for (std::size_t i = 0; i < group.refs.size(); ++i)
            {
                w += icon_width(ref_icon(*group.refs[i]), px(REF_ICON_EM))
                     + (i + 1 < group.refs.size() ? px(5.f) : 0.f);
            }
            return w;
        }

        // Width of the .ref-node at its content size: padding, [check + .6rem], name + .mr1, icons, padding.
        float ref_node_width(const RefGroup& group)
        {
            const float check = group.current ? px(REF_ICON_EM) + px(6.f) : 0.f;
            return px(REF_PAD) * 2.f + check + text_size(fonts().regular, theme::FONT_SIZE_ROW, group.name.c_str()).x
                   + px(5.f) + ref_icons_width(group);
        }

        struct RefNodeResult
        {
            bool clicked = false;
            bool double_clicked = false;
            bool right_clicked = false;
            bool hovered = false;
            float width = 0.f; // as drawn: a drop target loses its check and gains its 2px band
        };

        // A ref group being dragged: it starts 4px from the press (Windows'
        // SM_CXDRAG) and follows the mouse as Chromium's drag image, the pill at 75% opacity held where it was
        // grabbed. Mouse events stop during a native drag, so hover state stays as it was when it started.
        struct RefDrag
        {
            bool pressed = false;
            bool active = false;
            ImVec2 press;
            ImVec2 grab; // the press in the pill
            float width = 0.f;
            ImVec4 lane;
            std::string name;
            bool current = false;
            std::vector<git::Ref> refs; // copied: a refresh may rebuild the history mid-drag
            std::string repo;
        };

        RefDrag g_drag;
        // The pill under the mouse this frame that accepts the drop.
        std::optional<git::Ref> g_drop_target;

        // 1px of a box-shadow ring `offset` px outside [min, max].
        void shadow_ring(ImDrawList* draw, ImVec2 min, ImVec2 max, float offset, ImU32 color)
        {
            const float o = px(offset);
            const float one = px(1.f);
            draw->AddRectFilled(ImVec2(min.x - o - one, min.y - o - one), ImVec2(max.x + o + one, min.y - o), color);
            draw->AddRectFilled(ImVec2(min.x - o - one, max.y + o), ImVec2(max.x + o + one, max.y + o + one), color);
            draw->AddRectFilled(ImVec2(min.x - o - one, min.y - o), ImVec2(min.x - o, max.y + o), color);
            draw->AddRectFilled(ImVec2(max.x + o, min.y - o), ImVec2(max.x + o + one, max.y + o), color);
        }

        // The .ref-node pill: the lane colour mixed into the graph background at `weight`, the check of the
        // checked-out branch, the name and one icon per ref; `opacity` fades all of it.
        void draw_ref_pill(ImDrawList* draw, const RefGroup& group, ImVec2 min, ImVec2 max, ImVec4 lane_color,
                           float opacity, float weight, bool show_check, bool tooltips)
        {
            const theme::Palette& p = theme::palette();
            const auto fade = [&](ImVec4 c)
            {
                return theme::u32(theme::with_alpha(c, c.w * opacity));
            };
            draw->AddRectFilled(min, max, fade(theme::mix(lane_color, p.app_bg0, weight)), px(2.f));
            const ImU32 color = fade(group.current ? p.text_selected : p.text_normal);
            const float mid = (min.y + max.y) / 2.f;
            float x = min.x + px(REF_PAD);
            draw->PushClipRect(min, max, true);
            if (group.current && show_check)
            {
                const float w = px(REF_ICON_EM);
                draw_icon(draw, Icon::CHECK, ImVec2(x + w / 2.f, mid), w, color);
                if (tooltips)
                {
                    tooltip_on_rect(ImVec2(x, min.y), ImVec2(x + w, max.y), "current", true);
                }
                x += w + px(6.f);
            }
            const float icons_w = ref_icons_width(group);
            const float name_room = std::max(0.f, max.x - px(REF_PAD) - icons_w - px(5.f) - x);
            const float line_h = text_size(fonts().regular, theme::FONT_SIZE_ROW, "A").y;
            draw_text_ellipsis(draw, fonts().regular, theme::FONT_SIZE_ROW, ImVec2(x, mid - line_h / 2.f), name_room,
                               color, group.name.c_str());
            x += name_room + px(5.f);
            for (std::size_t i = 0; i < group.refs.size(); ++i)
            {
                const git::Ref& ref = *group.refs[i];
                const Icon icon = ref_icon(ref);
                const float w = icon_width(icon, px(REF_ICON_EM));
                draw_icon(draw, icon, ImVec2(x + w / 2.f, mid), px(REF_ICON_EM), color);
                if (tooltips)
                {
                    tooltip_on_rect(ImVec2(x, min.y), ImVec2(x + w, max.y), ref_icon_tooltip(ref).c_str(), true);
                }
                x += w + px(5.f);
            }
            draw->PopClipRect();
        }

        // .ref-node: lane colour mixed into the graph background at 25% (45% hovered, 50% pressed or for the
        // checked-out branch, which also gets the selected text colour); `opacity` dims ghost refs (.dim-ref).
        // Pressing it arms a drag; while another group is dragged it shows whether it takes the drop.
        RefNodeResult ref_node(app::Application& app, app::RepoState& state, const RefGroup& group, ImDrawList* draw,
                               ImVec2 pos, float width, ImVec4 lane_color, float opacity, const char* id)
        {
            (void)app;
            const float h = px(theme::REF_ZONE_TEXT_HEIGHT);
            const git::Ref& primary = *group.refs.front();
            RefNodeResult result;
            ImGui::SetCursorScreenPos(pos);
            ImGui::PushID(id);
            ImGui::PushID(primary.full_name.c_str());
            ImGui::InvisibleButton("##ref_node", ImVec2(width, h));
            result.hovered = ImGui::IsItemHovered() && !g_drag.active;
            result.clicked = ImGui::IsItemClicked(ImGuiMouseButton_Left);
            result.right_clicked = result.hovered && ImGui::IsMouseReleased(ImGuiMouseButton_Right);
            result.double_clicked = result.hovered && ImGui::IsMouseDoubleClicked(ImGuiMouseButton_Left);
            const bool pressed = ImGui::IsItemActive() && !g_drag.active;
            if (ImGui::IsItemActivated())
            {
                const ImVec2 mouse = ImGui::GetIO().MousePos;
                g_drag = {true,       false,
                          mouse,      ImVec2(mouse.x - pos.x, mouse.y - pos.y),
                          width,      lane_color,
                          group.name, group.current,
                          {},         state.repo.workdir()};
                for (const git::Ref* ref : group.refs)
                {
                    g_drag.refs.push_back(*ref);
                }
            }
            ImGui::PopID();
            ImGui::PopID();

            // While another group is dragged, the pills it can drop on (.ref-node-wrapper.droppable): a 2px gold band
            // (padding 2px 2px 2px 0) around the pill, 18px tall, with a gold glow, green under the mouse
            // (.hovering); their check icon is hidden.
            bool droppable = false;
            bool over = false;
            if (g_drag.active && g_drag.repo == state.repo.workdir() && !g_drag.refs.empty())
            {
                droppable = can_drop_ref(state, g_drag.refs.front(), primary);
            }
            if (droppable && group.current)
            {
                width -= px(REF_ICON_EM) + px(6.f);
            }
            if (droppable)
            {
                const ImVec2 mouse = ImGui::GetIO().MousePos;
                over = ImGui::IsWindowHovered(ImGuiHoveredFlags_ChildWindows
                                              | ImGuiHoveredFlags_AllowWhenBlockedByActiveItem)
                       && mouse.x >= pos.x && mouse.x < pos.x + width + px(2.f) && mouse.y >= pos.y
                       && mouse.y < pos.y + h;
                if (over)
                {
                    g_drop_target = primary;
                }
                draw->AddRectFilled(pos, ImVec2(pos.x + width + px(2.f), pos.y + h),
                                    theme::u32(theme::with_alpha(theme::palette().yellow, 0.3f * opacity)));
                // box-shadow: 0 0 2px 2px.
                const ImVec4 glow = over ? theme::with_alpha(theme::palette().green, 0.5f * opacity)
                                         : theme::with_alpha(theme::palette().yellow, 0.3f * opacity);
                const ImVec2 pill_min(pos.x, pos.y + px(2.f));
                const ImVec2 pill_max(pos.x + width, pos.y + h - px(2.f));
                shadow_ring(draw, pill_min, pill_max, 0.f, theme::u32(glow));
                shadow_ring(draw, pill_min, pill_max, 1.f, theme::u32(theme::with_alpha(glow, glow.w * 0.5f)));
                shadow_ring(draw, pill_min, pill_max, 2.f, theme::u32(theme::with_alpha(glow, glow.w * 0.12f)));
                draw_ref_pill(draw, group, pill_min, pill_max, lane_color, opacity, group.current ? 0.50f : 0.25f,
                              false, false);
                result.width = width + px(2.f);
                return result;
            }
            const float weight = group.current || pressed ? 0.50f : (result.hovered ? 0.45f : 0.25f);
            draw_ref_pill(draw, group, pos, ImVec2(pos.x + width, pos.y + h), lane_color, opacity, weight, true,
                          !g_drag.active);
            result.width = width;
            return result;
        }

        // .overflow-count: "+N" more ref groups, 3px padding, the lane colour at 25%.
        float overflow_count_width(std::size_t count)
        {
            const std::string text = "+" + std::to_string(count);
            return text_size(fonts().regular, theme::FONT_SIZE_ROW, text.c_str()).x + px(6.f);
        }

        void overflow_count(ImDrawList* draw, ImVec2 pos, std::size_t count, ImVec4 lane_color, float opacity)
        {
            const theme::Palette& p = theme::palette();
            const std::string text = "+" + std::to_string(count);
            const float w = overflow_count_width(count);
            const float h = px(theme::REF_ZONE_TEXT_HEIGHT);
            const ImVec4 bg = theme::mix(lane_color, p.app_bg0, 0.25f);
            draw->AddRectFilled(pos, ImVec2(pos.x + w, pos.y + h), theme::u32(theme::with_alpha(bg, bg.w * opacity)),
                                px(2.f));
            const float line_h = text_size(fonts().regular, theme::FONT_SIZE_ROW, "A").y;
            draw_text(draw, fonts().regular, theme::FONT_SIZE_ROW, ImVec2(pos.x + px(3.f), pos.y + (h - line_h) / 2.f),
                      theme::u32(theme::with_alpha(p.text_normal, p.text_normal.w * opacity)), text.c_str());
        }

        // The ref group whose rows stay lit: its name and the kinds of refs it holds.
        struct RefGroupKey
        {
            std::string name;
            bool head = false;
            std::vector<std::string> remotes{}; // remote names
            bool tag = false;

            [[nodiscard]] bool operator==(const RefGroupKey&) const = default;
        };

        RefGroupKey group_key(const RefGroup& group)
        {
            RefGroupKey key{group.name};
            for (const git::Ref* ref : group.refs)
            {
                switch (ref->type)
                {
                case git::RefType::LOCAL_BRANCH:
                    key.head = true;
                    break;
                case git::RefType::REMOTE_BRANCH:
                    key.remotes.push_back(ref->remote);
                    break;
                case git::RefType::TAG:
                    key.tag = true;
                    break;
                case git::RefType::STASH:
                    break;
                }
            }
            return key;
        }

        std::vector<const git::Ref*> branch_and_tag_refs(const app::RepoState& state, const git::Sha& sha)
        {
            std::vector<const git::Ref*> refs = state.refs_at(sha);
            std::erase_if(refs,
                          [](const git::Ref* ref)
                          {
                              return ref->type == git::RefType::STASH;
                          });
            return refs;
        }

        // The refs a row stands for: its own (a tag-only row's only while a tag is the key), else its childRefs.
        std::vector<const git::Ref*> row_refs(const app::RepoState& state, std::size_t row, bool tag_key)
        {
            const std::vector<git::Commit>& commits = state.history.commits;
            std::vector<const git::Ref*> refs = branch_and_tag_refs(state, commits[row].sha);
            const bool tags_only = !refs.empty()
                                   && std::all_of(refs.begin(), refs.end(),
                                                  [](const git::Ref* ref)
                                                  {
                                                      return ref->type == git::RefType::TAG;
                                                  });
            if (refs.empty() || (tags_only && !tag_key))
            {
                const int child = row < state.child_ref_rows.size() ? state.child_ref_rows[row] : -1;
                refs = child >= 0 ? branch_and_tag_refs(state, commits[static_cast<std::size_t>(child)].sha)
                                  : std::vector<const git::Ref*>{};
            }
            return refs;
        }

        // isMissingHoveredRefGroup: none of the row's refs is the key's branch, remote branch or tag.
        bool row_misses_group(const app::RepoState& state, std::size_t row, const RefGroupKey& key)
        {
            for (const git::Ref* ref : row_refs(state, row, key.tag))
            {
                if (group_name(*ref) != key.name)
                {
                    continue;
                }
                switch (ref->type)
                {
                case git::RefType::LOCAL_BRANCH:
                    if (key.head)
                    {
                        return false;
                    }
                    break;
                case git::RefType::REMOTE_BRANCH:
                    if (std::find(key.remotes.begin(), key.remotes.end(), ref->remote) != key.remotes.end())
                    {
                        return false;
                    }
                    break;
                case git::RefType::TAG:
                    if (key.tag)
                    {
                        return false;
                    }
                    break;
                case git::RefType::STASH:
                    break;
                }
            }
            return true;
        }

        // The timeline bucket of a commit `age` seconds old, on format_relative's steps: 0 within the hour, then
        // rising with each hour, "yesterday", day, week, month and year it reads.
        int timeline_bucket(std::int64_t age)
        {
            constexpr std::int64_t HOUR = 3600;
            constexpr std::int64_t DAY = 86400;
            if (age < HOUR)
            {
                return 0;
            }
            if (age < DAY)
            {
                return static_cast<int>(age / HOUR); // 1..23
            }
            if (age < DAY * 2)
            {
                return 24; // yesterday
            }
            if (age < DAY * 7)
            {
                return 23 + static_cast<int>(age / DAY); // 25..29
            }
            if (age < DAY * 30)
            {
                return 29 + static_cast<int>(age / (DAY * 7)); // 30..33
            }
            if (age < DAY * 365)
            {
                return 33 + static_cast<int>(age / (DAY * 30)); // 34..45
            }
            return 45 + static_cast<int>(std::min<std::int64_t>(age / (DAY * 365), 1000));
        }

        // A graph header cell: ".fs-1 .ml1 .font-monospace" in text-disabled, a 1px section border on its left.
        void header_cell(ImDrawList* draw, float x, float w, float y, float h, const char* label, bool border)
        {
            const theme::Palette& p = theme::palette();
            if (w <= 0.f)
            {
                return;
            }
            // Baseline 15.5px below the header's top edge, for the 10px Consolas line.
            const float ascent = std::ceil(px(10.f) * 1884.f / 2048.f);
            draw_text_ellipsis(draw, fonts().mono, 10.f, ImVec2(x + px(6.f), y + px(15.5f) - ascent), w - px(10.f),
                               theme::u32(p.text_disabled), label);
            if (border)
            {
                // The previous zone's resize handle has the 1px border-right, just left of this cell.
                draw->AddRectFilled(ImVec2(x - px(1.f), y), ImVec2(x, y + h), theme::u32(p.section_border));
            }
        }

        // WIP row stats: "✎ 66  + 34  − 3" (modified / added / deleted files), coloured.
        float draw_wip_stats(ImDrawList* draw, ImVec2 pos, const git::WorkDirStatus& status)
        {
            const theme::Palette& p = theme::palette();
            int modified = 0, added = 0, deleted = 0;
            const auto count = [&](const std::vector<git::FileChange>& changes)
            {
                for (const git::FileChange& change : changes)
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
                    default:
                        ++modified;
                        break;
                    }
                }
            };
            count(status.staged);
            count(status.unstaged);
            count(status.conflicted);
            // .tiny-files-readout: a 10px solid icon in the status colour, 5px, the count in text-normal, 6px.
            float x = pos.x;
            const float mid = pos.y + text_size(fonts().regular, theme::FONT_SIZE_ROW, "A").y / 2.f;
            const auto item = [&](const char* icon, int value, ImVec4 color)
            {
                if (value == 0)
                {
                    return;
                }
                const float icon_w = fa_icon_width(icon, px(10.f));
                draw_fa_icon(draw, icon, ImVec2(x + icon_w / 2.f, mid), px(10.f), theme::u32(color));
                x += icon_w + px(5.f);
                const std::string text = std::to_string(value);
                draw_text(draw, fonts().regular, theme::FONT_SIZE_ROW, ImVec2(x, pos.y), theme::u32(p.text_normal),
                          text.c_str());
                x += text_size(fonts().regular, theme::FONT_SIZE_ROW, text.c_str()).x + px(6.f);
            };
            item("fas:pencil", modified, p.orange);
            item("fas:plus", added, p.green);
            item("fas:minus", deleted, p.red);
            return x - pos.x;
        }

        // The commit search (Ctrl+F), absolute 4px below the graph's top and 12px from its right edge, over
        // the headers and rows: a .filtering (blue 50% over panel-bg1) rad4 bar holding a fixed-width far:search, a
        // 200px input (black 20%, 8% white border), the 85px "{0} of {1}" / "0 results" and the up / down / close
        // icons, in 14px text with 5px (.px1 / .mr1 / .mx1) and 10px (.px2) spacing. Its own window keeps the rows
        // under it from hovering.
        void commit_search_bar(app::RepoState& state, ImVec2 origin, ImVec2 size)
        {
            if (!state.search_open)
            {
                return;
            }
            const theme::Palette& p = theme::palette();
            constexpr float WIDTH = 411.6f;
            constexpr float HEIGHT = 34.f;
            const ImVec2 min(std::round(origin.x + size.x - px(12.f + WIDTH)), std::round(origin.y + px(4.f)));
            const ImVec2 max(min.x + std::round(px(WIDTH)), min.y + px(HEIGHT));
            ImGui::SetNextWindowPos(min);
            ImGui::SetNextWindowSize(ImVec2(max.x - min.x, max.y - min.y));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowMinSize, ImVec2(1.f, 1.f));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
            ImGui::Begin("##commit_search_bar", nullptr,
                         ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoBackground
                                 | ImGuiWindowFlags_NoSavedSettings | ImGuiWindowFlags_NoFocusOnAppearing
                                 | ImGuiWindowFlags_NoNav | ImGuiWindowFlags_NoMove
                                 | ImGuiWindowFlags_NoScrollWithMouse);
            ImGui::PopStyleVar(3);
            ImDrawList* draw = ImGui::GetWindowDrawList();
            draw->AddRectFilled(min, max, theme::u32(p.panel_bg1), px(4.f));
            draw->AddRectFilled(min, max, theme::u32(theme::with_alpha(p.accent, 0.5f)), px(4.f));
            const ImU32 white = theme::u32(p.text_selected);
            const float mid = min.y + px(HEIGHT / 2.f);
            const float baseline = min.y + px(22.f);
            const float ascent = font_px(fonts().regular, 14.f) * 2189.f / 2789.f;
            const auto at = [&](float x)
            {
                return min.x + px(x);
            };
            draw_fa_icon(draw, "far:search", ImVec2(at(18.75f), mid), px(14.f), white);

            // The input: Enter / Shift+Enter and Down / Up step through the matches; Ctrl+F selects its text.
            static bool refocus = false;
            const ImVec2 input_min(std::round(at(32.5f)), min.y + px(5.f));
            const ImVec2 input_max(input_min.x + px(200.f), input_min.y + px(24.f));
            // The border over the fill: the whole box black 20% + white 8%, its inside black 20% alone (opaque, so
            // the border pixels under it don't show through).
            const ImVec4 bar = theme::mix(p.accent, p.panel_bg1, 0.5f);
            const ImVec4 fill = theme::mix(ImVec4(0.f, 0.f, 0.f, 1.f), bar, 0.2f);
            draw->AddRectFilled(input_min, input_max, theme::u32(theme::mix(p.white, fill, 0.08f)), px(2.f));
            draw->AddRectFilled(ImVec2(input_min.x + 1.f, input_min.y + 1.f),
                                ImVec2(input_max.x - 1.f, input_max.y - 1.f), theme::u32(fill), px(1.f));
            const ImGuiID input_id = ImGui::GetID("##commit_search");
            const bool was_active = ImGui::GetActiveID() == input_id;
            const bool select_all = state.search_focus_requested;
            if (state.search_focus_requested || refocus)
            {
                ImGui::SetKeyboardFocusHere();
                state.search_focus_requested = false;
                refocus = false;
            }
            const float pad_y = std::max(0.f, baseline - ascent - (input_min.y + 1.f));
            ImGui::SetCursorScreenPos(ImVec2(input_min.x + 1.f, input_min.y + 1.f));
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(5.f), pad_y));
            ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
            ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgActive, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_Text, p.text_selected);
            ImGui::PushStyleColor(ImGuiCol_TextDisabled, p.text_disabled);
            push_font(fonts().regular, 14.f);
            ImGui::SetNextItemWidth(px(200.f) - 2.f);
            if (ImGui::InputTextWithHint("##commit_search", "find commit", &state.search_text,
                                         select_all ? ImGuiInputTextFlags_AutoSelectAll : 0))
            {
                state.run_search();
            }
            ImGui::PopFont();
            ImGui::PopStyleColor(5);
            ImGui::PopStyleVar(2);
            if (was_active)
            {
                if (ImGui::IsKeyPressed(ImGuiKey_Enter) || ImGui::IsKeyPressed(ImGuiKey_KeypadEnter))
                {
                    state.select_search_match(ImGui::GetIO().KeyShift ? -1 : 1);
                    refocus = true; // a single-line input lets go on Enter
                }
                else if (ImGui::IsKeyPressed(ImGuiKey_DownArrow))
                {
                    state.select_search_match(1);
                }
                else if (ImGui::IsKeyPressed(ImGuiKey_UpArrow))
                {
                    state.select_search_match(-1);
                }
            }

            const std::string count = state.search_matches.empty()
                                              ? "0 results"
                                              : std::to_string(state.search_index + 1) + " of "
                                                        + std::to_string(state.search_matches.size());
            draw_text(draw, fonts().regular, 14.f, ImVec2(at(237.5f), baseline - ascent), white, count.c_str());

            // .pointer.px1 icons: arrow-up (12.25px) .mr1, arrow-down (12.25px) .mr1, times (9.625px).
            struct Button
            {
                const char* id;
                const char* icon;
                float left;
                float width;
            };
            static constexpr Button BUTTONS[] = {{"##search_prev", "fas:arrow-up", 327.5f, 22.25f},
                                                 {"##search_next", "fas:arrow-down", 354.75f, 22.25f},
                                                 {"##search_close", "fas:times", 382.f, 19.625f}};
            for (int i = 0; i < 3; ++i)
            {
                const Button& button = BUTTONS[i];
                ImGui::SetCursorScreenPos(ImVec2(at(button.left), min.y));
                if (ImGui::InvisibleButton(button.id, ImVec2(px(button.width), px(HEIGHT))))
                {
                    if (i == 2)
                    {
                        state.search_open = false;
                        state.run_search();
                    }
                    else
                    {
                        state.select_search_match(i == 0 ? -1 : 1);
                    }
                }
                if (ImGui::IsItemHovered())
                {
                    ImGui::SetMouseCursor(ImGuiMouseCursor_Hand);
                }
                draw_fa_icon(draw, button.icon, ImVec2(at(button.left + button.width / 2.f), mid), px(14.f), white);
            }
            ImGui::End();
        }
    } // namespace

    void draw_graph_view(app::Application& app, app::RepoState& state, ImVec2 size)
    {
        const theme::Palette& p = theme::palette();
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        ImGui::BeginChild("##graph_view", size, ImGuiChildFlags_None, ImGuiWindowFlags_NoScrollbar);
        ImDrawList* draw = ImGui::GetWindowDrawList();

        if (!state.load_error.empty())
        {
            ImGui::SetCursorScreenPos(ImVec2(origin.x + px(20.f), origin.y + px(20.f)));
            ImGui::PushStyleColor(ImGuiCol_Text, p.red);
            ImGui::TextWrapped("%s", state.load_error.c_str());
            ImGui::PopStyleColor();
            ImGui::EndChild();
            return;
        }

        const float header_h = px(theme::GRAPH_HEADER_HEIGHT);
        const float row_h = px(theme::GRAPH_ROW_HEIGHT);
        const float scrollbar = ImGui::GetStyle().ScrollbarSize;
        const float gear_w = px(26.f);
        // The zones fill the graph from the rows' left edge, 4px left of the first header cell, to the panel's right
        // edge; the last one ends at the column settings button for the headers and rows drawn here.
        const float zones_x = origin.x + px(theme::GRAPH_LEFT_MARGIN);
        update_zones(app.settings, std::floor((size.x - px(theme::GRAPH_LEFT_MARGIN - 4.f)) / dpi_scale()),
                     state.layout.max_columns);
        apply_zone_drag(app);
        Zones z = place_zones(zones_x);
        {
            const float last_end = origin.x + size.x - scrollbar - gear_w;
            const ZoneType last_type = g_zones.zones.back().type;
            float* last_w = zone_w_of(z, last_type);
            const float last_x = *zone_x_of(z, last_type);
            // A narrow graph squeezes the last zone to its minimum, which already holds the scrollbar; there the
            // settings button only covers its header, and the zone keeps its content width (the author's avatar).
            const float content_minimum = px(zone_minimum(last_type));
            *last_w = std::max(last_end - last_x, std::min(*last_w, content_minimum));
        }
        const float header_y = origin.y;

        // Header (.panel-bg0 .graph-header)
        draw->AddRectFilled(ImVec2(origin.x, header_y), ImVec2(origin.x + size.x, header_y + header_h),
                            theme::u32(p.panel_bg0));
        const auto header_label = [&](ZoneType type)
        {
            return type == ZoneType::AUTHOR && z.author_w <= px(60.f) ? "" : zone_label(type);
        };
        for (std::size_t i = 0; i < g_zones.zones.size(); ++i)
        {
            const ZoneType type = g_zones.zones[i].type;
            header_cell(draw, *zone_x_of(z, type), *zone_w_of(z, type), header_y, header_h, header_label(type), i > 0);
        }
        zone_handles(zones_x, header_y, header_y + header_h, true);
        {
            // .columns-settings: far:cog (12px, text-disabled) 9px from the right edge; it opens the same native
            // menu as a right-click on the header.
            const ImVec2 gear_min(origin.x + size.x - px(16.f), header_y);
            ImGui::SetCursorScreenPos(gear_min);
            const bool gear = ImGui::InvisibleButton("##column_settings", ImVec2(px(16.f), header_h));
            const bool gear_hovered = ImGui::IsItemHovered();
            draw_fa_icon(draw, "far:cog", ImVec2(origin.x + size.x - px(9.f), header_y + px(12.17f)), px(12.f),
                         theme::u32(gear_hovered ? p.text_selected : p.text_disabled));
            if (gear_hovered)
            {
                styled_tooltip(gear_min, ImVec2(gear_min.x + px(16.f), header_y + header_h), "Column settings", true);
            }
            if (gear)
            {
                popup_menu(graph_header_menu(app));
            }
        }
        // The header cells, after the resize handles and the gear so those win the hover: pressing one and
        // moving 4px drags the column.
        {
            const ImVec2 mouse = ImGui::GetIO().MousePos;
            for (std::size_t i = 0; i < g_zones.zones.size(); ++i)
            {
                const ZoneType type = g_zones.zones[i].type;
                const float x = *zone_x_of(z, type);
                const float w = *zone_w_of(z, type);
                if (w <= 0.f)
                {
                    continue;
                }
                ImGui::SetCursorScreenPos(ImVec2(x, header_y));
                ImGui::InvisibleButton((std::string("##zone_header_") + zone_key(type)).c_str(), ImVec2(w, header_h));
                if (ImGui::IsItemHovered())
                {
                    set_drag_cursor(DragCursor::GRAB); // cursor: grab over the header cells
                }
                if (ImGui::IsItemActivated() && !g_zone_move.pressed)
                {
                    g_zone_move = {type, true, false, mouse.x, mouse.x - x, w};
                }
            }
            if (g_zone_move.pressed && !ImGui::IsMouseDown(ImGuiMouseButton_Left))
            {
                if (g_zone_move.active)
                {
                    app.settings.save(app.config_path("settings.txt"));
                }
                g_zone_move = {};
            }
            if (g_zone_move.pressed && !g_zone_move.active && std::abs(mouse.x - g_zone_move.press_x) >= px(4.f))
            {
                g_zone_move.active = true;
            }
            if (g_zone_move.pressed)
            {
                set_drag_cursor(DragCursor::GRAB);
            }
            if (g_zone_move.active)
            {
                // The floating cell's edges against the neighbours' centres: past one, the zones swap and the
                // layout is placed again for the rows below.
                const float left_edge = mouse.x - g_zone_move.grab;
                const float right_edge = left_edge + g_zone_move.width;
                bool moved = false;
                for (int guard = 0; guard < static_cast<int>(g_zones.zones.size()); ++guard)
                {
                    const int index = zone_index(g_zone_move.type);
                    if (index < 0)
                    {
                        break;
                    }
                    Zones placed = place_zones(zones_x);
                    int to = index;
                    if (index > 0)
                    {
                        const ZoneType left = g_zones.zones[static_cast<std::size_t>(index - 1)].type;
                        if (left_edge < *zone_x_of(placed, left) + *zone_w_of(placed, left) / 2.f)
                        {
                            to = index - 1;
                        }
                    }
                    if (to == index && index + 1 < static_cast<int>(g_zones.zones.size()))
                    {
                        const ZoneType right = g_zones.zones[static_cast<std::size_t>(index + 1)].type;
                        if (right_edge > *zone_x_of(placed, right) + *zone_w_of(placed, right) / 2.f)
                        {
                            to = index + 1;
                        }
                    }
                    if (to == index)
                    {
                        break;
                    }
                    move_zone(app, index, to);
                    moved = true;
                }
                if (moved)
                {
                    // Placed again, with the last zone's trailing room as above.
                    z = place_zones(zones_x);
                    const float last_end = origin.x + size.x - scrollbar - gear_w;
                    const ZoneType last_type = g_zones.zones.back().type;
                    float* last_w = zone_w_of(z, last_type);
                    const float last_x = *zone_x_of(z, last_type);
                    *last_w = std::max(last_end - last_x, std::min(*last_w, px(zone_minimum(last_type))));
                    draw->AddRectFilled(ImVec2(origin.x, header_y), ImVec2(origin.x + size.x, header_y + header_h),
                                        theme::u32(p.panel_bg0));
                    for (std::size_t i = 0; i < g_zones.zones.size(); ++i)
                    {
                        const ZoneType type = g_zones.zones[i].type;
                        header_cell(draw, *zone_x_of(z, type), *zone_w_of(z, type), header_y, header_h,
                                    header_label(type), i > 0);
                    }
                    draw_fa_icon(draw, "far:cog", ImVec2(origin.x + size.x - px(9.f), header_y + px(12.17f)),
                                 px(12.f), theme::u32(p.text_disabled));
                }
                // The dragged cell, over everything: the header's background and label following the mouse.
                ImDrawList* front = ImGui::GetForegroundDrawList();
                const float fx = std::round(mouse.x - g_zone_move.grab);
                front->AddRectFilled(ImVec2(fx, header_y), ImVec2(fx + g_zone_move.width, header_y + header_h),
                                     theme::u32(theme::with_alpha(p.panel_bg2, 0.95f)), px(2.f));
                front->AddRect(ImVec2(fx, header_y), ImVec2(fx + g_zone_move.width, header_y + header_h),
                               theme::u32(p.section_border), px(2.f), 0, 1.f);
                header_cell(front, fx, g_zone_move.width, header_y, header_h, header_label(g_zone_move.type), false);
            }
        }

        if (ImGui::IsWindowHovered(ImGuiHoveredFlags_ChildWindows) && ImGui::IsMouseReleased(ImGuiMouseButton_Right)
            && ImGui::IsMouseHoveringRect(ImVec2(origin.x, header_y), ImVec2(origin.x + size.x, header_y + header_h)))
        {
            popup_menu(graph_header_menu(app));
        }
        const float rows_top = header_y + header_h + px(theme::GRAPH_HEADER_MARGIN_BOTTOM);
        ImGui::SetCursorScreenPos(ImVec2(origin.x, rows_top));
        ImGui::BeginChild("##graph_rows", ImVec2(size.x, origin.y + size.y - rows_top));
        // Before the rows, so the zone edges win the hover over them.
        zone_handles(zones_x, rows_top, origin.y + size.y, false);
        // The graph zone scrolls sideways when narrower than its lanes: a horizontal wheel, Shift + wheel, or its
        // scrollbar (.scrollbar-outer.horizontal, along the zone's bottom over the rows).
        const float bar_h = ImGui::GetStyle().ScrollbarSize;
        const ImVec2 bar_min(lanes_x(z), origin.y + size.y - bar_h);
        const ImVec2 bar_max(bar_min.x + z.graph_w, origin.y + size.y);
        {
            const ImGuiIO& io = ImGui::GetIO();
            const ImVec2 m = io.MousePos;
            if (ImGui::IsWindowHovered() && m.x >= bar_min.x && m.x < bar_max.x)
            {
                const float wheel = io.MouseWheelH != 0.f ? -io.MouseWheelH : io.KeyShift ? -io.MouseWheel : 0.f;
                g_graph_scroll += wheel * 100.f; // Chromium scrolls 100px a notch
            }
        }
        z.lanes = graph_lanes(state.layout.max_columns);
        const ScrollbarX graph_bar =
                scrollbar_x("##graph_scroll_x", bar_min, bar_max, &g_graph_scroll, z.lanes.content, z.lanes.width);
        z.lanes = graph_lanes(state.layout.max_columns);
        ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing, ImVec2(ImGui::GetStyle().ItemSpacing.x, 0.f)); // rows abut
        ImDrawList* rows_draw = ImGui::GetWindowDrawList();
        gfx::set_grayscale_text(rows_draw, true); // grey-scale text, like a composited layer
        const std::vector<git::Commit>& commits = state.history.commits;
        const int row_count = static_cast<int>(commits.size());

        // Keyboard navigation
        if (ImGui::IsWindowFocused(ImGuiFocusedFlags_ChildWindows | ImGuiFocusedFlags_RootWindow)
            && !ImGui::GetIO().WantTextInput && row_count > 0)
        {
            const ImGuiIO& io = ImGui::GetIO();
            // The letters alone: Ctrl+J / Ctrl+K toggle the panels.
            const bool plain = !io.KeyCtrl && !io.KeyAlt && !io.KeySuper;
            int move = 0;
            if (ImGui::IsKeyPressed(ImGuiKey_DownArrow) || (plain && ImGui::IsKeyPressed(ImGuiKey_J)))
            {
                move = 1;
            }
            if (ImGui::IsKeyPressed(ImGuiKey_UpArrow) || (plain && ImGui::IsKeyPressed(ImGuiKey_K)))
            {
                move = -1;
            }
            if (ImGui::IsKeyPressed(ImGuiKey_Home))
            {
                state.select_only(commits.front().sha);
                state.scroll_to_row = 0;
            }
            if (ImGui::IsKeyPressed(ImGuiKey_End))
            {
                state.select_only(commits.back().sha);
                state.scroll_to_row = static_cast<std::size_t>(row_count - 1);
            }
            if (move != 0)
            {
                // A Shift range grows from its moving end; `focused` stays its anchor.
                const bool range = io.KeyShift;
                const git::Sha& from = range && state.history.row_by_sha.contains(state.range_cursor)
                                               ? state.range_cursor
                                               : state.focused;
                int index = 0;
                if (const auto row = state.history.row_by_sha.find(from); row != state.history.row_by_sha.end())
                {
                    index = std::clamp(static_cast<int>(row->second) + move, 0, row_count - 1);
                }
                const git::Sha target = commits[static_cast<std::size_t>(index)].sha;
                if (range)
                {
                    state.select(target, false, true);
                    state.range_cursor = target;
                }
                else
                {
                    state.select_only(target);
                }
                state.scroll_to_row = static_cast<std::size_t>(index);
            }
        }
        if (state.scroll_to_row)
        {
            const float target = static_cast<float>(*state.scroll_to_row) * row_h;
            const float scroll = ImGui::GetScrollY();
            const float visible = ImGui::GetWindowHeight();
            if (target < scroll || target + row_h > scroll + visible)
            {
                ImGui::SetScrollY(std::max(0.f, target - visible / 2.f + row_h / 2.f));
            }
            state.scroll_to_row.reset();
        }

        // A pressed pill becomes a drag 4px away; Esc cancels it.
        if (g_drag.pressed && g_drag.repo != state.repo.workdir())
        {
            g_drag = {};
        }
        if (g_drag.pressed && !g_drag.active && ImGui::IsMouseDown(ImGuiMouseButton_Left))
        {
            const ImVec2 m = ImGui::GetIO().MousePos;
            if (std::abs(m.x - g_drag.press.x) >= px(4.f) || std::abs(m.y - g_drag.press.y) >= px(4.f))
            {
                g_drag.active = true;
            }
        }
        if (g_drag.active && ImGui::IsKeyPressed(ImGuiKey_Escape))
        {
            g_drag = {};
        }
        g_drop_target.reset();

        static std::string context_sha;
        static std::optional<git::Ref> context_ref;
        static std::optional<RefGroupKey> hovered_group; // for dimming the rows it does not reach
        std::optional<RefGroupKey> hovered_group_now;
        // The row whose ref zone is hovered lists all its ref groups in a popover over the rows below.
        static std::string hovered_zone_sha;
        static bool zone_popover_ghost = false;
        static int zone_popover_row = -1;
        std::string hovered_zone_now;
        const ImVec2 rows_origin = ImGui::GetCursorScreenPos();
        const float rows_origin_y = rows_origin.y;
        bool open_commit_menu = false;
        bool open_ref_menu = false;
        std::optional<git::Ref> checkout_request;
        // A submitted inline branch / tag name: created after the rows, since it reloads the history they read.
        std::optional<app::RepoState::InlineRef> inline_done;
        bool show_more = false;
        const ImVec2 mouse = ImGui::GetIO().MousePos;
        const bool window_hovered = ImGui::IsWindowHovered(ImGuiHoveredFlags_ChildWindows);
        // A pressed pill is the active item: the ref zone stays hovered through it, so a double click lands on the
        // popover it was aimed at.
        const bool zone_window_hovered =
                ImGui::IsWindowHovered(ImGuiHoveredFlags_ChildWindows | ImGuiHoveredFlags_AllowWhenBlockedByActiveItem);
        const bool searching = state.search_open && !state.search_text.empty();
        std::unordered_set<std::size_t> search_rows(state.search_matches.begin(), state.search_matches.end());
        // .dimmed-row: rows that miss the hovered ref group (or, while Ctrl is held, the selected commit's first
        // group) fade their text to --text-dimmed after 1s over 0.5s ease-out; the class leaves instantly.
        std::optional<RefGroupKey> dim_key;
        if (ImGui::GetIO().KeyCtrl && ImGui::IsWindowFocused(ImGuiFocusedFlags_RootAndChildWindows))
        {
            if (const git::Commit* single = state.single_selected(); single != nullptr)
            {
                if (const auto row = state.history.row_by_sha.find(single->sha); row != state.history.row_by_sha.end())
                {
                    const std::vector<RefGroup> groups = ref_groups(row_refs(state, row->second, false));
                    if (!groups.empty())
                    {
                        dim_key = group_key(groups.front());
                    }
                }
            }
        }
        if (!dim_key && app.settings.highlight_rows_on_ref_hover)
        {
            dim_key = hovered_group;
        }
        static std::optional<RefGroupKey> dimming_for;
        static double dim_since = 0.0;
        if (dim_key != dimming_for)
        {
            dimming_for = dim_key;
            dim_since = ImGui::GetTime();
        }
        float dim = 0.f;
        if (dim_key)
        {
            const float t = std::clamp(static_cast<float>(ImGui::GetTime() - dim_since - 1.0) / 0.5f, 0.f, 1.f);
            dim = 1.f - (1.f - t) * (1.f - t); // ease-out
        }
        state.hovered_sha.clear();

        // The hovered ref zone's popover (.ref-zone ul): every group at its natural width, 22px apart.
        struct PopoverNode
        {
            ImVec2 pos;
            float width;
        };
        std::vector<PopoverNode> popover_nodes;
        ImDrawListSplitter popover_layer;
        popover_layer.Split(rows_draw, 2);
        popover_layer.SetCurrentChannel(rows_draw, 1);
        if (!hovered_zone_sha.empty() && zone_popover_row >= 0 && zone_popover_row < row_count
            && commits[static_cast<std::size_t>(zone_popover_row)].sha == hovered_zone_sha)
        {
            const auto row = static_cast<std::size_t>(zone_popover_row);
            int source = zone_popover_row;
            if (zone_popover_ghost)
            {
                source = row < state.ghost_ref_rows.size() ? state.ghost_ref_rows[row] : -1;
            }
            const std::vector<RefGroup> zone_popover_groups =
                    source >= 0 ? ref_groups(state.refs_at(commits[static_cast<std::size_t>(source)].sha))
                                : std::vector<RefGroup>{};
            const ImVec4 zone_popover_lane = theme::graph_color(state.layout.rows[row].column);
            float top = rows_origin_y + static_cast<float>(zone_popover_row) * row_h + px(theme::COMMIT_ZONE_MARGIN);
            for (std::size_t g = 0; g < zone_popover_groups.size() && g < 50; ++g)
            {
                const RefGroup& group = zone_popover_groups[g];
                const float width = ref_node_width(group);
                const RefNodeResult node =
                        ref_node(app, state, group, rows_draw, ImVec2(z.ref_x, top), width, zone_popover_lane,
                                 zone_popover_ghost ? 0.65f : 1.f, ("popover" + std::to_string(g)).c_str());
                popover_nodes.push_back({ImVec2(z.ref_x, top), width});
                if (node.hovered)
                {
                    hovered_group_now = group_key(group);
                }
                if (node.double_clicked)
                {
                    checkout_request = *group.refs.front();
                }
                else if (node.clicked)
                {
                    state.select_only(hovered_zone_sha);
                    state.close_diff();
                }
                if (node.right_clicked)
                {
                    context_ref = *group.refs.front();
                    open_ref_menu = true;
                }
                if (zone_window_hovered && mouse.x >= z.ref_x && mouse.x < z.ref_x + width && mouse.y >= top
                    && mouse.y < top + px(theme::REF_ZONE_TEXT_HEIGHT))
                {
                    hovered_zone_now = hovered_zone_sha;
                }
                top += px(theme::REF_ZONE_TEXT_HEIGHT);
            }
        }
        popover_layer.SetCurrentChannel(rows_draw, 0);
        ImGui::SetCursorScreenPos(rows_origin);

        // Timeline markers: the first row past each age step ("1 hour ago", "2 hours ago", "yesterday"...) gets a
        // line along its top edge and the age in a label at the rows' right end. Rows out of date order never step
        // back.
        struct TimelineMarker
        {
            int row;
            std::string label;
        };
        std::vector<TimelineMarker> timeline;
        {
            const auto since_epoch = std::chrono::system_clock::now().time_since_epoch();
            const std::int64_t now = std::chrono::duration_cast<std::chrono::seconds>(since_epoch).count();
            int reached = 0;
            for (int i = 0; i < row_count; ++i)
            {
                const git::Commit& commit = commits[static_cast<std::size_t>(i)];
                if (commit.type == git::NodeType::WORK_DIR)
                {
                    continue;
                }
                const int bucket = timeline_bucket(std::max<std::int64_t>(0, now - commit.time));
                if (bucket > reached)
                {
                    reached = bucket;
                    timeline.push_back({i, format_relative(commit.time)});
                }
            }
        }

        ImGuiListClipper clipper;
        clipper.Begin(row_count + (state.history.has_more ? 1 : 0), row_h);
        while (clipper.Step())
        {
            for (int i = clipper.DisplayStart; i < clipper.DisplayEnd; ++i)
            {
                const float y0 = ImGui::GetCursorScreenPos().y;
                if (i >= row_count)
                {
                    ImGui::SetCursorScreenPos(ImVec2(z.message_x + px(10.f), y0 + px(3.f)));
                    const std::string footer = "Displaying " + std::to_string(row_count) + " commits.";
                    secondary_text(footer.c_str());
                    ImGui::SameLine();
                    if (link_text("Show more commits"))
                    {
                        show_more = true;
                    }
                    ImGui::SetCursorScreenPos(ImVec2(origin.x, y0));
                    ImGui::Dummy(ImVec2(size.x, row_h));
                    continue;
                }
                const git::Commit& commit = commits[static_cast<std::size_t>(i)];
                const graph::RowLayout& row = state.layout.rows[static_cast<std::size_t>(i)];
                const bool selected = state.is_selected(commit.sha);
                const bool is_head = commit.sha == state.history.head_sha;
                const bool is_wip = commit.type == git::NodeType::WORK_DIR;
                const bool dimmed = searching && !search_rows.contains(static_cast<std::size_t>(i));
                const float alpha = dimmed ? 0.3f : 1.f;
                // The work-dir row has its own renderer without .dimmed-row.
                const float row_dim =
                        dim > 0.f && !is_wip && row_misses_group(state, static_cast<std::size_t>(i), *dim_key) ? dim
                                                                                                               : 0.f;
                // --text-dimmed (20% white), --text-dimmed-selected (50%) on a selected row.
                const auto dim_text = [&](ImVec4 color)
                {
                    const ImVec4 to = selected ? ImVec4(1.f, 1.f, 1.f, 0.5f) : ImVec4(1.f, 1.f, 1.f, 0.2f);
                    return ImVec4(color.x + (to.x - color.x) * row_dim, color.y + (to.y - color.y) * row_dim,
                                  color.z + (to.z - color.z) * row_dim, color.w + (to.w - color.w) * row_dim);
                };

                // Hover is known before any item is submitted so the ref pills (submitted first, so they win the
                // hover test) can be drawn, and the row button goes last to leave the cursor on the next row.
                const bool hovered = window_hovered && !g_drag.active && mouse.x >= origin.x
                                     && mouse.x < origin.x + size.x - scrollbar && mouse.y >= y0
                                     && mouse.y < y0 + row_h;
                // The ref zone does not hover the row (ImGit tracks row hover in the graph / message zones).
                const bool over_ref_zone = mouse.x >= z.ref_x - px(3.f) && mouse.x < z.ref_x + z.ref_w - px(3.f);
                const bool row_hovered = hovered && !over_ref_zone;
                const bool single_selected = selected && state.selected.size() == 1;
                if (hovered)
                {
                    state.hovered_sha = commit.sha;
                }
                ImGui::PushID(i);
                // The zones but the ref and graph ones get the row background over the inner 22px
                // (GRAPH_ROW_INNER_HEIGHT); the last one carries it to the panel's edge.
                const float inner_top = y0 + px(theme::COMMIT_ZONE_MARGIN);
                const float inner_bottom = inner_top + px(theme::GRAPH_ROW_INNER_HEIGHT);
                const auto fill_row = [&](ImU32 color)
                {
                    for (std::size_t zi = 0; zi < g_zones.zones.size(); ++zi)
                    {
                        const ZoneType type = g_zones.zones[zi].type;
                        if (type == ZoneType::REF || type == ZoneType::GRAPH)
                        {
                            continue;
                        }
                        const float x = *zone_x_of(z, type);
                        const bool last = zi + 1 == g_zones.zones.size();
                        const float right = last ? origin.x + size.x : x + *zone_w_of(z, type);
                        rows_draw->AddRectFilled(ImVec2(x, inner_top), ImVec2(right, inner_bottom), color);
                    }
                };
                if (selected)
                {
                    fill_row(theme::u32(p.selected_row));
                }
                else if (row_hovered)
                {
                    fill_row(theme::u32(p.hover_row));
                }
                if (searching && !dimmed && !selected)
                {
                    fill_row(theme::u32(theme::with_alpha(p.accent, 0.08f)));
                }

                // Ref zone: the first ref group and a "+N" count; hovering the zone lists every group (drawn after
                // the rows). Rows without refs show their nearest descendant's refs dimmed while hovered or
                // selected (ghost refs).
                const ImVec4 lane_color = theme::graph_color(row.column);
                const float pill_y = inner_top;
                const std::vector<RefGroup> groups = ref_groups(state.refs_at(commit.sha));
                std::vector<RefGroup> shown_groups = groups;
                const bool zone_hovered = hovered_zone_sha == commit.sha;
                bool ghost = false;
                if (groups.empty() && app.settings.show_ghost_refs && (row_hovered || single_selected)
                    && static_cast<std::size_t>(i) < state.ghost_ref_rows.size()
                    && state.ghost_ref_rows[static_cast<std::size_t>(i)] >= 0)
                {
                    const git::Commit& source =
                            commits[static_cast<std::size_t>(state.ghost_ref_rows[static_cast<std::size_t>(i)])];
                    shown_groups = ref_groups(state.refs_at(source.sha));
                    ghost = !shown_groups.empty();
                }
                const bool creating = state.inline_ref && state.inline_ref->sha == commit.sha;
                struct RefLine
                {
                    ImVec2 min, max;
                    ImU32 color;
                };
                std::optional<RefLine> ref_line;
                if (!groups.empty() || creating)
                {
                    // .ref-line: the lane colour from the zone (+3px) to the node, 1px at 25% opacity; 2px opaque on
                    // the row of the checked-out branch.
                    const bool head_row = std::any_of(groups.begin(), groups.end(),
                                                      [](const RefGroup& g)
                                                      {
                                                          return g.current;
                                                      });
                    float node_edge = node_x(z, row.column);
                    float node_half = px(theme::COMMIT_NODE_DIAMETER) / 2.f;
                    if (commit.type == git::NodeType::MERGE)
                    {
                        node_half = px(theme::COMMIT_MERGE_NODE_DIAMETER) / 2.f;
                    }
                    // From the zone to the node's near edge, whichever side of the graph the zone is on; with
                    // other zones between them it stops at the graph zone's edge instead of crossing their text.
                    const bool ref_left = z.ref_x < z.graph_x;
                    const bool ref_adjacent = std::abs(zone_index(ZoneType::REF) - zone_index(ZoneType::GRAPH)) == 1;
                    node_edge += ref_left ? -node_half : node_half;
                    const float line_y = inner_top + px(head_row ? 10.f : 11.f);
                    const float body_left = lanes_x(z);
                    const float body_right = body_left + z.graph_w;
                    const float line_min_x = ref_left ? (ref_adjacent ? z.ref_x : body_left) : node_edge;
                    const float line_max_x = ref_left ? node_edge : (ref_adjacent ? z.ref_x : body_right);
                    ref_line = RefLine{ImVec2(line_min_x, line_y),
                                       ImVec2(line_max_x, line_y + px(head_row ? 2.f : 1.f)),
                                       theme::u32(theme::with_alpha(lane_color, head_row ? alpha : 0.25f * alpha))};
                    // Outside the graph zone now; over it after the gutters, which it crosses (.ref-line is z4).
                    if (line_min_x < body_left)
                    {
                        rows_draw->AddRectFilled(ref_line->min,
                                                 ImVec2(std::min(line_max_x, body_left), ref_line->max.y),
                                                 ref_line->color);
                    }
                    if (line_max_x > body_right)
                    {
                        rows_draw->AddRectFilled(ImVec2(std::max(line_min_x, body_right), line_y), ref_line->max,
                                                 ref_line->color);
                    }
                }
                if (creating)
                {
                    // .form-control in the ref zone: the whole zone and row, rgba(0,0,0,.2), a 1px #4D88FF focus
                    // border, radius 4, 12px text 12px in.
                    app::RepoState::InlineRef& inline_ref = *state.inline_ref;
                    const ImVec2 min(z.ref_x - px(2.f), y0);
                    const ImVec2 max(min.x + z.ref_w + px(1.f), y0 + row_h);
                    rows_draw->AddRectFilled(min, max, theme::u32(p.app_bg0), px(4.f));
                    rows_draw->AddRectFilled(min, max, theme::u32(p.input_bg), px(4.f));
                    ImGui::SetCursorScreenPos(min);
                    const float text_h = text_size(fonts().regular, theme::FONT_SIZE_ROW, "A").y;
                    ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(13.f), (row_h - text_h) / 2.f));
                    ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
                    ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
                    ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, ImVec4(0.f, 0.f, 0.f, 0.f));
                    ImGui::PushStyleColor(ImGuiCol_FrameBgActive, ImVec4(0.f, 0.f, 0.f, 0.f));
                    ImGui::PushStyleColor(ImGuiCol_Text, p.text_normal);
                    push_font(fonts().regular, theme::FONT_SIZE_ROW);
                    if (inline_ref.focus)
                    {
                        ImGui::SetKeyboardFocusHere();
                        inline_ref.focus = false;
                    }
                    ImGui::SetNextItemWidth(max.x - min.x);
                    const bool submitted =
                            ImGui::InputText("##inline_ref", &inline_ref.name, ImGuiInputTextFlags_EnterReturnsTrue);
                    text_field_menu(inline_ref.name);
                    const bool cancelled = ImGui::IsItemDeactivated() && !submitted;
                    ImGui::PopFont();
                    ImGui::PopStyleColor(4);
                    ImGui::PopStyleVar(2);
                    rows_draw->AddRect(min, max, theme::u32(theme::palette().accent), px(4.f), 0, 1.f);
                    if (inline_ref.name.empty())
                    {
                        rows_draw->PushClipRect(min, ImVec2(max.x - px(13.f), max.y), true);
                        draw_text(rows_draw, fonts().regular, theme::FONT_SIZE_ROW,
                                  ImVec2(min.x + px(13.f), y0 + (row_h - text_h) / 2.f), theme::u32(p.text_secondary),
                                  inline_ref.tag ? "enter tag name" : "enter branch name");
                        rows_draw->PopClipRect();
                    }
                    if (submitted && !inline_ref.name.empty())
                    {
                        inline_done = inline_ref;
                        state.inline_ref.reset();
                    }
                    else if (cancelled || submitted || ImGui::IsKeyPressed(ImGuiKey_Escape))
                    {
                        state.inline_ref.reset();
                    }
                }
                else if (!shown_groups.empty() && !zone_hovered)
                {
                    const float opacity = ghost ? (zone_hovered ? 0.65f : 0.5f) : alpha;
                    const std::size_t more = shown_groups.size() - 1;
                    const float more_w = more > 0 ? overflow_count_width(more) + px(5.f) : 0.f;
                    const float room = z.ref_w - px(8.f) - more_w;
                    const float width = std::min(ref_node_width(shown_groups.front()), room);
                    if (width > px(20.f))
                    {
                        const RefNodeResult node = ref_node(app, state, shown_groups.front(), rows_draw,
                                                            ImVec2(z.ref_x, pill_y), width, lane_color, opacity, "row");
                        if (node.hovered)
                        {
                            hovered_group_now = group_key(shown_groups.front());
                        }
                        if (node.double_clicked)
                        {
                            checkout_request = *shown_groups.front().refs.front();
                        }
                        else if (node.clicked)
                        {
                            state.select_only(commit.sha);
                            state.close_diff();
                        }
                        if (node.right_clicked)
                        {
                            context_ref = *shown_groups.front().refs.front();
                            open_ref_menu = true;
                        }
                        if (more > 0)
                        {
                            overflow_count(rows_draw, ImVec2(z.ref_x + node.width + px(5.f), pill_y), more, lane_color,
                                           opacity);
                        }
                    }
                }
                // A popover item over this row keeps its own zone (else this row's ghost refs would replace it).
                if (window_hovered && !g_drag.active && hovered_zone_now.empty() && mouse.x >= z.ref_x - px(3.f)
                    && mouse.x < z.ref_x + z.ref_w - px(3.f) && mouse.y >= y0 && mouse.y < y0 + row_h
                    && (!shown_groups.empty()))
                {
                    hovered_zone_now = commit.sha;
                    zone_popover_row = i;
                    zone_popover_ghost = ghost;
                }

                // Graph zone: the commit's background streak from its node to the zone's end (.commit-bg-color,
                // lane colour at 10%, 50% when selected) ending in the 2px .color-strip.
                {
                    const float node_center = column_x(z, row.column);
                    const float graph_end = z.graph_x + z.graph_w;
                    const float strip_x = graph_end - px(2.f);
                    // A lane scrolled past the zone's end has no streak in it.
                    if (node_center < strip_x)
                    {
                        rows_draw->AddRectFilled(
                                ImVec2(node_center, inner_top), ImVec2(strip_x, inner_bottom),
                                theme::u32(theme::with_alpha(lane_color, (selected ? 0.5f : 0.1f) * alpha)));
                    }
                    if (!is_wip)
                    {
                        rows_draw->AddRectFilled(ImVec2(strip_x, inner_top), ImVec2(graph_end, inner_bottom),
                                                 theme::u32(theme::with_alpha(lane_color, alpha)));
                    }
                }
                const float body_x = lanes_x(z);
                rows_draw->PushClipRect(ImVec2(body_x, y0), ImVec2(body_x + z.graph_w, y0 + row_h), true);
                draw_edges(rows_draw, z, row, y0, alpha);
                {
                    // The gutters (.app-bg0 over the scrolled lanes): the left one LeftGutterWidth wide from the
                    // zone's start, the right one ending at the zone's end and carrying the row's streak.
                    constexpr float GUTTER = theme::COMMIT_ZONE_GUTTER_WIDTH;
                    const GraphLanes& l = z.lanes;
                    const float left_end = body_x + std::round((GUTTER + GUTTER / 2.f * l.factor) * dpi_scale());
                    rows_draw->AddRectFilled(ImVec2(body_x, y0), ImVec2(left_end, y0 + row_h), theme::u32(p.app_bg0));
                    const float left_alpha =
                            0.4f
                            * (1.f
                               - std::max(l.factor, std::clamp(1.f - l.scroll / theme::COMMIT_COLUMN_WIDTH, 0.f, 1.f)));
                    gutter_shadow(rows_draw, left_end, 1.f, y0, y0 + row_h, left_alpha);
                    const float right_w = (1.f - l.slide_out) * GUTTER;
                    if (right_w > 0.f)
                    {
                        const float right_end = body_x + z.graph_w;
                        const float right_start = right_end - std::round(right_w * dpi_scale());
                        rows_draw->AddRectFilled(ImVec2(right_start, y0), ImVec2(right_end, y0 + row_h),
                                                 theme::u32(p.app_bg0));
                        rows_draw->AddRectFilled(
                                ImVec2(right_start, inner_top), ImVec2(right_end, inner_bottom),
                                theme::u32(theme::with_alpha(lane_color, (selected ? 0.5f : 0.1f) * alpha)));
                        gutter_shadow(rows_draw, right_start, -1.f, y0, y0 + row_h,
                                      0.4f * (1.f - std::max(l.factor, l.slide_out)));
                    }
                }
                if (ref_line && ref_line->max.x > body_x && ref_line->min.x < body_x + z.graph_w)
                {
                    rows_draw->AddRectFilled(ImVec2(std::max(body_x, ref_line->min.x), ref_line->min.y),
                                             ImVec2(std::min(body_x + z.graph_w, ref_line->max.x), ref_line->max.y),
                                             ref_line->color);
                }
                const int node_start = rows_draw->VtxBuffer.Size;
                draw_node(rows_draw, z, commit, row, y0, is_head && !is_wip, alpha);
                fade_vertices(rows_draw, node_start, node_opacity(z, row.column));
                rows_draw->PopClipRect();

                // Message
                {
                    const float text_h = text_size(fonts().regular, theme::FONT_SIZE_ROW, "A").y;
                    const float ty = inner_top + (px(theme::GRAPH_ROW_INNER_HEIGHT) - text_h) / 2.f;
                    const float text_x = z.message_x + px(8.f); // .graph-zone { margin-left: 8px }
                    if (is_wip)
                    {
                        // .work-dir-input: a 100px cell holding a form-control (rgba(0,0,0,.2), 1px 8% white border,
                        // radius 4, 12px horizontal padding) bound to the commit summary; the stats follow 15px on.
                        const ImVec2 box_min(z.message_x + px(1.f), inner_top);
                        const ImVec2 box_max(box_min.x + px(97.f), inner_bottom);
                        // The zone hides what overflows it (a narrow graph cuts the input and the stats).
                        ImGui::PushClipRect(ImVec2(z.message_x, y0), ImVec2(z.message_x + z.message_w, y0 + row_h),
                                            true);
                        ImGui::SetCursorScreenPos(box_min);
                        ImGui::PushStyleVar(ImGuiStyleVar_FramePadding,
                                            ImVec2(px(13.f), (box_max.y - box_min.y - text_h) / 2.f));
                        ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
                        ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
                        ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, ImVec4(0.f, 0.f, 0.f, 0.f));
                        ImGui::PushStyleColor(ImGuiCol_FrameBgActive, ImVec4(0.f, 0.f, 0.f, 0.f));
                        ImGui::PushStyleColor(ImGuiCol_Text, p.text_normal);
                        push_font(fonts().regular, theme::FONT_SIZE_ROW);
                        rows_draw->AddRectFilled(box_min, box_max, IM_COL32(0, 0, 0, 51), px(4.f));
                        ImGui::SetNextItemWidth(box_max.x - box_min.x);
                        ImGui::InputText("##wip_summary", &state.commit_summary);
                        text_field_menu(state.commit_summary);
                        const bool editing = ImGui::IsItemActive();
                        ImGui::PopFont();
                        ImGui::PopStyleColor(4);
                        ImGui::PopStyleVar(2);
                        rows_draw->AddRect(box_min, box_max,
                                           editing ? theme::u32(theme::palette().accent) : IM_COL32(255, 255, 255, 20), px(4.f),
                                           0, 1.f);
                        if (state.commit_summary.empty() && !editing)
                        {
                            draw_text(rows_draw, fonts().regular, theme::FONT_SIZE_ROW,
                                      ImVec2(box_min.x + px(13.f), ty), theme::u32(p.text_disabled), "// WIP");
                        }
                        draw_wip_stats(rows_draw, ImVec2(z.message_x + px(113.f), ty), state.status);
                        ImGui::PopClipRect();
                    }
                    else
                    {
                        const ImVec4 base = dim_text(selected ? p.text_selected : p.text_normal);
                        const float used = draw_text_ellipsis(rows_draw, fonts().regular, theme::FONT_SIZE_ROW,
                                                              ImVec2(text_x, ty), z.message_w - px(13.f),
                                                              theme::u32(theme::with_alpha(base, base.w * alpha)),
                                                              commit.summary.c_str());
                        if (app.settings.show_description && !commit.body.empty())
                        {
                            // .message-zone--body: the rest of the message on one line, 8px after the summary at
                            // 60% opacity.
                            std::string body = commit.body;
                            std::replace(body.begin(), body.end(), '\n', ' ');
                            const float remaining = z.message_w - px(21.f) - used;
                            if (remaining > px(20.f))
                            {
                                draw_text_ellipsis(rows_draw, fonts().regular, theme::FONT_SIZE_ROW,
                                                   ImVec2(text_x + used + px(8.f), ty), remaining,
                                                   theme::u32(theme::with_alpha(base, base.w * 0.6f * alpha)),
                                                   body.c_str());
                            }
                        }
                    }
                }
                // Author
                if (z.author_w > 0.f)
                {
                    const float avatar = px(theme::AVATAR_DIAMETER);
                    draw_avatar(rows_draw, ImVec2(z.author_x + px(10.f) + avatar / 2.f, y0 + row_h / 2.f), avatar,
                                commit.author);
                    if (z.author_w > px(60.f))
                    {
                        const float text_h = text_size(fonts().regular, theme::FONT_SIZE_ROW, "A").y;
                        draw_text_ellipsis(rows_draw, fonts().regular, theme::FONT_SIZE_ROW,
                                           ImVec2(z.author_x + px(16.f) + avatar, y0 + (row_h - text_h) / 2.f),
                                           z.author_w - avatar - px(24.f),
                                           theme::u32(theme::with_alpha(dim_text(p.text_normal), alpha)),
                                           commit.author.name.c_str());
                    }
                    else if (hovered)
                    {
                        ImGui::SetTooltip("%s <%s>", commit.author.name.c_str(), commit.author.email.c_str());
                    }
                }
                // Date
                if (z.date_w > 0.f)
                {
                    const std::string date = is_wip ? format_relative(commit.time) : format_date_time(commit.time);
                    const float text_h = text_size(fonts().regular, theme::FONT_SIZE_ROW, "A").y;
                    draw_text_ellipsis(rows_draw, fonts().regular, theme::FONT_SIZE_ROW,
                                       ImVec2(z.date_x + px(10.f), y0 + (row_h - text_h) / 2.f), z.date_w - px(16.f),
                                       theme::u32(theme::with_alpha(dim_text(p.text_normal), alpha)), date.c_str());
                }
                // Sha
                if (z.sha_w > 0.f && !is_wip)
                {
                    const std::string sha = git::short_sha(commit.sha);
                    const float text_h = text_size(fonts().mono, theme::FONT_SIZE_ROW, "A").y;
                    draw_text(rows_draw, fonts().mono, theme::FONT_SIZE_ROW,
                              ImVec2(z.sha_x + px(10.f), y0 + (row_h - text_h) / 2.f),
                              theme::u32(theme::with_alpha(p.text_secondary, alpha)), sha.c_str());
                }
                // Changes (only known for the working directory without a diff per row)
                if (z.changes_w > 0.f && is_wip)
                {
                    const float text_h = text_size(fonts().regular, theme::FONT_SIZE_ROW, "A").y;
                    draw_wip_stats(rows_draw, ImVec2(z.changes_x + px(8.f), y0 + (row_h - text_h) / 2.f), state.status);
                }

                // The row itself, last: pills submitted above keep the hover when the mouse is on them.
                const float row_button_x =
                        (g_zones.zones.front().type == ZoneType::REF ? z.ref_x + z.ref_w : zones_x) - px(3.f);
                ImGui::SetCursorScreenPos(ImVec2(row_button_x, y0));
                ImGui::InvisibleButton("##row", ImVec2(origin.x + size.x - scrollbar - row_button_x, row_h));
                if (ImGui::IsItemClicked(ImGuiMouseButton_Left))
                {
                    const ImGuiIO& io = ImGui::GetIO();
                    state.select(commit.sha, io.KeyCtrl, io.KeyShift);
                    state.close_diff();
                }
                if (ImGui::IsItemClicked(ImGuiMouseButton_Right) && !selected)
                {
                    state.select_only(commit.sha);
                }
                // Windows opens context menus when the right button is released.
                if (ImGui::IsItemHovered() && ImGui::IsMouseReleased(ImGuiMouseButton_Right) && !open_ref_menu)
                {
                    context_sha = commit.sha;
                    open_commit_menu = true;
                }
                if (ImGui::IsItemHovered() && ImGui::IsMouseDoubleClicked(ImGuiMouseButton_Left) && !is_wip)
                {
                    app.show_detail_panel = true;
                }
                ImGui::PopID();
            }
        }
        clipper.End();
        // Over the rows, under the ref zone's popover.
        popover_layer.SetCurrentChannel(rows_draw, 0);
        {
            const float visible_top = ImGui::GetWindowPos().y;
            const float visible_bottom = visible_top + ImGui::GetWindowHeight();
            const float rows_right = origin.x + size.x - scrollbar;
            // The label keeps clear of the ref pills: before the ref zone when that is the last one.
            const float label_right = g_zones.zones.back().type == ZoneType::REF ? z.ref_x : rows_right;
            const float label_h = px(12.f);
            for (const TimelineMarker& marker : timeline)
            {
                const float y = std::round(rows_origin_y + static_cast<float>(marker.row) * row_h);
                if (y + label_h < visible_top || y - label_h > visible_bottom)
                {
                    continue;
                }
                // The first row has no row above to part from, and its label stays inside the rows.
                if (marker.row > 0)
                {
                    rows_draw->AddLine(ImVec2(origin.x, y + 0.5f), ImVec2(rows_right, y + 0.5f),
                                       theme::u32(theme::with_alpha(p.white, 0.08f)), 1.f);
                }
                const float label_y = marker.row > 0 ? y : y + label_h / 2.f;
                const ImVec2 label_size = text_size(fonts().regular, 10.f, marker.label.c_str());
                const ImVec2 label_max(label_right - px(8.f), std::round(label_y + label_h / 2.f));
                const ImVec2 label_min(std::round(label_max.x - label_size.x - px(10.f)), label_max.y - label_h);
                rows_draw->AddRectFilled(label_min, label_max, theme::u32(p.panel_bg0), px(3.f));
                draw_text(rows_draw, fonts().regular, 10.f,
                          ImVec2(label_min.x + px(5.f), std::round(label_min.y + (label_h - label_size.y) / 2.f)),
                          theme::u32(p.text_secondary), marker.label.c_str());
            }
        }
        popover_layer.Merge(rows_draw);
        ImGui::PopStyleVar();
        if (!g_drag.active)
        {
            hovered_group = hovered_group_now;
            if (hovered_zone_now.empty())
            {
                zone_popover_row = -1;
            }
            hovered_zone_sha = hovered_zone_now;
        }

        if (g_drag.active)
        {
            RefGroup dragged{g_drag.name, {}, g_drag.current};
            for (const git::Ref& ref : g_drag.refs)
            {
                dragged.refs.push_back(&ref);
            }
            // The drag image was captured on dragstart: hovered (45%), or the checked-out branch's 50%.
            const ImVec2 min(mouse.x - g_drag.grab.x, mouse.y - g_drag.grab.y);
            draw_ref_pill(ImGui::GetForegroundDrawList(), dragged, min,
                          ImVec2(min.x + g_drag.width, min.y + px(theme::REF_ZONE_TEXT_HEIGHT)), g_drag.lane, 0.75f,
                          g_drag.current ? 0.50f : 0.45f, true, false);
            set_drag_cursor(g_drop_target ? DragCursor::MOVE : DragCursor::NO_DROP);
        }
        if (g_drag.pressed && !ImGui::IsMouseDown(ImGuiMouseButton_Left))
        {
            if (g_drag.active && g_drop_target && !g_drag.refs.empty())
            {
                popup_menu(ref_drop_menu(app, state, g_drag.refs.front(), *g_drop_target));
            }
            g_drag = {};
        }

        if (graph_bar.thumb_w > 0.f)
        {
            // .pad-for-horizontal-scrollbar: the last row clears the scrollbar.
            ImGui::Dummy(ImVec2(1.f, bar_h));
        }
        draw_scrollbar_x(rows_draw, bar_min, bar_max, graph_bar);
        if (row_count == 0)
        {
            ImGui::SetCursorScreenPos(ImVec2(origin.x + px(20.f), header_y + header_h + px(20.f)));
            secondary_text(state.history.unborn ? "No commits yet. Stage files and make the first commit."
                                                : "No commits");
        }

        if (open_ref_menu && context_ref)
        {
            popup_menu(ref_menu(app, state, *context_ref, false));
        }
        else if (open_commit_menu)
        {
            popup_menu(commit_menu(app, state, context_sha));
        }
        ImGui::EndChild();
        ImGui::EndChild();
        commit_search_bar(state, origin, size);

        if (checkout_request)
        {
            checkout_ref(app, state, *checkout_request);
        }
        if (show_more)
        {
            state.commit_limit += 1000;
            state.refresh();
        }
        if (inline_done)
        {
            if (inline_done->tag)
            {
                create_tag_at(app, state, inline_done->name, inline_done->sha, "");
            }
            else
            {
                create_branch_at(app, state, inline_done->name, inline_done->sha, inline_done->checkout);
            }
        }
    }
} // namespace imgit::ui
