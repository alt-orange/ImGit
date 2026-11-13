//! The UI context threaded through every draw function, and the state it keeps between frames.

use std::path::PathBuf;

use dear_imgui_rs::{FontStackToken, Key, StyleColor, Ui, sys};

use crate::fonts::{Font, Fonts};
use crate::shell::ShellContext;
use crate::theme::{self, Color, Palette, Themes};
use crate::ui::avatars::Avatars;
use crate::ui::draw::DrawList;
use crate::ui::geom::{Rect, Vec2};
use crate::ui::icons::Icons;
use crate::ui::menu::MenuState;
use crate::ui::popover::Popovers;
use crate::ui::profile_widgets::ProfileMenuState;
use crate::ui::widgets::anim::Animations;
use crate::ui::widgets::tooltip::TooltipState;
use crate::window::CursorShape;

/// What the widgets remember from one frame to the next: the C++ file-level statics (the `smooth()` table, the
/// tooltip timer, the icon and avatar caches...) in one place. The application owns one for its lifetime and
/// lends it to each frame's [`Cx`].
pub struct UiState {
    /// The running `smooth()` / `smooth_value()` transitions.
    pub(crate) animations: Animations,
    /// Which tooltip is waiting or showing, and since when.
    pub(crate) tooltip: TooltipState,
    /// The multi-line field whose caret moves behind its text once it is active (`Cx::input_multiline`).
    pub(crate) pending_caret: Option<sys::ImGuiID>,
    /// The Font Awesome table and the rasterised icons.
    pub icons: Icons,
    /// The Gravatar downloads and their textures.
    pub avatars: Avatars,
    /// The editable context menu (Cut / Copy / Paste / Select All) run after every text field of
    /// `Cx::input_text` / `Cx::input_multiline` with the field as the last item; the menus set it.
    pub text_field_menu: Option<fn(&mut Cx<'_>, &str)>,
    /// What the diff editors remember per editor (syntax colours, the staging anchor).
    pub diff_editors: crate::ui::panels::diff_view::DiffEditors,
    /// The command a text field's context menu chose, waiting for the field to have the keyboard.
    pub text_edit: Option<crate::ui::panels::menus::PendingTextEdit>,
    /// The context menus and menu bar menus that are open (`ui::menu`).
    pub menus: MenuState,
    /// The dropdowns' layout state (`Cx::popover`).
    pub popovers: Popovers,
    /// The profile picker's keyboard / scroll position (`ui::profile_widgets`).
    pub profile_menu: ProfileMenuState,
    /// The names of the built-in and custom themes, and the current one (kept up to date by
    /// [`Self::begin_frame`]): what the Preferences' theme picker lists.
    pub theme_names: Vec<String>,
    pub theme_name: String,
    /// A reload of the custom themes asked for (the profile's themes folder), done by the next `begin_frame`.
    pending_theme_reload: Option<PathBuf>,
    /// The problems of the reload `begin_frame` did, until [`Self::take_theme_reload`] takes them.
    theme_reload_problems: Option<Vec<String>>,
    /// The system's locale ("en-US"...), read once: what the `System` locale preference formats dates in.
    pub(crate) system_locale: Option<String>,
    zoom: f32,
    pending_theme: Option<String>,
    /// Whether the ImGui style has to be made again (the theme, the zoom or the monitor scale changed).
    restyle: bool,
    styled_scale: f32,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            animations: Animations::default(),
            tooltip: TooltipState::default(),
            pending_caret: None,
            icons: Icons::new(),
            avatars: Avatars::new(),
            text_field_menu: None,
            diff_editors: crate::ui::panels::diff_view::DiffEditors::default(),
            text_edit: None,
            menus: MenuState::default(),
            popovers: Popovers::default(),
            profile_menu: ProfileMenuState::default(),
            theme_names: Vec::new(),
            theme_name: String::new(),
            pending_theme_reload: None,
            theme_reload_problems: None,
            system_locale: imgit_platform::system::system_locale(),
            zoom: 1.0,
            pending_theme: None,
            restyle: true,
            styled_scale: 0.0,
        }
    }

    /// The user zoom the layout is multiplied by on top of the monitor's scale.
    #[must_use]
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Sets the user zoom; takes effect with the next [`Self::begin_frame`].
    pub fn set_zoom(&mut self, zoom: f32) {
        if zoom.is_finite() && zoom > 0.0 && zoom != self.zoom {
            self.zoom = zoom;
            self.restyle = true;
        }
    }

    /// Switches to the theme called `name` with the next [`Self::begin_frame`] (the palette of the frame being
    /// drawn stays what it is).
    pub fn request_theme(&mut self, name: &str) {
        self.pending_theme = Some(name.to_owned());
    }

    /// Makes the ImGui style again with the next [`Self::begin_frame`]: after the custom themes were reloaded.
    pub fn request_restyle(&mut self) {
        self.restyle = true;
    }

    /// Reads the custom themes of `folder` (the profile's themes folder) again with the next
    /// [`Self::begin_frame`], before it applies a requested theme.
    pub fn request_custom_theme_reload(&mut self, folder: PathBuf) {
        self.pending_theme_reload = Some(folder);
    }

    /// The files of the last custom-themes reload that could not be used (one message each; empty when all
    /// worked); `None` when no reload happened since the last call.
    pub fn take_theme_reload(&mut self) -> Option<Vec<String>> {
        self.theme_reload_problems.take()
    }

    /// Call first in `Frame::draw`, before [`Cx::new`]: carries out a requested theme switch or zoom change
    /// (the C++ `Application::apply_settings()`): the palette, the ImGui style scaled by DPI x zoom, the
    /// window frame's shade and the clear colour.
    pub fn begin_frame(&mut self, ui: &Ui, themes: &mut Themes, shell: &mut ShellContext<'_>) {
        let mut themes_changed = self.theme_names.is_empty();
        if let Some(folder) = self.pending_theme_reload.take() {
            let problems = themes.load_custom_themes(&folder);
            self.theme_reload_problems
                .get_or_insert_with(Vec::new)
                .extend(problems);
            self.restyle = true;
            themes_changed = true;
        }
        if let Some(name) = self.pending_theme.take() {
            themes.set_theme(&name);
            self.restyle = true;
            themes_changed = true;
        }
        if themes_changed {
            self.theme_names = themes.theme_names().to_vec();
            self.theme_name = themes.current_theme().to_owned();
        }
        // Before anything draws: the icons the cache has outgrown are freed now, none of this frame's.
        self.icons.begin_frame(&mut *shell.textures);
        let scale = shell.dpi_scale() * self.zoom;
        if !self.restyle && scale == self.styled_scale {
            return;
        }
        // ImGui laid this frame out (NewFrame) with the old style and font scale: draw once more with the new.
        shell.request_animation();
        self.restyle = false;
        self.styled_scale = scale;
        let palette = themes.palette();
        theme::apply_imgui_style(palette, scale);
        // SAFETY: `ui` proves a frame of the current context is open on this thread; one float of its style
        // is written, with no reference to the style held.
        unsafe {
            (*sys::igGetStyle()).FontScaleMain = self.zoom;
        }
        let _ = ui;
        shell.set_window_dark(!palette.light);
        shell.set_background(palette.app_bg0.into());
    }

    /// Whether background work a capture should wait for is still running (the avatar downloads).
    #[must_use]
    pub fn busy(&self) -> bool {
        self.avatars.pending()
    }

    /// Stops the avatar worker and frees every texture the UI made. Call while the renderer is alive.
    pub fn shutdown(&mut self, textures: &mut dyn crate::gfx::Textures) {
        self.avatars.shutdown(textures);
        self.icons.clear(textures);
    }
}

/// Everything a draw function needs, built once per frame by the `Frame` implementation:
///
/// ```ignore
/// fn draw(&mut self, ui: &Ui, shell: &mut ShellContext<'_>) {
///     self.ui_state.begin_frame(ui, &mut self.themes, shell);
///     let mut cx = Cx::new(ui, shell, self.themes.palette(), &mut self.ui_state);
///     panels::draw(&mut cx, &mut self.app);
/// }
/// ```
///
/// The widgets are methods of it (`cx.primary_button(..)`, `cx.draw_icon(..)`), grouped by file under
/// `ui/widgets`.
pub struct Cx<'a> {
    /// The frame being built: windows, items, stacks.
    pub ui: &'a Ui,
    /// The loaded fonts.
    pub fonts: &'a Fonts,
    /// The current theme's colours.
    pub palette: &'a Palette,
    /// Logical (CSS) pixels to device pixels: the monitor's scale times the user zoom. See [`Cx::px`].
    pub scale: f32,
    /// What persists between frames.
    pub state: &'a mut UiState,
    /// The shell: textures, the window, the requests a frame can make.
    pub shell: ShellContext<'a>,
}

impl<'a> Cx<'a> {
    /// The context of one frame. Call [`UiState::begin_frame`] before it.
    pub fn new(
        ui: &'a Ui,
        shell: &'a mut ShellContext<'_>,
        palette: &'a Palette,
        state: &'a mut UiState,
    ) -> Self {
        Self {
            ui,
            fonts: shell.fonts,
            palette,
            scale: shell.dpi_scale() * state.zoom,
            state,
            shell: shell.reborrow(),
        }
    }

    /// Logical pixels (the CSS values) to whole device pixels (`ui::px()`).
    #[must_use]
    pub fn px(&self, logical: f32) -> f32 {
        (logical * self.scale).round()
    }

    /// The device-pixel size ImGui draws `font` at for a CSS font size (`ui::font_px()`): what
    /// [`DrawList::add_text`] takes.
    #[must_use]
    pub fn font_px(&self, font: Font, css_size: f32) -> f32 {
        css_size * self.scale * font.em_scale
    }

    /// Makes `font` at a CSS size the current font until the token is dropped (`ui::push_font()`).
    pub fn push_font(&self, font: Font, css_size: f32) -> FontStackToken<'a> {
        font.push(self.ui, css_size)
    }

    /// The current window's draw list.
    #[must_use]
    pub fn window_draw_list(&self) -> DrawList<'a> {
        DrawList::window(self.ui)
    }

    /// The draw list over every window.
    #[must_use]
    pub fn foreground_draw_list(&self) -> DrawList<'a> {
        DrawList::foreground(self.ui)
    }

    /// The draw list behind every window.
    #[must_use]
    pub fn background_draw_list(&self) -> DrawList<'a> {
        DrawList::background(self.ui)
    }

    /// Where the next item goes, in screen coordinates (`ImGui::GetCursorScreenPos()`).
    #[must_use]
    pub fn cursor(&self) -> Vec2 {
        self.ui.cursor_screen_pos().into()
    }

    /// `ImGui::SetCursorScreenPos()`.
    pub fn set_cursor(&self, pos: Vec2) {
        self.ui.set_cursor_screen_pos(pos);
    }

    /// The space left in the current window (`ImGui::GetContentRegionAvail()`).
    #[must_use]
    pub fn avail(&self) -> Vec2 {
        self.ui.get_content_region_avail().into()
    }

    /// The mouse position (`io.MousePos`).
    #[must_use]
    pub fn mouse_pos(&self) -> Vec2 {
        self.ui.mouse_pos().into()
    }

    /// The last item's rectangle (`GetItemRectMin()` / `Max()`).
    #[must_use]
    pub fn item_rect(&self) -> Rect {
        Rect::new(
            self.ui.item_rect_min().into(),
            self.ui.item_rect_max().into(),
        )
    }

    /// `ImGui::GetID(label)` in the current id scope.
    #[must_use]
    pub fn id(&self, label: &str) -> sys::ImGuiID {
        self.ui.get_id(label).raw()
    }

    /// `ImGui::GetTime()`: seconds since the first frame.
    #[must_use]
    pub fn time(&self) -> f64 {
        self.ui.time()
    }

    /// `ImGui::GetFrameCount()`.
    #[must_use]
    pub fn frame_count(&self) -> i32 {
        // SAFETY: `self.ui` proves the context is current on this thread.
        unsafe { sys::igGetFrameCount() }
    }

    /// `io.DeltaTime`: the seconds the last frame took.
    #[must_use]
    pub fn delta_time(&self) -> f32 {
        // SAFETY: the context of `self.ui` is current, so its IO exists; one float is read.
        unsafe { (*sys::igGetIO_Nil()).DeltaTime }
    }

    /// Whether ImGui follows the macOS conventions (`io.ConfigMacOSXBehaviors`): Cmd for shortcuts.
    #[must_use]
    pub fn macos_behaviors(&self) -> bool {
        // SAFETY: as for `delta_time`; one flag is read.
        unsafe { (*sys::igGetIO_Nil()).ConfigMacOSXBehaviors }
    }

    /// The modifier to feed `io.AddKeyEvent()` for a synthesized shortcut chord: ImGui swaps Ctrl and Super as
    /// a backend's events arrive on macOS, so the event that reads as Ctrl (Cmd) there is Super.
    #[must_use]
    pub fn shortcut_modifier_event(&self) -> Key {
        if self.macos_behaviors() {
            Key::ModSuper
        } else {
            Key::ModCtrl
        }
    }

    /// A colour of the ImGui style as it is now, pushed colours included (`style.Colors[..]`).
    #[must_use]
    pub fn style_color(&self, color: StyleColor) -> Color {
        self.ui.style_color(color).into()
    }

    /// OS cursors ImGui has no shape for (the shell shows them): during a drag the system drag-and-drop
    /// cursors, `Move` over a drop target and `NoDrop` elsewhere; over a resize handle Chromium's col-resize /
    /// row-resize. Set it every frame it is wanted (`ui::set_drag_cursor()`).
    pub fn set_drag_cursor(&mut self, cursor: CursorShape) {
        self.shell.set_cursor(cursor);
    }

    /// Switches to the theme called `name` from the next frame on.
    pub fn request_theme(&mut self, name: &str) {
        self.state.request_theme(name);
    }

    /// `ImGui::GetHoveredID()`: the item the mouse was over when the frame began.
    #[must_use]
    pub fn hovered_id(&self) -> sys::ImGuiID {
        // SAFETY: the context of `self.ui` is current.
        unsafe { sys::igGetHoveredID() }
    }

    /// `ImGui::ItemAdd()`: declares an item over `rect` without moving the layout cursor. `false` when it is
    /// clipped.
    pub fn item_add(&self, rect: Rect, id: sys::ImGuiID) -> bool {
        // SAFETY: a window is current while widgets are drawn; the rectangle is passed by value.
        unsafe { sys::igItemAdd(rect.into(), id, std::ptr::null(), 0) }
    }

    /// `ImGui::ButtonBehavior()` for the item `id` over `rect` (after [`Self::item_add`]).
    pub fn button_behavior(&self, rect: Rect, id: sys::ImGuiID) -> ButtonState {
        let mut state = ButtonState::default();
        // SAFETY: a window is current; the out-pointers are valid for the call.
        state.pressed = unsafe {
            sys::igButtonBehavior(rect.into(), id, &mut state.hovered, &mut state.held, 0)
        };
        state
    }

    /// A float of the current window's state storage (`ImGui::GetStateStorage()->GetFloat()`).
    #[must_use]
    pub fn storage_float(&self, key: sys::ImGuiID, default: f32) -> f32 {
        // SAFETY: a window is current, so its storage exists.
        unsafe { sys::ImGuiStorage_GetFloat(sys::igGetStateStorage(), key, default) }
    }

    pub fn set_storage_float(&self, key: sys::ImGuiID, value: f32) {
        // SAFETY: as for `storage_float`.
        unsafe { sys::ImGuiStorage_SetFloat(sys::igGetStateStorage(), key, value) }
    }

    /// A flag of the current window's state storage.
    #[must_use]
    pub fn storage_bool(&self, key: sys::ImGuiID, default: bool) -> bool {
        // SAFETY: as for `storage_float`.
        unsafe { sys::ImGuiStorage_GetBool(sys::igGetStateStorage(), key, default) }
    }

    pub fn set_storage_bool(&self, key: sys::ImGuiID, value: bool) {
        // SAFETY: as for `storage_float`.
        unsafe { sys::ImGuiStorage_SetBool(sys::igGetStateStorage(), key, value) }
    }
}

/// What `ImGui::ButtonBehavior()` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ButtonState {
    /// Clicked this frame.
    pub pressed: bool,
    pub hovered: bool,
    /// The mouse button is down on it.
    pub held: bool,
}
