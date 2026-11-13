//! The application shell: the main window, its OpenGL context, Dear ImGui and the frame loop.
//!
//! The UI plugs in as a [`Frame`]: the shell calls [`Frame::draw`] once per frame with the `Ui` and a
//! [`ShellContext`] holding the fonts, the texture backend, the window and the requests a frame can make
//! (keep animating, show a cursor, quit, ...).

use std::num::NonZeroU32;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, anyhow};
use dear_imgui_rs::{ConfigFlags, Context, Ui, sys};
use dear_imgui_winit::{HiDpiMode, WinitPlatform};
use glow::HasContext as _;
use glutin::config::{Config as GlConfig, ConfigTemplateBuilder, GlConfig as _};
use glutin::context::{
    ContextApi, ContextAttributesBuilder, GlProfile, NotCurrentGlContext as _,
    PossiblyCurrentContext, Version,
};
use glutin::display::{GetGlDisplay as _, GlDisplay as _};
use glutin::surface::{
    GlSurface as _, Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface,
};
use glutin_winit::{DisplayBuilder, GlWindow as _};
use raw_window_handle::HasWindowHandle as _;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Icon, Theme, Window, WindowId};

use crate::dev::{
    DEV_COMMAND_FRAME, DevCommand, DevOptions, SCREENSHOT_FRAME, SCREENSHOT_MAX_WAIT_FRAMES,
    ScriptAction, ScriptPlayer, load_script, save_bmp,
};
use crate::fonts::{self, FontRequest, Fonts, TextSettings};
use crate::gfx::{Renderer, Textures};
use crate::window::{CursorShape, WindowChrome, layout_scale};
use crate::{assets, gfx, theme};

/// The minimum window (950x675 outside its frame) leaves a 934x610 page; ImGit's client area also holds the
/// 26px menu bar where the app draws it (not on macOS, whose menu bar is the system's).
const MIN_PAGE_SIZE: [f32; 2] = [934.0, 610.0];

/// Idle, the loop waits this long for input before it draws a frame anyway (clocks, background results).
const IDLE_TIMEOUT: Duration = Duration::from_millis(100);

/// What the shell is started with.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// The command line.
    pub options: DevOptions,
    /// The Editor and Terminal faces of the preferences.
    pub fonts: FontRequest,
    /// Coverage curves of the text.
    pub text: TextSettings,
}

/// The UI drawn into the main window.
pub trait Frame {
    /// Builds one frame of UI. Called between `ImGui::NewFrame()` and `ImGui::Render()`.
    fn draw(&mut self, ui: &Ui, shell: &mut ShellContext<'_>);

    /// Carries out a request of the dev options or of a script (`--select-row`, `--open-file`, a script's
    /// `row`), right before [`Self::draw`].
    fn dev_command(&mut self, command: DevCommand, shell: &mut ShellContext<'_>) {
        let _ = (command, shell);
    }

    /// Whether background work the picture depends on is still running (avatar downloads, the File History
    /// view's jobs): `--screenshot` waits for it, up to about 5 s.
    fn busy(&self) -> bool {
        false
    }
}

/// What a frame asked of the shell; applied once the frame is built.
#[derive(Debug, Default)]
struct FrameRequests {
    animating: bool,
    cursor: Option<CursorShape>,
    quit: bool,
    toggle_full_screen: bool,
    dark_frame: Option<bool>,
    drag_region: Option<([f32; 2], [f32; 2])>,
    background: Option<[f32; 4]>,
}

/// The shell as a frame sees it.
pub struct ShellContext<'a> {
    /// The loaded fonts.
    pub fonts: &'a Fonts,
    /// Creates and destroys the UI's own textures.
    pub textures: &'a mut dyn Textures,
    /// The main window: the parent of system dialogs (`raw_window_handle::HasWindowHandle`).
    pub window: &'a Window,
    /// The command line.
    pub options: &'a DevOptions,
    /// Frames drawn so far.
    pub frame_index: u32,
    title_bar_inset: f32,
    requests: &'a mut FrameRequests,
}

impl ShellContext<'_> {
    /// The same shell for a shorter while: what a per-frame `ui::Cx` holds by value, so that it needs a single
    /// lifetime.
    pub fn reborrow(&mut self) -> ShellContext<'_> {
        ShellContext {
            fonts: self.fonts,
            textures: &mut *self.textures,
            window: self.window,
            options: self.options,
            frame_index: self.frame_index,
            title_bar_inset: self.title_bar_inset,
            requests: &mut *self.requests,
        }
    }

    /// The factor logical (CSS) pixels are multiplied by; ImGui's style sizes already include it.
    #[must_use]
    pub fn dpi_scale(&self) -> f32 {
        self.fonts.dpi_scale
    }

    /// Keeps the loop drawing every frame instead of waiting for input, so a running transition (a fade, the
    /// tab hover) moves smoothly. Ask again each frame while it runs (`ui::animating()` in C++).
    pub fn request_animation(&mut self) {
        self.requests.animating = true;
    }

    /// Shows `shape` instead of ImGui's cursor for this frame (a drag's, a resize handle's); ImGui's own
    /// cursor resumes when it is no longer asked for.
    pub fn set_cursor(&mut self, shape: CursorShape) {
        self.requests.cursor = Some(shape);
    }

    /// Closes the window after this frame.
    pub fn quit(&mut self) {
        self.requests.quit = true;
    }

    /// Enters or leaves (borderless) full screen after this frame.
    pub fn toggle_full_screen(&mut self) {
        self.requests.toggle_full_screen = true;
    }

    /// Makes the window frame follow the theme: the dark or light caption.
    pub fn set_window_dark(&mut self, dark: bool) {
        self.requests.dark_frame = Some(dark);
    }

    /// The colour the framebuffer is cleared to from this frame on: the theme's `app_bg0`.
    pub fn set_background(&mut self, color: [f32; 4]) {
        self.requests.background = Some(color);
    }

    /// Logical pixels the tabs bar leaves free at its left for the window's own controls (macOS' traffic
    /// lights); 0 elsewhere.
    #[must_use]
    pub fn title_bar_inset(&self) -> f32 {
        self.title_bar_inset
    }

    /// Where the tabs bar is the title bar (macOS): the part of it, in window coordinates, that moves the
    /// window when dragged. Set every frame; a no-op elsewhere.
    pub fn set_title_bar_drag_region(&mut self, min: [f32; 2], max: [f32; 2]) {
        self.requests.drag_region = Some((min, max));
    }
}

/// Opens the main window and runs the frame loop until it closes. `create` builds the UI once the window,
/// ImGui, the renderer and the fonts exist.
pub fn run<F, C>(config: Config, create: C) -> anyhow::Result<()>
where
    F: Frame,
    C: FnOnce(&mut ShellContext<'_>) -> F,
{
    let event_loop = EventLoop::new().context("no event loop")?;
    let mut app = App {
        config,
        create: Some(create),
        state: None,
        error: None,
    };
    event_loop
        .run_app(&mut app)
        .context("the event loop failed")?;
    if let Some(state) = app.state.take() {
        state.shutdown();
    }
    app.error.map_or(Ok(()), Err)
}

struct App<F, C> {
    config: Config,
    create: Option<C>,
    state: Option<State<F>>,
    /// The error that ended the loop.
    error: Option<anyhow::Error>,
}

impl<F, C> App<F, C> {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error.get_or_insert(error);
        event_loop.exit();
    }
}

impl<F, C> ApplicationHandler for App<F, C>
where
    F: Frame,
    C: FnOnce(&mut ShellContext<'_>) -> F,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Some(create) = self.create.take() else {
            return; // the window exists already
        };
        match State::new(event_loop, &self.config, create) {
            Ok(state) => {
                state.window.request_redraw();
                self.state = Some(state);
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        // The idle timeout ran out: draw a frame.
        let StartCause::ResumeTimeReached { .. } = cause else {
            return;
        };
        let Some(state) = &mut self.state else {
            return;
        };
        if state.minimized() {
            // A minimised window may get no redraw requests from the system; the frame runs from here so the
            // application keeps polling (terminal output, git operations, background jobs).
            match state.redraw(&self.config.options) {
                Ok(true) => {}
                Ok(false) => event_loop.exit(),
                Err(error) => self.fail(event_loop, error),
            }
        } else {
            state.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = &mut self.state else {
            return;
        };
        if window_id != state.window.id() {
            return;
        }
        // A scripted run takes no real input: the user's mouse and keyboard would feed into the script's
        // frames.
        if !self.config.options.scripted()
            && let Err(error) =
                state
                    .platform
                    .handle_window_event(&mut state.imgui, window_id, &event)
        {
            return self.fail(event_loop, error.into());
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => match state.redraw(&self.config.options) {
                Ok(true) => {}
                Ok(false) => event_loop.exit(),
                Err(error) => self.fail(event_loop, error),
            },
            WindowEvent::Resized(size) => {
                state.resize_surface(size);
                state.window.request_redraw();
            }
            // Any other event (input, focus, moves) is followed by a frame, like a GLFW loop that draws
            // whenever `glfwWaitEvents` returns.
            _ => state.window.request_redraw(),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(state) = &self.state else {
            return;
        };
        // Idle, the loop waits for input (or 0.1 s); a running transition, a capture or a script keeps it
        // drawing every frame.
        let continuous = self.config.options.continuous() || state.animating;
        if continuous && !state.minimized() {
            event_loop.set_control_flow(ControlFlow::Poll);
            state.window.request_redraw();
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + IDLE_TIMEOUT));
        }
    }
}

/// Everything that lives as long as the window. Fields drop in declaration order: the UI and ImGui's
/// attachments before ImGui, the GL objects before the surface, the context and the window.
struct State<F> {
    frame: F,
    script: Option<ScriptPlayer>,
    /// The `shot` step to capture once this frame is drawn.
    pending_shot: Option<String>,
    /// Dev commands for the next frame.
    commands: Vec<DevCommand>,
    frame_index: u32,
    screenshot_wait: u32,
    /// Whether the last frame asked for another one right away.
    animating: bool,
    /// `io.DisplaySize` of the last frame drawn with a visible window: the size the UI is laid out for while
    /// the window is minimised (its own size is 0x0).
    display_size: [f32; 2],
    /// Whether the window is in (borderless) full screen; winit restores its rectangle on the way out.
    full_screen: bool,
    /// What the framebuffer is cleared to: the theme's `app_bg0`.
    background: [f32; 4],
    fonts: Fonts,
    renderer: Renderer,
    platform: WinitPlatform,
    imgui: Context,
    chrome: WindowChrome,
    gl: Rc<glow::Context>,
    surface: Surface<WindowSurface>,
    gl_context: PossiblyCurrentContext,
    window: Arc<Window>,
}

impl<F: Frame> State<F> {
    fn new<C>(event_loop: &ActiveEventLoop, config: &Config, create: C) -> anyhow::Result<Self>
    where
        C: FnOnce(&mut ShellContext<'_>) -> F,
    {
        let options = &config.options;
        let (window, gl_config) = create_window(event_loop, options)?;
        let window = Arc::new(window);
        let (gl_context, surface, gl) = create_gl(&window, &gl_config)?;
        let chrome = WindowChrome::new(Arc::clone(&window));

        let dpi_scale = layout_scale(&window);
        let mut imgui = Context::create();
        imgui
            .set_ini_filename(None::<String>)
            .context("disabling imgui.ini")?;
        let io = imgui.io_mut();
        io.set_config_flags(io.config_flags() | ConfigFlags::NAV_ENABLE_KEYBOARD);
        // Dear ImGui 1.93 moved strokes inside closed shapes and anti-aliases line ends; the UI's geometry is
        // tuned to the 1.92 rendering of the C++ app, which this flag keeps for every draw list.
        // SAFETY: the context was just created on this thread and no frame is open; a plain flag is written.
        unsafe {
            (*sys::igGetIO_Nil()).ConfigDebugDrawListDefaultsToStrokeLegacy = true;
        }
        // The default theme until the UI applies the profile's (`UiState::begin_frame`).
        let palette = theme::Palette::default();
        theme::apply_imgui_style(&palette, dpi_scale);

        // Window coordinates are physical pixels on Windows and X11 (the layout itself is scaled by
        // `dpi_scale`), points on macOS, where the framebuffer alone is scaled.
        let hidpi_mode = if cfg!(target_os = "macos") {
            HiDpiMode::Default
        } else {
            HiDpiMode::Locked(1.0)
        };
        let mut platform = WinitPlatform::new(&mut imgui)?;
        platform.attach_window(Arc::clone(&window), hidpi_mode, &mut imgui)?;
        let (_, framebuffer_scale) = display_metrics(&window);
        let density = framebuffer_scale[0];

        // IMGIT_RENDERER=glow forces the stock renderer (and with it grey-scale text), for comparisons.
        let prefer_lcd =
            std::env::var_os("IMGIT_RENDERER").is_none_or(|renderer| renderer != "glow");
        let mut renderer = Renderer::new(Rc::clone(&gl), &mut imgui, prefer_lcd)?;
        let fonts = fonts::load_fonts(
            &mut imgui,
            &mut renderer,
            &config.fonts,
            config.text,
            dpi_scale,
            density,
        );

        let script = match &options.script {
            Some(path) => {
                Some(ScriptPlayer::new(load_script(path).with_context(|| {
                    format!("reading the script {}", path.display())
                })?))
            }
            None => None,
        };

        let mut requests = FrameRequests::default();
        let frame = create(&mut ShellContext {
            fonts: &fonts,
            textures: &mut renderer,
            window: &window,
            options,
            frame_index: 0,
            title_bar_inset: chrome.title_bar_inset(),
            requests: &mut requests,
        });
        let mut state = Self {
            frame,
            script,
            pending_shot: None,
            commands: Vec::new(),
            frame_index: 0,
            screenshot_wait: 0,
            animating: false,
            display_size: display_metrics(&window).0,
            full_screen: false,
            background: palette.app_bg0.into(),
            fonts,
            renderer,
            platform,
            imgui,
            chrome,
            gl,
            surface,
            gl_context,
            window,
        };
        state.apply_window_requests(&requests);
        Ok(state)
    }

    fn minimized(&self) -> bool {
        let size = self.window.inner_size();
        size.width == 0 || size.height == 0
    }

    fn resize_surface(&self, size: PhysicalSize<u32>) {
        if let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        {
            self.surface.resize(&self.gl_context, width, height);
        }
    }

    /// The window-level requests of a frame: the frame's shade, full screen, the drag region.
    fn apply_window_requests(&mut self, requests: &FrameRequests) {
        if let Some(dark) = requests.dark_frame {
            self.chrome.set_dark(dark);
        }
        if let Some(background) = requests.background {
            self.background = background;
        }
        if let Some((min, max)) = requests.drag_region {
            self.chrome.set_title_bar_drag_region(min, max);
        }
        if requests.toggle_full_screen {
            self.full_screen = !self.full_screen;
            self.window
                .set_fullscreen(self.full_screen.then_some(Fullscreen::Borderless(None)));
        }
    }

    /// Runs one frame. `Ok(false)` when the window should close. A minimised window (0x0, no framebuffer)
    /// still runs the UI's frame, laid out for the size it had, so that the application keeps polling; the
    /// result is rendered but neither cleared, captured nor presented.
    fn redraw(&mut self, options: &DevOptions) -> anyhow::Result<bool> {
        let size = self.window.inner_size();
        let minimized = self.minimized();
        let mut keep_running = true;

        if let Some(script) = &mut self.script {
            // Scripted input replaces the platform input so the real mouse cannot interfere.
            let (display_size, framebuffer_scale) = display_metrics(&self.window);
            let display_size = if minimized {
                self.display_size
            } else {
                display_size
            };
            let io = self.imgui.io_mut();
            io.set_display_size(display_size);
            io.set_display_framebuffer_scale(framebuffer_scale);
            io.set_delta_time(1.0 / 60.0);
            match script.advance(io) {
                ScriptAction::None => {}
                ScriptAction::Shot(name) => self.pending_shot = Some(name),
                ScriptAction::Command(command) => self.commands.push(command),
                ScriptAction::Finished => keep_running = false,
            }
        } else {
            self.platform.prepare_frame(&mut self.imgui, &self.window)?;
            if minimized {
                // The platform backend took the window's 0x0 size.
                self.imgui.io_mut().set_display_size(self.display_size);
            }
        }
        if !minimized {
            self.display_size = display_metrics(&self.window).0;
        }
        if options.screenshot.is_some() && self.frame_index == DEV_COMMAND_FRAME {
            self.commands
                .extend(options.select_row.map(DevCommand::SelectRow));
            self.commands
                .extend(options.open_file.clone().map(|path| DevCommand::OpenFile {
                    path,
                    file_history: options.file_history,
                    blame: options.blame,
                }));
        }

        let mut requests = FrameRequests::default();
        {
            let ui = self.imgui.frame();
            fonts::sync_rasterizer_density(ui);
            let mut shell = ShellContext {
                fonts: &self.fonts,
                textures: &mut self.renderer,
                window: &self.window,
                options,
                frame_index: self.frame_index,
                title_bar_inset: self.chrome.title_bar_inset(),
                requests: &mut requests,
            };
            for command in self.commands.drain(..) {
                self.frame.dev_command(command, &mut shell);
            }
            self.frame.draw(ui, &mut shell);

            // Shows the cursor the UI asked for this frame; ImGui's own resumes when it is no longer asked for.
            match requests.cursor {
                Some(shape) => {
                    self.chrome.set_cursor(shape);
                    set_backend_cursor_changes(ui, false);
                }
                None => {
                    self.chrome.reset_cursor();
                    set_backend_cursor_changes(ui, true);
                }
            }
            if self.script.is_none() {
                self.platform.prepare_render(ui, &self.window)?;
            }
        }

        if minimized {
            // The frame must still be closed (and its texture requests served); nothing is shown.
            self.renderer.render(&mut self.imgui)?;
            self.animating = false;
            self.apply_window_requests(&requests);
            return Ok(keep_running && !requests.quit);
        }

        // SAFETY: the GL context is current on this thread for the whole life of `State`.
        unsafe {
            self.gl
                .viewport(0, 0, size.width as i32, size.height as i32);
            self.gl.disable(glow::SCISSOR_TEST);
            let [red, green, blue, alpha] = self.background;
            self.gl.clear_color(red, green, blue, alpha);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        self.renderer.render(&mut self.imgui)?;

        // Screenshots wait (up to ~5 s) for the app's background work so the capture shows the real content.
        let mut counted = true;
        if let Some(path) = &options.screenshot
            && self.frame_index == SCREENSHOT_FRAME
        {
            if self.frame.busy() && self.screenshot_wait < SCREENSHOT_MAX_WAIT_FRAMES {
                self.screenshot_wait += 1;
                counted = false;
                thread::sleep(Duration::from_millis(16));
            } else {
                self.capture(path, size)?;
                keep_running = false;
            }
        }
        if let Some(name) = self.pending_shot.take() {
            self.capture(&options.shots_dir.join(format!("{name}.bmp")), size)?;
        }
        self.surface
            .swap_buffers(&self.gl_context)
            .context("presenting the frame")?;
        if counted {
            self.frame_index += 1;
        }

        self.animating = requests.animating;
        self.apply_window_requests(&requests);
        Ok(keep_running && !requests.quit)
    }

    fn capture(&self, path: &Path, size: PhysicalSize<u32>) -> anyhow::Result<()> {
        save_bmp(&self.gl, path, size.width, size.height)
            .with_context(|| format!("saving the capture {}", path.display()))
    }

    /// Detaches ImGui's backends in the order they require, while the GL context is current.
    fn shutdown(mut self) {
        self.chrome.reset_cursor();
        if let Err(error) = self.renderer.shutdown(&mut self.imgui) {
            eprintln!("imgit: renderer shutdown: {error}");
        }
        if let Err(error) = self.platform.shutdown(&mut self.imgui) {
            eprintln!("imgit: platform shutdown: {error}");
        }
    }
}

/// `(io.DisplaySize, io.DisplayFramebufferScale)` for the window: physical pixels at scale 1 where window
/// coordinates are pixels, points and the content scale on macOS.
fn display_metrics(window: &Window) -> ([f32; 2], [f32; 2]) {
    let size = window.inner_size();
    if cfg!(target_os = "macos") {
        let scale = window.scale_factor();
        let logical: LogicalSize<f32> = size.to_logical(scale);
        ([logical.width, logical.height], [scale as f32; 2])
    } else {
        ([size.width as f32, size.height as f32], [1.0; 2])
    }
}

/// Lets the platform backend set the OS cursor from ImGui's (`true`) or keeps its hands off while one of our
/// own cursors is up. The flag has to change while the frame is open, before the backend's
/// `prepare_render()`, and the safe API only reaches `Io` mutably between frames; hence the raw write, exactly
/// what the C++ `apply_drag_cursor()` does.
fn set_backend_cursor_changes(_ui: &Ui, enabled: bool) {
    let flag = sys::ImGuiConfigFlags_NoMouseCursorChange as sys::ImGuiConfigFlags;
    // SAFETY: `_ui` proves a frame of the current context is open on this thread, so its IO is alive, and no
    // Rust reference to it is held across this write of a plain integer field.
    unsafe {
        let io = sys::igGetIO_Nil();
        if !io.is_null() {
            if enabled {
                (*io).ConfigFlags &= !flag;
            } else {
                (*io).ConfigFlags |= flag;
            }
        }
    }
}

/// The main window with an OpenGL-capable pixel format: `width` x `height` logical pixels of client area,
/// scaled to its monitor.
fn create_window(
    event_loop: &ActiveEventLoop,
    options: &DevOptions,
) -> anyhow::Result<(Window, GlConfig)> {
    let menu_bar = if cfg!(target_os = "macos") {
        0.0
    } else {
        theme::MENU_BAR_HEIGHT
    };
    let mut attributes = Window::default_attributes()
        .with_title("ImGit Desktop")
        .with_inner_size(LogicalSize::new(options.width, options.height))
        .with_min_inner_size(LogicalSize::new(
            MIN_PAGE_SIZE[0],
            MIN_PAGE_SIZE[1] + menu_bar,
        ))
        .with_theme(Some(Theme::Dark));
    if !cfg!(target_os = "macos") {
        // macOS takes the icon from the bundle.
        attributes = attributes.with_window_icon(app_icon(64));
    }
    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(8)
        .with_depth_size(0)
        .with_stencil_size(0);
    let (window, gl_config) = DisplayBuilder::new()
        .with_window_attributes(Some(attributes))
        .build(event_loop, template, |configs| {
            // No multisampling: ImGui anti-aliases its own geometry.
            configs
                .min_by_key(|config| config.num_samples())
                .expect("the display offers a pixel format")
        })
        .map_err(|error| anyhow!("creating the window: {error}"))?;
    let window = window.ok_or_else(|| anyhow!("the display gave no window"))?;
    Ok((window, gl_config))
}

/// The application icon at `size` pixels, from the embedded PNG.
fn app_icon(size: u32) -> Option<Icon> {
    let image = gfx::image::decode_image(assets::data("images/app_icon.png")?, Some(size))?;
    Icon::from_rgba(image.rgba, image.width, image.height).ok()
}

/// An OpenGL 3.3 core context current on the window, with vsync, and its function table.
fn create_gl(
    window: &Window,
    gl_config: &GlConfig,
) -> anyhow::Result<(
    PossiblyCurrentContext,
    Surface<WindowSurface>,
    Rc<glow::Context>,
)> {
    let display = gl_config.display();
    let surface_attributes = window
        .build_surface_attributes(SurfaceAttributesBuilder::<WindowSurface>::new())
        .context("the window has no native handle")?;
    let raw_window = window
        .window_handle()
        .context("the window has no native handle")?
        .as_raw();
    // macOS only gives a core profile context (3.2 to 4.1) when it is forward-compatible, which glutin asks
    // for there on its own.
    let context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
        .with_profile(GlProfile::Core)
        .build(Some(raw_window));
    // SAFETY: the raw window handle in both attribute sets belongs to `window`, which outlives the surface and
    // the context (see the field order of `State`).
    let (context, surface) = unsafe {
        let context = display
            .create_context(gl_config, &context_attributes)
            .context("no OpenGL 3.3 core context")?;
        let surface = display
            .create_window_surface(gl_config, &surface_attributes)
            .context("no OpenGL surface for the window")?;
        (context, surface)
    };
    let context = context
        .make_current(&surface)
        .context("making the OpenGL context current")?;
    if let Err(error) = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::MIN)) {
        eprintln!("imgit: no vsync: {error}");
    }
    // SAFETY: the context was just made current on this thread; the loader returns its entry points.
    let gl =
        unsafe { glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name)) };
    Ok((context, surface, Rc::new(gl)))
}
