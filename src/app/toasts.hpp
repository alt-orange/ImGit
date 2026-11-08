#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace imgit::app
{
    enum class ToastKind
    {
        INFO,
        SUCCESS,
        ERROR,
    };

    // The window corner the toast cards stack in (Preferences → UI Customization → Notification Location).
    enum class ToastCorner
    {
        TOP_LEFT,
        TOP_RIGHT,
        BOTTOM_LEFT,
        BOTTOM_RIGHT,
    };
    // "Top Right" -> TOP_RIGHT; BOTTOM_LEFT for anything else.
    [[nodiscard]] ToastCorner toast_corner(std::string_view name);

    // Notifications in a corner of the window. Every toast is also kept for the notifications popover.
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
        void draw(ToastCorner corner = ToastCorner::BOTTOM_LEFT); // call once per frame, after the main window
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
        std::vector<Entry> m_history; // newest first
    };
} // namespace imgit::app
