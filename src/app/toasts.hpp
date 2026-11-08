#pragma once

#include <cstdint>
#include <imgui.h>
#include <optional>
#include <string>
#include <utility>
#include <vector>

namespace imgit::app
{
    enum class ToastKind
    {
        INFO,
        SUCCESS,
        ERROR,
    };

    // Bottom-left notifications. Every toast is also kept for the notifications popover.
    class Toasts
    {
    public:
        struct Entry
        {
            ToastKind kind;
            std::string title;
            std::string detail;
            std::int64_t time; // unix seconds
            bool read = false;
        };

        void push(ToastKind kind, std::string title, std::string detail = {});
        void draw(); // call once per frame, after the main window
        // Keeps this frame's toasts above a screen rectangle they would otherwise cover (buttons near the bottom).
        void keep_clear(ImVec2 min, ImVec2 max);
        [[nodiscard]] std::vector<Entry>& history() noexcept
        {
            return m_history;
        }
        [[nodiscard]] int unread() const noexcept;
        void mark_all_read() noexcept;

    private:
        struct Toast
        {
            ToastKind kind;
            std::string title;
            std::string detail;
            double shown_at; // ImGui time: the slide in starts here
            double expires_at;
            std::optional<double> hidden_at; // the slide out started (expired or dismissed)
        };
        std::vector<Toast> m_toasts;
        std::vector<std::pair<ImVec2, ImVec2>> m_keep_clear; // this frame's rectangles, cleared by draw()
        std::vector<Entry> m_history; // newest first
    };
} // namespace imgit::app
