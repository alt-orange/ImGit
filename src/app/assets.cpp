#include "app/assets.hpp"

namespace imgit::assets
{
    std::span<const unsigned char> data(std::string_view path) noexcept
    {
        for (std::size_t i = 0; i < detail::ENTRY_COUNT; ++i)
        {
            const detail::Entry& entry = detail::ENTRIES[i];
            if (path == entry.path)
            {
                return {entry.data, *entry.size};
            }
        }
        return {};
    }

    std::string_view text(std::string_view path) noexcept
    {
        const std::span<const unsigned char> bytes = data(path);
        return {reinterpret_cast<const char*>(bytes.data()), bytes.size()};
    }
} // namespace imgit::assets
