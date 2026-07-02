use eframe::{
    egui,
    egui_wgpu::{self, wgpu},
};
use openipc_video::{
    AndroidImagePlane, AndroidVideoFrame, DecodedFrame, DecodedSurface, FrameDimensions,
    PixelFormat,
};

const SHADER: &str = r#"
@group(0) @binding(0) var video_sampler: sampler;
@group(0) @binding(1) var y_texture: texture_2d<f32>;
@group(0) @binding(2) var u_texture: texture_2d<f32>;
@group(0) @binding(3) var v_texture: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: VertexOutput;
    output.position = vec4<f32>(positions[index], 0.0, 1.0);
    output.uv = vec2<f32>(
        (positions[index].x + 1.0) * 0.5,
        1.0 - (positions[index].y + 1.0) * 0.5,
    );
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let y = textureSample(y_texture, video_sampler, input.uv).r;
    let u = textureSample(u_texture, video_sampler, input.uv).r - 0.5;
    let v = textureSample(v_texture, video_sampler, input.uv).r - 0.5;
    let luma = max(y - 16.0 / 255.0, 0.0) * 1.1643836;
    return vec4<f32>(
        luma + 1.5960272 * v,
        luma - 0.3917623 * u - 0.8129683 * v,
        luma + 2.0172321 * u,
        1.0,
    );
}
"#;

pub(crate) struct AndroidYuvRenderer {
    render_state: egui_wgpu::RenderState,
}

/// CPU-packed planar YUV frame prepared away from egui's UI thread.
#[derive(Debug)]
pub(crate) struct AndroidYuvFrame {
    dimensions: FrameDimensions,
    planes: [Vec<u8>; 3],
}

impl DecodedSurface for AndroidYuvFrame {
    fn dimensions(&self) -> FrameDimensions {
        self.dimensions
    }

    fn pixel_format(&self) -> PixelFormat {
        PixelFormat::Native(35)
    }
}

pub(crate) fn pack_android_frame(
    frame: DecodedFrame<AndroidVideoFrame>,
) -> Result<DecodedFrame<AndroidYuvFrame>, String> {
    let dimensions = frame.dimensions();
    let width = dimensions.width as usize;
    let height = dimensions.height as usize;
    let [crop_x, crop_y] = frame.surface.crop_origin();
    let planes = frame
        .surface
        .with_mapped_planes(|planes| {
            let [y, u, v, ..] = planes else {
                return Err("MediaCodec frame did not expose Y/U/V planes".to_owned());
            };
            let chroma_width = width.div_ceil(2);
            let chroma_height = height.div_ceil(2);
            let mut packed = std::array::from_fn(|_| Vec::new());
            pack_plane(&mut packed[0], y, crop_x, crop_y, width, height)?;
            pack_plane(
                &mut packed[1],
                u,
                crop_x / 2,
                crop_y / 2,
                chroma_width,
                chroma_height,
            )?;
            pack_plane(
                &mut packed[2],
                v,
                crop_x / 2,
                crop_y / 2,
                chroma_width,
                chroma_height,
            )?;
            Ok(packed)
        })
        .map_err(|error| error.to_string())??;
    Ok(DecodedFrame {
        surface: AndroidYuvFrame { dimensions, planes },
        timestamp: frame.timestamp,
        duration: frame.duration,
    })
}

struct YuvResources {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    frame: Option<GpuFrame>,
}

struct GpuFrame {
    textures: [wgpu::Texture; 3],
    bind_group: wgpu::BindGroup,
    dimensions: [u32; 2],
}

struct YuvPaintCallback;

impl AndroidYuvRenderer {
    pub(crate) fn new(context: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let render_state = context
            .wgpu_render_state
            .clone()
            .ok_or_else(|| "Android YUV renderer requires the wgpu backend".to_owned())?;
        let resources = YuvResources::new(&render_state.device, render_state.target_format);
        render_state
            .renderer
            .write()
            .callback_resources
            .insert(resources);
        Ok(Self { render_state })
    }

    pub(crate) fn upload(&mut self, frame: &DecodedFrame<AndroidYuvFrame>) -> Result<(), String> {
        let dimensions = frame.dimensions();
        let mut renderer = self.render_state.renderer.write();
        let resources = renderer
            .callback_resources
            .get_mut::<YuvResources>()
            .ok_or_else(|| "Android YUV GPU resources are unavailable".to_owned())?;
        resources.upload(
            &self.render_state.device,
            &self.render_state.queue,
            [dimensions.width, dimensions.height],
            &frame.surface.planes,
        )
    }

    pub(crate) fn paint(&self, painter: &egui::Painter, rect: egui::Rect) {
        painter.add(egui_wgpu::Callback::new_paint_callback(
            rect,
            YuvPaintCallback,
        ));
    }
}

impl YuvResources {
    fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nebulus-android-yuv-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_layout(1),
                texture_layout(2),
                texture_layout(3),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nebulus-android-yuv-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nebulus-android-yuv-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nebulus-android-yuv-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nebulus-android-yuv-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            bind_group_layout,
            sampler,
            frame: None,
        }
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dimensions: [u32; 2],
        planes: &[Vec<u8>; 3],
    ) -> Result<(), String> {
        let [width, height] = dimensions;
        if let Some(frame) = self
            .frame
            .as_ref()
            .filter(|frame| frame.dimensions == dimensions)
        {
            write_plane(queue, &frame.textures[0], &planes[0], width, height)?;
            write_plane(
                queue,
                &frame.textures[1],
                &planes[1],
                width.div_ceil(2),
                height.div_ceil(2),
            )?;
            write_plane(
                queue,
                &frame.textures[2],
                &planes[2],
                width.div_ceil(2),
                height.div_ceil(2),
            )?;
            return Ok(());
        }
        let textures = [
            create_texture(device, "nebulus-android-y", width, height),
            create_texture(
                device,
                "nebulus-android-u",
                width.div_ceil(2),
                height.div_ceil(2),
            ),
            create_texture(
                device,
                "nebulus-android-v",
                width.div_ceil(2),
                height.div_ceil(2),
            ),
        ];
        write_plane(queue, &textures[0], &planes[0], width, height)?;
        write_plane(
            queue,
            &textures[1],
            &planes[1],
            width.div_ceil(2),
            height.div_ceil(2),
        )?;
        write_plane(
            queue,
            &textures[2],
            &planes[2],
            width.div_ceil(2),
            height.div_ceil(2),
        )?;
        let views = textures
            .each_ref()
            .map(|texture| texture.create_view(&Default::default()));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nebulus-android-yuv-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                texture_entry(1, &views[0]),
                texture_entry(2, &views[1]),
                texture_entry(3, &views[2]),
            ],
        });
        self.frame = Some(GpuFrame {
            textures,
            bind_group,
            dimensions,
        });
        Ok(())
    }
}

impl egui_wgpu::CallbackTrait for YuvPaintCallback {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(resources) = callback_resources.get::<YuvResources>() else {
            return;
        };
        let Some(frame) = resources.frame.as_ref() else {
            return;
        };
        render_pass.set_pipeline(&resources.pipeline);
        render_pass.set_bind_group(0, &frame.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

fn pack_plane(
    output: &mut Vec<u8>,
    plane: &AndroidImagePlane<'_>,
    origin_x: usize,
    origin_y: usize,
    width: usize,
    height: usize,
) -> Result<(), String> {
    output.resize(width.saturating_mul(height), 0);
    for row in 0..height {
        let row_start = origin_y
            .saturating_add(row)
            .saturating_mul(plane.row_stride());
        if plane.pixel_stride() == 1 {
            let source_start = row_start.saturating_add(origin_x);
            let source_end = source_start.saturating_add(width);
            let source = plane
                .data()
                .get(source_start..source_end)
                .ok_or_else(|| "MediaCodec plane is shorter than its layout".to_owned())?;
            output[row * width..(row + 1) * width].copy_from_slice(source);
            continue;
        }
        for column in 0..width {
            let source = row_start.saturating_add(
                origin_x
                    .saturating_add(column)
                    .saturating_mul(plane.pixel_stride()),
            );
            let Some(sample) = plane.data().get(source) else {
                return Err("MediaCodec plane is shorter than its layout".to_owned());
            };
            output[row * width + column] = *sample;
        }
    }
    Ok(())
}

fn texture_layout(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn texture_entry<'a>(binding: u32, view: &'a wgpu::TextureView) -> wgpu::BindGroupEntry<'a> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

fn create_texture(
    device: &wgpu::Device,
    label: &'static str,
    width: u32,
    height: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_plane(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<(), String> {
    let expected = (width as usize).saturating_mul(height as usize);
    if bytes.len() < expected {
        return Err("packed Android plane is shorter than expected".to_owned());
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Ok(())
}
