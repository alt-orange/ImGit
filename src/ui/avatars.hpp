#pragma once

#include <imgui.h>
#include <string>

// Author avatars from https://www.gravatar.com/avatar/<md5(lowercase email)>?d=404&s=N.
// Images download on a background thread into a disk cache next to the settings; until one is ready (or when the
// download fails) callers draw the initials fallback.
namespace imgit::ui
{
    void set_avatar_cache_directory(const std::string& directory);
    void set_avatars_enabled(bool enabled) noexcept;
    // Texture for `email` at `size` device pixels, or ImTextureID_Invalid while it is not available.
    [[nodiscard]] ImTextureID avatar_texture(const std::string& email, int size);
    // True while downloads are queued or running (the headless screenshot mode waits for them).
    [[nodiscard]] bool avatars_pending() noexcept;
    void shutdown_avatars();
} // namespace imgit::ui
