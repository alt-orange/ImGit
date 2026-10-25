// The Cocoa touches on the main window that need Objective-C; window_posix.cpp does the rest on macOS.
#import <Cocoa/Cocoa.h>

namespace
{
    NSWindow* as_window(void* ns_window)
    {
#if __has_feature(objc_arc)
        return (__bridge NSWindow*) ns_window;
#else
        return static_cast<NSWindow*>(ns_window);
#endif
    }
} // namespace

namespace imgit::platform::detail
{
    void configure_window(void* ns_window)
    {
        NSWindow* window = as_window(ns_window);
        if (window == nil)
        {
            return;
        }
        // ImGit draws a dark UI on every platform: the frame matches it instead of following the system
        // appearance.
        window.appearance = [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];
        // The title bar is hidden and the content runs under it, so the tabs bar is the top of the window with the
        // traffic lights over its left end (Electron's "hiddenInset" look). An empty toolbar in the compact style
        // makes that band toolbar-high, which centres the lights on the tabs bar and insets them a little.
        window.titlebarAppearsTransparent = YES;
        window.titleVisibility = NSWindowTitleHidden;
        window.styleMask |= NSWindowStyleMaskFullSizeContentView;
        NSToolbar* toolbar = [[NSToolbar alloc] initWithIdentifier:@"imgit.titlebar"];
        toolbar.showsBaselineSeparator = NO;
        if (@available(macOS 11.0, *))
        {
            window.toolbarStyle = NSWindowToolbarStyleUnifiedCompact;
        }
        window.toolbar = toolbar;
#if !__has_feature(objc_arc)
        [toolbar release]; // the window holds it
#endif
    }

    void drag_window(void* ns_window)
    {
        NSWindow* window = as_window(ns_window);
        // The press that ImGui just reported is (or started) the current event; AppKit moves the window with the
        // mouse from it until the release.
        NSEvent* event = [NSApp currentEvent];
        if (window != nil && event != nil
            && (event.type == NSEventTypeLeftMouseDown || event.type == NSEventTypeLeftMouseDragged))
        {
            [window performWindowDragWithEvent:event];
        }
    }

    void title_bar_double_click(void* ns_window)
    {
        NSWindow* window = as_window(ns_window);
        if (window == nil)
        {
            return;
        }
        // System Settings > Desktop & Dock > "Double-click a window's title bar to": Fill / Zoom, Minimize, or
        // nothing.
        NSString* action = [[NSUserDefaults standardUserDefaults] stringForKey:@"AppleActionOnDoubleClick"];
        if ([action isEqualToString:@"Minimize"])
        {
            [window miniaturize:nil];
        }
        else if (![action isEqualToString:@"None"])
        {
            [window zoom:nil];
        }
    }
} // namespace imgit::platform::detail
