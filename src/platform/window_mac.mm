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

// A transparent view over GLFW's content view that owns the tabs bar's empty part, the way a title bar owns its
// surface: AppKit hit-tests it first, so a press there starts the window drag from the press event itself (and a
// double-click zooms or minimises), while everything outside `region` falls through to GLFW and ImGui. Being the
// real event's target is what keeps this working after the window was tiled or zoomed, where handing AppKit a
// later "current event" did nothing.
@interface ImGitDragView : NSView
@property (nonatomic) NSRect region; // top-left origin, points
@end

@implementation ImGitDragView

- (BOOL)isFlipped
{
    return YES;
}

- (NSView*)hitTest:(NSPoint)point
{
    const NSPoint local = [self convertPoint:point fromView:self.superview];
    return NSPointInRect(local, self.region) ? self : nil;
}

- (BOOL)acceptsFirstMouse:(NSEvent*)event
{
    return YES; // like a title bar, a press moves an inactive window too
}

- (BOOL)mouseDownCanMoveWindow
{
    return NO; // mouseDown: does it, with the double-click
}

- (void)mouseDown:(NSEvent*)event
{
    NSWindow* window = self.window;
    if (window == nil)
    {
        return;
    }
    if ((window.styleMask & NSWindowStyleMaskFullScreen) != 0)
    {
        return; // nothing to move or zoom
    }
    if (event.clickCount == 2)
    {
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
        return;
    }
    [window performWindowDragWithEvent:event];
}

@end

namespace
{
    NSWindow* g_window = nil;
    NSToolbar* g_toolbar = nil; // held while full screen takes it off the window
    ImGitDragView* g_drag_view = nil;

    bool full_screen()
    {
        return g_window != nil && (g_window.styleMask & NSWindowStyleMaskFullScreen) != 0;
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
        g_window = window;
        g_toolbar = [[NSToolbar alloc] initWithIdentifier:@"imgit.titlebar"];
        g_toolbar.showsBaselineSeparator = NO;
        if (@available(macOS 11.0, *))
        {
            window.toolbarStyle = NSWindowToolbarStyleUnifiedCompact;
        }
        window.toolbar = g_toolbar;
        // In full screen a toolbar stays as an opaque band over the top of the content (the tabs bar); the
        // traffic lights are hidden there anyway, so the toolbar goes while it lasts.
        NSNotificationCenter* center = [NSNotificationCenter defaultCenter];
        [center addObserverForName:NSWindowWillEnterFullScreenNotification
                            object:window
                             queue:nil
                        usingBlock:^(NSNotification*) {
                          window.toolbar = nil;
                        }];
        [center addObserverForName:NSWindowDidExitFullScreenNotification
                            object:window
                             queue:nil
                        usingBlock:^(NSNotification*) {
                          window.toolbar = g_toolbar;
                        }];
        // The drag view spans the content view and follows its size; its region starts empty.
        NSView* content = window.contentView;
        if (content != nil && g_drag_view == nil)
        {
            g_drag_view = [[ImGitDragView alloc] initWithFrame:content.bounds];
            g_drag_view.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
            g_drag_view.region = NSZeroRect;
            [content addSubview:g_drag_view];
        }
    }

    float title_bar_inset()
    {
        // The lights are hidden in full screen (they only slide in with the menu bar).
        return full_screen() ? 0.f : 72.f;
    }

    void set_drag_region(float min_x, float min_y, float max_x, float max_y)
    {
        if (g_drag_view == nil)
        {
            return;
        }
        g_drag_view.region = max_x > min_x && max_y > min_y ? NSMakeRect(min_x, min_y, max_x - min_x, max_y - min_y)
                                                            : NSZeroRect;
    }
} // namespace imgit::platform::detail
