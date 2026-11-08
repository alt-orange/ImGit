#include "app/theme_file.hpp"
#include <algorithm>
#include <cctype>
#include <charconv>

namespace imgit::theme
{
    namespace
    {
        // Just enough JSON for a theme file: objects, arrays, strings, numbers, true / false / null.
        struct Json
        {
            enum class Type
            {
                NONE,
                BOOLEAN,
                NUMBER,
                STRING,
                ARRAY,
                OBJECT,
            };
            Type type = Type::NONE;
            bool boolean = false;
            double number = 0.0;
            std::string string;
            std::vector<Json> array;
            std::vector<std::pair<std::string, Json>> object; // in file order

            [[nodiscard]] const Json* member(std::string_view key) const
            {
                for (const auto& [name, value] : object)
                {
                    if (name == key)
                    {
                        return &value;
                    }
                }
                return nullptr;
            }
        };

        class Parser
        {
        public:
            explicit Parser(std::string_view text): m_text(text)
            {
            }

            std::expected<Json, std::string> document()
            {
                auto value = parse_value(0);
                if (!value)
                {
                    return value;
                }
                skip_space();
                if (m_pos != m_text.size())
                {
                    return fail("unexpected text after the theme");
                }
                return value;
            }

        private:
            static constexpr int MAX_DEPTH = 32;

            std::unexpected<std::string> fail(const std::string& what) const
            {
                // The line the parser stopped on, 1-based.
                const auto line =
                        1 + std::count(m_text.begin(), m_text.begin() + static_cast<std::ptrdiff_t>(m_pos), '\n');
                return std::unexpected("line " + std::to_string(line) + ": " + what);
            }

            void skip_space()
            {
                while (m_pos < m_text.size() && std::isspace(static_cast<unsigned char>(m_text[m_pos])))
                {
                    ++m_pos;
                }
            }

            bool consume(std::string_view word)
            {
                if (m_text.substr(m_pos, word.size()) == word)
                {
                    m_pos += word.size();
                    return true;
                }
                return false;
            }

            std::expected<Json, std::string> parse_value(int depth)
            {
                if (depth > MAX_DEPTH)
                {
                    return fail("nested too deeply");
                }
                skip_space();
                if (m_pos >= m_text.size())
                {
                    return fail("unexpected end of the file");
                }
                Json value;
                const char c = m_text[m_pos];
                if (c == '{')
                {
                    ++m_pos;
                    value.type = Json::Type::OBJECT;
                    skip_space();
                    if (consume("}"))
                    {
                        return value;
                    }
                    while (true)
                    {
                        skip_space();
                        auto key = parse_string();
                        if (!key)
                        {
                            return std::unexpected(key.error());
                        }
                        skip_space();
                        if (!consume(":"))
                        {
                            return fail("expected ':'");
                        }
                        auto member = parse_value(depth + 1);
                        if (!member)
                        {
                            return member;
                        }
                        value.object.emplace_back(std::move(*key), std::move(*member));
                        skip_space();
                        if (consume("}"))
                        {
                            return value;
                        }
                        if (!consume(","))
                        {
                            return fail("expected ',' or '}'");
                        }
                    }
                }
                if (c == '[')
                {
                    ++m_pos;
                    value.type = Json::Type::ARRAY;
                    skip_space();
                    if (consume("]"))
                    {
                        return value;
                    }
                    while (true)
                    {
                        auto item = parse_value(depth + 1);
                        if (!item)
                        {
                            return item;
                        }
                        value.array.push_back(std::move(*item));
                        skip_space();
                        if (consume("]"))
                        {
                            return value;
                        }
                        if (!consume(","))
                        {
                            return fail("expected ',' or ']'");
                        }
                    }
                }
                if (c == '"')
                {
                    auto text = parse_string();
                    if (!text)
                    {
                        return std::unexpected(text.error());
                    }
                    value.type = Json::Type::STRING;
                    value.string = std::move(*text);
                    return value;
                }
                if (consume("true"))
                {
                    value.type = Json::Type::BOOLEAN;
                    value.boolean = true;
                    return value;
                }
                if (consume("false"))
                {
                    value.type = Json::Type::BOOLEAN;
                    return value;
                }
                if (consume("null"))
                {
                    return value;
                }
                const char* begin = m_text.data() + m_pos;
                const char* end = m_text.data() + m_text.size();
                const auto [next, error] = std::from_chars(begin, end, value.number);
                if (error != std::errc{} || next == begin)
                {
                    return fail("unexpected character");
                }
                m_pos += static_cast<std::size_t>(next - begin);
                value.type = Json::Type::NUMBER;
                return value;
            }

            std::expected<std::string, std::string> parse_string()
            {
                if (!consume("\""))
                {
                    return fail("expected a string");
                }
                std::string text;
                while (m_pos < m_text.size())
                {
                    const char c = m_text[m_pos++];
                    if (c == '"')
                    {
                        return text;
                    }
                    if (c != '\\')
                    {
                        text += c;
                        continue;
                    }
                    if (m_pos >= m_text.size())
                    {
                        break;
                    }
                    const char escaped = m_text[m_pos++];
                    switch (escaped)
                    {
                    case 'n':
                        text += '\n';
                        break;
                    case 't':
                        text += '\t';
                        break;
                    case 'r':
                        text += '\r';
                        break;
                    case 'b':
                        text += '\b';
                        break;
                    case 'f':
                        text += '\f';
                        break;
                    case 'u':
                    {
                        // \uXXXX as UTF-8 (surrogate pairs are not joined: a theme has no use for them).
                        unsigned code = 0;
                        if (m_pos + 4 > m_text.size()
                            || std::from_chars(m_text.data() + m_pos, m_text.data() + m_pos + 4, code, 16).ec
                                       != std::errc{})
                        {
                            return fail("bad \\u escape");
                        }
                        m_pos += 4;
                        if (code < 0x80)
                        {
                            text += static_cast<char>(code);
                        }
                        else if (code < 0x800)
                        {
                            text += static_cast<char>(0xC0 | (code >> 6));
                            text += static_cast<char>(0x80 | (code & 0x3F));
                        }
                        else
                        {
                            text += static_cast<char>(0xE0 | (code >> 12));
                            text += static_cast<char>(0x80 | ((code >> 6) & 0x3F));
                            text += static_cast<char>(0x80 | (code & 0x3F));
                        }
                        break;
                    }
                    default: // \" \\ \/
                        text += escaped;
                        break;
                    }
                }
                return fail("unterminated string");
            }

            std::string_view m_text;
            std::size_t m_pos = 0;
        };

        std::optional<unsigned> hex_digit(char c)
        {
            if (c >= '0' && c <= '9')
            {
                return static_cast<unsigned>(c - '0');
            }
            c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
            if (c >= 'a' && c <= 'f')
            {
                return static_cast<unsigned>(c - 'a' + 10);
            }
            return std::nullopt;
        }

        std::string_view trim(std::string_view text)
        {
            while (!text.empty() && std::isspace(static_cast<unsigned char>(text.front())))
            {
                text.remove_prefix(1);
            }
            while (!text.empty() && std::isspace(static_cast<unsigned char>(text.back())))
            {
                text.remove_suffix(1);
            }
            return text;
        }
    } // namespace

    std::optional<Rgba> parse_color(std::string_view text)
    {
        text = trim(text);
        if (text.starts_with('#'))
        {
            text.remove_prefix(1);
            std::vector<unsigned> digits;
            for (const char c : text)
            {
                const auto digit = hex_digit(c);
                if (!digit)
                {
                    return std::nullopt;
                }
                digits.push_back(*digit);
            }
            std::vector<float> channels;
            if (digits.size() == 3 || digits.size() == 4)
            {
                for (const unsigned d : digits)
                {
                    channels.push_back(static_cast<float>(d * 17) / 255.f);
                }
            }
            else if (digits.size() == 6 || digits.size() == 8)
            {
                for (std::size_t i = 0; i < digits.size(); i += 2)
                {
                    channels.push_back(static_cast<float>(digits[i] * 16 + digits[i + 1]) / 255.f);
                }
            }
            else
            {
                return std::nullopt;
            }
            return Rgba{channels[0], channels[1], channels[2], channels.size() == 4 ? channels[3] : 1.f};
        }
        const bool has_alpha = text.starts_with("rgba(");
        if (!has_alpha && !text.starts_with("rgb("))
        {
            return std::nullopt;
        }
        text.remove_prefix(has_alpha ? 5 : 4);
        if (!text.ends_with(')'))
        {
            return std::nullopt;
        }
        text.remove_suffix(1);
        std::vector<float> values;
        while (true)
        {
            const auto comma = text.find(',');
            const std::string_view part = trim(text.substr(0, comma));
            float value = 0.f;
            const auto [next, error] = std::from_chars(part.data(), part.data() + part.size(), value);
            if (error != std::errc{} || next != part.data() + part.size())
            {
                return std::nullopt;
            }
            values.push_back(value);
            if (comma == std::string_view::npos)
            {
                break;
            }
            text.remove_prefix(comma + 1);
        }
        if (values.size() != (has_alpha ? 4u : 3u))
        {
            return std::nullopt;
        }
        const auto channel = [](float v)
        {
            return std::clamp(v / 255.f, 0.f, 1.f);
        };
        return Rgba{channel(values[0]), channel(values[1]), channel(values[2]),
                    has_alpha ? std::clamp(values[3], 0.f, 1.f) : 1.f};
    }

    std::expected<ThemeFile, std::string> parse_theme_file(std::string_view json)
    {
        auto document = Parser(json).document();
        if (!document)
        {
            return std::unexpected(document.error());
        }
        if (document->type != Json::Type::OBJECT)
        {
            return std::unexpected("a theme is a JSON object");
        }
        ThemeFile theme;
        const Json* name = document->member("name");
        if (name == nullptr || name->type != Json::Type::STRING || trim(name->string).empty())
        {
            return std::unexpected("the theme has no \"name\"");
        }
        theme.name = std::string(trim(name->string));
        if (const Json* base = document->member("base"); base != nullptr)
        {
            if (base->type != Json::Type::STRING)
            {
                return std::unexpected("\"base\" is not a theme name");
            }
            theme.base = base->string;
        }
        const std::pair<const char*, std::optional<Rgba>*> keys[] = {
                {"background", &theme.background},   {"accent", &theme.accent}, {"link", &theme.link},
                {"text_accent", &theme.text_accent}, {"avatar", &theme.avatar}, {"prompt", &theme.prompt},
        };
        for (const auto& [key, target] : keys)
        {
            const Json* value = document->member(key);
            if (value == nullptr)
            {
                continue;
            }
            const auto color = value->type == Json::Type::STRING ? parse_color(value->string) : std::nullopt;
            if (!color)
            {
                return std::unexpected(std::string("\"") + key + "\" is not a colour");
            }
            *target = color;
        }
        if (const Json* colors = document->member("colors"); colors != nullptr)
        {
            if (colors->type != Json::Type::OBJECT)
            {
                return std::unexpected("\"colors\" is not an object");
            }
            for (const auto& [key, value] : colors->object)
            {
                const auto color = value.type == Json::Type::STRING ? parse_color(value.string) : std::nullopt;
                if (!color)
                {
                    return std::unexpected("colour \"" + key + "\" is not a colour");
                }
                theme.colors.emplace_back(key, *color);
            }
        }
        return theme;
    }
} // namespace imgit::theme
