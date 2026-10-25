#pragma once

#include <filesystem>
#include <optional>
#include <span>
#include <vector>

// Image files to pixels (stb_image), on every platform.
namespace imgit::gfx
{
    struct Image
    {
        int width = 0;
        int height = 0;
        std::vector<unsigned char> rgba; // straight (not premultiplied) alpha, rows top to bottom
    };

    // PNG / JPEG / GIF (first frame) / BMP to RGBA8. With `scale_to` > 0, resampled to that many pixels square with
    // a box filter on alpha-weighted pixels (what WIC's Fant scaler did, and close to Chromium's <img> downscale).
    [[nodiscard]] std::optional<Image> decode_image(std::span<const unsigned char> bytes, int scale_to = 0);
    [[nodiscard]] std::optional<Image> load_image(const std::filesystem::path& path, int scale_to = 0);
} // namespace imgit::gfx
