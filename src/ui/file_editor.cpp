#include "app/application.hpp"
#include "app/theme.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <imgui.h>
#include <iterator>
#include <string>

// The file view's editor: the working directory's copy of a file as plain text in the code font, with Save and
// Cancel in place of the toolbar. Line endings are edited as "\n" and written back as the file had them.
namespace imgit::ui
{
    namespace
    {
        constexpr std::uintmax_t MAX_EDIT_SIZE = 8u << 20; // larger files open in an external editor instead

        std::filesystem::path workdir_file(const app::RepoState& state, const std::string& path)
        {
            return std::filesystem::path(state.repo.workdir()) / path;
        }

        float css_text_y(float size, float box_top)
        {
            const float ascent = size * 2189.f / 2048.f * dpi_scale();
            return std::round(box_top + std::round(ascent)) - std::ceil(ascent);
        }

        // A .btn-xs (20px) in `tone` with an optional 12px icon before the label, at `pos`; `width` gets its width.
        bool editor_button(const char* id, const char* label, const char* icon, ImVec2 pos, ImVec4 tone, bool enabled,
                           float* width)
        {
            const theme::Palette& p = theme::palette();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const float icon_w = icon != nullptr ? px(17.f) : 0.f;
            const float w = text_size(fonts().regular, theme::FONT_SIZE_ROW, label).x + icon_w + px(12.f);
            const ImVec2 max(pos.x + w, pos.y + px(20.f));
            ImGui::SetCursorScreenPos(pos);
            const bool clicked = ImGui::InvisibleButton(id, ImVec2(w, px(20.f))) && enabled;
            const bool hovered = ImGui::IsItemHovered() && enabled;
            const float alpha = enabled ? 1.f : 0.5f;
            draw->AddRectFilled(pos, max, theme::u32(theme::with_alpha(tone, (hovered ? 0.3f : 0.2f) * alpha)),
                                px(2.f));
            draw->AddRect(pos, max, theme::u32(theme::with_alpha(tone, alpha)), px(2.f), 0, 1.f);
            const ImU32 color = theme::u32(theme::with_alpha(p.text_normal, p.text_normal.w * alpha));
            if (icon != nullptr)
            {
                draw_fa_icon(draw, icon, ImVec2(pos.x + px(12.f), pos.y + px(10.f)), px(12.f), color);
            }
            draw_text(draw, fonts().regular, theme::FONT_SIZE_ROW,
                      ImVec2(pos.x + px(6.f) + icon_w, css_text_y(12.f, pos.y + px(1.f))), color, label);
            *width = w;
            return clicked;
        }

        // Writes the editor's text; true when it reached the disk.
        bool save(app::Application& app, app::RepoState& state)
        {
            app::RepoState::FileEdit& edit = *state.file_edit;
            std::string text = edit.text;
            if (edit.crlf)
            {
                std::string converted;
                converted.reserve(text.size() + text.size() / 32);
                for (const char c : text)
                {
                    if (c == '\n')
                    {
                        converted += '\r';
                    }
                    converted += c;
                }
                text = std::move(converted);
            }
            std::ofstream out(workdir_file(state, edit.path), std::ios::binary | std::ios::trunc);
            out.write(text.data(), static_cast<std::streamsize>(text.size()));
            out.close();
            if (!out)
            {
                app.toasts.push(app::ToastKind::ERROR, "Could not save " + edit.path, "The file could not be written.");
                return false;
            }
            edit.saved = edit.text;
            return true;
        }

        // Back to the file's diff in the working directory, which now holds the saved text.
        void close_editor(app::RepoState& state)
        {
            const std::string path = state.file_edit->path;
            state.file_edit.reset();
            state.refresh_status();
            git::FileStatus status = git::FileStatus::MODIFIED;
            for (const git::FileChange& change : state.status.unstaged)
            {
                if (change.path == path)
                {
                    status = change.status;
                }
            }
            state.open_diff({app::DiffTarget::Source::UNSTAGED, {}, path, status});
        }
    } // namespace

    bool open_file_editor(app::Application& app, app::RepoState& state, const std::string& path)
    {
        const std::filesystem::path file = workdir_file(state, path);
        std::error_code error;
        if (!std::filesystem::is_regular_file(file, error))
        {
            app.toasts.push(app::ToastKind::ERROR, "Could not edit " + path,
                            "The file is not in the working directory.");
            return false;
        }
        if (std::filesystem::file_size(file, error) > MAX_EDIT_SIZE)
        {
            app.toasts.push(app::ToastKind::INFO, "Edit file", path + " is too large for the built-in editor.");
            return false;
        }
        std::ifstream in(file, std::ios::binary);
        std::string bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
        if (!in.eof() && in.fail())
        {
            app.toasts.push(app::ToastKind::ERROR, "Could not edit " + path, "The file could not be read.");
            return false;
        }
        if (bytes.find('\0') != std::string::npos)
        {
            app.toasts.push(app::ToastKind::INFO, "Edit file", path + " is a binary file.");
            return false;
        }
        app::RepoState::FileEdit edit;
        edit.path = path;
        edit.crlf = bytes.find("\r\n") != std::string::npos;
        edit.text.reserve(bytes.size());
        for (std::size_t i = 0; i < bytes.size(); ++i)
        {
            if (!(edit.crlf && bytes[i] == '\r' && i + 1 < bytes.size() && bytes[i + 1] == '\n'))
            {
                edit.text += bytes[i];
            }
        }
        edit.saved = edit.text;
        // The file view shows the file while it is edited: its working directory diff, unless one is open.
        if (!state.diff_target || state.diff_target->path != path)
        {
            git::FileStatus status = git::FileStatus::UNKNOWN;
            for (const git::FileChange& change : state.status.unstaged)
            {
                if (change.path == path)
                {
                    status = change.status;
                }
            }
            state.open_diff({app::DiffTarget::Source::UNSTAGED, {}, path, status});
        }
        state.file_edit = std::move(edit);
        return true;
    }

    void draw_file_editor(app::Application& app, app::RepoState& state, ImVec2 toolbar_pos, ImVec2 min, ImVec2 max)
    {
        const theme::Palette& p = theme::palette();
        app::RepoState::FileEdit& edit = *state.file_edit;
        const bool dirty = edit.text != edit.saved;
        ImDrawList* draw = ImGui::GetWindowDrawList();

        // Toolbar: Save, Cancel, then the file's path and whether it has unsaved changes.
        float w = 0.f;
        const bool save_shortcut = ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_S);
        bool save_clicked = editor_button("##editor_save", "Save", "far:save", toolbar_pos, p.green, dirty, &w);
        float x = toolbar_pos.x + w + px(10.f);
        const bool cancel_clicked = editor_button("##editor_cancel", dirty ? "Cancel" : "Close", nullptr,
                                                  ImVec2(x, toolbar_pos.y), p.accent, true, &w);
        x += w + px(15.f);
        const std::string label = "Editing " + edit.path + (dirty ? "  \xE2\x80\xA2 unsaved changes" : "");
        draw_text_ellipsis(draw, fonts().regular, theme::FONT_SIZE_ROW,
                           ImVec2(x, css_text_y(12.f, toolbar_pos.y + px(1.f))), max.x - x - px(10.f),
                           theme::u32(dirty ? p.orange : p.text_secondary), label.c_str());

        // The text, in the code font, 10px in.
        ImGui::SetCursorScreenPos(min);
        ImGui::PushStyleColor(ImGuiCol_FrameBg, p.app_bg0);
        ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(10.f), px(4.f)));
        ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
        push_font(fonts().mono, 12.f);
        if (edit.focus)
        {
            ImGui::SetKeyboardFocusHere();
            edit.focus = false;
        }
        input_multiline("##file_editor_text", edit.text, "", ImVec2(max.x - min.x, max.y - min.y));
        ImGui::PopFont();
        ImGui::PopStyleVar(2);
        ImGui::PopStyleColor();

        if ((save_clicked || save_shortcut) && dirty)
        {
            if (save(app, state))
            {
                app.toasts.push(app::ToastKind::SUCCESS, "Saved " + edit.path, "");
                close_editor(state);
            }
            return;
        }
        if (cancel_clicked)
        {
            if (!dirty)
            {
                close_editor(state);
                return;
            }
            app.dialog.open(app::Dialog::Kind::CONFIRM);
            app.dialog.title = "Discard Changes";
            app.dialog.message = "Discard your unsaved changes to " + edit.path + "?";
            app.dialog.confirm_label = "Discard";
            app.dialog.danger = true;
            app.dialog.on_confirm = [&state]
            {
                if (state.file_edit)
                {
                    close_editor(state);
                }
            };
        }
    }
} // namespace imgit::ui
