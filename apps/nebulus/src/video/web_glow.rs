use std::sync::Arc;

use eframe::{egui, egui_glow, glow};
use glow::HasContext as _;
use openipc_video::{DecodedFrame, WebVideoFrame};

const VERTEX_SHADER: &str = r#"#version 300 es
precision mediump float;
out vec2 video_uv;
void main() {
    vec2 position = vec2(
        gl_VertexID == 1 ? 3.0 : -1.0,
        gl_VertexID == 2 ? 3.0 : -1.0
    );
    gl_Position = vec4(position, 0.0, 1.0);
    video_uv = vec2((position.x + 1.0) * 0.5, 1.0 - (position.y + 1.0) * 0.5);
}
"#;

const FRAGMENT_SHADER: &str = r#"#version 300 es
precision mediump float;
uniform sampler2D video_texture;
in vec2 video_uv;
out vec4 output_color;
void main() {
    output_color = texture(video_texture, video_uv);
}
"#;

/// Direct WebCodecs `VideoFrame` to WebGL presenter.
pub(crate) struct WebGlowRenderer {
    gl: Arc<glow::Context>,
    program: glow::Program,
    vertex_array: glow::VertexArray,
    texture: glow::Texture,
    dimensions: Option<[u32; 2]>,
}

impl WebGlowRenderer {
    pub(crate) fn new(context: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let gl = context
            .gl
            .clone()
            .ok_or_else(|| "browser video renderer requires WebGL".to_owned())?;
        // SAFETY: Creation runs on eframe's WebGL thread with its context current.
        unsafe {
            let program = create_program(&gl)?;
            let vertex_array = gl
                .create_vertex_array()
                .map_err(|error| format!("create video vertex array: {error}"))?;
            let texture = gl
                .create_texture()
                .map_err(|error| format!("create video texture: {error}"))?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
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
            gl.bind_texture(glow::TEXTURE_2D, None);
            Ok(Self {
                gl,
                program,
                vertex_array,
                texture,
                dimensions: None,
            })
        }
    }

    pub(crate) fn upload(&mut self, frame: &DecodedFrame<WebVideoFrame>) -> Result<(), String> {
        let dimensions = frame.dimensions();
        let dimensions = [dimensions.width, dimensions.height];
        // SAFETY: The retained WebCodecs frame and eframe WebGL context are
        // live on this browser thread for the duration of the upload.
        unsafe {
            self.gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            if self.dimensions == Some(dimensions) {
                self.gl.tex_sub_image_2d_with_video_frame(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    dimensions[0] as i32,
                    dimensions[1] as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    frame.surface.video_frame(),
                );
            } else {
                self.gl.tex_image_2d_with_video_frame(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    frame.surface.video_frame(),
                );
                self.dimensions = Some(dimensions);
            }
            self.gl.bind_texture(glow::TEXTURE_2D, None);
        }
        Ok(())
    }

    pub(crate) fn paint(&self, painter: &egui::Painter, rect: egui::Rect) {
        let program = self.program;
        let vertex_array = self.vertex_array;
        let texture = self.texture;
        painter.add(egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |_info, painter| {
                let gl = painter.gl();
                // SAFETY: eframe invokes this callback while its WebGL context
                // is current and restores egui's GL state afterward.
                unsafe {
                    gl.use_program(Some(program));
                    gl.bind_vertex_array(Some(vertex_array));
                    gl.active_texture(glow::TEXTURE0);
                    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                    let location = gl.get_uniform_location(program, "video_texture");
                    gl.uniform_1_i32(location.as_ref(), 0);
                    gl.draw_arrays(glow::TRIANGLES, 0, 3);
                    gl.bind_texture(glow::TEXTURE_2D, None);
                    gl.bind_vertex_array(None);
                    gl.use_program(None);
                }
            })),
        });
    }
}

impl Drop for WebGlowRenderer {
    fn drop(&mut self) {
        // SAFETY: Nebulus owns these objects and drops them on the WebGL thread.
        unsafe {
            self.gl.delete_texture(self.texture);
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_program(self.program);
        }
    }
}

unsafe fn create_program(gl: &glow::Context) -> Result<glow::Program, String> {
    // SAFETY: All objects are created and used with the current WebGL context.
    unsafe {
        let program = gl
            .create_program()
            .map_err(|error| format!("create video program: {error}"))?;
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
            let log = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(format!("link video program: {log}"));
        }
        Ok(program)
    }
}

unsafe fn compile_shader(
    gl: &glow::Context,
    kind: u32,
    source: &str,
) -> Result<glow::Shader, String> {
    // SAFETY: Shader compilation uses the current WebGL context and static GLSL.
    unsafe {
        let shader = gl
            .create_shader(kind)
            .map_err(|error| format!("create video shader: {error}"))?;
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            let log = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            return Err(format!("compile video shader: {log}"));
        }
        Ok(shader)
    }
}
