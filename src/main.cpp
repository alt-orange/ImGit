#include "app/application.hpp"
#include "app/assets.hpp"
#include "app/theme.hpp"
#include "gfx/lcd_renderer.hpp"
#include "gfx/native_font_loader.hpp"
#include "git/git_cli.hpp"
#include "git/repository.hpp"
#include "platform/process.hpp"
#include "platform/system.hpp"
#include "platform/window.hpp"
#include "ui/avatars.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <GLFW/glfw3.h>
#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <imgui.h>
#include <imgui_impl_glfw.h>
#include <imgui_impl_opengl3.h>
#ifdef IMGUI_ENABLE_FREETYPE
#include <imgui_freetype.h>
#include <imgui_internal.h>
#endif
#include <cmath>
#include <span>
#include <sstream>
#include <string>
#include <thread>
#include <utility>
#include <vector>

namespace
{
    // Development aid: `imgit <repo> [--size WxH] --screenshot out.bmp [--select-row N] [--open-file path]` renders a
    // few frames, saves the framebuffer and exits. `--script file --shots dir` instead replays scripted input (move /
    // click / rclick / dblclick / drag / key / type / wait / shot / row) one step per frame.
    struct DevOptions
    {
        std::string repo;
        std::string screenshot;
        int select_row = -1;
        std::string open_file;
        std::string script;
        std::string shots_dir = ".";
        int width = 1440;
        int height = 900;
        bool terminal = false;
    };

    // One primitive input step of a dev script.
    struct ScriptStep
    {
        enum class Kind
        {
            MOVE,
            BUTTON,
            KEY,
            CHARS,
            WAIT,
            SHOT,
            ROW,
            WHEEL,
        };
        Kind kind = Kind::WAIT;
        float x = 0.f, y = 0.f;
        int button = 0;
        bool down = false;
        ImGuiKey key = ImGuiKey_None;
        std::string text;
        int frames = 0;
    };

    ImGuiKey key_from_name(const std::string& name)
    {
        static const std::pair<const char*, ImGuiKey> named[] = {
                {"ctrl", ImGuiMod_Ctrl},
                {"shift", ImGuiMod_Shift},
                {"alt", ImGuiMod_Alt},
                {"enter", ImGuiKey_Enter},
                {"escape", ImGuiKey_Escape},
                {"esc", ImGuiKey_Escape},
                {"tab", ImGuiKey_Tab},
                {"delete", ImGuiKey_Delete},
                {"backspace", ImGuiKey_Backspace},
                {"up", ImGuiKey_UpArrow},
                {"down", ImGuiKey_DownArrow},
                {"left", ImGuiKey_LeftArrow},
                {"right", ImGuiKey_RightArrow},
                {"home", ImGuiKey_Home},
                {"end", ImGuiKey_End},
                {"f5", ImGuiKey_F5},
                {"space", ImGuiKey_Space},
                {"=", ImGuiKey_Equal},
                {"-", ImGuiKey_Minus},
                {"`", ImGuiKey_GraveAccent},
                {"/", ImGuiKey_Slash},
        };
        for (const auto& [n, key] : named)
        {
            if (name == n)
            {
                return key;
            }
        }
        if (name.size() == 1 && name[0] >= 'a' && name[0] <= 'z')
        {
            return static_cast<ImGuiKey>(ImGuiKey_A + (name[0] - 'a'));
        }
        if (name.size() == 1 && name[0] >= '0' && name[0] <= '9')
        {
            return static_cast<ImGuiKey>(ImGuiKey_0 + (name[0] - '0'));
        }
        return ImGuiKey_None;
    }

    // Expands the script's lines into primitive steps.
    std::vector<ScriptStep> load_script(const std::string& path)
    {
        std::vector<ScriptStep> steps;
        std::ifstream file(path);
        std::string line;
        const auto wait = [&](int frames)
        {
            ScriptStep step;
            step.kind = ScriptStep::Kind::WAIT;
            step.frames = frames;
            steps.push_back(step);
        };
        const auto move = [&](float x, float y)
        {
            ScriptStep step;
            step.kind = ScriptStep::Kind::MOVE;
            step.x = x;
            step.y = y;
            steps.push_back(step);
        };
        const auto button = [&](int b, bool down)
        {
            ScriptStep step;
            step.kind = ScriptStep::Kind::BUTTON;
            step.button = b;
            step.down = down;
            steps.push_back(step);
        };
        const auto click = [&](float x, float y, int b)
        {
            move(x, y);
            wait(2);
            button(b, true);
            wait(2);
            button(b, false);
            wait(4);
        };
        while (std::getline(file, line))
        {
            std::istringstream in(line);
            std::string command;
            in >> command;
            if (command.empty() || command[0] == '#')
            {
                continue;
            }
            if (command == "move")
            {
                float x, y;
                in >> x >> y;
                move(x, y);
            }
            else if (command == "click" || command == "rclick" || command == "dblclick")
            {
                float x, y;
                in >> x >> y;
                const int b = command == "rclick" ? 1 : 0;
                click(x, y, b);
                if (command == "dblclick")
                {
                    click(x, y, b);
                }
            }
            else if (command == "press" || command == "release")
            {
                // A held button for mid-drag shots: "press x y", moves, shots, then "release".
                if (command == "press")
                {
                    float x, y;
                    in >> x >> y;
                    move(x, y);
                    wait(2);
                }
                button(0, command == "press");
                wait(2);
            }
            else if (command == "drag")
            {
                float x1, y1, x2, y2;
                in >> x1 >> y1 >> x2 >> y2;
                move(x1, y1);
                wait(2);
                button(0, true);
                wait(2);
                move((x1 + x2) / 2.f, (y1 + y2) / 2.f);
                wait(2);
                move(x2, y2);
                wait(6);
                button(0, false);
                wait(4);
            }
            else if (command == "key")
            {
                std::string chord;
                in >> chord;
                std::vector<ImGuiKey> keys;
                std::size_t start = 0;
                while (start <= chord.size())
                {
                    const std::size_t plus = chord.find('+', start);
                    const std::string part =
                            chord.substr(start, plus == std::string::npos ? std::string::npos : plus - start);
                    if (!part.empty())
                    {
                        keys.push_back(key_from_name(part));
                    }
                    if (plus == std::string::npos)
                    {
                        break;
                    }
                    start = plus + 1;
                }
                for (const ImGuiKey key : keys)
                {
                    ScriptStep step;
                    step.kind = ScriptStep::Kind::KEY;
                    step.key = key;
                    step.down = true;
                    steps.push_back(step);
                }
                wait(2);
                for (auto it = keys.rbegin(); it != keys.rend(); ++it)
                {
                    ScriptStep step;
                    step.kind = ScriptStep::Kind::KEY;
                    step.key = *it;
                    step.down = false;
                    steps.push_back(step);
                }
                wait(3);
            }
            else if (command == "keydown" || command == "keyup")
            {
                std::string name;
                in >> name;
                ScriptStep step;
                step.kind = ScriptStep::Kind::KEY;
                step.key = key_from_name(name);
                step.down = command == "keydown";
                steps.push_back(step);
                wait(2);
            }
            else if (command == "type")
            {
                ScriptStep step;
                step.kind = ScriptStep::Kind::CHARS;
                std::getline(in, step.text);
                if (!step.text.empty() && step.text.front() == ' ')
                {
                    step.text.erase(step.text.begin());
                }
                steps.push_back(step);
                wait(2);
            }
            else if (command == "wait")
            {
                int frames = 1;
                in >> frames;
                wait(frames);
            }
            else if (command == "shot")
            {
                ScriptStep step;
                step.kind = ScriptStep::Kind::SHOT;
                in >> step.text;
                steps.push_back(step);
            }
            else if (command == "row")
            {
                ScriptStep step;
                step.kind = ScriptStep::Kind::ROW;
                in >> step.frames;
                steps.push_back(step);
            }
            else if (command == "wheel")
            {
                // "wheel dx dy": notches, horizontal then vertical (positive: left / up, as ImGui counts them).
                ScriptStep step;
                step.kind = ScriptStep::Kind::WHEEL;
                in >> step.x >> step.y;
                steps.push_back(step);
            }
        }
        return steps;
    }

    DevOptions parse_args(int argc, char** argv)
    {
        DevOptions options;
        for (int i = 1; i < argc; ++i)
        {
            const std::string arg = argv[i];
            if (arg == "--screenshot" && i + 1 < argc)
            {
                options.screenshot = argv[++i];
            }
            else if (arg == "--select-row" && i + 1 < argc)
            {
                options.select_row = std::atoi(argv[++i]);
            }
            else if (arg == "--open-file" && i + 1 < argc)
            {
                options.open_file = argv[++i];
            }
            else if (arg == "--script" && i + 1 < argc)
            {
                options.script = argv[++i];
            }
            else if (arg == "--shots" && i + 1 < argc)
            {
                options.shots_dir = argv[++i];
            }
            else if (arg == "--terminal")
            {
                options.terminal = true;
            }
            else if (arg == "--size" && i + 1 < argc)
            {
                const std::string size = argv[++i];
                const std::size_t x = size.find('x');
                if (x != std::string::npos)
                {
                    options.width = std::atoi(size.substr(0, x).c_str());
                    options.height = std::atoi(size.substr(x + 1).c_str());
                }
            }
            else if (options.repo.empty())
            {
                options.repo = arg;
            }
        }
        return options;
    }

    void save_bmp(const std::filesystem::path& path, int width, int height)
    {
        std::vector<unsigned char> pixels(static_cast<std::size_t>(width) * height * 3);
        glPixelStorei(GL_PACK_ALIGNMENT, 1);
        glReadPixels(0, 0, width, height, GL_RGB, GL_UNSIGNED_BYTE, pixels.data());
        const int row_bytes = width * 3;
        const int padded = (row_bytes + 3) & ~3;
        const std::uint32_t image_size = static_cast<std::uint32_t>(padded) * static_cast<std::uint32_t>(height);
        unsigned char header[54] = {'B', 'M'};
        const std::uint32_t file_size = 54 + image_size;
        std::memcpy(header + 2, &file_size, 4);
        const std::uint32_t offset = 54;
        std::memcpy(header + 10, &offset, 4);
        const std::uint32_t dib = 40;
        std::memcpy(header + 14, &dib, 4);
        std::memcpy(header + 18, &width, 4);
        std::memcpy(header + 22, &height, 4);
        const std::uint16_t planes = 1, bpp = 24;
        std::memcpy(header + 26, &planes, 2);
        std::memcpy(header + 28, &bpp, 2);
        std::memcpy(header + 34, &image_size, 4);
        std::FILE* file = std::fopen(path.string().c_str(), "wb");
        if (file == nullptr)
        {
            return;
        }
        std::fwrite(header, 1, sizeof(header), file);
        std::vector<unsigned char> row(static_cast<std::size_t>(padded));
        for (int y = 0; y < height; ++y) // BMP rows are bottom-up, like glReadPixels
        {
            const unsigned char* src = pixels.data() + static_cast<std::size_t>(y) * row_bytes;
            for (int x = 0; x < width; ++x)
            {
                row[x * 3 + 0] = src[x * 3 + 2];
                row[x * 3 + 1] = src[x * 3 + 1];
                row[x * 3 + 2] = src[x * 3 + 0];
            }
            std::fwrite(row.data(), 1, row.size(), file);
        }
        std::fclose(file);
    }

    // ImGui places glyph quads at fractional x positions; with bilinear sampling every glyph is smeared over two
    // pixel columns. Nearest sampling of the font atlas lands each glyph on whole pixels, the way Chromium's
    // DirectWrite text looks. Applied every frame because the backend recreates the texture when the dynamic
    // atlas grows.
    void sharpen_font_atlas(ImDrawData* draw_data)
    {
        // Create / update the textures first so a texture made this frame is already nearest-sampled.
        if (draw_data->Textures != nullptr)
        {
            for (ImTextureData* tex : *draw_data->Textures)
            {
                if (tex->Status != ImTextureStatus_OK)
                {
                    ImGui_ImplOpenGL3_UpdateTexture(tex);
                }
            }
        }
        for (ImTextureData* tex : ImGui::GetPlatformIO().Textures)
        {
            if (tex->Status != ImTextureStatus_OK || tex->TexID == ImTextureID_Invalid)
            {
                continue;
            }
            glBindTexture(GL_TEXTURE_2D, static_cast<GLuint>(tex->TexID));
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        }
    }

    // GL 1.1 entry points (opengl32) are enough for small static RGBA textures.
    ImTextureID create_gl_texture(int width, int height, const unsigned char* rgba)
    {
        constexpr GLint CLAMP_TO_EDGE = 0x812F;
        GLuint texture = 0;
        glGenTextures(1, &texture);
        glBindTexture(GL_TEXTURE_2D, texture);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, CLAMP_TO_EDGE);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, CLAMP_TO_EDGE);
        glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
        glPixelStorei(GL_UNPACK_ROW_LENGTH, 0);
        glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, width, height, 0, GL_RGBA, GL_UNSIGNED_BYTE, rgba);
        return static_cast<ImTextureID>(texture);
    }

    void destroy_gl_texture(ImTextureID texture)
    {
        const GLuint id = static_cast<GLuint>(texture);
        glDeleteTextures(1, &id);
    }

    // A font compiled into the executable (assets/fonts).
    ImFont* load_font(ImGuiIO& io, const char* asset, float size)
    {
        const std::span<const unsigned char> data = imgit::assets::data(asset);
        if (data.empty())
        {
            return nullptr;
        }
        ImFontConfig config;
        config.FontDataOwnedByAtlas = false; // the executable's own bytes
        return io.Fonts->AddFontFromMemoryTTF(const_cast<unsigned char*>(data.data()), static_cast<int>(data.size()),
                                              size, &config);
    }

    // A font installed with the system.
    ImFont* load_system_font(ImGuiIO& io, imgit::platform::SystemFont font, float size)
    {
        const std::filesystem::path path = imgit::platform::system_font_file(font);
        if (path.empty())
        {
            return nullptr;
        }
        return io.Fonts->AddFontFromFileTTF(path.string().c_str(), size);
    }

#ifdef IMGUI_ENABLE_FREETYPE
    // Chromium blends ClearType text with gamma correction, so light text on the dark panels carries more
    // ink than ImGui's linear grey-scale coverage. The FreeType loader is wrapped to pass each new glyph's coverage
    // through the same kind of gamma curve as it lands in the atlas.
    ImFontLoader g_gamma_loader;
    bool (*g_base_load_glyph)(ImFontAtlas*, ImFontConfig*, ImFontBaked*, void*, ImWchar, ImFontGlyph*,
                              float*) = nullptr;
    unsigned char g_gamma_lut[256];

    bool load_glyph_with_gamma(ImFontAtlas* atlas, ImFontConfig* src, ImFontBaked* baked, void* loader_data,
                               ImWchar codepoint, ImFontGlyph* out_glyph, float* out_advance_x)
    {
        if (!g_base_load_glyph(atlas, src, baked, loader_data, codepoint, out_glyph, out_advance_x))
        {
            return false;
        }
        if (out_glyph == nullptr || !out_glyph->Visible || out_glyph->Colored
            || out_glyph->PackId == ImFontAtlasRectId_Invalid)
        {
            return true;
        }
        const ImTextureRect* rect = ImFontAtlasPackGetRect(atlas, out_glyph->PackId);
        ImTextureData* tex = atlas->TexData;
        const int bpp = tex->Format == ImTextureFormat_RGBA32 ? 4 : 1;
        const int alpha = bpp == 4 ? 3 : 0;
        for (int y = 0; y < rect->h; ++y)
        {
            auto* row = static_cast<unsigned char*>(tex->GetPixelsAt(rect->x, rect->y + y));
            for (int x = 0; x < rect->w; ++x)
            {
                row[x * bpp + alpha] = g_gamma_lut[row[x * bpp + alpha]];
            }
        }
        return true;
    }

    void install_gamma_font_loader(ImFontAtlas* atlas, float gamma)
    {
        for (int i = 0; i < 256; ++i)
        {
            g_gamma_lut[i] = static_cast<unsigned char>(std::lround(std::pow(i / 255.f, 1.f / gamma) * 255.f));
        }
        const ImFontLoader* base = ImGuiFreeType::GetFontLoader();
        g_gamma_loader = *base;
        g_base_load_glyph = base->FontBakedLoadGlyph;
        g_gamma_loader.Name = "FreeType + gamma";
        g_gamma_loader.FontBakedLoadGlyph = load_glyph_with_gamma;
        atlas->SetFontLoader(&g_gamma_loader);
    }
#endif

    bool g_lcd_text = false;
    // Coverage curves fitted to Chromium's ClearType: ink and colour fringes within ~3%.
    float g_text_gamma = 1.9f;
    float g_text_contrast = 0.f;

    void load_fonts(float dpi_scale)
    {
        ImGuiIO& io = ImGui::GetIO();
        // The text is Chromium's DirectWrite ClearType. Use the same rasteriser where it exists (drawn by
        // the sub-pixel renderer); elsewhere unhinted FreeType with a coverage gamma for the same stem weight.
        if (const ImFontLoader* native = imgit::gfx::native_font_loader(); native != nullptr && g_lcd_text)
        {
            imgit::gfx::set_native_coverage_curve(g_text_gamma, g_text_contrast);
            io.Fonts->SetFontLoader(native);
        }
        else
        {
#ifdef IMGUI_ENABLE_FREETYPE
            io.Fonts->FontLoaderFlags = ImGuiFreeTypeLoaderFlags_NoHinting;
            install_gamma_font_loader(io.Fonts, 3.0f);
#endif
        }
        imgit::ui::Fonts& fonts = imgit::ui::fonts();
        fonts.regular = load_font(io, "fonts/OpenSans-Regular.ttf", imgit::theme::FONT_SIZE_NORMAL);
        fonts.semibold = load_font(io, "fonts/OpenSans-Semibold.ttf", imgit::theme::FONT_SIZE_NORMAL);
        fonts.bold = load_font(io, "fonts/OpenSans-Bold.ttf", imgit::theme::FONT_SIZE_NORMAL);
        fonts.italic = load_font(io, "fonts/OpenSans-Italic.ttf", imgit::theme::FONT_SIZE_NORMAL);
        using imgit::platform::SystemFont;
        fonts.mono = load_system_font(io, SystemFont::MONOSPACE, imgit::theme::FONT_SIZE_SMALL);
        fonts.system = load_system_font(io, SystemFont::UI, imgit::theme::FONT_SIZE_SMALL);
        fonts.arial = load_system_font(io, SystemFont::ARIAL, imgit::theme::FONT_SIZE_SMALL);
        if (fonts.regular == nullptr)
        {
            fonts.regular = io.Fonts->AddFontDefault();
        }
        if (fonts.semibold == nullptr)
        {
            fonts.semibold = fonts.regular;
        }
        if (fonts.bold == nullptr)
        {
            fonts.bold = fonts.semibold;
        }
        if (fonts.mono == nullptr)
        {
            fonts.mono = fonts.regular;
        }
        if (fonts.system == nullptr)
        {
            fonts.system = fonts.regular;
        }
        if (fonts.arial == nullptr)
        {
            fonts.arial = fonts.regular;
        }
        io.FontDefault = fonts.regular;
        ImGuiStyle& style = ImGui::GetStyle();
        style.FontSizeBase = imgit::theme::FONT_SIZE_NORMAL * imgit::ui::font_em_scale(fonts.regular);
        style.FontScaleDpi = dpi_scale;
    }

    // Shows the cursor the UI asked for this frame (a drag's, a resize handle's); ImGui's own cursor resumes when
    // it is no longer asked for.
    void apply_drag_cursor(GLFWwindow* window, ImGuiIO& io)
    {
        static bool showing = false;
        const imgit::ui::DragCursor wanted = imgit::ui::take_drag_cursor();
        if (wanted == imgit::ui::DragCursor::NONE)
        {
            if (showing)
            {
                imgit::platform::reset_cursor(window);
                io.ConfigFlags &= ~ImGuiConfigFlags_NoMouseCursorChange;
                showing = false;
            }
            return;
        }
        using imgit::platform::CursorShape;
        const CursorShape shape = wanted == imgit::ui::DragCursor::MOVE         ? CursorShape::MOVE
                                  : wanted == imgit::ui::DragCursor::NO_DROP    ? CursorShape::NO_DROP
                                  : wanted == imgit::ui::DragCursor::COL_RESIZE ? CursorShape::COL_RESIZE
                                                                                : CursorShape::ROW_RESIZE;
        imgit::platform::set_cursor(window, shape);
        io.ConfigFlags |= ImGuiConfigFlags_NoMouseCursorChange;
        showing = true;
    }
} // namespace

int main(int argc, char** argv)
{
    // git runs ImGit as its GIT_ASKPASS: answer the prompt and exit before any window exists.
    if (argc == 2 && imgit::git::cli::is_askpass_invocation())
    {
        return imgit::git::cli::askpass_main(argv[1]);
    }
    glfwSetErrorCallback(
            [](int error, const char* description)
            {
                std::fprintf(stderr, "GLFW error %d: %s\n", error, description);
            });
    if (!glfwInit())
    {
        return 1;
    }
    glfwWindowHint(GLFW_CONTEXT_VERSION_MAJOR, 3);
    glfwWindowHint(GLFW_CONTEXT_VERSION_MINOR, 3);
    glfwWindowHint(GLFW_OPENGL_PROFILE, GLFW_OPENGL_CORE_PROFILE);
    glfwWindowHint(GLFW_SCALE_TO_MONITOR, GLFW_TRUE);
    const DevOptions options = parse_args(argc, argv);
    GLFWwindow* window = glfwCreateWindow(options.width, options.height, "ImGit Desktop", nullptr, nullptr);
    if (window == nullptr)
    {
        glfwTerminate();
        return 1;
    }
    glfwMakeContextCurrent(window);
    imgit::platform::init_window(window);
    glfwSwapInterval(1);

    float dpi_scale = 1.f;
    glfwGetWindowContentScale(window, &dpi_scale, nullptr);
    imgit::ui::set_dpi_scale(dpi_scale);
    // The minimum window (950x675 outside its frame) leaves a 934x610 page; ImGit's client area also holds
    // the 26px menu bar.
    glfwSetWindowSizeLimits(window, static_cast<int>(934.f * dpi_scale), static_cast<int>(636.f * dpi_scale),
                            GLFW_DONT_CARE, GLFW_DONT_CARE);

    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImGuiIO& io = ImGui::GetIO();
    io.IniFilename = nullptr;
    io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard;
    imgit::theme::apply_imgui_style();
    ImGui::GetStyle().ScaleAllSizes(dpi_scale);
    g_lcd_text = imgit::gfx::lcd_renderer_init(
            [](const char* name) -> void*
            {
                return reinterpret_cast<void*>(glfwGetProcAddress(name));
            });
    if (const char* gamma = std::getenv("IMGIT_TEXT_GAMMA"); gamma != nullptr)
    {
        g_text_gamma = static_cast<float>(std::atof(gamma));
    }
    if (const char* gray = std::getenv("IMGIT_GRAY_GAMMA"); gray != nullptr)
    {
        imgit::gfx::set_grayscale_gamma(static_cast<float>(std::atof(gray)));
    }
    if (const char* contrast = std::getenv("IMGIT_TEXT_CONTRAST"); contrast != nullptr)
    {
        g_text_contrast = static_cast<float>(std::atof(contrast));
    }
    load_fonts(dpi_scale);

    // A scripted run takes no real input: the backend's GLFW callbacks would feed the user's mouse and keyboard
    // into the script's frames.
    ImGui_ImplGlfw_InitForOpenGL(window, options.script.empty());
    ImGui_ImplOpenGL3_Init("#version 330");
    imgit::ui::set_texture_backend({create_gl_texture, destroy_gl_texture});

    imgit::git::Library git_library;
    {
        imgit::app::Application app;
        app.base_dpi_scale = dpi_scale;
        app.quit = [window]
        {
            glfwSetWindowShouldClose(window, GLFW_TRUE);
        };
        app.relaunch = [&app, window]
        {
            // Written now, not at exit: the new instance reads these files as soon as it starts.
            app.save_state();
            imgit::platform::spawn_detached({imgit::platform::executable_path()});
            glfwSetWindowShouldClose(window, GLFW_TRUE);
        };
        app.toggle_full_screen = [window]
        {
            static int saved[4] = {};
            if (glfwGetWindowMonitor(window) != nullptr)
            {
                glfwSetWindowMonitor(window, nullptr, saved[0], saved[1], saved[2], saved[3], 0);
                return;
            }
            glfwGetWindowPos(window, &saved[0], &saved[1]);
            glfwGetWindowSize(window, &saved[2], &saved[3]);
            GLFWmonitor* monitor = glfwGetPrimaryMonitor();
            const GLFWvidmode* mode = monitor != nullptr ? glfwGetVideoMode(monitor) : nullptr;
            if (mode == nullptr)
            {
                return; // no monitor to go full screen on (disconnected meanwhile)
            }
            glfwSetWindowMonitor(window, monitor, 0, 0, mode->width, mode->height, mode->refreshRate);
        };
        imgit::ui::set_avatar_cache_directory(app.app_path("gravatars"));
        app.apply_settings(); // applies the saved zoom on top of the monitor scale
        if (!options.repo.empty())
        {
            app.open_repository(options.repo);
        }
        else if (app.settings.remember_tabs)
        {
            app.restore_tabs();
        }
        if (options.terminal)
        {
            app.show_terminal_panel = true;
        }
        if (app.tabs.empty() && !app.recent_repos.empty() && options.repo.empty())
        {
            app.open_repository(app.recent_repos.front());
        }

        const imgit::theme::Palette& palette = imgit::theme::palette();
        std::vector<ScriptStep> script;
        std::size_t script_index = 0;
        int script_wait = 0;
        std::string pending_shot;
        const bool scripted = !options.script.empty();
        if (scripted)
        {
            script = load_script(options.script);
            script_wait = 5; // let the first frames settle
        }
        int frame = 0;
        int screenshot_wait = 0;
        while (!glfwWindowShouldClose(window))
        {
            // Idle, the loop waits for input (or 0.1s); a running transition keeps it drawing every frame so fades
            // like the tab hover move smoothly.
            if (options.screenshot.empty() && !scripted && !imgit::ui::animating())
            {
                glfwWaitEventsTimeout(0.1);
            }
            else
            {
                glfwPollEvents();
            }
            ImGui_ImplOpenGL3_NewFrame();
            if (scripted)
            {
                // Scripted input replaces the GLFW platform input so the real mouse cannot interfere.
                int w = 0, h = 0, fw = 0, fh = 0;
                glfwGetWindowSize(window, &w, &h);
                glfwGetFramebufferSize(window, &fw, &fh);
                io.DisplaySize = ImVec2(static_cast<float>(w), static_cast<float>(h));
                if (w > 0 && h > 0)
                {
                    io.DisplayFramebufferScale = ImVec2(static_cast<float>(fw) / w, static_cast<float>(fh) / h);
                }
                io.DeltaTime = 1.f / 60.f;
                if (script_wait > 0)
                {
                    --script_wait;
                }
                else if (script_index < script.size())
                {
                    const ScriptStep& step = script[script_index++];
                    switch (step.kind)
                    {
                    case ScriptStep::Kind::MOVE:
                        io.AddMousePosEvent(step.x, step.y);
                        break;
                    case ScriptStep::Kind::BUTTON:
                        io.AddMouseButtonEvent(step.button, step.down);
                        break;
                    case ScriptStep::Kind::KEY:
                        io.AddKeyEvent(step.key, step.down);
                        break;
                    case ScriptStep::Kind::CHARS:
                        io.AddInputCharactersUTF8(step.text.c_str());
                        break;
                    case ScriptStep::Kind::WAIT:
                        script_wait = step.frames;
                        break;
                    case ScriptStep::Kind::SHOT:
                        pending_shot = step.text;
                        break;
                    case ScriptStep::Kind::WHEEL:
                        io.AddMouseWheelEvent(step.x, step.y);
                        break;
                    case ScriptStep::Kind::ROW:
                        if (imgit::app::RepoState* state = app.active();
                            state != nullptr && step.frames >= 0
                            && step.frames < static_cast<int>(state->history.commits.size()))
                        {
                            state->select_only(state->history.commits[static_cast<std::size_t>(step.frames)].sha);
                            state->scroll_to_row = static_cast<std::size_t>(step.frames);
                        }
                        break;
                    }
                }
                else
                {
                    glfwSetWindowShouldClose(window, GLFW_TRUE);
                }
            }
            else
            {
                ImGui_ImplGlfw_NewFrame();
            }
            ImGui::NewFrame();

            if (!options.screenshot.empty() && frame == 2)
            {
                if (imgit::app::RepoState* state = app.active(); state != nullptr)
                {
                    if (options.select_row >= 0 && options.select_row < static_cast<int>(state->history.commits.size()))
                    {
                        state->select_only(state->history.commits[static_cast<std::size_t>(options.select_row)].sha);
                        state->scroll_to_row = static_cast<std::size_t>(options.select_row);
                    }
                    if (!options.open_file.empty())
                    {
                        const imgit::git::Commit* commit = state->single_selected();
                        if (commit != nullptr && commit->type != imgit::git::NodeType::WORK_DIR)
                        {
                            state->open_diff({imgit::app::DiffTarget::Source::COMMIT, commit->sha, options.open_file,
                                              imgit::git::FileStatus::MODIFIED});
                        }
                        else
                        {
                            state->open_diff({imgit::app::DiffTarget::Source::UNSTAGED,
                                              {},
                                              options.open_file,
                                              imgit::git::FileStatus::MODIFIED});
                        }
                    }
                }
            }

            app.draw_frame();
            apply_drag_cursor(window, io);

            ImGui::Render();
            int width = 0, height = 0;
            glfwGetFramebufferSize(window, &width, &height);
            glViewport(0, 0, width, height);
            glClearColor(palette.app_bg0.x, palette.app_bg0.y, palette.app_bg0.z, 1.f);
            glClear(GL_COLOR_BUFFER_BIT);
            sharpen_font_atlas(ImGui::GetDrawData());
            if (g_lcd_text)
            {
                imgit::gfx::lcd_renderer_render(ImGui::GetDrawData(), io.Fonts->TexData->GetTexID());
            }
            else
            {
                ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
            }
            // Screenshots wait (up to ~5 s) for avatar downloads so the capture shows the real images.
            if (!options.screenshot.empty() && frame == 8 && imgit::ui::avatars_pending() && screenshot_wait < 300)
            {
                ++screenshot_wait;
                --frame;
                std::this_thread::sleep_for(std::chrono::milliseconds(16));
            }
            else if (!options.screenshot.empty() && frame == 8)
            {
                save_bmp(options.screenshot, width, height);
                glfwSetWindowShouldClose(window, GLFW_TRUE);
            }
            if (!pending_shot.empty())
            {
                save_bmp(std::filesystem::path(options.shots_dir) / (pending_shot + ".bmp"), width, height);
                pending_shot.clear();
            }
            glfwSwapBuffers(window);
            ++frame;
        }
    }

    imgit::ui::shutdown_avatars();
    imgit::gfx::lcd_renderer_shutdown();
    ImGui_ImplOpenGL3_Shutdown();
    ImGui_ImplGlfw_Shutdown();
    ImGui::DestroyContext();
    glfwDestroyWindow(window);
    glfwTerminate();
    return 0;
}
