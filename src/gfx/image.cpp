#include "gfx/image.hpp"
#include <climits>
#include <fstream>
#include <iterator>

#define STB_IMAGE_IMPLEMENTATION
#define STBI_ONLY_PNG
#define STBI_ONLY_JPEG
#define STBI_ONLY_GIF
#define STBI_ONLY_BMP
#include <stb_image.h>
#define STB_IMAGE_RESIZE_IMPLEMENTATION
#include <stb_image_resize2.h>

namespace imgit::gfx
{
    std::optional<Image> decode_image(std::span<const unsigned char> bytes, int scale_to)
    {
        if (bytes.empty() || bytes.size() > static_cast<std::size_t>(INT_MAX))
        {
            return std::nullopt;
        }
        int width = 0;
        int height = 0;
        int channels = 0;
        stbi_uc* pixels =
                stbi_load_from_memory(bytes.data(), static_cast<int>(bytes.size()), &width, &height, &channels, 4);
        if (pixels == nullptr)
        {
            return std::nullopt;
        }
        Image image;
        if (scale_to > 0)
        {
            image.width = scale_to;
            image.height = scale_to;
            image.rgba.resize(static_cast<std::size_t>(scale_to) * scale_to * 4);
            // STBIR_RGBA: straight alpha in and out, weighted by alpha while filtering.
            const void* resized =
                    stbir_resize(pixels, width, height, width * 4, image.rgba.data(), scale_to, scale_to, scale_to * 4,
                                 STBIR_RGBA, STBIR_TYPE_UINT8, STBIR_EDGE_CLAMP, STBIR_FILTER_BOX);
            stbi_image_free(pixels);
            if (resized == nullptr)
            {
                return std::nullopt;
            }
            return image;
        }
        image.width = width;
        image.height = height;
        image.rgba.assign(pixels, pixels + static_cast<std::size_t>(width) * height * 4);
        stbi_image_free(pixels);
        return image;
    }

    std::optional<Image> load_image(const std::filesystem::path& path, int scale_to)
    {
        std::ifstream file(path, std::ios::binary);
        if (!file)
        {
            return std::nullopt;
        }
        const std::vector<unsigned char> bytes((std::istreambuf_iterator<char>(file)),
                                               std::istreambuf_iterator<char>());
        return decode_image(bytes, scale_to);
    }
} // namespace imgit::gfx
