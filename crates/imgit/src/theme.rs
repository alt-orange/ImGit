//! The palette of the current theme, resolved to RGBA, the layout constants of the graph and the ImGui style.
//!
//! Themes share the layout and the semantic colours (red, green, the graph lanes...) and differ in their
//! backgrounds and accent. A [`Themes`] holds the built-in themes, the custom ones loaded from the profile and
//! the [`Palette`] of the one in use; the UI reads the palette through `Cx::palette`.

use std::fs;
use std::path::Path;

use dear_imgui_rs::sys;
use imgit_app::theme_file::{Rgba, ThemeFile};
use imgit_syntax::Token;

/// A colour: straight-alpha RGBA, each channel in 0..=1. The one colour type of the UI: style colours take it
/// as `[f32; 4]` (`.into()`), draw lists pack it with [`Color::to_u32`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);
    /// Nothing: black at zero opacity.
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);

    #[must_use]
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque colour from `0xRRGGBB`.
    #[must_use]
    pub const fn rgb(hex: u32) -> Self {
        Self::rgba(hex, 1.0)
    }

    /// `0xRRGGBB` at `alpha`.
    #[must_use]
    pub const fn rgba(hex: u32, alpha: f32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as f32 / 255.0,
            g: ((hex >> 8) & 0xFF) as f32 / 255.0,
            b: (hex & 0xFF) as f32 / 255.0,
            a: alpha,
        }
    }

    /// From 8-bit channels: `IM_COL32(r, g, b, a)`.
    #[must_use]
    pub const fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// The same colour at `alpha` (replacing its own).
    #[must_use]
    pub const fn with_alpha(self, alpha: f32) -> Self {
        Self { a: alpha, ..self }
    }

    /// The same colour with its alpha multiplied by `factor`: a fade.
    #[must_use]
    pub const fn faded(self, factor: f32) -> Self {
        Self {
            a: self.a * factor,
            ..self
        }
    }

    /// LESS `mix(color, background, weight)`: `weight` of this colour over `1 - weight` of `background`,
    /// opaque.
    #[must_use]
    pub fn mix(self, background: Self, weight: f32) -> Self {
        Self {
            r: self.r * weight + background.r * (1.0 - weight),
            g: self.g * weight + background.g * (1.0 - weight),
            b: self.b * weight + background.b * (1.0 - weight),
            a: 1.0,
        }
    }

    /// Packed as ImGui's `ImU32` (`0xAABBGGRR`), each channel saturated and rounded like
    /// `ImGui::ColorConvertFloat4ToU32`.
    #[must_use]
    pub fn to_u32(self) -> u32 {
        // IM_F32_TO_INT8_SAT: (int)(saturate(v) * 255 + 0.5).
        let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
        channel(self.r) | channel(self.g) << 8 | channel(self.b) << 16 | channel(self.a) << 24
    }

    /// From a packed `ImU32` (`0xAABBGGRR`).
    #[must_use]
    pub const fn from_u32(packed: u32) -> Self {
        Self::from_rgba8(
            (packed & 0xFF) as u8,
            ((packed >> 8) & 0xFF) as u8,
            ((packed >> 16) & 0xFF) as u8,
            (packed >> 24) as u8,
        )
    }

    /// `0xRRGGBB`, each channel clamped and rounded to the nearest of 255 steps; the alpha is dropped.
    #[must_use]
    pub fn to_hex(self) -> u32 {
        let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
        channel(self.r) << 16 | channel(self.g) << 8 | channel(self.b)
    }

    #[must_use]
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

impl From<Color> for [f32; 4] {
    fn from(color: Color) -> Self {
        color.to_array()
    }
}

impl From<[f32; 4]> for Color {
    fn from([r, g, b, a]: [f32; 4]) -> Self {
        Self { r, g, b, a }
    }
}

impl From<Color> for sys::ImVec4 {
    fn from(color: Color) -> Self {
        Self {
            x: color.r,
            y: color.g,
            z: color.b,
            w: color.a,
        }
    }
}

impl From<sys::ImVec4> for Color {
    fn from(color: sys::ImVec4) -> Self {
        Self::new(color.x, color.y, color.z, color.w)
    }
}

impl From<Rgba> for Color {
    fn from(color: Rgba) -> Self {
        Self::new(color.r, color.g, color.b, color.a)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Hsl {
    h: f32,
    s: f32,
    l: f32,
}

fn to_hsl(c: Color) -> Hsl {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let l = (max + min) / 2.0;
    if max == min {
        return Hsl { h: 0.0, s: 0.0, l };
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == c.r {
        (c.g - c.b) / d + if c.g < c.b { 6.0 } else { 0.0 }
    } else if max == c.g {
        (c.b - c.r) / d + 2.0
    } else {
        (c.r - c.g) / d + 4.0
    };
    Hsl { h: h / 6.0, s, l }
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

fn from_hsl(c: Hsl, alpha: f32) -> Color {
    if c.s == 0.0 {
        return Color::new(c.l, c.l, c.l, alpha);
    }
    let q = if c.l < 0.5 {
        c.l * (1.0 + c.s)
    } else {
        c.l + c.s - c.l * c.s
    };
    let p = 2.0 * c.l - q;
    Color::new(
        hue_to_rgb(p, q, c.h + 1.0 / 3.0),
        hue_to_rgb(p, q, c.h),
        hue_to_rgb(p, q, c.h - 1.0 / 3.0),
        alpha,
    )
}

/// LESS `lighten()`: an absolute lightness offset in HSL space (negative darkens).
fn lighten(c: Color, amount: f32) -> Color {
    let mut hsl = to_hsl(c);
    hsl.l = (hsl.l + amount).clamp(0.0, 1.0);
    from_hsl(hsl, c.a)
}

/// The number of lane colours of the graph.
pub const GRAPH_COLOR_COUNT: usize = 10;

/// The colours of a theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// A light background with dark text.
    pub light: bool,
    /// White itself: marks on the accent or a lane colour.
    pub white: Color,
    /// The text and overlay base: white on a dark theme, near black on a light one. Use it ([`Palette::ink`])
    /// where a colour is "the foreground at some opacity".
    pub ink: Color,
    pub red: Color,
    pub orange: Color,
    pub yellow: Color,
    pub green: Color,
    pub teal: Color,
    pub blue: Color,
    pub ltblue: Color,
    pub purple: Color,
    /// Selection, focus rings, primary buttons, badges.
    pub accent: Color,

    pub app_bg0: Color,
    pub toolbar_bg0: Color,
    pub toolbar_bg1: Color,
    pub toolbar_bg2: Color,
    pub panel_bg0: Color,
    pub panel_bg1: Color,
    pub panel_bg2: Color,
    pub input_bg: Color,
    pub panel_border: Color,
    pub section_border: Color,
    pub subtle_border: Color,
    pub modal_overlay: Color,

    pub graph_colors: [Color; GRAPH_COLOR_COUNT],
    pub stats_added: Color,
    pub stats_deleted: Color,
    pub stats_files: Color,

    pub text_selected: Color,
    pub text_normal: Color,
    pub text_secondary: Color,
    pub text_disabled: Color,
    pub text_accent: Color,
    pub text_inverse: Color,
    pub text_dimmed: Color,

    pub hover_row: Color,
    pub selected_row: Color,
    pub link: Color,
    pub wip_status: Color,
    pub checked_out: Color,
    pub added_line: Color,
    pub deleted_line: Color,
    pub hunk_header: Color,
    /// `DEFAULT_AVATAR_BACKGROUND_COLOR`.
    pub avatar_default: Color,
    pub scroll_thumb: Color,
    /// `toggle-button-border`.
    pub toggle_border: Color,
    /// `filter-match`.
    pub filter_match: Color,
    /// `droppable` / `drop-target`.
    pub droppable: Color,
    pub drop_target: Color,
    /// `terminal__background`.
    pub terminal_bg: Color,
    pub terminal_fg: Color,
    /// `terminal__repo-name-color`.
    pub terminal_prompt: Color,
    pub terminal_red: Color,

    // Syntax highlighting in the diff and merge editors: Monaco's vs-dark token colours, or its vs ones when
    // the theme's background is light. Plain text keeps each editor's own colour.
    pub syntax_comment: Color,
    pub syntax_string: Color,
    pub syntax_number: Color,
    pub syntax_keyword: Color,
    pub syntax_type: Color,
    pub syntax_function: Color,
    pub syntax_variable: Color,
    pub syntax_operator: Color,
    pub syntax_punctuation: Color,
    pub syntax_tag: Color,
    pub syntax_attribute: Color,
    pub syntax_regexp: Color,
    pub syntax_constant: Color,
}

/// The palette's colours by the names theme files use: `(name, getter)`.
macro_rules! named_colors {
    ($($name:ident),* $(,)?) => {
        /// Every name a theme file's "colors" may set, without the lanes ("graph_0".."graph_9").
        pub const NAMED_COLORS: &[&str] = &[$(stringify!($name)),*];

        impl Palette {
            /// The colour a theme file calls `name` ("graph_0".."graph_9" for the lanes); `None` for a name
            /// the palette does not have.
            pub fn named_color_mut(&mut self, name: &str) -> Option<&mut Color> {
                match name {
                    $(stringify!($name) => Some(&mut self.$name),)*
                    _ => {
                        let lane = name.strip_prefix("graph_")?;
                        let &[digit @ b'0'..=b'9'] = lane.as_bytes() else {
                            return None;
                        };
                        Some(&mut self.graph_colors[usize::from(digit - b'0')])
                    }
                }
            }
        }
    };
}

named_colors!(
    red,
    orange,
    yellow,
    green,
    teal,
    blue,
    ltblue,
    purple,
    accent,
    ink,
    app_bg0,
    toolbar_bg0,
    toolbar_bg1,
    toolbar_bg2,
    panel_bg0,
    panel_bg1,
    panel_bg2,
    input_bg,
    panel_border,
    section_border,
    subtle_border,
    modal_overlay,
    stats_added,
    stats_deleted,
    stats_files,
    text_selected,
    text_normal,
    text_secondary,
    text_disabled,
    text_accent,
    text_inverse,
    text_dimmed,
    hover_row,
    selected_row,
    link,
    wip_status,
    checked_out,
    added_line,
    deleted_line,
    hunk_header,
    avatar_default,
    scroll_thumb,
    toggle_border,
    filter_match,
    droppable,
    drop_target,
    terminal_bg,
    terminal_fg,
    terminal_prompt,
    terminal_red,
    syntax_comment,
    syntax_string,
    syntax_number,
    syntax_keyword,
    syntax_type,
    syntax_function,
    syntax_variable,
    syntax_operator,
    syntax_punctuation,
    syntax_tag,
    syntax_attribute,
    syntax_regexp,
    syntax_constant,
);

impl Palette {
    /// The theme's ink at `alpha` (`theme::ink()` in C++).
    #[must_use]
    pub const fn ink(&self, alpha: f32) -> Color {
        self.ink.with_alpha(alpha)
    }

    /// The same with an alpha out of 255 (`theme::ink_u32()`).
    #[must_use]
    pub const fn ink_255(&self, alpha: u8) -> Color {
        self.ink.with_alpha(alpha as f32 / 255.0)
    }

    /// The lane colour of a graph column; columns wrap round the lane colours, negative ones included.
    #[must_use]
    pub fn graph_color(&self, column: i32) -> Color {
        self.graph_colors[column.rem_euclid(GRAPH_COLOR_COUNT as i32) as usize]
    }

    /// The colour of a syntax token; `None` for plain text, which keeps each editor's own colour.
    #[must_use]
    pub const fn syntax_color(&self, token: Token) -> Option<Color> {
        Some(match token {
            Token::Text => return None,
            Token::Comment => self.syntax_comment,
            Token::String => self.syntax_string,
            Token::Number => self.syntax_number,
            Token::Keyword => self.syntax_keyword,
            Token::Type => self.syntax_type,
            Token::Function => self.syntax_function,
            Token::Variable => self.syntax_variable,
            Token::Operator => self.syntax_operator,
            Token::Punctuation => self.syntax_punctuation,
            Token::Tag => self.syntax_tag,
            Token::Attribute => self.syntax_attribute,
            Token::Regexp => self.syntax_regexp,
            Token::Constant => self.syntax_constant,
        })
    }
}

impl Default for Palette {
    /// The palette of [`DEFAULT_THEME`].
    fn default() -> Self {
        build_palette(&THEMES[0])
    }
}

/// The ink at a fixed opacity, resolved against a palette when it is used, so a constant made of it follows
/// theme switches: `const RULE: Ink = Ink(0.12);` ... `RULE.of(cx.palette)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ink(pub f32);

impl Ink {
    /// The ink at an alpha out of 255 (the C++ `InkU32`).
    #[must_use]
    pub const fn alpha_255(alpha: u8) -> Self {
        Self(alpha as f32 / 255.0)
    }

    /// The colour in `palette`.
    #[must_use]
    pub const fn of(self, palette: &Palette) -> Color {
        palette.ink(self.0)
    }
}

/// What a theme changes: its background (the rest of the surfaces are steps of it: lighter ones on a dark
/// background, darker and lighter ones around a light background, which also gets dark text) and its accent. A
/// theme with `exact` replaces the derived surfaces and text colours with its own afterwards.
#[derive(Debug, Clone, Copy)]
struct ThemeSpec {
    name: &'static str,
    /// `app__bg0`.
    background: u32,
    accent: u32,
    link: u32,
    /// Counts, highlighted numbers.
    text_accent: u32,
    /// The default avatar disc.
    avatar: u32,
    /// The terminal's repository name.
    prompt: u32,
    /// The lane colour the graph starts with, so its first lane matches the accent.
    first_lane: usize,
    exact: Option<fn(&mut Palette)>,
}

impl ThemeSpec {
    #[allow(clippy::too_many_arguments)]
    const fn new(
        name: &'static str,
        background: u32,
        accent: u32,
        link: u32,
        text_accent: u32,
        avatar: u32,
        prompt: u32,
        first_lane: usize,
    ) -> Self {
        Self {
            name,
            background,
            accent,
            link,
            text_accent,
            avatar,
            prompt,
            first_lane,
            exact: None,
        }
    }

    const fn exact(mut self, exact: fn(&mut Palette)) -> Self {
        self.exact = Some(exact);
        self
    }
}

/// The colours of Claude Code Desktop's dark theme, as it draws them: the window, its darker sidebar, the
/// neutral grey hover and selection rows, its text ramp and the diff backgrounds of its code preview.
fn claude_code_palette(p: &mut Palette) {
    p.blue = Color::rgb(0x2776D0); // the toggles

    // The chrome is as dark as the content: the title bar and sidebar one step darker than the window, the
    // toolbar the window itself, and only inputs, popovers and dropdowns a step lighter.
    p.app_bg0 = Color::rgb(0x151515);
    p.toolbar_bg0 = Color::rgb(0x101010); // the tabs bar
    p.toolbar_bg1 = Color::rgb(0x151515); // the toolbar
    p.toolbar_bg2 = Color::rgb(0x1F1F1F); // popovers
    p.panel_bg0 = Color::rgb(0x101010); // the sidebar
    p.panel_bg1 = Color::rgb(0x1A1A18); // the preferences sidebar, popups, toasts
    p.panel_bg2 = Color::rgb(0x1F1F1F); // dropdowns, segmented controls
    p.input_bg = Color::rgb(0x1F1F1F); // the prompt box
    p.panel_border = Color::rgb(0x2F2F2F);
    p.section_border = Color::rgb(0x2C2D2A);
    p.subtle_border = Color::rgb(0x222222);

    p.text_selected = Color::rgb(0xEEEEEC);
    p.text_normal = Color::rgb(0xC2C3C0);
    p.text_secondary = Color::rgb(0x8B8B89);
    p.text_disabled = Color::rgb(0x626260);
    p.text_dimmed = Color::rgb(0x414141);

    p.hover_row = Color::rgb(0x2F2F2F);
    p.selected_row = Color::rgb(0x414141);
    p.added_line = Color::rgb(0x253B24);
    p.deleted_line = Color::rgb(0x452524);
    p.hunk_header = Color::rgb(0x1A1A18);
    p.scroll_thumb = Color::rgb(0x464646);
    p.toggle_border = Color::rgb(0x464644);
    p.terminal_bg = Color::rgb(0x101010);
    p.terminal_fg = Color::rgb(0xC2C3C0);
}

/// Monaco's token colours: vs-dark on a dark background, vs on a light one.
fn syntax_palette(p: &mut Palette) {
    if to_hsl(p.app_bg0).l > 0.5 {
        p.syntax_comment = Color::rgb(0x008000);
        p.syntax_string = Color::rgb(0xA31515);
        p.syntax_number = Color::rgb(0x098658);
        p.syntax_keyword = Color::rgb(0x0000FF);
        p.syntax_type = Color::rgb(0x267F99);
        p.syntax_function = Color::rgb(0x795E26);
        p.syntax_variable = Color::rgb(0x001080);
        p.syntax_operator = Color::rgb(0x000000);
        p.syntax_punctuation = Color::rgb(0x383838);
        p.syntax_tag = Color::rgb(0x800000);
        p.syntax_attribute = Color::rgb(0xE50000);
        p.syntax_regexp = Color::rgb(0x811F3F);
        p.syntax_constant = Color::rgb(0x0000FF);
        return;
    }
    p.syntax_comment = Color::rgb(0x6A9955);
    p.syntax_string = Color::rgb(0xCE9178);
    p.syntax_number = Color::rgb(0xB5CEA8);
    p.syntax_keyword = Color::rgb(0x569CD6);
    p.syntax_type = Color::rgb(0x4EC9B0);
    p.syntax_function = Color::rgb(0xDCDCAA);
    p.syntax_variable = Color::rgb(0x9CDCFE);
    p.syntax_operator = Color::rgb(0xD4D4D4);
    p.syntax_punctuation = Color::rgb(0xD4D4D4);
    p.syntax_tag = Color::rgb(0x569CD6);
    p.syntax_attribute = Color::rgb(0x9CDCFE);
    p.syntax_regexp = Color::rgb(0xD16969);
    p.syntax_constant = Color::rgb(0x569CD6);
}

/// The theme a profile starts with.
pub const DEFAULT_THEME: &str = "ImGit Dark";
pub const LIGHT_THEME: &str = "ImGit Light";

const THEMES: [ThemeSpec; 7] = [
    ThemeSpec::new(
        DEFAULT_THEME,
        0x1E1C1A,
        0xF28C28,
        0xFFAE5C,
        0xFFBE7D,
        0xC2661D,
        0xFFAE5C,
        0,
    ),
    // The light one: warm paper greys, a deeper orange so text in it stays readable.
    ThemeSpec::new(
        LIGHT_THEME,
        0xF6F5F3,
        0xE07B12,
        0xB35C00,
        0xA0520A,
        0xC2661D,
        0xB35C00,
        0,
    ),
    ThemeSpec::new(
        "Ocean", 0x161D26, 0x3A9DE8, 0x6CC4F5, 0x9CCBF2, 0x2A6FA8, 0x6CC4F5, 5,
    ),
    ThemeSpec::new(
        "Midnight Teal",
        0x121518,
        0x1FB5A3,
        0x4FD8C8,
        0x86E0D5,
        0x167F73,
        0x4FD8C8,
        4,
    ),
    ThemeSpec::new(
        "Amethyst", 0x1D1B24, 0x9B6BFF, 0xC59BFF, 0xC6AEF7, 0x6B46B5, 0xC59BFF, 7,
    ),
    ThemeSpec::new(
        "Rose", 0x211C1F, 0xE5486C, 0xFF8FA8, 0xF7A8B9, 0xAD3450, 0xFF8FA8, 9,
    ),
    // Claude Code Desktop's dark theme: its terracotta accent over its own greys (claude_code_palette).
    ThemeSpec::new(
        "Claude Code",
        0x151515,
        0xD97757,
        0xD97757,
        0xD97757,
        0x444444,
        0xD97757,
        0,
    )
    .exact(claude_code_palette),
];

/// The lane colours, round the colour wheel from orange.
const GRAPH_LANES: [u32; GRAPH_COLOR_COUNT] = [
    0xF28C28, 0xF2C230, 0xB9D23A, 0x52C45A, 0x1FB5A3, 0x2BA6E0, 0x5B7CFA, 0xA85CF0, 0xD9479F,
    0xE5486C,
];

fn builtin_theme(name: &str) -> Option<&'static ThemeSpec> {
    THEMES.iter().find(|theme| theme.name == name)
}

fn build_palette(theme: &ThemeSpec) -> Palette {
    let app_bg0 = Color::rgb(theme.background);
    let light = to_hsl(app_bg0).l > 0.5;
    let accent = Color::rgb(theme.accent);
    let ink = if light {
        Color::rgb(0x1B1A19)
    } else {
        Color::WHITE
    };
    // Darker greens and reds read on white.
    let red = Color::rgb(if light { 0xCF3A32 } else { 0xE5534B });
    let orange = Color::rgb(if light { 0xC77C12 } else { 0xE8A23A });
    let yellow = Color::rgb(if light { 0xB38B00 } else { 0xE8C547 });
    let green = Color::rgb(if light { 0x2E9A4B } else { 0x46B864 });

    // Light: the chrome a step darker than the content (the tabs bar darkest), popups and inputs lighter.
    // Dark: the surfaces are lighter steps of the background.
    let toolbar_bg0 = lighten(app_bg0, if light { -0.08 } else { 0.06 });
    let toolbar_bg1 = if light {
        lighten(app_bg0, -0.04)
    } else {
        lighten(toolbar_bg0, 0.04)
    };
    let toolbar_bg2 = if light {
        lighten(app_bg0, 0.03)
    } else {
        lighten(toolbar_bg1, 0.06)
    };
    let panel_bg0 = lighten(app_bg0, if light { -0.03 } else { 0.05 });
    let panel_bg1 = if light {
        lighten(app_bg0, 0.03)
    } else {
        lighten(panel_bg0, 0.05)
    };
    let panel_bg2 = if light {
        lighten(app_bg0, -0.06)
    } else {
        lighten(panel_bg1, 0.05)
    };
    let rule = if light { 0x000000 } else { 0xFFFFFF };

    // The text ramp: the ink at falling opacities (a little stronger on a light background).
    let text_normal = ink.with_alpha(if light { 0.85 } else { 0.75 });

    let mut palette = Palette {
        light,
        white: Color::WHITE,
        ink,
        red,
        orange,
        yellow,
        green,
        teal: Color::rgb(0x2DBE9C),
        blue: Color::rgb(0x4C8DF0),
        ltblue: Color::rgb(0x4FB3E0),
        purple: Color::rgb(0xB05CF0),
        accent,

        app_bg0,
        toolbar_bg0,
        toolbar_bg1,
        toolbar_bg2,
        panel_bg0,
        panel_bg1,
        panel_bg2,
        input_bg: if light {
            Color::rgba(0xFFFFFF, 0.75)
        } else {
            Color::rgba(0x000000, 0.20)
        },
        panel_border: Color::rgba(rule, if light { 0.12 } else { 0.08 }),
        section_border: Color::rgba(rule, if light { 0.12 } else { 0.08 }),
        subtle_border: Color::rgba(rule, if light { 0.06 } else { 0.04 }),
        modal_overlay: Color::rgba(0x000000, if light { 0.3 } else { 0.5 }),

        // A theme starts at its accent's lane; on a light background the lanes are a step darker.
        graph_colors: std::array::from_fn(|index| {
            let lane = Color::rgb(GRAPH_LANES[(index + theme.first_lane) % GRAPH_COLOR_COUNT]);
            if light { lighten(lane, -0.08) } else { lane }
        }),
        stats_added: Color::rgb(0x347D39),
        stats_deleted: Color::rgb(0xC93C37),
        stats_files: Color::rgb(0xE2C08D),

        text_selected: ink.with_alpha(1.0),
        text_normal,
        text_secondary: ink.with_alpha(if light { 0.65 } else { 0.60 }),
        text_disabled: ink.with_alpha(if light { 0.45 } else { 0.40 }),
        text_accent: Color::rgb(theme.text_accent),
        text_inverse: Color::rgb(0x222222),
        text_dimmed: ink.with_alpha(text_normal.a * 0.20),

        hover_row: accent.with_alpha(0.10),
        selected_row: accent.with_alpha(0.20),
        link: Color::rgb(theme.link),
        wip_status: accent.with_alpha(0.40),
        checked_out: green.with_alpha(0.30),
        added_line: green.with_alpha(0.20),
        deleted_line: red.with_alpha(0.20),
        hunk_header: Color::rgba(0x000000, if light { 0.06 } else { 0.25 }),
        avatar_default: Color::rgb(theme.avatar),
        scroll_thumb: ink.with_alpha(if light { 0.22 } else { 0.15 }),
        toggle_border: ink.with_alpha(if light { 0.25 } else { 0.20 }),
        filter_match: accent.with_alpha(0.50),
        droppable: yellow.with_alpha(0.30),
        drop_target: green.with_alpha(0.50),
        terminal_bg: lighten(app_bg0, if light { 0.02 } else { -0.04 }),
        terminal_fg: Color::rgb(if light { 0x2B2B2B } else { 0xDCDCDC }),
        terminal_prompt: Color::rgb(theme.prompt),
        terminal_red: Color::rgb(if light { 0xC4312A } else { 0xF0716A }),

        // Filled in by syntax_palette() below, once the background is final.
        syntax_comment: Color::TRANSPARENT,
        syntax_string: Color::TRANSPARENT,
        syntax_number: Color::TRANSPARENT,
        syntax_keyword: Color::TRANSPARENT,
        syntax_type: Color::TRANSPARENT,
        syntax_function: Color::TRANSPARENT,
        syntax_variable: Color::TRANSPARENT,
        syntax_operator: Color::TRANSPARENT,
        syntax_punctuation: Color::TRANSPARENT,
        syntax_tag: Color::TRANSPARENT,
        syntax_attribute: Color::TRANSPARENT,
        syntax_regexp: Color::TRANSPARENT,
        syntax_constant: Color::TRANSPARENT,
    };
    if let Some(exact) = theme.exact {
        exact(&mut palette);
    }
    syntax_palette(&mut palette);
    palette
}

/// A theme loaded from a file: the built-in spec it starts from with its own colours put in, then its palette
/// colours by name.
#[derive(Debug, Clone)]
struct CustomTheme {
    name: String,
    spec: ThemeSpec,
    colors: Vec<(String, Color)>,
}

impl CustomTheme {
    fn palette(&self) -> Palette {
        let mut palette = build_palette(&self.spec);
        for (key, color) in &self.colors {
            if let Some(target) = palette.named_color_mut(key) {
                *target = *color;
            }
        }
        palette
    }

    /// The theme of a parsed file, with one message per palette colour it names that does not exist.
    ///
    /// # Errors
    /// The message for a file whose base is not a built-in theme.
    fn from_file(file: ThemeFile) -> Result<(Self, Vec<String>), String> {
        let base_name = file.base.as_deref().unwrap_or(DEFAULT_THEME);
        let base = builtin_theme(base_name)
            .ok_or_else(|| format!("no built-in theme called \"{base_name}\""))?;
        let mut spec = *base;
        let put = |color: Option<Rgba>, target: &mut u32| {
            if let Some(color) = color {
                *target = Color::from(color).to_hex();
            }
        };
        put(file.background, &mut spec.background);
        put(file.accent, &mut spec.accent);
        put(file.link, &mut spec.link);
        put(file.text_accent, &mut spec.text_accent);
        put(file.avatar, &mut spec.avatar);
        put(file.prompt, &mut spec.prompt);
        if file.background.is_some() {
            // Its own background, its own surfaces: the base's exact greys would not match it.
            spec.exact = None;
        }
        let mut problems = Vec::new();
        let mut colors = Vec::new();
        let mut probe = Palette::default();
        for (key, color) in file.colors {
            if probe.named_color_mut(&key).is_none() {
                problems.push(format!("no palette colour called \"{key}\""));
                continue;
            }
            colors.push((key, Color::from(color)));
        }
        Ok((
            Self {
                name: file.name,
                spec,
                colors,
            },
            problems,
        ))
    }
}

/// The themes: the built-in ones, the custom ones loaded from the profile, and the palette of the one in use.
///
/// The application owns one and lends its [`Themes::palette`] to each frame's `Cx`. A theme switch asked for
/// while a frame is drawn (`Cx::request_theme`) is carried out before the next one: [`Themes::set_theme`], then
/// [`apply_imgui_style`].
#[derive(Debug, Clone)]
pub struct Themes {
    custom: Vec<CustomTheme>,
    names: Vec<String>,
    current: String,
    palette: Palette,
}

impl Default for Themes {
    fn default() -> Self {
        Self::new()
    }
}

impl Themes {
    /// The built-in themes, with the default one current.
    #[must_use]
    pub fn new() -> Self {
        Self {
            custom: Vec::new(),
            names: THEMES.iter().map(|theme| theme.name.to_owned()).collect(),
            current: DEFAULT_THEME.to_owned(),
            palette: Palette::default(),
        }
    }

    /// The palette of the current theme.
    #[must_use]
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The themes' names: the built-in ones (the default first), then the custom ones.
    #[must_use]
    pub fn theme_names(&self) -> &[String] {
        &self.names
    }

    /// The names of the built-in themes, the default first.
    pub fn builtin_names() -> impl Iterator<Item = &'static str> {
        THEMES.iter().map(|theme| theme.name)
    }

    #[must_use]
    pub fn current_theme(&self) -> &str {
        &self.current
    }

    /// Switches the palette to the theme called `name` (the default one for a name it does not know). Call
    /// [`apply_imgui_style`] after it.
    pub fn set_theme(&mut self, name: &str) {
        if let Some(custom) = self.custom.iter().find(|custom| custom.name == name) {
            self.palette = custom.palette();
            self.current.clone_from(&custom.name);
            return;
        }
        let theme = builtin_theme(name).unwrap_or(&THEMES[0]);
        self.palette = build_palette(theme);
        theme.name.clone_into(&mut self.current);
    }

    /// Custom themes: every `*.json` file in `folder` (the profile's themes folder), replacing the ones loaded
    /// before. A file holds one theme (see [`imgit_app::theme_file`]); a theme named like a built-in one, or
    /// like one loaded before it, is skipped. Returns one message per file (or colour) that could not be used.
    /// The current theme stays current when it still exists; call [`apply_imgui_style`] after this.
    pub fn load_custom_themes(&mut self, folder: &Path) -> Vec<String> {
        let mut problems = Vec::new();
        self.custom.clear();
        let mut files: Vec<_> = fs::read_dir(folder)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        for file in files {
            let label = file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            // An unreadable file reads as empty text, which is not a theme (as the C++ stream did).
            let text = fs::read(&file)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default();
            let parsed = match ThemeFile::parse(&text) {
                Ok(parsed) => parsed,
                Err(error) => {
                    problems.push(format!("{label}: {error}"));
                    continue;
                }
            };
            if builtin_theme(&parsed.name).is_some()
                || self.custom.iter().any(|other| other.name == parsed.name)
            {
                problems.push(format!(
                    "{label}: a theme called \"{}\" already exists",
                    parsed.name
                ));
                continue;
            }
            match CustomTheme::from_file(parsed) {
                Ok((theme, colour_problems)) => {
                    problems.extend(
                        colour_problems
                            .into_iter()
                            .map(|problem| format!("{label}: {problem}")),
                    );
                    self.custom.push(theme);
                }
                Err(problem) => problems.push(format!("{label}: {problem}")),
            }
        }
        self.names = THEMES
            .iter()
            .map(|theme| theme.name.to_owned())
            .chain(self.custom.iter().map(|theme| theme.name.clone()))
            .collect();
        let current = std::mem::take(&mut self.current);
        self.set_theme(&current);
        problems
    }
}

/// Applies the palette to an ImGui style (colours, rounding, paddings), with every size multiplied by `scale`
/// (the DPI scale times the zoom): the C++ `apply_imgui_style()` followed by `ScaleAllSizes(scale)`.
///
/// It starts from ImGui's default style each time, so it can be applied again (another theme, another zoom)
/// without the sizes it leaves alone being scaled twice. The font scales of the style are kept.
pub fn apply_style(p: &Palette, style: &mut sys::ImGuiStyle, scale: f32) {
    let fonts = (
        style.FontSizeBase,
        style.FontScaleMain,
        style.FontScaleDpi,
        style._NextFrameFontSizeBase,
    );
    // SAFETY: the constructor returns a heap-allocated, fully initialised default style, read (it is a plain
    // `Copy` struct) and freed again before anything else can see it.
    unsafe {
        let defaults = sys::ImGuiStyle_ImGuiStyle();
        if !defaults.is_null() {
            *style = *defaults;
            sys::ImGuiStyle_destroy(defaults);
        }
    }
    (
        style.FontSizeBase,
        style.FontScaleMain,
        style.FontScaleDpi,
        style._NextFrameFontSizeBase,
    ) = fonts;

    let vec2 = |x: f32, y: f32| sys::ImVec2 { x, y };
    style.WindowPadding = vec2(0.0, 0.0);
    style.WindowRounding = 0.0;
    style.WindowBorderSize = 0.0;
    style.ChildRounding = 0.0;
    style.ChildBorderSize = 0.0;
    style.FramePadding = vec2(8.0, 5.0);
    style.FrameRounding = 3.0; // --input-radius
    style.FrameBorderSize = 0.0;
    style.ItemSpacing = vec2(6.0, 4.0);
    style.ItemInnerSpacing = vec2(4.0, 4.0);
    style.ScrollbarSize = 10.0;
    style.ScrollbarRounding = 4.0;
    style.GrabRounding = 3.0;
    style.PopupRounding = 4.0;
    style.PopupBorderSize = 1.0;
    style.TabRounding = 0.0;
    style.WindowTitleAlign = vec2(0.5, 0.5);
    style.SelectableTextAlign = vec2(0.0, 0.5);
    style.CellPadding = vec2(6.0, 3.0);
    // (The C++ also turns `AntiAliasedLinesUseTex` off for its nearest-sampled atlas; Dear ImGui 1.93 draws
    // no lines from the atlas any more and has no such switch.)

    let none = Color::TRANSPARENT;
    let frame_hovered = if p.light {
        Color::rgba(0xFFFFFF, 0.95)
    } else {
        Color::rgba(0x000000, 0.30)
    };
    let frame_active = if p.light {
        Color::rgba(0xFFFFFF, 1.0)
    } else {
        Color::rgba(0x000000, 0.35)
    };
    let colors = [
        (sys::ImGuiCol_Text, p.text_normal),
        (sys::ImGuiCol_TextDisabled, p.text_disabled),
        (sys::ImGuiCol_WindowBg, p.app_bg0),
        (sys::ImGuiCol_ChildBg, none),
        (sys::ImGuiCol_PopupBg, p.panel_bg1),
        (sys::ImGuiCol_Border, p.panel_border),
        (sys::ImGuiCol_BorderShadow, none),
        (sys::ImGuiCol_FrameBg, p.input_bg),
        (sys::ImGuiCol_FrameBgHovered, frame_hovered),
        (sys::ImGuiCol_FrameBgActive, frame_active),
        (sys::ImGuiCol_TitleBg, p.panel_bg0),
        (sys::ImGuiCol_TitleBgActive, p.panel_bg0),
        (sys::ImGuiCol_TitleBgCollapsed, p.panel_bg0),
        (sys::ImGuiCol_MenuBarBg, p.toolbar_bg0),
        (sys::ImGuiCol_ScrollbarBg, none),
        (sys::ImGuiCol_ScrollbarGrab, p.scroll_thumb),
        (
            sys::ImGuiCol_ScrollbarGrabHovered,
            p.ink(if p.light { 0.32 } else { 0.25 }),
        ),
        (
            sys::ImGuiCol_ScrollbarGrabActive,
            p.ink(if p.light { 0.42 } else { 0.35 }),
        ),
        (sys::ImGuiCol_CheckMark, p.text_inverse),
        // New in Dear ImGui 1.93 (a checked box's own background); 1.92 used FrameBg for both states.
        (sys::ImGuiCol_CheckboxSelectedBg, p.input_bg),
        (sys::ImGuiCol_SliderGrab, p.accent),
        (sys::ImGuiCol_SliderGrabActive, p.accent),
        (sys::ImGuiCol_Button, none),
        (sys::ImGuiCol_ButtonHovered, p.hover_row),
        (sys::ImGuiCol_ButtonActive, p.selected_row),
        (sys::ImGuiCol_Header, p.selected_row),
        (sys::ImGuiCol_HeaderHovered, p.hover_row),
        (sys::ImGuiCol_HeaderActive, p.selected_row),
        (sys::ImGuiCol_Separator, p.section_border),
        (sys::ImGuiCol_SeparatorHovered, p.accent),
        (sys::ImGuiCol_SeparatorActive, p.accent),
        (sys::ImGuiCol_ResizeGrip, none),
        (sys::ImGuiCol_ResizeGripHovered, p.hover_row),
        (sys::ImGuiCol_ResizeGripActive, p.selected_row),
        (sys::ImGuiCol_Tab, none),
        (sys::ImGuiCol_TabHovered, p.hover_row),
        (sys::ImGuiCol_TabSelected, p.app_bg0),
        (sys::ImGuiCol_TextSelectedBg, p.accent.with_alpha(0.35)),
        (sys::ImGuiCol_NavCursor, none),
        (sys::ImGuiCol_ModalWindowDimBg, p.modal_overlay),
    ];
    for (index, color) in colors {
        style.Colors[index as usize] = color.into();
    }

    // SAFETY: `style` is a valid style; the call only multiplies its size fields.
    unsafe { sys::ImGuiStyle_ScaleAllSizes(style, scale) };
}

/// [`apply_style`] on the style of the ImGui context current on this thread: the one a frame is being built
/// for, or the one just created. Does nothing without a context.
pub fn apply_imgui_style(p: &Palette, scale: f32) {
    // SAFETY: the style lives in the current context, which is only used from this thread; no other reference
    // to it is held across this call (the safe API hands out none that outlive a statement here).
    unsafe {
        if sys::igGetCurrentContext().is_null() {
            return;
        }
        if let Some(style) = sys::igGetStyle().as_mut() {
            apply_style(p, style, scale);
        }
    }
}

// Layout constants of the graph, in logical pixels.
pub const TABS_BAR_HEIGHT: f32 = 34.0;
pub const TOOLBAR_HEIGHT: f32 = 48.0;
pub const GRAPH_HEADER_HEIGHT: f32 = 22.0;
/// `HEADER_ROW_MARGIN_BOTTOM`.
pub const GRAPH_HEADER_MARGIN_BOTTOM: f32 = 2.0;
/// `LeftPanelToGraphMarginGap`.
pub const GRAPH_LEFT_MARGIN: f32 = 10.0;
pub const GRAPH_REF_ZONE_DEFAULT_WIDTH: f32 = 129.0;
pub const GRAPH_ROW_HEIGHT: f32 = 28.0;
pub const GRAPH_ROW_INNER_HEIGHT: f32 = 22.0;
/// Top and bottom.
pub const COMMIT_ZONE_MARGIN: f32 = 3.0;
pub const COMMIT_COLUMN_WIDTH: f32 = 22.0;
pub const COMMIT_ZONE_GUTTER_WIDTH: f32 = 28.0;
/// `COMMIT_ZONE_PADDING_LEFT` / `_RIGHT`.
pub const COMMIT_ZONE_PADDING: f32 = 3.0;
pub const COMMIT_ZONE_LINE_WIDTH: f32 = 2.0;
pub const COMMIT_NODE_DIAMETER: f32 = 22.0;
pub const COMMIT_MERGE_NODE_DIAMETER: f32 = 12.0;
pub const COMMIT_ZONE_EDGE_ARC_RADIUS: f32 = 11.0;
/// The ref node's height (`GRAPH_ROW_INNER_HEIGHT`).
pub const REF_ZONE_TEXT_HEIGHT: f32 = 22.0;
pub const AVATAR_DIAMETER: f32 = 22.0;
/// The node diameter less twice the line width.
pub const COMMIT_ZONE_AVATAR_DIAMETER: f32 = 18.0;
/// Including its 1px border.
pub const LEFT_PANEL_DEFAULT_WIDTH: f32 = 208.0;
/// Including its 1px border.
pub const DETAIL_PANEL_DEFAULT_WIDTH: f32 = 438.0;
pub const LEFT_PANEL_ROW_HEIGHT: f32 = 24.0;
/// `--left-panel-header-height`.
pub const LEFT_PANEL_HEADER_HEIGHT: f32 = 60.0;
/// `--info-bar-height` (the status bar).
pub const INFO_BAR_HEIGHT: f32 = 26.0;
/// The native Windows menu bar's height, which the app draws itself (not on macOS).
pub const MENU_BAR_HEIGHT: f32 = 26.0;
pub const TERMINAL_DEFAULT_HEIGHT: f32 = 275.0;

/// Graph rows, left panel.
pub const FONT_SIZE_ROW: f32 = 12.0;
/// `--fs-3`, the UI's body text.
pub const FONT_SIZE_NORMAL: f32 = 14.0;
/// `--fs-2`.
pub const FONT_SIZE_SMALL: f32 = 12.0;
/// Section headers.
pub const FONT_SIZE_TINY: f32 = 11.0;
/// `--fs-4`.
pub const FONT_SIZE_LARGE: f32 = 16.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Color, b: Color) -> bool {
        let near = |x: f32, y: f32| (x - y).abs() < 1.0 / 512.0;
        near(a.r, b.r) && near(a.g, b.g) && near(a.b, b.b) && near(a.a, b.a)
    }

    #[test]
    fn colors_convert() {
        let orange = Color::rgb(0xF28C28);
        assert_eq!(orange.to_hex(), 0xF28C28);
        // ImU32 is ABGR.
        assert_eq!(orange.to_u32(), 0xFF28_8CF2);
        assert_eq!(Color::from_u32(0xFF28_8CF2), orange);
        assert_eq!(Color::from_rgba8(0xF2, 0x8C, 0x28, 0xFF), orange);
        assert_eq!(
            <[f32; 4]>::from(orange),
            [orange.r, orange.g, orange.b, 1.0]
        );
        assert_eq!(Color::from(orange.to_array()), orange);
        // Rounded, not truncated, and saturated: 0.5 * 255 + 0.5 = 128.
        assert_eq!(Color::new(0.5, 2.0, -1.0, 0.5).to_u32(), 0x8000_FF80);
        assert_eq!(Color::rgba(0x102030, 0.25).with_alpha(0.5).a, 0.5);
        assert_eq!(Color::WHITE.faded(0.25).a, 0.25);
        assert_eq!(
            Color::WHITE.mix(Color::BLACK, 0.25),
            Color::new(0.25, 0.25, 0.25, 1.0)
        );
        // Every 8-bit value survives the trip through floats.
        for value in 0..=255u8 {
            let packed = Color::from_rgba8(value, value, value, value).to_u32();
            assert_eq!(packed, u32::from_ne_bytes([value; 4]));
        }
    }

    #[test]
    fn hsl_round_trips() {
        for hex in [0x1E1C1A, 0xF6F5F3, 0xF28C28, 0x3A9DE8, 0x000000, 0xFFFFFF] {
            let color = Color::rgb(hex);
            assert!(close(from_hsl(to_hsl(color), 1.0), color), "{hex:06X}");
        }
        // LESS lighten(#1E1C1A, 6%) = #2e2b28.
        assert_eq!(lighten(Color::rgb(0x1E1C1A), 0.06).to_hex(), 0x2E2B28);
        assert_eq!(lighten(Color::WHITE, 0.5), Color::WHITE);
    }

    #[test]
    fn dark_palette() {
        let p = Palette::default();
        assert!(!p.light);
        assert_eq!(p.ink, Color::WHITE);
        assert_eq!(p.app_bg0, Color::rgb(0x1E1C1A));
        assert_eq!(p.accent, Color::rgb(0xF28C28));
        assert_eq!(p.red, Color::rgb(0xE5534B));
        assert_eq!(p.text_normal, Color::WHITE.with_alpha(0.75));
        assert_eq!(p.text_dimmed, Color::WHITE.with_alpha(0.75 * 0.20));
        assert_eq!(p.input_bg, Color::rgba(0x000000, 0.20));
        assert_eq!(p.graph_colors[0], Color::rgb(0xF28C28));
        assert_eq!(p.graph_color(10), p.graph_colors[0]);
        assert_eq!(p.graph_color(-1), p.graph_colors[9]);
        assert_eq!(p.syntax_keyword, Color::rgb(0x569CD6));
        assert_eq!(p.syntax_color(Token::Text), None);
        assert_eq!(p.syntax_color(Token::Comment), Some(Color::rgb(0x6A9955)));
        // Surfaces get lighter step by step.
        let lightness = |c: Color| to_hsl(c).l;
        assert!(lightness(p.toolbar_bg0) > lightness(p.app_bg0));
        assert!(lightness(p.toolbar_bg1) > lightness(p.toolbar_bg0));
        assert!(lightness(p.panel_bg2) > lightness(p.panel_bg1));
        assert!(lightness(p.terminal_bg) < lightness(p.app_bg0));
        assert_eq!(p.ink(0.5), Color::WHITE.with_alpha(0.5));
        assert_eq!(p.ink_255(255), Color::WHITE);
        assert_eq!(Ink(0.5).of(&p), p.ink(0.5));
        assert_eq!(Ink::alpha_255(51).of(&p), p.ink(0.2));
    }

    #[test]
    fn light_palette_is_derived_from_its_background() {
        let mut themes = Themes::new();
        themes.set_theme(LIGHT_THEME);
        let p = *themes.palette();
        assert!(p.light);
        assert_eq!(themes.current_theme(), LIGHT_THEME);
        assert_eq!(p.ink, Color::rgb(0x1B1A19));
        assert_eq!(p.white, Color::WHITE);
        // Darker semantic colours, dark text, Monaco's light token colours.
        assert_eq!(p.red, Color::rgb(0xCF3A32));
        assert_eq!(p.green, Color::rgb(0x2E9A4B));
        assert_eq!(p.text_normal, p.ink.with_alpha(0.85));
        assert_eq!(p.text_secondary, p.ink.with_alpha(0.65));
        assert_eq!(p.syntax_keyword, Color::rgb(0x0000FF));
        assert_eq!(p.input_bg, Color::rgba(0xFFFFFF, 0.75));
        assert_eq!(p.panel_border, Color::rgba(0x000000, 0.12));
        assert_eq!(p.deleted_line, p.red.with_alpha(0.20));
        // The chrome is darker than the content, popups lighter; the lanes a step darker than the dark theme's.
        let lightness = |c: Color| to_hsl(c).l;
        assert!(lightness(p.toolbar_bg0) < lightness(p.toolbar_bg1));
        assert!(lightness(p.toolbar_bg1) < lightness(p.app_bg0));
        assert!(lightness(p.panel_bg1) > lightness(p.app_bg0));
        let dark = Palette::default();
        for (light_lane, dark_lane) in p.graph_colors.iter().zip(dark.graph_colors) {
            assert!((lightness(*light_lane) - (lightness(dark_lane) - 0.08)).abs() < 1e-3);
        }
    }

    #[test]
    fn themes_by_name() {
        let mut themes = Themes::new();
        assert_eq!(themes.theme_names().len(), 7);
        assert_eq!(themes.theme_names()[0], DEFAULT_THEME);
        assert!(Themes::builtin_names().eq(themes.theme_names().iter().map(String::as_str)));

        themes.set_theme("Ocean");
        assert_eq!(themes.current_theme(), "Ocean");
        // The first lane is the accent's.
        assert_eq!(themes.palette().graph_colors[0], Color::rgb(0x2BA6E0));

        themes.set_theme("Claude Code");
        assert_eq!(themes.palette().panel_bg0, Color::rgb(0x101010));
        assert_eq!(themes.palette().text_normal, Color::rgb(0xC2C3C0));
        assert_eq!(themes.palette().blue, Color::rgb(0x2776D0));

        themes.set_theme("No Such Theme");
        assert_eq!(themes.current_theme(), DEFAULT_THEME);
        assert_eq!(*themes.palette(), Palette::default());
    }

    #[test]
    fn named_colors() {
        let mut p = Palette::default();
        assert_eq!(NAMED_COLORS.len(), 63);
        for name in NAMED_COLORS {
            assert!(p.named_color_mut(name).is_some(), "{name}");
        }
        *p.named_color_mut("graph_3").expect("a lane") = Color::BLACK;
        assert_eq!(p.graph_colors[3], Color::BLACK);
        *p.named_color_mut("text_normal").expect("a colour") = Color::WHITE;
        assert_eq!(p.text_normal, Color::WHITE);
        for name in ["graph_10", "graph_", "graph_x", "light", "white", "", "Red"] {
            assert!(p.named_color_mut(name).is_none(), "{name}");
        }
    }

    #[test]
    fn custom_themes_load_from_a_folder() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let write = |name: &str, text: &str| {
            fs::write(folder.path().join(name), text).expect("writable");
        };
        write(
            "a-paper.json",
            r##"{"name": "Paper", "base": "ImGit Light", "accent": "#2F7D6D",
                "colors": {"text_normal": "#102030", "graph_2": "#FF0000", "nonsense": "#000"}}"##,
        );
        write(
            "b-night.json",
            r##"{"name": "Night", "base": "Claude Code", "background": "#000010"}"##,
        );
        write("c-duplicate.json", r#"{"name": "Paper"}"#);
        write("d-builtin.json", r#"{"name": "Ocean"}"#);
        write("e-base.json", r#"{"name": "Lost", "base": "Nowhere"}"#);
        write("f-broken.json", "{");
        write("ignored.txt", r#"{"name": "Ignored"}"#);

        let mut themes = Themes::new();
        themes.set_theme("Rose");
        let problems = themes.load_custom_themes(folder.path());
        assert_eq!(themes.theme_names().len(), 9);
        assert_eq!(themes.theme_names()[7..], ["Paper", "Night"]);
        assert_eq!(themes.current_theme(), "Rose");
        assert_eq!(problems.len(), 5, "{problems:?}");
        assert_eq!(
            problems[0],
            "a-paper.json: no palette colour called \"nonsense\""
        );
        assert_eq!(
            problems[1],
            "c-duplicate.json: a theme called \"Paper\" already exists"
        );
        assert_eq!(
            problems[2],
            "d-builtin.json: a theme called \"Ocean\" already exists"
        );
        assert_eq!(
            problems[3],
            "e-base.json: no built-in theme called \"Nowhere\""
        );
        assert!(problems[4].starts_with("f-broken.json: "));

        themes.set_theme("Paper");
        let p = *themes.palette();
        assert!(p.light);
        assert_eq!(p.accent, Color::rgb(0x2F7D6D));
        assert_eq!(p.app_bg0, Color::rgb(0xF6F5F3));
        assert_eq!(p.text_normal, Color::rgb(0x102030));
        assert_eq!(p.graph_colors[2], Color::rgb(0xFF0000));

        // Its own background: the base's exact greys are derived again.
        themes.set_theme("Night");
        let p = *themes.palette();
        assert_eq!(p.app_bg0, Color::rgb(0x000010));
        assert_ne!(p.panel_bg0, Color::rgb(0x101010));
        assert_eq!(p.accent, Color::rgb(0xD97757));

        // Reloading keeps a custom theme current; when its file is gone, the default takes over.
        assert!(themes.load_custom_themes(folder.path()).len() == 5);
        assert_eq!(themes.current_theme(), "Night");
        fs::remove_file(folder.path().join("b-night.json")).expect("removable");
        themes.load_custom_themes(folder.path());
        assert_eq!(themes.current_theme(), DEFAULT_THEME);
        assert!(
            themes
                .load_custom_themes(Path::new("no/such/folder"))
                .is_empty()
        );
        assert_eq!(themes.theme_names().len(), 7);
    }

    #[test]
    fn style_follows_the_palette() {
        let _context = dear_imgui_rs::Context::create();
        let p = Palette::default();
        apply_imgui_style(&p, 2.0);
        apply_imgui_style(&p, 2.0); // applying twice does not scale twice
        // SAFETY: the context created above is current; the style is read once.
        let style = unsafe { *sys::igGetStyle() };
        assert_eq!(style.FramePadding, sys::ImVec2 { x: 16.0, y: 10.0 });
        assert_eq!(style.ScrollbarSize, 20.0);
        assert_eq!(style.WindowPadding, sys::ImVec2 { x: 0.0, y: 0.0 });
        assert_eq!(
            Color::from(style.Colors[sys::ImGuiCol_WindowBg as usize]),
            p.app_bg0
        );
        assert_eq!(
            Color::from(style.Colors[sys::ImGuiCol_Button as usize]),
            Color::TRANSPARENT
        );
    }
}
