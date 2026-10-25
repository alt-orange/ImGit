#pragma once

#include <cstddef>
#include <span>
#include <string_view>

// The files under assets/ (fonts, images, icons, cursors), compiled into the executable by
// cmake/EmbedAssets.cmake.
namespace imgit::assets
{
    // The bytes of the asset at `path` (relative to assets/, forward slashes, e.g. "fonts/OpenSans-Regular.ttf"),
    // or an empty span when there is none. The data lives as long as the program and is followed by a zero byte.
    [[nodiscard]] std::span<const unsigned char> data(std::string_view path) noexcept;
    // The same bytes as text.
    [[nodiscard]] std::string_view text(std::string_view path) noexcept;

    namespace detail
    {
        struct Entry
        {
            const char* path;
            const unsigned char* data;
            const std::size_t* size;
        };

        // Defined in the generated embedded_assets.cpp.
        extern const Entry ENTRIES[];
        extern const std::size_t ENTRY_COUNT;
    } // namespace detail
} // namespace imgit::assets
