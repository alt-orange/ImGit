#include "ui/vector_icons.hpp"
#include "app/assets.hpp"
#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdlib>
#include <string>
#include <unordered_map>
#include <vector>

namespace imgit::ui
{
    namespace
    {
        // A Font Awesome icon as ImGit embeds it (assets/icons/fa_icons.tsv): its viewBox, its path and, for a
        // duotone icon, the secondary path drawn at 40% opacity. The views point into the embedded asset.
        struct FaIcon
        {
            std::string_view name;
            int width = 0;
            int height = 0;
            std::string_view path;
            std::string_view secondary;
        };

        // One icon per line: name, width, height, path, secondary path, separated by tabs; '#' starts a comment.
        std::vector<FaIcon> load_fa_icons()
        {
            std::vector<FaIcon> icons;
            std::string_view text = assets::text("icons/fa_icons.tsv");
            while (!text.empty())
            {
                const std::size_t end = text.find('\n');
                std::string_view line = text.substr(0, end);
                if (!line.empty() && line.back() == '\r')
                {
                    line.remove_suffix(1);
                }
                text = end == std::string_view::npos ? std::string_view() : text.substr(end + 1);
                if (line.empty() || line.front() == '#')
                {
                    continue;
                }
                std::string_view fields[5];
                for (std::string_view& field : fields)
                {
                    const std::size_t tab = line.find('\t');
                    field = line.substr(0, tab);
                    line = tab == std::string_view::npos ? std::string_view() : line.substr(tab + 1);
                }
                icons.push_back({fields[0], std::atoi(std::string(fields[1]).c_str()),
                                 std::atoi(std::string(fields[2]).c_str()), fields[3], fields[4]});
            }
            return icons;
        }

        TextureBackend g_backend;

        using Contour = std::vector<ImVec2>;

        // ---- SVG path data -> polygons (viewBox units) ----

        class PathParser
        {
        public:
            explicit PathParser(std::string_view data): m_data(data)
            {
            }

            std::vector<Contour> parse()
            {
                char command = 0;
                while (true)
                {
                    skip_separators();
                    if (m_pos >= m_data.size())
                    {
                        break;
                    }
                    const char c = m_data[m_pos];
                    if (std::isalpha(static_cast<unsigned char>(c)) != 0)
                    {
                        command = c;
                        ++m_pos;
                    }
                    else if (command == 0)
                    {
                        break; // malformed
                    }
                    if (!run(command))
                    {
                        break;
                    }
                    // An implicit repeat of M is L.
                    if (command == 'M')
                    {
                        command = 'L';
                    }
                    else if (command == 'm')
                    {
                        command = 'l';
                    }
                }
                close();
                return std::move(m_contours);
            }

        private:
            std::string_view m_data;
            std::size_t m_pos = 0;
            std::vector<Contour> m_contours;
            Contour m_current;
            ImVec2 m_point{0.f, 0.f};
            ImVec2 m_start{0.f, 0.f};
            ImVec2 m_last_control{0.f, 0.f};
            char m_last_command = 0;

            void skip_separators()
            {
                while (m_pos < m_data.size()
                       && (m_data[m_pos] == ' ' || m_data[m_pos] == ',' || m_data[m_pos] == '\n'
                           || m_data[m_pos] == '\t' || m_data[m_pos] == '\r'))
                {
                    ++m_pos;
                }
            }

            bool number(float& out)
            {
                skip_separators();
                if (m_pos >= m_data.size())
                {
                    return false;
                }
                const char* begin = m_data.data() + m_pos;
                std::size_t i = m_pos;
                if (i < m_data.size() && (m_data[i] == '-' || m_data[i] == '+'))
                {
                    ++i;
                }
                bool digits = false;
                bool dot = false;
                while (i < m_data.size())
                {
                    const char c = m_data[i];
                    if (std::isdigit(static_cast<unsigned char>(c)) != 0)
                    {
                        digits = true;
                        ++i;
                    }
                    else if (c == '.' && !dot)
                    {
                        dot = true;
                        ++i;
                    }
                    else
                    {
                        break;
                    }
                }
                if (!digits)
                {
                    return false;
                }
                if (i < m_data.size() && (m_data[i] == 'e' || m_data[i] == 'E'))
                {
                    std::size_t j = i + 1;
                    if (j < m_data.size() && (m_data[j] == '-' || m_data[j] == '+'))
                    {
                        ++j;
                    }
                    if (j < m_data.size() && std::isdigit(static_cast<unsigned char>(m_data[j])) != 0)
                    {
                        i = j;
                        while (i < m_data.size() && std::isdigit(static_cast<unsigned char>(m_data[i])) != 0)
                        {
                            ++i;
                        }
                    }
                }
                const std::string text(begin, i - m_pos);
                out = std::strtof(text.c_str(), nullptr);
                m_pos = i;
                return true;
            }

            bool flag(float& out)
            {
                skip_separators();
                if (m_pos < m_data.size() && (m_data[m_pos] == '0' || m_data[m_pos] == '1'))
                {
                    out = m_data[m_pos] == '1' ? 1.f : 0.f;
                    ++m_pos;
                    return true;
                }
                return false;
            }

            void line_to(ImVec2 p)
            {
                if (m_current.empty())
                {
                    m_current.push_back(m_point);
                }
                m_current.push_back(p);
                m_point = p;
            }

            void close()
            {
                if (m_current.size() >= 3)
                {
                    m_contours.push_back(std::move(m_current));
                }
                m_current.clear();
            }

            void cubic(ImVec2 c1, ImVec2 c2, ImVec2 end)
            {
                const ImVec2 p0 = m_point;
                constexpr int STEPS = 16;
                for (int i = 1; i <= STEPS; ++i)
                {
                    const float t = static_cast<float>(i) / STEPS;
                    const float u = 1.f - t;
                    const float a = u * u * u, b = 3.f * u * u * t, c = 3.f * u * t * t, d = t * t * t;
                    line_to(ImVec2(a * p0.x + b * c1.x + c * c2.x + d * end.x,
                                   a * p0.y + b * c1.y + c * c2.y + d * end.y));
                }
            }

            void quadratic(ImVec2 c1, ImVec2 end)
            {
                const ImVec2 p0 = m_point;
                constexpr int STEPS = 12;
                for (int i = 1; i <= STEPS; ++i)
                {
                    const float t = static_cast<float>(i) / STEPS;
                    const float u = 1.f - t;
                    line_to(ImVec2(u * u * p0.x + 2.f * u * t * c1.x + t * t * end.x,
                                   u * u * p0.y + 2.f * u * t * c1.y + t * t * end.y));
                }
            }

            // SVG arc (endpoint parameterisation, SVG 1.1 F.6.5).
            void arc(float rx, float ry, float rotation_deg, bool large, bool sweep, ImVec2 end)
            {
                const ImVec2 p0 = m_point;
                if (rx == 0.f || ry == 0.f || (p0.x == end.x && p0.y == end.y))
                {
                    line_to(end);
                    return;
                }
                rx = std::fabs(rx);
                ry = std::fabs(ry);
                const float phi = rotation_deg * 3.14159265f / 180.f;
                const float cos_phi = std::cos(phi), sin_phi = std::sin(phi);
                const float dx = (p0.x - end.x) / 2.f, dy = (p0.y - end.y) / 2.f;
                const float x1 = cos_phi * dx + sin_phi * dy;
                const float y1 = -sin_phi * dx + cos_phi * dy;
                const float lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
                if (lambda > 1.f)
                {
                    rx *= std::sqrt(lambda);
                    ry *= std::sqrt(lambda);
                }
                const float num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
                const float den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
                float coef = den == 0.f ? 0.f : std::sqrt(std::max(0.f, num / den));
                if (large == sweep)
                {
                    coef = -coef;
                }
                const float cx1 = coef * (rx * y1 / ry);
                const float cy1 = coef * -(ry * x1 / rx);
                const float cx = cos_phi * cx1 - sin_phi * cy1 + (p0.x + end.x) / 2.f;
                const float cy = sin_phi * cx1 + cos_phi * cy1 + (p0.y + end.y) / 2.f;
                const auto angle = [](float ux, float uy, float vx, float vy)
                {
                    return std::atan2(ux * vy - uy * vx, ux * vx + uy * vy);
                };
                const float theta1 = angle(1.f, 0.f, (x1 - cx1) / rx, (y1 - cy1) / ry);
                float delta = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
                constexpr float TWO_PI = 6.28318531f;
                if (!sweep && delta > 0.f)
                {
                    delta -= TWO_PI;
                }
                else if (sweep && delta < 0.f)
                {
                    delta += TWO_PI;
                }
                const int steps = std::max(4, static_cast<int>(std::ceil(std::fabs(delta) / (TWO_PI / 48.f))));
                for (int i = 1; i <= steps; ++i)
                {
                    const float t = theta1 + delta * static_cast<float>(i) / static_cast<float>(steps);
                    const float ex = rx * std::cos(t), ey = ry * std::sin(t);
                    line_to(i == steps ? end
                                       : ImVec2(cos_phi * ex - sin_phi * ey + cx, sin_phi * ex + cos_phi * ey + cy));
                }
            }

            bool run(char command)
            {
                const bool relative = std::islower(static_cast<unsigned char>(command)) != 0;
                const ImVec2 base = relative ? m_point : ImVec2(0.f, 0.f);
                const char upper = static_cast<char>(std::toupper(static_cast<unsigned char>(command)));
                float v[7];
                switch (upper)
                {
                case 'M':
                    if (!number(v[0]) || !number(v[1]))
                    {
                        return false;
                    }
                    close();
                    m_point = ImVec2(base.x + v[0], base.y + v[1]);
                    m_start = m_point;
                    break;
                case 'L':
                    if (!number(v[0]) || !number(v[1]))
                    {
                        return false;
                    }
                    line_to(ImVec2(base.x + v[0], base.y + v[1]));
                    break;
                case 'H':
                    if (!number(v[0]))
                    {
                        return false;
                    }
                    line_to(ImVec2(base.x + v[0], m_point.y));
                    break;
                case 'V':
                    if (!number(v[0]))
                    {
                        return false;
                    }
                    line_to(ImVec2(m_point.x, base.y + v[0]));
                    break;
                case 'C':
                    for (int i = 0; i < 6; ++i)
                    {
                        if (!number(v[i]))
                        {
                            return false;
                        }
                    }
                    m_last_control = ImVec2(base.x + v[2], base.y + v[3]);
                    cubic(ImVec2(base.x + v[0], base.y + v[1]), m_last_control, ImVec2(base.x + v[4], base.y + v[5]));
                    break;
                case 'S':
                {
                    for (int i = 0; i < 4; ++i)
                    {
                        if (!number(v[i]))
                        {
                            return false;
                        }
                    }
                    const bool smooth = m_last_command == 'C' || m_last_command == 'S';
                    const ImVec2 c1 =
                            smooth ? ImVec2(2.f * m_point.x - m_last_control.x, 2.f * m_point.y - m_last_control.y)
                                   : m_point;
                    m_last_control = ImVec2(base.x + v[0], base.y + v[1]);
                    cubic(c1, m_last_control, ImVec2(base.x + v[2], base.y + v[3]));
                    break;
                }
                case 'Q':
                    for (int i = 0; i < 4; ++i)
                    {
                        if (!number(v[i]))
                        {
                            return false;
                        }
                    }
                    m_last_control = ImVec2(base.x + v[0], base.y + v[1]);
                    quadratic(m_last_control, ImVec2(base.x + v[2], base.y + v[3]));
                    break;
                case 'T':
                {
                    if (!number(v[0]) || !number(v[1]))
                    {
                        return false;
                    }
                    const bool smooth = m_last_command == 'Q' || m_last_command == 'T';
                    m_last_control =
                            smooth ? ImVec2(2.f * m_point.x - m_last_control.x, 2.f * m_point.y - m_last_control.y)
                                   : m_point;
                    quadratic(m_last_control, ImVec2(base.x + v[0], base.y + v[1]));
                    break;
                }
                case 'A':
                    if (!number(v[0]) || !number(v[1]) || !number(v[2]) || !flag(v[3]) || !flag(v[4]) || !number(v[5])
                        || !number(v[6]))
                    {
                        return false;
                    }
                    arc(v[0], v[1], v[2], v[3] != 0.f, v[4] != 0.f, ImVec2(base.x + v[5], base.y + v[6]));
                    break;
                case 'Z':
                    if (!m_current.empty())
                    {
                        line_to(m_start);
                    }
                    close();
                    m_point = m_start;
                    break;
                default:
                    return false;
                }
                m_last_command = upper;
                return true;
            }
        };

        // ---- polygons -> coverage (non-zero winding, exact horizontal coverage, 16 sub-scanlines) ----

        void rasterize(const std::vector<Contour>& contours, int width, int height, float scale, ImVec2 offset,
                       float opacity, std::vector<float>& coverage, bool even_odd = false)
        {
            struct Edge
            {
                float x0, y0, x1, y1;
                int dir;
            };
            std::vector<Edge> edges;
            for (const Contour& contour : contours)
            {
                for (std::size_t i = 0; i < contour.size(); ++i)
                {
                    const ImVec2 a = contour[i];
                    const ImVec2 b = contour[(i + 1) % contour.size()];
                    const float ax = a.x * scale + offset.x, ay = a.y * scale + offset.y;
                    const float bx = b.x * scale + offset.x, by = b.y * scale + offset.y;
                    if (ay == by)
                    {
                        continue;
                    }
                    if (ay < by)
                    {
                        edges.push_back({ax, ay, bx, by, 1});
                    }
                    else
                    {
                        edges.push_back({bx, by, ax, ay, -1});
                    }
                }
            }
            constexpr int SUB = 16;
            std::vector<float> row(static_cast<std::size_t>(width));
            std::vector<std::pair<float, int>> crossings;
            for (int y = 0; y < height; ++y)
            {
                std::fill(row.begin(), row.end(), 0.f);
                for (int s = 0; s < SUB; ++s)
                {
                    const float sy = static_cast<float>(y) + (static_cast<float>(s) + 0.5f) / SUB;
                    crossings.clear();
                    for (const Edge& e : edges)
                    {
                        if (sy >= e.y0 && sy < e.y1)
                        {
                            const float t = (sy - e.y0) / (e.y1 - e.y0);
                            crossings.emplace_back(e.x0 + t * (e.x1 - e.x0), e.dir);
                        }
                    }
                    std::sort(crossings.begin(), crossings.end(),
                              [](const auto& a, const auto& b)
                              {
                                  return a.first < b.first;
                              });
                    int winding = 0;
                    for (std::size_t i = 0; i + 1 < crossings.size(); ++i)
                    {
                        winding += crossings[i].second;
                        if (even_odd ? (winding & 1) == 0 : winding == 0)
                        {
                            continue;
                        }
                        const float xa = std::clamp(crossings[i].first, 0.f, static_cast<float>(width));
                        const float xb = std::clamp(crossings[i + 1].first, 0.f, static_cast<float>(width));
                        if (xb <= xa)
                        {
                            continue;
                        }
                        const int ia = static_cast<int>(xa);
                        const int ib = std::min(width - 1, static_cast<int>(xb));
                        for (int x = ia; x <= ib; ++x)
                        {
                            const float lo = std::max(xa, static_cast<float>(x));
                            const float hi = std::min(xb, static_cast<float>(x + 1));
                            if (hi > lo)
                            {
                                row[static_cast<std::size_t>(x)] += (hi - lo) / SUB;
                            }
                        }
                    }
                }
                for (int x = 0; x < width; ++x)
                {
                    float& dst = coverage[static_cast<std::size_t>(y) * width + x];
                    const float src = std::min(1.f, row[static_cast<std::size_t>(x)]) * opacity;
                    dst = src + dst * (1.f - src);
                }
            }
        }

        struct CachedTexture
        {
            ImTextureID texture = ImTextureID_Invalid;
            int width = 0;
            int height = 0;
        };

        std::unordered_map<std::string, CachedTexture> g_cache;

        void rotate(std::vector<Contour>& contours, ImVec2 pivot, float degrees)
        {
            if (degrees == 0.f)
            {
                return;
            }
            const float radians = degrees * 3.14159265f / 180.f;
            const float c = std::cos(radians), sn = std::sin(radians);
            for (Contour& contour : contours)
            {
                for (ImVec2& point : contour)
                {
                    const float dx = point.x - pivot.x, dy = point.y - pivot.y;
                    point = ImVec2(pivot.x + dx * c - dy * sn, pivot.y + dx * sn + dy * c);
                }
            }
        }

        // Rasterises `paths` (primary, optional secondary at 40%) to `width` x `height` pixels. The viewBox is
        // rotated about `pivot`, scaled uniformly by `scale` and shifted by `offset` pixels.
        CachedTexture build_texture(std::string_view primary, std::string_view secondary, int width, int height,
                                    float scale, ImVec2 offset, ImVec2 pivot = {0.f, 0.f}, float degrees = 0.f)
        {
            CachedTexture result;
            result.width = width;
            result.height = height;
            if (width <= 0 || height <= 0 || g_backend.create == nullptr)
            {
                return result;
            }
            std::vector<float> coverage(static_cast<std::size_t>(width) * height, 0.f);
            if (!secondary.empty())
            {
                std::vector<Contour> contours = PathParser(secondary).parse();
                rotate(contours, pivot, degrees);
                rasterize(contours, width, height, scale, offset, 0.4f, coverage);
            }
            // A leading '!' marks an SVG path drawn with fill-rule="evenodd".
            const bool even_odd = !primary.empty() && primary.front() == '!';
            if (even_odd)
            {
                primary.remove_prefix(1);
            }
            std::vector<Contour> contours = PathParser(primary).parse();
            rotate(contours, pivot, degrees);
            rasterize(contours, width, height, scale, offset, 1.f, coverage, even_odd);
            std::vector<unsigned char> rgba(coverage.size() * 4);
            for (std::size_t i = 0; i < coverage.size(); ++i)
            {
                rgba[i * 4 + 0] = 255;
                rgba[i * 4 + 1] = 255;
                rgba[i * 4 + 2] = 255;
                rgba[i * 4 + 3] = static_cast<unsigned char>(std::lround(std::clamp(coverage[i], 0.f, 1.f) * 255.f));
            }
            result.texture = g_backend.create(width, height, rgba.data());
            return result;
        }

        const FaIcon* find_icon(std::string_view name) noexcept
        {
            static const std::vector<FaIcon> icons = load_fa_icons();
            static const std::unordered_map<std::string_view, const FaIcon*> index = []
            {
                std::unordered_map<std::string_view, const FaIcon*> map;
                for (const FaIcon& icon : icons)
                {
                    map.emplace(icon.name, &icon);
                }
                return map;
            }();
            const auto found = index.find(name);
            return found == index.end() ? nullptr : found->second;
        }
    } // namespace

    std::string_view svg_path(std::string_view name)
    {
        static std::unordered_map<std::string, std::string_view> cache;
        std::string key(name);
        auto found = cache.find(key);
        if (found == cache.end())
        {
            const std::string_view svg = assets::text("svg/" + key + ".svg");
            std::string_view path;
            if (const std::size_t start = svg.find(" d=\""); start != std::string_view::npos)
            {
                const std::size_t begin = start + 4;
                path = svg.substr(begin, svg.find('"', begin) - begin);
            }
            found = cache.emplace(std::move(key), path).first;
        }
        return found->second;
    }

    void set_texture_backend(TextureBackend backend) noexcept
    {
        g_backend = backend;
    }

    ImTextureID create_texture(int width, int height, const unsigned char* rgba)
    {
        return g_backend.create == nullptr ? ImTextureID_Invalid : g_backend.create(width, height, rgba);
    }

    void destroy_texture(ImTextureID texture)
    {
        if (g_backend.destroy != nullptr && texture != ImTextureID_Invalid)
        {
            g_backend.destroy(texture);
        }
    }

    bool has_fa_icon(std::string_view name) noexcept
    {
        return find_icon(name) != nullptr;
    }

    float fa_icon_width(std::string_view name, float size) noexcept
    {
        const FaIcon* icon = find_icon(name);
        return icon == nullptr ? size : size * static_cast<float>(icon->width) / static_cast<float>(icon->height);
    }

    namespace
    {
        enum class Layer
        {
            BOTH, // a duotone's secondary path at 40% under the primary
            PRIMARY,
            SECONDARY,
        };

        void draw_icon_layer(ImDrawList* draw, std::string_view name, ImVec2 pos, float size, ImU32 color, Layer layer,
                             bool mirrored = false)
        {
            const FaIcon* icon = find_icon(name);
            if (icon == nullptr || size <= 0.f)
            {
                return;
            }
            // Snap the em box to the pixel grid and keep the sub-pixel remainder inside the raster, like Skia does
            // for an inline SVG.
            const float width = size * static_cast<float>(icon->width) / static_cast<float>(icon->height);
            const float x0 = std::floor(pos.x), y0 = std::floor(pos.y);
            const float fx = pos.x - x0, fy = pos.y - y0;
            const int tex_w = static_cast<int>(std::ceil(width + fx));
            const int tex_h = static_cast<int>(std::ceil(size + fy));
            const int qx = static_cast<int>(std::lround(fx * 4.f)), qy = static_cast<int>(std::lround(fy * 4.f));
            std::string key(name);
            key += layer == Layer::BOTH ? "@" : layer == Layer::PRIMARY ? "@p" : "@s";
            key += std::to_string(static_cast<int>(std::lround(size * 4.f)));
            key += ':';
            key += std::to_string(qx);
            key += ',';
            key += std::to_string(qy);
            auto found = g_cache.find(key);
            if (found == g_cache.end())
            {
                const float scale = size / static_cast<float>(icon->height);
                const std::string_view primary = layer == Layer::SECONDARY ? icon->secondary : icon->path;
                const std::string_view secondary = layer == Layer::BOTH ? icon->secondary : std::string_view();
                found = g_cache.emplace(key, build_texture(primary, secondary, tex_w, tex_h, scale,
                                                           ImVec2(static_cast<float>(qx) / 4.f,
                                                                  static_cast<float>(qy) / 4.f)))
                                .first;
            }
            const CachedTexture& tex = found->second;
            if (tex.texture == ImTextureID_Invalid)
            {
                return;
            }
            // Mirrored: the same raster with its u coordinates swapped (CSS fa-flip-horizontal).
            draw->AddImage(ImTextureRef(tex.texture), ImVec2(x0, y0),
                           ImVec2(x0 + static_cast<float>(tex.width), y0 + static_cast<float>(tex.height)),
                           ImVec2(mirrored ? 1.f : 0.f, 0.f), ImVec2(mirrored ? 0.f : 1.f, 1.f), color);
        }
    } // namespace

    void draw_fa_icon_at(ImDrawList* draw, std::string_view name, ImVec2 pos, float size, ImU32 color)
    {
        draw_icon_layer(draw, name, pos, size, color, Layer::BOTH);
    }

    void draw_fa_icon_mirrored(ImDrawList* draw, std::string_view name, ImVec2 center, float size, ImU32 color)
    {
        const float width = fa_icon_width(name, size);
        draw_icon_layer(draw, name, ImVec2(center.x - width / 2.f, center.y - size / 2.f), size, color, Layer::BOTH,
                        true);
    }

    void draw_fa_duotone(ImDrawList* draw, std::string_view name, ImVec2 center, float size, ImU32 primary,
                         ImU32 secondary)
    {
        const ImVec2 pos(center.x - fa_icon_width(name, size) / 2.f, center.y - size / 2.f);
        draw_icon_layer(draw, name, pos, size, secondary, Layer::SECONDARY);
        draw_icon_layer(draw, name, pos, size, primary, Layer::PRIMARY);
    }

    void draw_fa_icon(ImDrawList* draw, std::string_view name, ImVec2 center, float size, ImU32 color,
                      float rotation_degrees)
    {
        if (rotation_degrees == 0.f)
        {
            const float width = fa_icon_width(name, size);
            draw_fa_icon_at(draw, name, ImVec2(center.x - width / 2.f, center.y - size / 2.f), size, color);
            return;
        }
        const FaIcon* icon = find_icon(name);
        if (icon == nullptr || size <= 0.f)
        {
            return;
        }
        // A rotated glyph is rasterised into a square big enough for its rotated em box.
        const float scale = size / static_cast<float>(icon->height);
        const float diagonal = std::sqrt(static_cast<float>(icon->width * icon->width + icon->height * icon->height));
        const int side = static_cast<int>(std::ceil(diagonal * scale)) + 2;
        const float x0 = std::floor(center.x - static_cast<float>(side) / 2.f);
        const float y0 = std::floor(center.y - static_cast<float>(side) / 2.f);
        std::string key(name);
        key += '@';
        key += std::to_string(static_cast<int>(std::lround(size * 4.f)));
        key += 'r';
        key += std::to_string(static_cast<int>(std::lround(rotation_degrees)));
        auto found = g_cache.find(key);
        if (found == g_cache.end())
        {
            const ImVec2 pivot(static_cast<float>(icon->width) / 2.f, static_cast<float>(icon->height) / 2.f);
            const ImVec2 offset(static_cast<float>(side) / 2.f - pivot.x * scale,
                                static_cast<float>(side) / 2.f - pivot.y * scale);
            found = g_cache.emplace(key, build_texture(icon->path, icon->secondary, side, side, scale, offset, pivot,
                                                       rotation_degrees))
                            .first;
        }
        const CachedTexture& tex = found->second;
        if (tex.texture == ImTextureID_Invalid)
        {
            return;
        }
        draw->AddImage(ImTextureRef(tex.texture), ImVec2(x0, y0),
                       ImVec2(x0 + static_cast<float>(tex.width), y0 + static_cast<float>(tex.height)),
                       ImVec2(0.f, 0.f), ImVec2(1.f, 1.f), color);
    }

    void draw_svg_path(ImDrawList* draw, std::string_view key, std::string_view path, float view_w, float view_h,
                       ImVec2 p0, ImVec2 p1, ImU32 color)
    {
        const float x0 = std::floor(p0.x), y0 = std::floor(p0.y);
        const int tex_w = static_cast<int>(std::ceil(p1.x - x0));
        const int tex_h = static_cast<int>(std::ceil(p1.y - y0));
        std::string cache_key(key);
        cache_key += '#';
        cache_key += std::to_string(tex_w);
        cache_key += 'x';
        cache_key += std::to_string(tex_h);
        auto found = g_cache.find(cache_key);
        if (found == g_cache.end())
        {
            const float scale = std::min((p1.x - p0.x) / view_w, (p1.y - p0.y) / view_h);
            found = g_cache.emplace(cache_key,
                                    build_texture(path, "", tex_w, tex_h, scale, ImVec2(p0.x - x0, p0.y - y0)))
                            .first;
        }
        const CachedTexture& tex = found->second;
        if (tex.texture == ImTextureID_Invalid)
        {
            return;
        }
        draw->AddImage(ImTextureRef(tex.texture), ImVec2(x0, y0),
                       ImVec2(x0 + static_cast<float>(tex.width), y0 + static_cast<float>(tex.height)),
                       ImVec2(0.f, 0.f), ImVec2(1.f, 1.f), color);
    }
} // namespace imgit::ui
