use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use eframe::{egui, glow, glow::HasContext as _};
use openipc_video::{AndroidPresentedFrame, DecodedFrame};

const TEXTURE_EXTERNAL_OES: u32 = 0x8D65;

const VERTEX_SHADER: &str = r#"#version 300 es
precision mediump float;
out vec2 video_uv;
void main() {
    vec2 position = vec2(
        gl_VertexID == 1 ? 3.0 : -1.0,
        gl_VertexID == 2 ? 3.0 : -1.0
    );
    gl_Position = vec4(position, 0.0, 1.0);
    video_uv = vec2((position.x + 1.0) * 0.5, (position.y + 1.0) * 0.5);
}
"#;

const FRAGMENT_SHADER: &str = r#"#version 300 es
#extension GL_OES_EGL_image_external_essl3 : require
precision mediump float;
uniform samplerExternalOES video_texture;
uniform mat4 texture_transform;
in vec2 video_uv;
out vec4 output_color;
void main() {
    vec4 transformed = texture_transform * vec4(video_uv, 0.0, 1.0);
    output_color = texture(video_texture, transformed.xy);
}
"#;

/// Zero-copy Android SurfaceTexture presenter for egui's Glow renderer.
pub(crate) struct AndroidGlowRenderer {
    gl: Arc<glow::Context>,
    program: glow::Program,
    vertex_array: glow::VertexArray,
    texture: glow::Texture,
    surface: Option<Arc<crate::android::AndroidVideoSurface>>,
    dimensions: Option<[u32; 2]>,
    update_error_logged: Arc<AtomicBool>,
}

impl AndroidGlowRenderer {
    pub(crate) fn new(context: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let gl = context
            .gl
            .clone()
            .ok_or_else(|| "Android video renderer requires OpenGL ES".to_owned())?;
        // SAFETY: app creation runs on eframe's GL thread with its context current.
        unsafe {
            let program = create_program(&gl)?;
            let vertex_array = gl
                .create_vertex_array()
                .map_err(|error| format!("create Android video vertex array: {error}"))?;
            let texture = gl
                .create_texture()
                .map_err(|error| format!("create Android external video texture: {error}"))?;
            gl.bind_texture(TEXTURE_EXTERNAL_OES, Some(texture));
            gl.tex_parameter_i32(
                TEXTURE_EXTERNAL_OES,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                TEXTURE_EXTERNAL_OES,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                TEXTURE_EXTERNAL_OES,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                TEXTURE_EXTERNAL_OES,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.bind_texture(TEXTURE_EXTERNAL_OES, None);
            let surface = match crate::android::AndroidVideoSurface::create(texture.0.get()) {
                Ok(surface) => Arc::new(surface),
                Err(error) => {
                    gl.delete_texture(texture);
                    gl.delete_vertex_array(vertex_array);
                    gl.delete_program(program);
                    return Err(error);
                }
            };
            Ok(Self {
                gl,
                program,
                vertex_array,
                texture,
                surface: Some(surface),
                dimensions: None,
                update_error_logged: Arc::new(AtomicBool::new(false)),
            })
        }
    }

    pub(crate) fn output_window(&self) -> ndk::native_window::NativeWindow {
        self.surface
            .as_ref()
            .expect("Android video surface is live")
            .native_window()
    }

    pub(crate) fn upload(
        &mut self,
        frame: &DecodedFrame<AndroidPresentedFrame>,
    ) -> Result<(), String> {
        let dimensions = frame.dimensions();
        let dimensions = [dimensions.width, dimensions.height];
        if self.dimensions != Some(dimensions) {
            self.surface
                .as_ref()
                .expect("Android video surface is live")
                .set_buffer_size(dimensions[0], dimensions[1])?;
            self.dimensions = Some(dimensions);
        }
        Ok(())
    }

    pub(crate) fn paint(&self, painter: &egui::Painter, rect: egui::Rect) {
        let program = self.program;
        let vertex_array = self.vertex_array;
        let texture = self.texture;
        let surface = Arc::clone(
            self.surface
                .as_ref()
                .expect("Android video surface is live"),
        );
        let update_error_logged = Arc::clone(&self.update_error_logged);
        painter.add(egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |_info, painter| {
                let transform = match surface.update_texture() {
                    Ok(transform) => transform,
                    Err(error) => {
                        if !update_error_logged.swap(true, Ordering::Relaxed) {
                            log::warn!(
                                target: "nebulus::video",
                                "SurfaceTexture update failed: {error}"
                            );
                        }
                        return;
                    }
                };
                let gl = painter.gl();
                // SAFETY: eframe invokes the callback with its GLES context current.
                unsafe {
                    gl.use_program(Some(program));
                    gl.bind_vertex_array(Some(vertex_array));
                    gl.active_texture(glow::TEXTURE0);
                    gl.bind_texture(TEXTURE_EXTERNAL_OES, Some(texture));
                    let sampler = gl.get_uniform_location(program, "video_texture");
                    gl.uniform_1_i32(sampler.as_ref(), 0);
                    let matrix = gl.get_uniform_location(program, "texture_transform");
                    gl.uniform_matrix_4_f32_slice(matrix.as_ref(), false, &transform);
                    gl.draw_arrays(glow::TRIANGLES, 0, 3);
                    gl.bind_texture(TEXTURE_EXTERNAL_OES, None);
                    gl.bind_vertex_array(None);
                    gl.use_program(None);
                }
            })),
        });
    }
}

impl Drop for AndroidGlowRenderer {
    fn drop(&mut self) {
        // Release SurfaceTexture before deleting the GL name it references.
        drop(self.surface.take());
        // SAFETY: Nebulus drops renderer resources on eframe's GL thread.
        unsafe {
            self.gl.delete_texture(self.texture);
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_program(self.program);
        }
    }
}

unsafe fn create_program(gl: &glow::Context) -> Result<glow::Program, String> {
    // SAFETY: all objects are created with the current GLES context.
    unsafe {
        let program = gl
            .create_program()
            .map_err(|error| format!("create Android video program: {error}"))?;
        let vertex = compile_shader(gl, glow::VERTEX_SHADER, VERTEX_SHADER)?;
        let fragment = compile_shader(gl, glow::FRAGMENT_SHADER, FRAGMENT_SHADER)?;
        gl.attach_shader(program, vertex);
        gl.attach_shader(program, fragment);
        gl.link_program(program);
        gl.detach_shader(program, vertex);
        gl.detach_shader(program, fragment);
        gl.delete_shader(vertex);
        gl.delete_shader(fragment);
        if !gl.get_program_link_status(program) {
            let message = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(format!("link Android video program: {message}"));
        }
        Ok(program)
    }
}

unsafe fn compile_shader(
    gl: &glow::Context,
    kind: u32,
    source: &str,
) -> Result<glow::Shader, String> {
    // SAFETY: shader compilation uses the current GLES context and static GLSL.
    unsafe {
        let shader = gl
            .create_shader(kind)
            .map_err(|error| format!("create Android video shader: {error}"))?;
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            let message = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            return Err(format!("compile Android video shader: {message}"));
        }
        Ok(shader)
    }
}
