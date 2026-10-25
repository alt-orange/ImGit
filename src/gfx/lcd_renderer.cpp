#include "gfx/lcd_renderer.hpp"
#include <cstddef>
#include <cstdint>
// The platform's GL 1.1 header (GL/gl.h, OpenGL/gl.h on macOS) with the Windows macros it needs.
#include <GLFW/glfw3.h>

namespace imgit::gfx
{
    namespace
    {
        // The GL 2.0+ subset this renderer uses (opengl32 only exports GL 1.1).
        using GLchar = char;
        using GLsizeiptr = std::ptrdiff_t;
        constexpr GLenum ARRAY_BUFFER = 0x8892;
        constexpr GLenum ELEMENT_ARRAY_BUFFER = 0x8893;
        constexpr GLenum STREAM_DRAW = 0x88E0;
        constexpr GLenum FRAGMENT_SHADER = 0x8B30;
        constexpr GLenum VERTEX_SHADER = 0x8B31;
        constexpr GLenum COMPILE_STATUS = 0x8B81;
        constexpr GLenum LINK_STATUS = 0x8B82;
        constexpr GLenum TEXTURE0 = 0x84C0;
        constexpr GLenum FUNC_ADD = 0x8006;
        constexpr GLenum ONE_MINUS_SRC1_COLOR = 0x88FA;

// GL entry points use the system calling convention (stdcall on 32-bit Windows; the same as cdecl on x64).
#if defined(_WIN32)
#define IMGIT_GL_API __stdcall
#else
#define IMGIT_GL_API
#endif
        struct Gl
        {
            GLuint(IMGIT_GL_API* CreateShader)(GLenum);
            void(IMGIT_GL_API* ShaderSource)(GLuint, GLsizei, const GLchar* const*, const GLint*);
            void(IMGIT_GL_API* CompileShader)(GLuint);
            void(IMGIT_GL_API* GetShaderiv)(GLuint, GLenum, GLint*);
            GLuint(IMGIT_GL_API* CreateProgram)();
            void(IMGIT_GL_API* AttachShader)(GLuint, GLuint);
            void(IMGIT_GL_API* LinkProgram)(GLuint);
            void(IMGIT_GL_API* GetProgramiv)(GLuint, GLenum, GLint*);
            void(IMGIT_GL_API* UseProgram)(GLuint);
            GLint(IMGIT_GL_API* GetUniformLocation)(GLuint, const GLchar*);
            GLint(IMGIT_GL_API* GetAttribLocation)(GLuint, const GLchar*);
            void(IMGIT_GL_API* Uniform1i)(GLint, GLint);
            void(IMGIT_GL_API* Uniform1f)(GLint, GLfloat);
            void(IMGIT_GL_API* UniformMatrix4fv)(GLint, GLsizei, GLboolean, const GLfloat*);
            void(IMGIT_GL_API* GenVertexArrays)(GLsizei, GLuint*);
            void(IMGIT_GL_API* BindVertexArray)(GLuint);
            void(IMGIT_GL_API* DeleteVertexArrays)(GLsizei, const GLuint*);
            void(IMGIT_GL_API* GenBuffers)(GLsizei, GLuint*);
            void(IMGIT_GL_API* BindBuffer)(GLenum, GLuint);
            void(IMGIT_GL_API* BufferData)(GLenum, GLsizeiptr, const void*, GLenum);
            void(IMGIT_GL_API* DeleteBuffers)(GLsizei, const GLuint*);
            void(IMGIT_GL_API* VertexAttribPointer)(GLuint, GLint, GLenum, GLboolean, GLsizei, const void*);
            void(IMGIT_GL_API* EnableVertexAttribArray)(GLuint);
            void(IMGIT_GL_API* ActiveTexture)(GLenum);
            void(IMGIT_GL_API* BlendEquation)(GLenum);
            void(IMGIT_GL_API* BlendFuncSeparate)(GLenum, GLenum, GLenum, GLenum);
            void(IMGIT_GL_API* DrawElementsBaseVertex)(GLenum, GLsizei, GLenum, const void*, GLint);
            void(IMGIT_GL_API* DeleteProgram)(GLuint);
            void(IMGIT_GL_API* DeleteShader)(GLuint);
        };

        Gl gl{};
        GLuint g_program = 0;
        GLuint g_vao = 0;
        GLuint g_vbo = 0;
        GLuint g_ebo = 0;
        GLint g_proj = -1;
        GLint g_texture = -1;
        GLint g_lcd = -1;
        GLint g_gray_gamma = -1;
        float g_gray_gamma_value = 1.38f;
        bool g_subpixel_atlas = false;
        bool g_ready = false;

        template<class Fn>
        bool load(GlProcLoader loader, Fn& fn, const char* name)
        {
            fn = reinterpret_cast<Fn>(loader(name));
            return fn != nullptr;
        }

        void text_mode_callback(const ImDrawList*, const ImDrawCmd*)
        {
            // Handled by lcd_renderer_render; nothing to do when another renderer runs the callback.
        }

        constexpr const char* VERTEX_SOURCE = R"(#version 330 core
layout (location = 0) in vec2 Position;
layout (location = 1) in vec2 UV;
layout (location = 2) in vec4 Color;
uniform mat4 ProjMtx;
out vec2 Frag_UV;
out vec4 Frag_Color;
void main()
{
    Frag_UV = UV;
    Frag_Color = Color;
    gl_Position = ProjMtx * vec4(Position.xy, 0, 1);
}
)";

        // Lcd = 2: the same coverage averaged to grey-scale.
        // Lcd = 1: the texture's RGB is per-channel coverage (times A). Dual-source blending applies
        // dst = colour * coverage + dst * (1 - coverage) per channel, one triangle after the other.
        constexpr const char* FRAGMENT_SOURCE = R"(#version 330 core
in vec2 Frag_UV;
in vec4 Frag_Color;
uniform sampler2D Texture;
uniform int Lcd;
uniform float GrayGamma;
layout (location = 0, index = 0) out vec4 Out_Color;
layout (location = 0, index = 1) out vec4 Out_Coverage;
// The atlas row is a 3x horizontal coverage signal (R, G, B = left, middle, right third of a pixel). Each output
// sub-pixel reads the source sub-pixel under it, so a glyph quad at a fractional x lands at 1/3 px precision
// instead of snapping to whole pixels.
vec3 subpixel_coverage()
{
    ivec2 size = textureSize(Texture, 0);
    float x = Frag_UV.x * float(size.x);
    int row = int(floor(Frag_UV.y * float(size.y)));
    vec3 result;
    for (int k = 0; k < 3; ++k)
    {
        int j = int(floor(3.0 * x - 1.5 + float(k) + 0.5));
        int texel = j / 3;
        int channel = j - texel * 3;
        vec4 t = texelFetch(Texture, ivec2(texel, row), 0);
        result[k] = (channel == 0 ? t.r : (channel == 1 ? t.g : t.b)) * t.a;
    }
    return result;
}

void main()
{
    vec3 coverage;
    vec3 color;
    if (Lcd == 1)
    {
        coverage = subpixel_coverage() * Frag_Color.a;
        color = Frag_Color.rgb;
    }
    else if (Lcd == 2)
    {
        vec3 c = subpixel_coverage();
        coverage = vec3(pow((c.r + c.g + c.b) / 3.0, 1.0 / GrayGamma) * Frag_Color.a);
        color = Frag_Color.rgb;
    }
    else
    {
        vec4 t = texture(Texture, Frag_UV.st);
        coverage = vec3(t.a * Frag_Color.a);
        color = t.rgb * Frag_Color.rgb;
    }
    Out_Color = vec4(color * coverage, max(coverage.r, max(coverage.g, coverage.b)));
    Out_Coverage = vec4(coverage, 1.0);
}
)";

        GLuint compile(GLenum type, const char* source)
        {
            const GLuint shader = gl.CreateShader(type);
            gl.ShaderSource(shader, 1, &source, nullptr);
            gl.CompileShader(shader);
            GLint ok = 0;
            gl.GetShaderiv(shader, COMPILE_STATUS, &ok);
            if (ok == 0)
            {
                gl.DeleteShader(shader);
                return 0;
            }
            return shader;
        }
    } // namespace

    bool lcd_renderer_init(GlProcLoader loader)
    {
        bool ok = true;
        ok &= load(loader, gl.CreateShader, "glCreateShader");
        ok &= load(loader, gl.ShaderSource, "glShaderSource");
        ok &= load(loader, gl.CompileShader, "glCompileShader");
        ok &= load(loader, gl.GetShaderiv, "glGetShaderiv");
        ok &= load(loader, gl.CreateProgram, "glCreateProgram");
        ok &= load(loader, gl.AttachShader, "glAttachShader");
        ok &= load(loader, gl.LinkProgram, "glLinkProgram");
        ok &= load(loader, gl.GetProgramiv, "glGetProgramiv");
        ok &= load(loader, gl.UseProgram, "glUseProgram");
        ok &= load(loader, gl.GetUniformLocation, "glGetUniformLocation");
        ok &= load(loader, gl.GetAttribLocation, "glGetAttribLocation");
        ok &= load(loader, gl.Uniform1i, "glUniform1i");
        ok &= load(loader, gl.Uniform1f, "glUniform1f");
        ok &= load(loader, gl.UniformMatrix4fv, "glUniformMatrix4fv");
        ok &= load(loader, gl.GenVertexArrays, "glGenVertexArrays");
        ok &= load(loader, gl.BindVertexArray, "glBindVertexArray");
        ok &= load(loader, gl.DeleteVertexArrays, "glDeleteVertexArrays");
        ok &= load(loader, gl.GenBuffers, "glGenBuffers");
        ok &= load(loader, gl.BindBuffer, "glBindBuffer");
        ok &= load(loader, gl.BufferData, "glBufferData");
        ok &= load(loader, gl.DeleteBuffers, "glDeleteBuffers");
        ok &= load(loader, gl.VertexAttribPointer, "glVertexAttribPointer");
        ok &= load(loader, gl.EnableVertexAttribArray, "glEnableVertexAttribArray");
        ok &= load(loader, gl.ActiveTexture, "glActiveTexture");
        ok &= load(loader, gl.BlendEquation, "glBlendEquation");
        ok &= load(loader, gl.BlendFuncSeparate, "glBlendFuncSeparate");
        ok &= load(loader, gl.DrawElementsBaseVertex, "glDrawElementsBaseVertex");
        ok &= load(loader, gl.DeleteProgram, "glDeleteProgram");
        ok &= load(loader, gl.DeleteShader, "glDeleteShader");
        if (!ok)
        {
            return false;
        }
        const GLuint vs = compile(VERTEX_SHADER, VERTEX_SOURCE);
        const GLuint fs = compile(FRAGMENT_SHADER, FRAGMENT_SOURCE);
        if (vs == 0 || fs == 0)
        {
            if (vs != 0)
            {
                gl.DeleteShader(vs);
            }
            if (fs != 0)
            {
                gl.DeleteShader(fs);
            }
            return false;
        }
        g_program = gl.CreateProgram();
        gl.AttachShader(g_program, vs);
        gl.AttachShader(g_program, fs);
        gl.LinkProgram(g_program);
        gl.DeleteShader(vs);
        gl.DeleteShader(fs);
        GLint linked = 0;
        gl.GetProgramiv(g_program, LINK_STATUS, &linked);
        if (linked == 0)
        {
            gl.DeleteProgram(g_program);
            g_program = 0;
            return false;
        }
        g_proj = gl.GetUniformLocation(g_program, "ProjMtx");
        g_texture = gl.GetUniformLocation(g_program, "Texture");
        g_lcd = gl.GetUniformLocation(g_program, "Lcd");
        g_gray_gamma = gl.GetUniformLocation(g_program, "GrayGamma");

        gl.GenVertexArrays(1, &g_vao);
        gl.GenBuffers(1, &g_vbo);
        gl.GenBuffers(1, &g_ebo);
        gl.BindVertexArray(g_vao);
        gl.BindBuffer(ARRAY_BUFFER, g_vbo);
        gl.BindBuffer(ELEMENT_ARRAY_BUFFER, g_ebo);
        gl.EnableVertexAttribArray(0);
        gl.EnableVertexAttribArray(1);
        gl.EnableVertexAttribArray(2);
        gl.VertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, sizeof(ImDrawVert),
                               reinterpret_cast<const void*>(offsetof(ImDrawVert, pos)));
        gl.VertexAttribPointer(1, 2, GL_FLOAT, GL_FALSE, sizeof(ImDrawVert),
                               reinterpret_cast<const void*>(offsetof(ImDrawVert, uv)));
        gl.VertexAttribPointer(2, 4, GL_UNSIGNED_BYTE, GL_TRUE, sizeof(ImDrawVert),
                               reinterpret_cast<const void*>(offsetof(ImDrawVert, col)));
        gl.BindVertexArray(0);
        g_ready = true;
        return true;
    }

    void lcd_renderer_render(ImDrawData* draw_data, ImTextureID lcd_texture)
    {
        const int fb_width = static_cast<int>(draw_data->DisplaySize.x * draw_data->FramebufferScale.x);
        const int fb_height = static_cast<int>(draw_data->DisplaySize.y * draw_data->FramebufferScale.y);
        if (!g_ready || fb_width <= 0 || fb_height <= 0)
        {
            return;
        }
        glEnable(GL_BLEND);
        gl.BlendEquation(FUNC_ADD);
        gl.BlendFuncSeparate(GL_ONE, ONE_MINUS_SRC1_COLOR, GL_ONE, GL_ONE_MINUS_SRC_ALPHA);
        glDisable(GL_CULL_FACE);
        glDisable(GL_DEPTH_TEST);
        glDisable(GL_STENCIL_TEST);
        glEnable(GL_SCISSOR_TEST);
        glViewport(0, 0, fb_width, fb_height);
        const float l = draw_data->DisplayPos.x;
        const float r = draw_data->DisplayPos.x + draw_data->DisplaySize.x;
        const float t = draw_data->DisplayPos.y;
        const float b = draw_data->DisplayPos.y + draw_data->DisplaySize.y;
        const float ortho[4][4] = {
                {2.f / (r - l), 0.f, 0.f, 0.f},
                {0.f, 2.f / (t - b), 0.f, 0.f},
                {0.f, 0.f, -1.f, 0.f},
                {(r + l) / (l - r), (t + b) / (b - t), 0.f, 1.f},
        };
        gl.UseProgram(g_program);
        gl.Uniform1i(g_texture, 0);
        gl.Uniform1f(g_gray_gamma, g_gray_gamma_value);
        gl.UniformMatrix4fv(g_proj, 1, GL_FALSE, &ortho[0][0]);
        gl.ActiveTexture(TEXTURE0);
        gl.BindVertexArray(g_vao);
        gl.BindBuffer(ARRAY_BUFFER, g_vbo);
        gl.BindBuffer(ELEMENT_ARRAY_BUFFER, g_ebo);

        const ImVec2 clip_off = draw_data->DisplayPos;
        const ImVec2 clip_scale = draw_data->FramebufferScale;
        for (const ImDrawList* list : draw_data->CmdLists)
        {
            bool grayscale = false;
            gl.BufferData(ARRAY_BUFFER, static_cast<GLsizeiptr>(list->VtxBuffer.Size) * sizeof(ImDrawVert),
                          list->VtxBuffer.Data, STREAM_DRAW);
            gl.BufferData(ELEMENT_ARRAY_BUFFER, static_cast<GLsizeiptr>(list->IdxBuffer.Size) * sizeof(ImDrawIdx),
                          list->IdxBuffer.Data, STREAM_DRAW);
            for (const ImDrawCmd& cmd : list->CmdBuffer)
            {
                if (cmd.UserCallback == text_mode_callback)
                {
                    grayscale = cmd.UserCallbackData != nullptr;
                    continue;
                }
                if (cmd.UserCallback != nullptr)
                {
                    if (cmd.UserCallback != ImDrawCallback_ResetRenderState)
                    {
                        cmd.UserCallback(list, &cmd);
                    }
                    continue;
                }
                const ImVec2 clip_min((cmd.ClipRect.x - clip_off.x) * clip_scale.x,
                                      (cmd.ClipRect.y - clip_off.y) * clip_scale.y);
                const ImVec2 clip_max((cmd.ClipRect.z - clip_off.x) * clip_scale.x,
                                      (cmd.ClipRect.w - clip_off.y) * clip_scale.y);
                if (clip_max.x <= clip_min.x || clip_max.y <= clip_min.y)
                {
                    continue;
                }
                glScissor(static_cast<int>(clip_min.x), static_cast<int>(static_cast<float>(fb_height) - clip_max.y),
                          static_cast<int>(clip_max.x - clip_min.x), static_cast<int>(clip_max.y - clip_min.y));
                const ImTextureID texture = cmd.GetTexID();
                glBindTexture(GL_TEXTURE_2D, static_cast<GLuint>(texture));
                const bool subpixel = g_subpixel_atlas && texture == lcd_texture;
                gl.Uniform1i(g_lcd, subpixel ? (grayscale ? 2 : 1) : 0);
                const void* indices =
                        reinterpret_cast<const void*>(static_cast<std::uintptr_t>(cmd.IdxOffset * sizeof(ImDrawIdx)));
                const GLenum index_type = sizeof(ImDrawIdx) == 2 ? GL_UNSIGNED_SHORT : GL_UNSIGNED_INT;
                gl.DrawElementsBaseVertex(GL_TRIANGLES, static_cast<GLsizei>(cmd.ElemCount), index_type, indices,
                                          static_cast<GLint>(cmd.VtxOffset));
            }
        }
        gl.BindVertexArray(0);
        gl.UseProgram(0);
        glDisable(GL_SCISSOR_TEST);
    }

    void set_grayscale_gamma(float gamma)
    {
        g_gray_gamma_value = gamma;
    }

    void set_subpixel_atlas(bool subpixel)
    {
        g_subpixel_atlas = subpixel;
    }

    void set_grayscale_text(ImDrawList* list, bool on)
    {
        list->AddCallback(text_mode_callback, on ? reinterpret_cast<void*>(1) : nullptr);
    }

    void lcd_renderer_shutdown()
    {
        if (!g_ready)
        {
            return;
        }
        gl.DeleteBuffers(1, &g_vbo);
        gl.DeleteBuffers(1, &g_ebo);
        gl.DeleteVertexArrays(1, &g_vao);
        gl.DeleteProgram(g_program);
        g_ready = false;
    }
} // namespace imgit::gfx
