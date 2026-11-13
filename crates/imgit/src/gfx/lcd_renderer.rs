//! The app's own OpenGL 3.3 renderer for ImGui draw data.
//!
//! It renders like `imgui_impl_opengl3`, but blends each colour channel with its own coverage so the ClearType
//! glyphs of [`super::native_font_loader`] keep their sub-pixel sharpness (Chromium's LCD text). Dual-source
//! blending computes `dst = colour * coverage + dst * (1 - coverage)` per channel, which equals ordinary alpha
//! blending for everything that is not sub-pixel text.
//!
//! Unlike the C++ renderer, which left texture creation to the OpenGL3 backend, this one is a complete
//! dear-imgui-rs renderer: it owns the Context's synchronous consumer and answers the managed-texture requests
//! of ImGui 1.92+ (the font atlas) itself. That is what lets it
//!
//! * hand ImGui plain GL texture names as texture ids, so the UI's own textures need no registry, and
//! * sample the font atlas with `GL_NEAREST`. ImGui places glyph quads at fractional x positions; with bilinear
//!   sampling every glyph is smeared over two pixel columns. Nearest sampling lands each glyph on whole pixels,
//!   the way Chromium's DirectWrite text looks.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::num::NonZeroU32;
use std::rc::Rc;

use dear_imgui_rs::internal::RawWrapper as _;
use dear_imgui_rs::render::{
    DrawCmd, DrawData, DrawIdx, DrawVert, PendingFrame, SnapshotTextureId,
    SynchronousRendererConsumer, TextureFeedback, TextureOp, TextureUploadIdentity,
};
use dear_imgui_rs::{BackendFlags, Context, DrawListMut, TextureFormat, TextureId, sys};
use glow::HasContext as _;

use super::Error;

/// Extra coverage exponent for grey-scale text unless overridden: Chromium boosts its A8 text more than its
/// LCD text.
const DEFAULT_GRAYSCALE_GAMMA: f32 = 1.38;

const VERTEX_SOURCE: &str = r"#version 330 core
layout (location = 0) in vec2 Position;
layout (location = 1) in vec2 UV;
layout (location = 2) in vec4 Color;
uniform mat4 ProjMtx;
out vec2 Frag_UV;
out vec4 Frag_Color;
void main()
{
    Frag_UV = UV;
    Frag_Color = Color;
    gl_Position = ProjMtx * vec4(Position.xy, 0, 1);
}
";

// Lcd = 2: the same coverage averaged to grey-scale.
// Lcd = 1: the texture's RGB is per-channel coverage (times A). Dual-source blending applies
// dst = colour * coverage + dst * (1 - coverage) per channel, one triangle after the other.
const FRAGMENT_SOURCE: &str = r"#version 330 core
in vec2 Frag_UV;
in vec4 Frag_Color;
uniform sampler2D Texture;
uniform int Lcd;
uniform float GrayGamma;
layout (location = 0, index = 0) out vec4 Out_Color;
layout (location = 0, index = 1) out vec4 Out_Coverage;
// The atlas row is a 3x horizontal coverage signal (R, G, B = left, middle, right third of a pixel). Each output
// sub-pixel reads the source sub-pixel under it, so a glyph quad at a fractional x lands at 1/3 px precision
// instead of snapping to whole pixels.
vec3 subpixel_coverage()
{
    ivec2 size = textureSize(Texture, 0);
    float x = Frag_UV.x * float(size.x);
    int row = int(floor(Frag_UV.y * float(size.y)));
    vec3 result;
    for (int k = 0; k < 3; ++k)
    {
        int j = int(floor(3.0 * x - 1.5 + float(k) + 0.5));
        int texel = j / 3;
        int channel = j - texel * 3;
        vec4 t = texelFetch(Texture, ivec2(texel, row), 0);
        result[k] = (channel == 0 ? t.r : (channel == 1 ? t.g : t.b)) * t.a;
    }
    return result;
}

void main()
{
    vec3 coverage;
    vec3 color;
    if (Lcd == 1)
    {
        coverage = subpixel_coverage() * Frag_Color.a;
        color = Frag_Color.rgb;
    }
    else if (Lcd == 2)
    {
        vec3 c = subpixel_coverage();
        coverage = vec3(pow((c.r + c.g + c.b) / 3.0, 1.0 / GrayGamma) * Frag_Color.a);
        color = Frag_Color.rgb;
    }
    else
    {
        vec4 t = texture(Texture, Frag_UV.st);
        coverage = vec3(t.a * Frag_Color.a);
        color = t.rgb * Frag_Color.rgb;
    }
    Out_Color = vec4(color * coverage, max(coverage.r, max(coverage.g, coverage.b)));
    Out_Coverage = vec4(coverage, 1.0);
}
";

/// A draw callback has no state of its own; the distinct volatile read keeps the linker from folding it into
/// another empty function, so its address stays a usable identity.
#[used]
static TEXT_MODE_MARKER: u8 = 0x7E;

/// The marker command of [`set_grayscale_text`]. Handled by [`LcdRenderer`], which reads the command's user
/// data instead of calling it; there is nothing to do when another renderer runs the callback.
#[inline(never)]
unsafe extern "C" fn text_mode_callback(
    _list: *const sys::ImDrawList,
    _command: *const sys::ImDrawCmd,
) {
    // SAFETY: reads a static.
    let _ = unsafe { std::ptr::read_volatile(&TEXT_MODE_MARKER) };
}

/// Chromium draws text in composited scrolling layers with grey-scale anti-aliasing. Adding this to a draw list
/// switches the rest of that list's text to grey-scale (the channel average of the ClearType coverage);
/// `on = false` switches back. Harmless without the sub-pixel renderer.
pub fn set_grayscale_text(list: &DrawListMut<'_>, on: bool) {
    // SAFETY: the callback is a no-op that never unwinds; the user data is a plain flag stored verbatim
    // (size 0), never dereferenced.
    unsafe { list.add_callback(text_mode_callback, usize::from(on) as *mut c_void, 0) };
}

/// [`set_grayscale_text`] for a draw list held as a pointer (`ui::DrawList`).
///
/// # Safety
/// `list` must be a draw list of the frame being built by the ImGui context current on this thread.
pub unsafe fn set_grayscale_text_raw(list: *mut sys::ImDrawList, on: bool) {
    // SAFETY: the caller vouches for `list`; the callback is a no-op that never unwinds and its user data a
    // plain flag stored verbatim (size 0), never dereferenced.
    unsafe {
        sys::ImDrawList_AddCallback(
            list,
            Some(text_mode_callback),
            usize::from(on) as *mut c_void,
            0,
        );
    }
}

/// A texture ImGui manages (the font atlas, in practice), as uploaded.
struct ManagedTexture {
    texture: glow::Texture,
    format: TextureFormat,
    width: u32,
    height: u32,
    /// The last request applied, so a retried request is not uploaded twice.
    upload: TextureUploadIdentity,
    /// Whether this is an allocation of the font atlas (drawn with per-channel coverage when the atlas holds
    /// sub-pixel glyphs).
    font_atlas: bool,
}

struct Uniforms {
    projection: Option<glow::UniformLocation>,
    texture: Option<glow::UniformLocation>,
    lcd: Option<glow::UniformLocation>,
    gray_gamma: Option<glow::UniformLocation>,
}

/// See the module documentation.
pub struct LcdRenderer {
    gl: Rc<glow::Context>,
    /// `None` once shut down.
    consumer: Option<SynchronousRendererConsumer>,
    program: glow::Program,
    vertex_array: glow::VertexArray,
    vertex_buffer: glow::Buffer,
    index_buffer: glow::Buffer,
    uniforms: Uniforms,
    textures: HashMap<SnapshotTextureId, ManagedTexture>,
    /// Managed textures already destroyed, whose late requests are answered as superseded.
    destroyed: HashSet<SnapshotTextureId>,
    /// The GL names of the live font atlas textures.
    atlas_textures: HashSet<TextureId>,
    /// Textures made by [`Self::create_texture`].
    user_textures: HashSet<TextureId>,
    subpixel_atlas: bool,
    grayscale_gamma: f32,
}

impl LcdRenderer {
    /// Builds the shader and attaches to `imgui` as its renderer. Fails when the context cannot do it (no
    /// GL 3.3 dual-source blending); the caller then falls back to the stock renderer. The GL context behind
    /// `gl` must be current now and for every later call.
    pub fn new(gl: Rc<glow::Context>, imgui: &mut Context) -> Result<Self, Error> {
        let version = gl.version();
        if version.is_embedded || (version.major, version.minor) < (3, 3) {
            return Err(Error::Gl(format!(
                "OpenGL 3.3 is required for sub-pixel text, the context is {}.{}",
                version.major, version.minor
            )));
        }
        // SAFETY: the GL context is current (the caller's contract); every object created here is owned by
        // the renderer and deleted in `shutdown`, or right here on failure.
        unsafe {
            let program = link_program(&gl)?;
            let uniforms = Uniforms {
                projection: gl.get_uniform_location(program, "ProjMtx"),
                texture: gl.get_uniform_location(program, "Texture"),
                lcd: gl.get_uniform_location(program, "Lcd"),
                gray_gamma: gl.get_uniform_location(program, "GrayGamma"),
            };
            let objects = (|| {
                let vertex_array = gl.create_vertex_array()?;
                let vertex_buffer = gl.create_buffer()?;
                let index_buffer = gl.create_buffer()?;
                Ok::<_, String>((vertex_array, vertex_buffer, index_buffer))
            })();
            let (vertex_array, vertex_buffer, index_buffer) = match objects {
                Ok(objects) => objects,
                Err(error) => {
                    gl.delete_program(program);
                    return Err(Error::Gl(error));
                }
            };
            gl.bind_vertex_array(Some(vertex_array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vertex_buffer));
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(index_buffer));
            let stride = size_of::<DrawVert>() as i32;
            for (index, size, kind, normalized, offset) in [
                (
                    0,
                    2,
                    glow::FLOAT,
                    false,
                    std::mem::offset_of!(DrawVert, pos),
                ),
                (1, 2, glow::FLOAT, false, std::mem::offset_of!(DrawVert, uv)),
                (
                    2,
                    4,
                    glow::UNSIGNED_BYTE,
                    true,
                    std::mem::offset_of!(DrawVert, col),
                ),
            ] {
                gl.enable_vertex_attrib_array(index);
                gl.vertex_attrib_pointer_f32(index, size, kind, normalized, stride, offset as i32);
            }
            gl.bind_vertex_array(None);

            let attach = (|| {
                imgui
                    .set_renderer_name(Some("imgit_lcd_renderer"))
                    .map_err(|error| Error::Gl(error.to_string()))?;
                Ok::<_, Error>(imgui.create_synchronous_renderer_consumer()?)
            })();
            let consumer = match attach {
                Ok(consumer) => consumer,
                Err(error) => {
                    gl.delete_vertex_array(vertex_array);
                    gl.delete_buffer(vertex_buffer);
                    gl.delete_buffer(index_buffer);
                    gl.delete_program(program);
                    return Err(error);
                }
            };
            let mut flags = imgui.io().backend_flags();
            flags.insert(
                BackendFlags::RENDERER_HAS_TEXTURES | BackendFlags::RENDERER_HAS_VTX_OFFSET,
            );
            imgui.io_mut().set_backend_flags(flags);
            // The largest atlas ImGui may ask for.
            let max_texture_size = gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE);
            let platform_io = sys::igGetPlatformIO_Nil();
            if !platform_io.is_null() && max_texture_size > 0 {
                (*platform_io).Renderer_TextureMaxWidth = max_texture_size;
                (*platform_io).Renderer_TextureMaxHeight = max_texture_size;
            }

            Ok(Self {
                gl,
                consumer: Some(consumer),
                program,
                vertex_array,
                vertex_buffer,
                index_buffer,
                uniforms,
                textures: HashMap::new(),
                destroyed: HashSet::new(),
                atlas_textures: HashSet::new(),
                user_textures: HashSet::new(),
                subpixel_atlas: false,
                grayscale_gamma: DEFAULT_GRAYSCALE_GAMMA,
            })
        }
    }

    /// Whether the font atlas holds the native loader's 3x1 sub-pixel coverage in RGB. A grey-scale atlas (one
    /// coverage per texel in A) is drawn like any other texture; sampling it per sub-pixel would smear each
    /// glyph a third of a pixel to each side with colour fringes. False until set.
    pub fn set_subpixel_atlas(&mut self, subpixel: bool) {
        self.subpixel_atlas = subpixel;
    }

    /// Extra coverage exponent for grey-scale text ([`set_grayscale_text`]).
    pub fn set_grayscale_gamma(&mut self, gamma: f32) {
        self.grayscale_gamma = gamma;
    }

    /// Closes the frame `imgui` has open and draws it into the bound framebuffer.
    pub fn render(&mut self, imgui: &mut Context) -> Result<(), Error> {
        let consumer = self.consumer.as_ref().ok_or(Error::RendererShutDown)?;
        let pending = imgui.try_render(consumer)?;
        let feedback = self.apply_texture_requests(&pending)?;
        let frame = pending.reconcile_texture_feedback(feedback)?;
        self.draw(frame.draw_data());
        Ok(())
    }

    /// A linear-filtered, edge-clamped RGBA8 texture for the UI's own images. The returned id is what
    /// draw-list image calls take (it is the GL texture name).
    pub fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<TextureId, Error> {
        if rgba.len() != width as usize * height as usize * 4 || width == 0 || height == 0 {
            return Err(Error::InvalidTexture {
                width,
                height,
                bytes: rgba.len(),
            });
        }
        // SAFETY: the GL context is current and `rgba` holds exactly width * height texels.
        let texture = unsafe {
            create_gl_texture(&self.gl, TextureFormat::RGBA32, width, height, rgba, false)?
        };
        let id = texture_id(texture);
        self.user_textures.insert(id);
        Ok(id)
    }

    /// Deletes a texture made by [`Self::create_texture`]; other ids are ignored.
    pub fn destroy_texture(&mut self, id: TextureId) {
        if self.user_textures.remove(&id)
            && let Some(texture) = gl_texture(id)
        {
            // SAFETY: the GL context is current; the name was created by `create_texture` and not yet deleted.
            unsafe { self.gl.delete_texture(texture) };
        }
    }

    /// Releases every GL object and detaches from `imgui`. Call while the GL context is current, before the
    /// Context is dropped.
    pub fn shutdown(&mut self, imgui: &mut Context) -> Result<(), Error> {
        let Some(consumer) = self.consumer.as_ref() else {
            return Ok(());
        };
        let reset = imgui.prepare_renderer_texture_reset(consumer)?;
        // SAFETY: the GL context is current; every name deleted here was created by this renderer and is
        // forgotten right after.
        unsafe {
            for texture in self.textures.drain().map(|(_, managed)| managed.texture) {
                self.gl.delete_texture(texture);
            }
            for texture in self.user_textures.drain().filter_map(gl_texture) {
                self.gl.delete_texture(texture);
            }
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_buffer(self.vertex_buffer);
            self.gl.delete_buffer(self.index_buffer);
            self.gl.delete_program(self.program);
        }
        reset.commit();
        self.destroyed.clear();
        self.atlas_textures.clear();
        self.consumer = None;
        imgui.poll_snapshot_completions()?;
        Ok(())
    }

    /// Creates, updates and destroys the textures ImGui manages, answering each request exactly once.
    fn apply_texture_requests(
        &mut self,
        frame: &PendingFrame<'_>,
    ) -> Result<Vec<TextureFeedback>, Error> {
        let mut feedback = Vec::with_capacity(frame.texture_requests().len());
        for request in frame.texture_requests() {
            let key = request.texture();
            match request.operation() {
                TextureOp::Destroy => {
                    self.destroyed.insert(key);
                    if let Some(managed) = self.textures.remove(&key) {
                        self.atlas_textures.remove(&texture_id(managed.texture));
                        // SAFETY: the GL context is current; the texture was created by this renderer.
                        unsafe { self.gl.delete_texture(managed.texture) };
                    }
                    feedback.push(request.destroyed()?);
                }
                _ if self.destroyed.contains(&key) => feedback.push(request.superseded()),
                TextureOp::Create {
                    format,
                    width,
                    height,
                    row_pitch,
                    pixels,
                } => {
                    let upload = request
                        .upload_identity()
                        .expect("a create request has an upload identity");
                    if let Some(managed) = self.textures.get(&key)
                        && managed.upload == upload
                    {
                        feedback.push(request.uploaded(texture_id(managed.texture))?);
                        continue;
                    }
                    let tight = tight_rows(*format, *width, *height, *row_pitch, pixels)?;
                    // Created before the old allocation (if any) is dropped, so a failure leaves it intact.
                    // SAFETY: the GL context is current; `tight` holds width * height tightly packed texels.
                    let font_atlas = matches!(key, SnapshotTextureId::FontAtlas { .. });
                    let texture = unsafe {
                        create_gl_texture(&self.gl, *format, *width, *height, &tight, font_atlas)?
                    };
                    let managed = ManagedTexture {
                        texture,
                        format: *format,
                        width: *width,
                        height: *height,
                        upload,
                        font_atlas,
                    };
                    if let Some(previous) = self.textures.insert(key, managed) {
                        self.atlas_textures.remove(&texture_id(previous.texture));
                        // SAFETY: as above; the replaced texture is no longer referenced by ImGui.
                        unsafe { self.gl.delete_texture(previous.texture) };
                    }
                    if font_atlas {
                        self.atlas_textures.insert(texture_id(texture));
                    }
                    feedback.push(request.uploaded(texture_id(texture))?);
                }
                TextureOp::Update {
                    format,
                    width,
                    height,
                    rects,
                } => {
                    let upload = request
                        .upload_identity()
                        .expect("an update request has an upload identity");
                    let Some(managed) = self.textures.get_mut(&key) else {
                        feedback.push(request.retry());
                        continue;
                    };
                    if managed.upload == upload {
                        feedback.push(request.uploaded(texture_id(managed.texture))?);
                        continue;
                    }
                    if (managed.format, managed.width, managed.height) != (*format, *width, *height)
                    {
                        return Err(Error::Gl(
                            "a texture update does not match the texture's layout".to_owned(),
                        ));
                    }
                    for update in rects {
                        let rect = update.rect;
                        let tight = tight_rows(
                            *format,
                            u32::from(rect.w),
                            u32::from(rect.h),
                            update.row_pitch,
                            &update.data,
                        )?;
                        let (_, gl_format) = gl_formats(*format);
                        // SAFETY: the GL context is current; the rectangle lies inside the texture (ImGui's
                        // contract) and `tight` holds its w * h tightly packed texels.
                        unsafe {
                            self.gl
                                .bind_texture(glow::TEXTURE_2D, Some(managed.texture));
                            reset_unpack_state(&self.gl);
                            self.gl.tex_sub_image_2d(
                                glow::TEXTURE_2D,
                                0,
                                i32::from(rect.x),
                                i32::from(rect.y),
                                i32::from(rect.w),
                                i32::from(rect.h),
                                gl_format,
                                glow::UNSIGNED_BYTE,
                                glow::PixelUnpackData::Slice(Some(&tight)),
                            );
                        }
                    }
                    managed.upload = upload;
                    feedback.push(request.uploaded(texture_id(managed.texture))?);
                }
            }
        }
        Ok(feedback)
    }

    fn setup_render_state(&self, draw_data: &DrawData, framebuffer: [i32; 2]) {
        let gl = &self.gl;
        let [x, y] = draw_data.display_pos();
        let [width, height] = draw_data.display_size();
        let (left, right, top, bottom) = (x, x + width, y, y + height);
        #[rustfmt::skip]
        let projection = [
            2.0 / (right - left), 0.0, 0.0, 0.0,
            0.0, 2.0 / (top - bottom), 0.0, 0.0,
            0.0, 0.0, -1.0, 0.0,
            (right + left) / (left - right), (top + bottom) / (bottom - top), 0.0, 1.0,
        ];
        // SAFETY: the GL context is current; every object bound here is owned by the renderer and alive.
        unsafe {
            gl.enable(glow::BLEND);
            gl.blend_equation(glow::FUNC_ADD);
            gl.blend_func_separate(
                glow::ONE,
                glow::ONE_MINUS_SRC1_COLOR,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::STENCIL_TEST);
            gl.enable(glow::SCISSOR_TEST);
            gl.viewport(0, 0, framebuffer[0], framebuffer[1]);
            gl.use_program(Some(self.program));
            gl.uniform_1_i32(self.uniforms.texture.as_ref(), 0);
            gl.uniform_1_f32(self.uniforms.gray_gamma.as_ref(), self.grayscale_gamma);
            gl.uniform_matrix_4_f32_slice(self.uniforms.projection.as_ref(), false, &projection);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_sampler(0, None);
            gl.bind_vertex_array(Some(self.vertex_array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vertex_buffer));
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.index_buffer));
        }
    }

    fn draw(&self, draw_data: &DrawData) {
        let [scale_x, scale_y] = draw_data.framebuffer_scale();
        let [display_width, display_height] = draw_data.display_size();
        let framebuffer = [
            (display_width * scale_x) as i32,
            (display_height * scale_y) as i32,
        ];
        if framebuffer[0] <= 0 || framebuffer[1] <= 0 {
            return;
        }
        let gl = &self.gl;
        self.setup_render_state(draw_data, framebuffer);
        let clip_offset = draw_data.display_pos();
        let index_type = if size_of::<DrawIdx>() == 2 {
            glow::UNSIGNED_SHORT
        } else {
            glow::UNSIGNED_INT
        };

        for list in draw_data.draw_lists() {
            let mut grayscale = false;
            // SAFETY: the GL context is current. The vertex and index slices are plain-old-data owned by
            // the draw list for the whole frame, viewed as bytes for the upload.
            unsafe {
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    as_bytes(list.vtx_buffer()),
                    glow::STREAM_DRAW,
                );
                gl.buffer_data_u8_slice(
                    glow::ELEMENT_ARRAY_BUFFER,
                    as_bytes(list.idx_buffer()),
                    glow::STREAM_DRAW,
                );
            }
            // The safe iterator classifies the commands; the raw buffer beside it tells our own marker
            // callback (and its flag) from foreign callbacks.
            // SAFETY: read-only view of the draw list's command buffer, alive for the frame.
            let raw_commands = unsafe {
                let buffer = &list.raw().CmdBuffer;
                if buffer.Data.is_null() || buffer.Size <= 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(buffer.Data, buffer.Size as usize)
                }
            };
            for (command, raw) in list.commands().zip(raw_commands) {
                match command {
                    DrawCmd::Elements { count, cmd_params } => {
                        let clip_min = [
                            (cmd_params.clip_rect[0] - clip_offset[0]) * scale_x,
                            (cmd_params.clip_rect[1] - clip_offset[1]) * scale_y,
                        ];
                        let clip_max = [
                            (cmd_params.clip_rect[2] - clip_offset[0]) * scale_x,
                            (cmd_params.clip_rect[3] - clip_offset[1]) * scale_y,
                        ];
                        if clip_max[0] <= clip_min[0] || clip_max[1] <= clip_min[1] {
                            continue;
                        }
                        let texture = cmd_params.texture_id;
                        let subpixel =
                            self.subpixel_atlas && self.atlas_textures.contains(&texture);
                        let lcd = match (subpixel, grayscale) {
                            (false, _) => 0,
                            (true, false) => 1,
                            (true, true) => 2,
                        };
                        // SAFETY: the GL context is current with the state of `setup_render_state`; the
                        // index range and base vertex come from ImGui and lie inside the buffers just
                        // uploaded; the texture name is one ImGui was given by this renderer or the UI.
                        unsafe {
                            gl.scissor(
                                clip_min[0] as i32,
                                (framebuffer[1] as f32 - clip_max[1]) as i32,
                                (clip_max[0] - clip_min[0]) as i32,
                                (clip_max[1] - clip_min[1]) as i32,
                            );
                            gl.bind_texture(glow::TEXTURE_2D, gl_texture(texture));
                            gl.uniform_1_i32(self.uniforms.lcd.as_ref(), lcd);
                            gl.draw_elements_base_vertex(
                                glow::TRIANGLES,
                                count as i32,
                                index_type,
                                (cmd_params.idx_offset * size_of::<DrawIdx>()) as i32,
                                cmd_params.vtx_offset as i32,
                            );
                        }
                    }
                    DrawCmd::ResetRenderState => self.setup_render_state(draw_data, framebuffer),
                    // Sampling is a property of each texture here (nearest for the atlas, linear for images).
                    DrawCmd::SetSamplerLinear
                    | DrawCmd::SetSamplerNearest
                    | DrawCmd::SetSamplerFromTexture => {}
                    DrawCmd::RawCallback(callback) => {
                        let ours = raw.UserCallback.is_some_and(|function| {
                            std::ptr::fn_addr_eq(
                                function,
                                text_mode_callback
                                    as unsafe extern "C" fn(
                                        *const sys::ImDrawList,
                                        *const sys::ImDrawCmd,
                                    ),
                            )
                        });
                        if ours {
                            grayscale = !raw.UserCallbackData.is_null();
                        } else {
                            // SAFETY: a callback the UI added with `DrawListMut::add_callback`, whose
                            // contract makes it callable here, once, with the renderer's state installed.
                            unsafe { callback.invoke() };
                        }
                    }
                }
            }
        }
        // SAFETY: the GL context is current.
        unsafe {
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::SCISSOR_TEST);
        }
    }
}

/// The id ImGui knows a GL texture by: its name.
fn texture_id(texture: glow::Texture) -> TextureId {
    TextureId::new(u64::from(texture.0.get()))
}

/// The GL texture behind an id handed out by this renderer; `None` for the null id.
fn gl_texture(id: TextureId) -> Option<glow::Texture> {
    id.try_as_u32()
        .and_then(NonZeroU32::new)
        .map(glow::NativeTexture)
}

/// `(internal format, pixel format)` for an ImGui texture format.
fn gl_formats(format: TextureFormat) -> (i32, u32) {
    match format {
        TextureFormat::RGBA32 => (glow::RGBA8 as i32, glow::RGBA),
        TextureFormat::Alpha8 => (glow::R8 as i32, glow::RED),
    }
}

fn bytes_per_pixel(format: TextureFormat) -> usize {
    match format {
        TextureFormat::RGBA32 => 4,
        TextureFormat::Alpha8 => 1,
    }
}

/// The rows of an upload without their padding (`row_pitch` may exceed the row's bytes).
fn tight_rows(
    format: TextureFormat,
    width: u32,
    height: u32,
    row_pitch: usize,
    pixels: &[u8],
) -> Result<std::borrow::Cow<'_, [u8]>, Error> {
    let row = width as usize * bytes_per_pixel(format);
    let size = row * height as usize;
    let invalid = || Error::Gl("a texture upload is smaller than its dimensions".to_owned());
    if row_pitch == row {
        return pixels
            .get(..size)
            .map(std::borrow::Cow::Borrowed)
            .ok_or_else(invalid);
    }
    if row_pitch < row {
        return Err(invalid());
    }
    let mut tight = Vec::with_capacity(size);
    for y in 0..height as usize {
        tight.extend_from_slice(
            pixels
                .get(y * row_pitch..y * row_pitch + row)
                .ok_or_else(invalid)?,
        );
    }
    Ok(std::borrow::Cow::Owned(tight))
}

/// # Safety
/// The GL context must be current.
unsafe fn reset_unpack_state(gl: &glow::Context) {
    // SAFETY: plain state changes on the current context.
    unsafe {
        gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
        gl.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 0);
        gl.pixel_store_i32(glow::UNPACK_SKIP_ROWS, 0);
    }
}

/// An edge-clamped texture, `nearest`- or linear-filtered. A single-channel texture reads as white with the
/// channel in alpha.
///
/// # Safety
/// The GL context must be current and `pixels` must hold `width * height` tightly packed texels of `format`.
unsafe fn create_gl_texture(
    gl: &glow::Context,
    format: TextureFormat,
    width: u32,
    height: u32,
    pixels: &[u8],
    nearest: bool,
) -> Result<glow::Texture, Error> {
    let (internal_format, pixel_format) = gl_formats(format);
    let filter = if nearest { glow::NEAREST } else { glow::LINEAR } as i32;
    // SAFETY: the caller's contract.
    unsafe {
        let texture = gl.create_texture().map_err(Error::Gl)?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, filter);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, filter);
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        if format == TextureFormat::Alpha8 {
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_SWIZZLE_R, glow::ONE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_SWIZZLE_G, glow::ONE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_SWIZZLE_B, glow::ONE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_SWIZZLE_A, glow::RED as i32);
        }
        reset_unpack_state(gl);
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            internal_format,
            width as i32,
            height as i32,
            0,
            pixel_format,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(pixels)),
        );
        Ok(texture)
    }
}

/// # Safety
/// The GL context must be current.
unsafe fn link_program(gl: &glow::Context) -> Result<glow::Program, Error> {
    // SAFETY: the caller's contract; shaders and the program are deleted on every failure path.
    unsafe {
        let compile = |kind: u32, source: &str| {
            let shader = gl.create_shader(kind).map_err(Error::Gl)?;
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            if gl.get_shader_compile_status(shader) {
                Ok(shader)
            } else {
                let log = gl.get_shader_info_log(shader);
                gl.delete_shader(shader);
                Err(Error::Gl(format!("shader compilation failed: {log}")))
            }
        };
        let vertex = compile(glow::VERTEX_SHADER, VERTEX_SOURCE)?;
        let fragment = match compile(glow::FRAGMENT_SHADER, FRAGMENT_SOURCE) {
            Ok(fragment) => fragment,
            Err(error) => {
                gl.delete_shader(vertex);
                return Err(error);
            }
        };
        let program = gl.create_program().map_err(Error::Gl);
        let linked = program.and_then(|program| {
            gl.attach_shader(program, vertex);
            gl.attach_shader(program, fragment);
            gl.link_program(program);
            if gl.get_program_link_status(program) {
                Ok(program)
            } else {
                let log = gl.get_program_info_log(program);
                gl.delete_program(program);
                Err(Error::Gl(format!("shader linking failed: {log}")))
            }
        });
        gl.delete_shader(vertex);
        gl.delete_shader(fragment);
        linked
    }
}

/// Vertices or indices as the bytes GL uploads.
///
/// # Safety
/// `T` must be plain old data without padding (`DrawVert`, `DrawIdx`).
unsafe fn as_bytes<T>(slice: &[T]) -> &[u8] {
    // SAFETY: the caller's contract; the length is the slice's size in bytes.
    unsafe { std::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), size_of_val(slice)) }
}
