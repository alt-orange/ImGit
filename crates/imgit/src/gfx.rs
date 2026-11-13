//! Drawing: the renderer for ImGui's draw data, the text rasterisers and image decoding.

use std::collections::HashMap;
use std::rc::Rc;

use dear_imgui_glow::{GlowRenderer, RendererTextureId};
use dear_imgui_rs::{Context, TextureFormat, TextureId};

pub mod gamma_font_loader;
pub mod image;
#[cfg(feature = "lcd-text")]
pub mod lcd_renderer;
pub mod native_font_loader;

/// `ImFontAtlasRectId_Invalid` of `imgui.h` (a macro, so absent from the bindings): no rectangle was packed.
const FONT_ATLAS_RECT_ID_INVALID: dear_imgui_rs::sys::ImFontAtlasRectId = -1;

/// What can go wrong between ImGui and OpenGL.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An OpenGL object could not be created, or the context is too old.
    #[error("OpenGL: {0}")]
    Gl(String),
    #[error("a {width}x{height} RGBA texture needs {} bytes, got {bytes}", u64::from(*.width) * u64::from(*.height) * 4)]
    InvalidTexture {
        width: u32,
        height: u32,
        bytes: usize,
    },
    #[error("the renderer is shut down")]
    RendererShutDown,
    #[error(transparent)]
    Consumer(#[from] dear_imgui_rs::render::RendererConsumerError),
    #[error(transparent)]
    Snapshot(#[from] dear_imgui_rs::render::SnapshotError),
    #[error(transparent)]
    TextureFeedback(#[from] dear_imgui_rs::render::TextureFeedbackError),
    #[error(transparent)]
    GlowInit(#[from] dear_imgui_glow::InitError),
    #[error(transparent)]
    GlowRender(#[from] dear_imgui_glow::RenderError),
}

/// Creates and destroys the UI's own textures (avatars, icons, images): the C++ `ui::set_texture_backend`.
///
/// A panel gets one through [`ShellContext::textures`](crate::shell::ShellContext::textures); the returned
/// [`TextureId`] is what `Ui::image` and the draw list's `add_image*` calls take.
pub trait Textures {
    /// Uploads `width * height` straight-alpha RGBA8 texels (rows top to bottom) as a linear-filtered,
    /// edge-clamped texture.
    fn create_texture(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<TextureId, Error>;
    /// Releases a texture made by [`Self::create_texture`]. It must no longer be referenced by this frame's
    /// draw lists.
    fn destroy_texture(&mut self, texture: TextureId);
}

/// The renderer in use: the app's own where the context allows it, dear-imgui-glow's otherwise.
pub enum Renderer {
    /// Sub-pixel capable, nearest-sampled font atlas (see [`lcd_renderer`]).
    #[cfg(feature = "lcd-text")]
    Lcd(Box<lcd_renderer::LcdRenderer>),
    /// The stock renderer: grey-scale text, bilinear-sampled atlas.
    Glow {
        renderer: Box<GlowRenderer>,
        /// The typed handles behind the ids given to the UI.
        textures: HashMap<TextureId, RendererTextureId>,
    },
}

impl Renderer {
    /// Attaches a renderer to `imgui`. With `prefer_lcd` the app's own renderer is tried first, falling back
    /// to dear-imgui-glow when the context cannot run it (as the C++ app falls back to `imgui_impl_opengl3`).
    /// The GL context behind `gl` must be current now and for every later call.
    pub fn new(
        gl: Rc<glow::Context>,
        imgui: &mut Context,
        prefer_lcd: bool,
    ) -> Result<Self, Error> {
        #[cfg(feature = "lcd-text")]
        if prefer_lcd {
            match lcd_renderer::LcdRenderer::new(Rc::clone(&gl), imgui) {
                Ok(renderer) => return Ok(Self::Lcd(Box::new(renderer))),
                Err(error) => {
                    eprintln!("imgit: no sub-pixel text renderer ({error}); using dear-imgui-glow")
                }
            }
        }
        #[cfg(not(feature = "lcd-text"))]
        let _ = prefer_lcd;
        let renderer = GlowRenderer::with_shared_context(gl, imgui)?;
        Ok(Self::Glow {
            renderer: Box::new(renderer),
            textures: HashMap::new(),
        })
    }

    /// Whether this renderer can draw a sub-pixel (per-channel coverage) font atlas.
    #[must_use]
    pub fn supports_lcd_text(&self) -> bool {
        match self {
            #[cfg(feature = "lcd-text")]
            Self::Lcd(_) => true,
            Self::Glow { .. } => false,
        }
    }

    /// Tells the renderer whether the font atlas holds sub-pixel coverage (see
    /// [`lcd_renderer::LcdRenderer::set_subpixel_atlas`]).
    pub fn set_subpixel_atlas(&mut self, subpixel: bool) {
        match self {
            #[cfg(feature = "lcd-text")]
            Self::Lcd(renderer) => renderer.set_subpixel_atlas(subpixel),
            Self::Glow { .. } => {
                debug_assert!(!subpixel, "dear-imgui-glow cannot draw a sub-pixel atlas")
            }
        }
    }

    /// Extra coverage exponent for grey-scale text in a sub-pixel atlas.
    pub fn set_grayscale_gamma(&mut self, gamma: f32) {
        match self {
            #[cfg(feature = "lcd-text")]
            Self::Lcd(renderer) => renderer.set_grayscale_gamma(gamma),
            Self::Glow { .. } => {
                let _ = gamma;
            }
        }
    }

    /// Closes the frame `imgui` has open (`ImGui::Render`), brings the textures ImGui manages up to date and
    /// draws into the bound framebuffer.
    pub fn render(&mut self, imgui: &mut Context) -> Result<(), Error> {
        match self {
            #[cfg(feature = "lcd-text")]
            Self::Lcd(renderer) => renderer.render(imgui),
            Self::Glow { renderer, .. } => {
                let frame = imgui.try_render(renderer.renderer_consumer()?)?;
                Ok(renderer.render(frame)?)
            }
        }
    }

    /// Releases the GL objects and detaches from `imgui`, while the GL context is still current.
    pub fn shutdown(&mut self, imgui: &mut Context) -> Result<(), Error> {
        match self {
            #[cfg(feature = "lcd-text")]
            Self::Lcd(renderer) => renderer.shutdown(imgui),
            Self::Glow { renderer, textures } => {
                textures.clear();
                Ok(renderer.shutdown(imgui)?)
            }
        }
    }
}

impl Textures for Renderer {
    fn create_texture(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<TextureId, Error> {
        match self {
            #[cfg(feature = "lcd-text")]
            Self::Lcd(renderer) => renderer.create_texture(width, height, rgba),
            Self::Glow { renderer, textures } => {
                if rgba.len() != width as usize * height as usize * 4 || width == 0 || height == 0 {
                    return Err(Error::InvalidTexture {
                        width,
                        height,
                        bytes: rgba.len(),
                    });
                }
                let handle =
                    renderer.register_texture(width, height, TextureFormat::RGBA32, rgba)?;
                textures.insert(handle.texture_id(), handle);
                Ok(handle.texture_id())
            }
        }
    }

    fn destroy_texture(&mut self, texture: TextureId) {
        match self {
            #[cfg(feature = "lcd-text")]
            Self::Lcd(renderer) => renderer.destroy_texture(texture),
            Self::Glow { renderer, textures } => {
                if let Some(handle) = textures.remove(&texture) {
                    // Only fails when the handle is unknown or the renderer already shut down.
                    let _ = renderer.unregister_texture(handle);
                }
            }
        }
    }
}
