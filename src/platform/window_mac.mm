// The Cocoa touches on the main window that need Objective-C; window_posix.cpp does the rest on macOS.
#import <Cocoa/Cocoa.h>

namespace imgit::platform::detail
{
    void set_dark_appearance(void* ns_window)
    {
        if (ns_window == nullptr)
        {
            return;
        }
#if __has_feature(objc_arc)
        NSWindow* window = (__bridge NSWindow*) ns_window;
#else
        NSWindow* window = static_cast<NSWindow*>(ns_window);
#endif
        // ImGit draws a dark UI on every platform: the title bar matches it instead of following the system
        // appearance.
        window.appearance = [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];
    }
} // namespace imgit::platform::detail
