//! The icons: Font Awesome Free SVG paths (`assets/icons/fa_icons.tsv`), rasterised on demand into small alpha
//! textures (one per icon and pixel size) and drawn tinted, like an inline `<svg>` with `fill: currentColor`.

use std::collections::HashMap;

use dear_imgui_rs::TextureId;

use crate::assets;
use crate::gfx::Textures;
use crate::theme::Color;
use crate::ui::Cx;
use crate::ui::draw::DrawList;
use crate::ui::geom::{Vec2, vec2};

pub mod raster;
mod vector;

use raster::{Placement, rasterize_paths};

/// Small icons for the toolbar and panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Pull,
    Push,
    Branch,
    Stash,
    Pop,
    Tag,
    Remote,
    Folder,
    Laptop,
    Close,
    Plus,
    Minus,
    Back,
    Refresh,
    ChevronDown,
    ChevronRight,
    ChevronLeft,
    ChevronUp,
    Check,
    Warning,
    Copy,
    Undo,
    Redo,
    Terminal,
    Search,
    Gear,
    Bell,
    Rocket,
    Trash,
    Eye,
    EyeOff,
    Cloud,
    Pencil,
    Sort,
    Expand,
    Collapse,
    List,
    Commit,
    Headphones,
    Plug,
    User,
    ArrowRight,
    Worktree,
    Submodule,
    Patch,
    Issue,
    Team,
    Lfs,
    Spinner,
    External,
    File,
    Magic,
    Robot,
    Keyboard,
    Globe,
    SearchPlus,
    CheckedOut,
    CaretDown,
    Sparkles,
    Bars,
    FolderTree,
    Gift,
}

impl Icon {
    /// Every icon, in declaration order.
    pub const ALL: [Self; 62] = [
        Self::Pull,
        Self::Push,
        Self::Branch,
        Self::Stash,
        Self::Pop,
        Self::Tag,
        Self::Remote,
        Self::Folder,
        Self::Laptop,
        Self::Close,
        Self::Plus,
        Self::Minus,
        Self::Back,
        Self::Refresh,
        Self::ChevronDown,
        Self::ChevronRight,
        Self::ChevronLeft,
        Self::ChevronUp,
        Self::Check,
        Self::Warning,
        Self::Copy,
        Self::Undo,
        Self::Redo,
        Self::Terminal,
        Self::Search,
        Self::Gear,
        Self::Bell,
        Self::Rocket,
        Self::Trash,
        Self::Eye,
        Self::EyeOff,
        Self::Cloud,
        Self::Pencil,
        Self::Sort,
        Self::Expand,
        Self::Collapse,
        Self::List,
        Self::Commit,
        Self::Headphones,
        Self::Plug,
        Self::User,
        Self::ArrowRight,
        Self::Worktree,
        Self::Submodule,
        Self::Patch,
        Self::Issue,
        Self::Team,
        Self::Lfs,
        Self::Spinner,
        Self::External,
        Self::File,
        Self::Magic,
        Self::Robot,
        Self::Keyboard,
        Self::Globe,
        Self::SearchPlus,
        Self::CheckedOut,
        Self::CaretDown,
        Self::Sparkles,
        Self::Bars,
        Self::FolderTree,
        Self::Gift,
    ];

    /// The Font Awesome icon drawn for it ("far:undo-alt", "fas:flask"...); `None` for the ones that are only
    /// line-drawn.
    #[must_use]
    pub const fn fa_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Pull => "far:arrow-to-bottom",
            Self::Push => "far:arrow-from-bottom",
            Self::Branch => "far:code-branch",
            Self::Stash => "far:inbox-in",
            Self::Pop => "far:inbox-out",
            Self::Tag => "far:tag",
            Self::Remote | Self::Cloud => "far:cloud",
            Self::Folder => "far:folder",
            Self::Laptop => "far:laptop",
            Self::Close => "far:times",
            Self::Plus => "far:plus",
            Self::Minus => "far:minus",
            Self::Back => "fas:chevron-circle-left",
            Self::Refresh => "far:sync",
            Self::ChevronDown => "far:chevron-down",
            Self::ChevronRight => "far:chevron-right",
            Self::ChevronLeft => "far:chevron-left",
            Self::ChevronUp => "far:chevron-up",
            Self::Check => "far:check",
            Self::Warning => "far:exclamation-triangle",
            Self::Copy => "far:copy",
            Self::Undo => "far:undo-alt",
            Self::Redo => "far:redo-alt",
            Self::Terminal => "far:terminal",
            Self::Search => "far:search",
            Self::Gear => "far:cog",
            Self::Bell => "far:bell",
            Self::Rocket => "far:rocket",
            Self::Trash => "far:trash-alt",
            Self::Eye => "far:eye",
            Self::EyeOff => "far:eye-slash",
            Self::Pencil => "far:pencil",
            Self::Sort => "far:sort-alpha-up",
            Self::List => "far:list",
            Self::Commit => "far:code-commit",
            Self::Plug => "fas:plug",
            Self::User => "far:user",
            Self::ArrowRight => "far:long-arrow-right",
            Self::Worktree => "far:folder-tree",
            Self::Submodule => "far:layer-group",
            Self::Patch => "fas:flask",
            Self::Issue => "fas:tasks",
            Self::Team => "fas:users",
            Self::Spinner => "far:circle-notch",
            Self::External => "far:external-link-alt",
            Self::File => "far:file",
            Self::Magic => "fal:magic",
            Self::Robot => "fas:robot",
            Self::Keyboard => "far:keyboard",
            Self::Globe => "far:globe",
            Self::SearchPlus => "far:search-plus",
            Self::CheckedOut => "fas:check",
            Self::CaretDown => "fas:caret-down",
            Self::Sparkles => "far:sparkles",
            Self::Bars => "far:bars",
            Self::FolderTree => "fas:folder-tree",
            Self::Gift => "far:gift",
            Self::Expand | Self::Collapse | Self::Headphones | Self::Lfs => return None,
        })
    }
}

/// A Font Awesome icon as ImGit embeds it: its viewBox, its path and, for a duotone icon, the secondary path
/// drawn at 40% opacity. The strings point into the embedded asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaIcon {
    pub name: &'static str,
    pub width: i32,
    pub height: i32,
    pub path: &'static str,
    pub secondary: &'static str,
}

/// The icons of a table: one per line, `name, width, height, path, secondary path` separated by tabs; `#`
/// starts a comment. A number that does not parse reads as 0 (C's `atoi`).
#[must_use]
pub fn parse_fa_icons(text: &'static str) -> Vec<FaIcon> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut fields = line.split('\t');
            let mut field = || fields.next().unwrap_or_default();
            let name = field();
            let number = |text: &str| text.trim().parse().unwrap_or(0);
            FaIcon {
                name,
                width: number(field()),
                height: number(field()),
                path: field(),
                secondary: field(),
            }
        })
        .collect()
}

/// The path data (the first `<path>`'s `d`) of `assets/svg/<name>.svg`; `None` when there is none.
#[must_use]
pub fn svg_path(name: &str) -> Option<&'static str> {
    let svg = assets::text(&format!("svg/{name}.svg"))?;
    let begin = svg.find(" d=\"")? + 4;
    let rest = &svg[begin..];
    Some(&rest[..rest.find('"').unwrap_or(rest.len())])
}

/// A rasterised icon; `texture` is `None` when it could not be made (an empty raster, a failed upload).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CachedTexture {
    texture: Option<TextureId>,
    width: usize,
    height: usize,
}

/// Which paths of an icon a raster holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    /// A duotone's secondary path at 40% under the primary.
    Both,
    Primary,
    Secondary,
}

/// How many rasters the icon cache holds before the least recently used go: far more than a screen draws (a
/// spinner alone makes one per degree), so normal use never evicts.
const CACHE_CAPACITY: usize = 4096;

/// A cache that keeps at most `capacity` values between frames, dropping the least recently used first. Values
/// are only evicted by [`Self::begin_frame`], never while a frame is being drawn, so one handed out this frame
/// stays valid for the rest of it.
#[derive(Debug)]
struct LruCache<V> {
    /// Each value with the frame that last asked for it.
    entries: HashMap<String, (V, u64)>,
    frame: u64,
    capacity: usize,
}

impl<V: Copy> LruCache<V> {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            frame: 0,
            capacity,
        }
    }

    /// Starts a frame: returns (and forgets) the least recently used values beyond the capacity. None of them
    /// was used in the frame that now begins.
    fn begin_frame(&mut self) -> Vec<V> {
        let excess = self.entries.len().saturating_sub(self.capacity);
        let mut evicted = Vec::with_capacity(excess);
        if excess > 0 {
            let mut by_age: Vec<(u64, &String)> = self
                .entries
                .iter()
                .map(|(key, &(_, used))| (used, key))
                .collect();
            by_age.sort_unstable();
            let oldest: Vec<String> = by_age
                .into_iter()
                .take(excess)
                .map(|(_, key)| key.clone())
                .collect();
            for key in oldest {
                if let Some((value, _)) = self.entries.remove(&key) {
                    evicted.push(value);
                }
            }
        }
        self.frame += 1;
        evicted
    }

    /// The value under `key`, made by `make` when there is none; either way it counts as used this frame.
    fn get_or_insert_with(&mut self, key: String, make: impl FnOnce() -> V) -> V {
        let frame = self.frame;
        let entry = self.entries.entry(key).or_insert_with(|| (make(), frame));
        entry.1 = frame;
        entry.0
    }

    /// Forgets everything, returning the values.
    fn drain(&mut self) -> Vec<V> {
        self.entries.drain().map(|(_, (value, _))| value).collect()
    }
}

/// The Font Awesome table and the textures rasterised from it, in [`UiState`](crate::ui::UiState).
#[derive(Debug)]
pub struct Icons {
    table: Vec<FaIcon>,
    index: HashMap<&'static str, usize>,
    cache: LruCache<CachedTexture>,
}

impl Default for Icons {
    fn default() -> Self {
        Self::new()
    }
}

impl Icons {
    /// The icons of the embedded `icons/fa_icons.tsv`, nothing rasterised yet.
    #[must_use]
    pub fn new() -> Self {
        let table = parse_fa_icons(assets::text("icons/fa_icons.tsv").unwrap_or_default());
        let mut index = HashMap::with_capacity(table.len());
        for (position, icon) in table.iter().enumerate() {
            // The first of a name counts.
            index.entry(icon.name).or_insert(position);
        }
        Self {
            table,
            index,
            cache: LruCache::new(CACHE_CAPACITY),
        }
    }

    /// Starts a frame: frees the rasters beyond the cache's capacity that no recent frame drew. Call before
    /// anything is drawn, so no icon of the frame is freed.
    pub fn begin_frame(&mut self, textures: &mut dyn Textures) {
        for cached in self.cache.begin_frame() {
            if let Some(texture) = cached.texture {
                textures.destroy_texture(texture);
            }
        }
    }

    /// The icon called `name` ("far:undo-alt", "fas:flask"...).
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&FaIcon> {
        self.index.get(name).map(|&position| &self.table[position])
    }

    /// Frees every rasterised icon (they are made again when drawn).
    pub fn clear(&mut self, textures: &mut dyn Textures) {
        for cached in self.cache.drain() {
            if let Some(texture) = cached.texture {
                textures.destroy_texture(texture);
            }
        }
    }

    /// The texture cached under `key`, rasterised by `build` (width, height, RGBA) on first use.
    fn texture(
        &mut self,
        textures: &mut dyn Textures,
        key: String,
        width: usize,
        height: usize,
        build: impl FnOnce() -> Vec<u8>,
    ) -> CachedTexture {
        self.cache.get_or_insert_with(key, || {
            let texture = (width > 0 && height > 0)
                .then(|| {
                    textures
                        .create_texture(width as u32, height as u32, &build())
                        .ok()
                })
                .flatten();
            CachedTexture {
                texture,
                width,
                height,
            }
        })
    }
}

/// Draws a cached raster with its top-left texel at `pos` (whole pixels), tinted by `color`.
fn draw_raster(draw: DrawList<'_>, cached: CachedTexture, pos: Vec2, color: Color, mirrored: bool) {
    let Some(texture) = cached.texture else {
        return;
    };
    // Mirrored: the same raster with its u coordinates swapped (CSS fa-flip-horizontal).
    let (u0, u1) = if mirrored { (1.0, 0.0) } else { (0.0, 1.0) };
    draw.add_image(
        texture,
        pos,
        pos + vec2(cached.width as f32, cached.height as f32),
        vec2(u0, 0.0),
        vec2(u1, 1.0),
        color,
    );
}

impl Cx<'_> {
    /// Whether `name` ("far:undo-alt", "fas:flask", ...) is a known icon.
    #[must_use]
    pub fn has_fa_icon(&self, name: &str) -> bool {
        self.state.icons.find(name).is_some()
    }

    /// The width of the icon drawn at `size` (its em height, the CSS font-size) in device pixels; `size` for
    /// an unknown one.
    #[must_use]
    pub fn fa_icon_width(&self, name: &str, size: f32) -> f32 {
        self.state
            .icons
            .find(name)
            .map_or(size, |icon| size * icon.width as f32 / icon.height as f32)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_icon_layer(
        &mut self,
        draw: DrawList<'_>,
        name: &str,
        pos: Vec2,
        size: f32,
        color: Color,
        layer: Layer,
        mirrored: bool,
    ) {
        let Some(&icon) = self.state.icons.find(name) else {
            return;
        };
        if size <= 0.0 {
            return;
        }
        // Snap the em box to the pixel grid and keep the sub-pixel remainder inside the raster, like Skia does
        // for an inline SVG.
        let width = size * icon.width as f32 / icon.height as f32;
        let origin = pos.floor();
        let fraction = pos - origin;
        let tex_w = (width + fraction.x).ceil() as usize;
        let tex_h = (size + fraction.y).ceil() as usize;
        let quarter_x = (fraction.x * 4.0).round() as i32;
        let quarter_y = (fraction.y * 4.0).round() as i32;
        let suffix = match layer {
            Layer::Both => "@",
            Layer::Primary => "@p",
            Layer::Secondary => "@s",
        };
        let key = format!(
            "{name}{suffix}{}:{quarter_x},{quarter_y}",
            (size * 4.0).round() as i32
        );
        let cached = self
            .state
            .icons
            .texture(self.shell.textures, key, tex_w, tex_h, || {
                let (primary, secondary) = match layer {
                    Layer::Both => (icon.path, icon.secondary),
                    Layer::Primary => (icon.path, ""),
                    Layer::Secondary => (icon.secondary, ""),
                };
                rasterize_paths(
                    primary,
                    secondary,
                    tex_w,
                    tex_h,
                    Placement::new(
                        size / icon.height as f32,
                        vec2(quarter_x as f32 / 4.0, quarter_y as f32 / 4.0),
                    ),
                )
            });
        draw_raster(draw, cached, origin, color, mirrored);
    }

    /// Draws the Font Awesome icon `name` with its em box centred on `center`; `size` is the em height in
    /// device pixels.
    pub fn draw_fa_icon(
        &mut self,
        draw: DrawList<'_>,
        name: &str,
        center: Vec2,
        size: f32,
        color: Color,
    ) {
        let width = self.fa_icon_width(name, size);
        self.draw_fa_icon_at(
            draw,
            name,
            vec2(center.x - width / 2.0, center.y - size / 2.0),
            size,
            color,
        );
    }

    /// [`Self::draw_fa_icon`] turned by `degrees` (clockwise) about its centre, like a CSS
    /// `transform: rotate()`.
    pub fn draw_fa_icon_rotated(
        &mut self,
        draw: DrawList<'_>,
        name: &str,
        center: Vec2,
        size: f32,
        color: Color,
        degrees: f32,
    ) {
        if degrees == 0.0 {
            self.draw_fa_icon(draw, name, center, size, color);
            return;
        }
        let Some(&icon) = self.state.icons.find(name) else {
            return;
        };
        if size <= 0.0 {
            return;
        }
        // A rotated glyph is rasterised into a square big enough for its rotated em box.
        let scale = size / icon.height as f32;
        let diagonal = ((icon.width * icon.width + icon.height * icon.height) as f32).sqrt();
        let side = (diagonal * scale).ceil() as usize + 2;
        let half = side as f32 / 2.0;
        let origin = vec2((center.x - half).floor(), (center.y - half).floor());
        let key = format!(
            "{name}@{}r{}",
            (size * 4.0).round() as i32,
            degrees.round() as i32
        );
        let cached = self
            .state
            .icons
            .texture(self.shell.textures, key, side, side, || {
                let pivot = vec2(icon.width as f32 / 2.0, icon.height as f32 / 2.0);
                rasterize_paths(
                    icon.path,
                    icon.secondary,
                    side,
                    side,
                    Placement {
                        scale,
                        offset: vec2(half - pivot.x * scale, half - pivot.y * scale),
                        pivot,
                        degrees,
                    },
                )
            });
        draw_raster(draw, cached, origin, color, false);
    }

    /// [`Self::draw_fa_icon`] mirrored left to right (CSS fa-flip-horizontal).
    pub fn draw_fa_icon_mirrored(
        &mut self,
        draw: DrawList<'_>,
        name: &str,
        center: Vec2,
        size: f32,
        color: Color,
    ) {
        let width = self.fa_icon_width(name, size);
        self.draw_icon_layer(
            draw,
            name,
            vec2(center.x - width / 2.0, center.y - size / 2.0),
            size,
            color,
            Layer::Both,
            true,
        );
    }

    /// [`Self::draw_fa_icon`] with the em box's top-left corner at `pos`.
    pub fn draw_fa_icon_at(
        &mut self,
        draw: DrawList<'_>,
        name: &str,
        pos: Vec2,
        size: f32,
        color: Color,
    ) {
        self.draw_icon_layer(draw, name, pos, size, color, Layer::Both, false);
    }

    /// A duotone ("fad:...") with each path in its own colour (`--fa-primary-color` /
    /// `--fa-secondary-color`, both at full opacity).
    pub fn draw_fa_duotone(
        &mut self,
        draw: DrawList<'_>,
        name: &str,
        center: Vec2,
        size: f32,
        primary: Color,
        secondary: Color,
    ) {
        let pos = vec2(
            center.x - self.fa_icon_width(name, size) / 2.0,
            center.y - size / 2.0,
        );
        self.draw_icon_layer(draw, name, pos, size, secondary, Layer::Secondary, false);
        self.draw_icon_layer(draw, name, pos, size, primary, Layer::Primary, false);
    }

    /// Rasterises an arbitrary SVG path (a `view_w` x `view_h` viewBox) into a cached texture and draws it
    /// into the rectangle `[p0, p1]`. `key` identifies the path in the cache; a leading `!` on `path` selects
    /// fill-rule evenodd.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_svg_path(
        &mut self,
        draw: DrawList<'_>,
        key: &str,
        path: &str,
        view_w: f32,
        view_h: f32,
        p0: Vec2,
        p1: Vec2,
        color: Color,
    ) {
        let origin = p0.floor();
        let tex_w = (p1.x - origin.x).ceil().max(0.0) as usize;
        let tex_h = (p1.y - origin.y).ceil().max(0.0) as usize;
        let cache_key = format!("{key}#{tex_w}x{tex_h}");
        let cached = self
            .state
            .icons
            .texture(self.shell.textures, cache_key, tex_w, tex_h, || {
                let scale = ((p1.x - p0.x) / view_w).min((p1.y - p0.y) / view_h);
                rasterize_paths(path, "", tex_w, tex_h, Placement::new(scale, p0 - origin))
            });
        draw_raster(draw, cached, origin, color, false);
    }

    /// The width of `icon` drawn at em height `size` (Font Awesome glyphs are not square).
    #[must_use]
    pub fn icon_width(&self, icon: Icon, size: f32) -> f32 {
        match icon.fa_name() {
            Some(name) => self.fa_icon_width(name, size),
            None => size,
        }
    }

    /// Draws the Font Awesome glyph for `icon` centred on `center` (the line-drawn one for an icon without);
    /// `size` is its em height in device pixels.
    pub fn draw_icon(
        &mut self,
        draw: DrawList<'_>,
        icon: Icon,
        center: Vec2,
        size: f32,
        color: Color,
    ) {
        match icon.fa_name().filter(|name| self.has_fa_icon(name)) {
            Some(name) => self.draw_fa_icon(draw, name, center, size, color),
            None => self.draw_icon_vector(draw, icon, center, size, color),
        }
    }

    /// A square button of `size + 10` logical px showing `icon` at `size` logical px in `color`; hovered or
    /// `active` it gets a rounded background and the selected text colour. `tip` is its tooltip.
    pub fn icon_button(
        &mut self,
        id: &str,
        icon: Icon,
        size: f32,
        color: Color,
        tip: Option<&str>,
        active: bool,
    ) -> bool {
        let ui = self.ui;
        let side = self.px(size + 10.0);
        let pos = self.cursor();
        let pressed = ui.invisible_button(id, vec2(side, side));
        let hovered = ui.is_item_hovered();
        let draw = self.window_draw_list();
        let lit = hovered || active;
        if lit {
            draw.add_rect_filled(
                pos,
                pos + vec2(side, side),
                if active {
                    self.palette.selected_row
                } else {
                    self.palette.hover_row
                },
                self.px(3.0),
            );
        }
        let color = if lit {
            self.palette.text_selected
        } else {
            color
        };
        self.draw_icon(
            draw,
            icon,
            pos + vec2(side / 2.0, side / 2.0),
            self.px(size),
            color,
        );
        if let Some(tip) = tip {
            self.tooltip(tip);
        }
        pressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_icon_table() {
        let icons = parse_fa_icons(
            "# a comment\n\nfar:one\t512\t448\tM0 0H1V1Z\t\r\nfad:two\t640\t512\tM0 0\tM1 1\nbad\tx\t\n",
        );
        assert_eq!(icons.len(), 3);
        assert_eq!(
            icons[0],
            FaIcon {
                name: "far:one",
                width: 512,
                height: 448,
                path: "M0 0H1V1Z",
                secondary: "",
            }
        );
        assert_eq!(icons[1].secondary, "M1 1");
        assert_eq!((icons[2].width, icons[2].height, icons[2].path), (0, 0, ""));
    }

    #[test]
    fn every_icon_has_its_glyph_in_the_embedded_table() {
        let icons = Icons::new();
        assert!(icons.table.len() > 60);
        assert_eq!(Icon::ALL.len(), 62);
        for icon in Icon::ALL {
            let Some(name) = icon.fa_name() else {
                assert!(
                    matches!(
                        icon,
                        Icon::Expand | Icon::Collapse | Icon::Headphones | Icon::Lfs
                    ),
                    "{icon:?}"
                );
                continue;
            };
            let glyph = icons
                .find(name)
                .unwrap_or_else(|| panic!("{icon:?}: {name}"));
            assert!(glyph.width > 0 && glyph.height > 0, "{name}");
            assert!(!raster::parse_path(glyph.path).is_empty(), "{name}");
        }
        assert!(icons.find("far:no-such-icon").is_none());
        // `ALL` lists each icon once, in declaration order.
        for (index, icon) in Icon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, index);
        }
    }

    #[test]
    fn the_cache_evicts_the_least_recently_used_between_frames() {
        let mut cache = LruCache::<u32>::new(2);
        assert!(cache.begin_frame().is_empty());
        for (key, value) in [("a", 1), ("b", 2), ("c", 3)] {
            cache.get_or_insert_with(key.to_owned(), || value);
        }
        // Over capacity, but nothing leaves during the frame that used them all.
        assert_eq!(cache.entries.len(), 3);
        // Next frame: one has to go (a tie in age: the smallest key).
        assert_eq!(cache.begin_frame(), [1]);
        assert_eq!(cache.entries.len(), 2);
        // A used entry outlives an older one.
        cache.get_or_insert_with("b".to_owned(), || 0);
        cache.begin_frame();
        cache.get_or_insert_with("d".to_owned(), || 4);
        assert_eq!(cache.begin_frame(), [3]);
    }

    #[test]
    fn an_evicted_entry_asked_for_in_the_same_frame_is_made_again() {
        let mut cache = LruCache::<u32>::new(1);
        cache.begin_frame();
        cache.get_or_insert_with("a".to_owned(), || 1);
        cache.begin_frame();
        cache.get_or_insert_with("b".to_owned(), || 2);
        // "a" was used a frame before "b": it is the one evicted, and asking for it builds a new value.
        let evicted = cache.begin_frame();
        assert_eq!(evicted, [1]);
        assert_eq!(cache.get_or_insert_with("a".to_owned(), || 10), 10);
        // Entries used this frame are never evicted by it.
        assert_eq!(cache.get_or_insert_with("b".to_owned(), || 20), 2);
        assert_eq!(cache.drain().len(), 2);
    }

    #[test]
    fn reads_svg_assets() {
        let path = svg_path("code-branch").expect("embedded");
        assert!(path.starts_with('M'));
        assert!(!raster::parse_path(path).is_empty());
        assert!(svg_path("no-such-file").is_none());
    }
}
