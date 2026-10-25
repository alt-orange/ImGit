#include "gfx/native_font_loader.hpp"
#include <algorithm>
#include <cmath>
#include <imgui_internal.h>
#include <vector>
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <dwrite_3.h>
#include <windows.h>

namespace imgit::gfx
{
    namespace
    {
        IDWriteFactory5* g_factory = nullptr;
        IDWriteInMemoryFontFileLoader* g_memory_loader = nullptr;
        IDWriteRenderingParams* g_params = nullptr;
        unsigned char g_curve[256];
        bool g_curve_ready = false;

        struct SourceData
        {
            IDWriteFontFace* face = nullptr;
            DWRITE_FONT_METRICS metrics{};
        };

        // Per (source, baked size): the em size DirectWrite renders at.
        struct BakedData
        {
            float em = 0.f;
            DWRITE_RENDERING_MODE mode = DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC;
        };

        template<class Interface>
        void release(Interface*& pointer)
        {
            if (pointer != nullptr)
            {
                pointer->Release();
                pointer = nullptr;
            }
        }

        void build_curve(float gamma, float contrast)
        {
            for (int i = 0; i < 256; ++i)
            {
                float c = std::pow(static_cast<float>(i) / 255.f, 1.f / gamma);
                c = std::clamp(c + contrast * c * (1.f - c), 0.f, 1.f);
                g_curve[i] = static_cast<unsigned char>(std::lround(c * 255.f));
            }
            g_curve_ready = true;
        }

        bool loader_init(ImFontAtlas*)
        {
            if (g_factory != nullptr)
            {
                return true;
            }
            if (FAILED(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory5),
                                           reinterpret_cast<IUnknown**>(&g_factory))))
            {
                return false;
            }
            if (FAILED(g_factory->CreateInMemoryFontFileLoader(&g_memory_loader))
                || FAILED(g_factory->RegisterFontFileLoader(g_memory_loader)))
            {
                release(g_memory_loader);
                release(g_factory);
                return false;
            }
            g_factory->CreateRenderingParams(&g_params);
            if (!g_curve_ready)
            {
                build_curve(1.f, 0.f);
            }
            return true;
        }

        void loader_shutdown(ImFontAtlas*)
        {
            if (g_factory != nullptr && g_memory_loader != nullptr)
            {
                g_factory->UnregisterFontFileLoader(g_memory_loader);
            }
            release(g_params);
            release(g_memory_loader);
            release(g_factory);
        }

        bool source_init(ImFontAtlas* atlas, ImFontConfig* src)
        {
            if (!loader_init(atlas))
            {
                return false;
            }
            IDWriteFontFile* file = nullptr;
            if (FAILED(g_memory_loader->CreateInMemoryFontFileReference(
                        g_factory, src->FontData, static_cast<UINT32>(src->FontDataSize), nullptr, &file)))
            {
                return false;
            }
            // The face type comes from the file: a CFF .otf or a .ttc collection is not DWRITE_FONT_FACE_TYPE_TRUETYPE.
            BOOL supported = FALSE;
            DWRITE_FONT_FILE_TYPE file_type = DWRITE_FONT_FILE_TYPE_UNKNOWN;
            DWRITE_FONT_FACE_TYPE face_type = DWRITE_FONT_FACE_TYPE_UNKNOWN;
            UINT32 face_count = 0;
            if (FAILED(file->Analyze(&supported, &file_type, &face_type, &face_count)) || !supported)
            {
                file->Release();
                return false;
            }
            auto* data = IM_NEW(SourceData)();
            const HRESULT created = g_factory->CreateFontFace(face_type, 1, &file, static_cast<UINT32>(src->FontNo),
                                                              DWRITE_FONT_SIMULATIONS_NONE, &data->face);
            file->Release();
            if (FAILED(created))
            {
                IM_DELETE(data);
                return false;
            }
            data->face->GetMetrics(&data->metrics);
            src->FontLoaderData = data;
            return true;
        }

        void source_destroy(ImFontAtlas*, ImFontConfig* src)
        {
            auto* data = static_cast<SourceData*>(src->FontLoaderData);
            if (data != nullptr)
            {
                release(data->face);
                IM_DELETE(data);
            }
            src->FontLoaderData = nullptr;
        }

        UINT16 glyph_index(const SourceData* data, ImWchar codepoint)
        {
            const UINT32 cp = codepoint;
            UINT16 index = 0;
            data->face->GetGlyphIndices(&cp, 1, &index);
            return index;
        }

        bool source_contains_glyph(ImFontAtlas*, ImFontConfig* src, ImWchar codepoint)
        {
            return glyph_index(static_cast<const SourceData*>(src->FontLoaderData), codepoint) != 0;
        }

        bool baked_init(ImFontAtlas*, ImFontConfig* src, ImFontBaked* baked, void* loader_data)
        {
            const auto* data = static_cast<const SourceData*>(src->FontLoaderData);
            float size = baked->Size;
            const float ref_size = baked->OwnerFont->Sources[0]->SizePixels;
            if (src->MergeMode && src->SizePixels != 0.f)
            {
                size *= src->SizePixels / ref_size;
            }
            size *= src->ExtraSizeScale;
            const float density = src->RasterizerDensity * baked->RasterizerDensity;
            // ImGui sizes a font by ascent + descent (FreeType's REAL_DIM request); DirectWrite takes the em.
            const float units = static_cast<float>(data->metrics.ascent + data->metrics.descent);
            auto* baked_data = IM_PLACEMENT_NEW(loader_data) BakedData();
            baked_data->em = size * density * static_cast<float>(data->metrics.designUnitsPerEm) / units;
            DWRITE_RENDERING_MODE mode = DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC;
            if (g_params != nullptr
                && SUCCEEDED(data->face->GetRecommendedRenderingMode(baked_data->em, 1.f, DWRITE_MEASURING_MODE_NATURAL,
                                                                     g_params, &mode))
                && mode != DWRITE_RENDERING_MODE_OUTLINE && mode != DWRITE_RENDERING_MODE_ALIASED)
            {
                baked_data->mode = mode;
            }
            if (!src->MergeMode)
            {
                const float to_px = baked_data->em / static_cast<float>(data->metrics.designUnitsPerEm);
                const float scale = 1.f / (density * src->ExtraSizeScale);
                baked->Ascent = std::ceil(static_cast<float>(data->metrics.ascent) * to_px) * scale;
                baked->Descent = -std::floor(static_cast<float>(data->metrics.descent) * to_px) * scale;
            }
            return true;
        }

        void baked_destroy(ImFontAtlas*, ImFontConfig*, ImFontBaked*, void* loader_data)
        {
            static_cast<BakedData*>(loader_data)->~BakedData();
        }

        bool baked_load_glyph(ImFontAtlas* atlas, ImFontConfig* src, ImFontBaked* baked, void* loader_data,
                              ImWchar codepoint, ImFontGlyph* out_glyph, float* out_advance_x)
        {
            const auto* data = static_cast<const SourceData*>(src->FontLoaderData);
            const auto* baked_data = static_cast<const BakedData*>(loader_data);
            const UINT16 index = glyph_index(data, codepoint);
            if (index == 0)
            {
                return false;
            }
            const float density = src->RasterizerDensity * baked->RasterizerDensity;
            DWRITE_GLYPH_METRICS glyph_metrics{};
            data->face->GetDesignGlyphMetrics(&index, 1, &glyph_metrics, FALSE);
            const float to_px = baked_data->em / static_cast<float>(data->metrics.designUnitsPerEm);
            const float advance_x = static_cast<float>(glyph_metrics.advanceWidth) * to_px / density;
            if (out_advance_x != nullptr)
            {
                *out_advance_x = advance_x;
                return true;
            }
            out_glyph->Codepoint = codepoint;
            out_glyph->AdvanceX = advance_x;

            const FLOAT advance = 0.f;
            const DWRITE_GLYPH_OFFSET offset{};
            DWRITE_GLYPH_RUN run{};
            run.fontFace = data->face;
            run.fontEmSize = baked_data->em;
            run.glyphCount = 1;
            run.glyphIndices = &index;
            run.glyphAdvances = &advance;
            run.glyphOffsets = &offset;
            IDWriteGlyphRunAnalysis* analysis = nullptr;
            // IDWriteFactory2's overload (later interfaces hide it behind DWRITE_RENDERING_MODE1 versions).
            IDWriteFactory2* factory2 = g_factory;
            if (FAILED(factory2->CreateGlyphRunAnalysis(&run, nullptr, baked_data->mode, DWRITE_MEASURING_MODE_NATURAL,
                                                        DWRITE_GRID_FIT_MODE_DEFAULT,
                                                        DWRITE_TEXT_ANTIALIAS_MODE_CLEARTYPE, 0.f, 0.f, &analysis)))
            {
                return false;
            }
            RECT bounds{};
            analysis->GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds);
            const int w = static_cast<int>(bounds.right - bounds.left);
            const int h = static_cast<int>(bounds.bottom - bounds.top);
            if (w <= 0 || h <= 0)
            {
                analysis->Release();
                return true; // a space: advance only
            }
            std::vector<unsigned char> coverage(static_cast<std::size_t>(w) * h * 3);
            const HRESULT rendered = analysis->CreateAlphaTexture(
                    DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds, coverage.data(), static_cast<UINT32>(coverage.size()));
            analysis->Release();
            if (FAILED(rendered))
            {
                return false;
            }
            // One empty column on each side: the renderer shifts glyphs by up to half a pixel for sub-pixel
            // positioning, and the quad must cover the shifted coverage.
            const int padded_w = w + 2;
            std::vector<unsigned char> rgba(static_cast<std::size_t>(padded_w) * h * 4, 0);
            for (int y = 0; y < h; ++y)
            {
                for (int x = 0; x < w; ++x)
                {
                    const std::size_t i = static_cast<std::size_t>(y) * w + x;
                    const std::size_t o = (static_cast<std::size_t>(y) * padded_w + x + 1) * 4;
                    const unsigned char r = g_curve[coverage[i * 3 + 0]];
                    const unsigned char g = g_curve[coverage[i * 3 + 1]];
                    const unsigned char b = g_curve[coverage[i * 3 + 2]];
                    rgba[o + 0] = r;
                    rgba[o + 1] = g;
                    rgba[o + 2] = b;
                    rgba[o + 3] = (r | g | b) != 0 ? 255 : 0;
                }
            }
            const ImFontAtlasRectId pack_id = ImFontAtlasPackAddRect(atlas, padded_w, h);
            if (pack_id == ImFontAtlasRectId_Invalid)
            {
                return false;
            }
            ImTextureRect* rect = ImFontAtlasPackGetRect(atlas, pack_id);
            const float ref_size = baked->OwnerFont->Sources[0]->SizePixels;
            const float offsets_scale = ref_size != 0.f ? baked->Size / ref_size : 1.f;
            const float font_off_x = std::floor(src->GlyphOffset.x * offsets_scale + 0.5f);
            const float font_off_y = std::floor(src->GlyphOffset.y * offsets_scale + 0.5f) + baked->Ascent;
            out_glyph->X0 = static_cast<float>(bounds.left - 1) / density + font_off_x;
            out_glyph->Y0 = static_cast<float>(bounds.top) / density + font_off_y;
            out_glyph->X1 = static_cast<float>(bounds.right + 1) / density + font_off_x;
            out_glyph->Y1 = static_cast<float>(bounds.bottom) / density + font_off_y;
            out_glyph->Visible = true;
            out_glyph->Colored = false;
            out_glyph->PackId = pack_id;
            ImFontAtlasBakedSetFontGlyphBitmap(atlas, baked, src, out_glyph, rect, rgba.data(), ImTextureFormat_RGBA32,
                                               padded_w * 4);
            return true;
        }

        ImFontLoader make_loader()
        {
            ImFontLoader loader;
            loader.Name = "DirectWrite ClearType";
            loader.LoaderInit = loader_init;
            loader.LoaderShutdown = loader_shutdown;
            loader.FontSrcInit = source_init;
            loader.FontSrcDestroy = source_destroy;
            loader.FontSrcContainsGlyph = source_contains_glyph;
            loader.FontBakedInit = baked_init;
            loader.FontBakedDestroy = baked_destroy;
            loader.FontBakedLoadGlyph = baked_load_glyph;
            loader.FontBakedSrcLoaderDataSize = sizeof(BakedData);
            return loader;
        }
    } // namespace

    const ImFontLoader* native_font_loader()
    {
        static const ImFontLoader loader = make_loader();
        return loader_init(nullptr) ? &loader : nullptr;
    }

    void set_native_coverage_curve(float gamma, float contrast)
    {
        build_curve(gamma, contrast);
    }
} // namespace imgit::gfx
