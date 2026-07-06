use eframe::{
    egui,
    egui_wgpu::{self, wgpu},
};
use openipc_video::{DecodedFrame, DecodedSurface as _, PixelFormat};

#[cfg(target_os = "macos")]
use objc2_core_foundation::CFRetained;
#[cfg(target_os = "macos")]
use objc2_core_video::{
    kCVReturnSuccess, CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture,
};
#[cfg(target_os = "macos")]
use objc2_metal::{MTLPixelFormat, MTLTextureType};

const SHADER: &str = r#"
@group(0) @binding(0) var video_sampler: sampler;
@group(0) @binding(1) var y_texture: texture_2d<f32>;
@group(0) @binding(2) var uv_texture: texture_2d<f32>;

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
    let chroma = textureSample(uv_texture, video_sampler, input.uv).rg - vec2<f32>(0.5, 0.5);
    let luma = max(y - 16.0 / 255.0, 0.0) * 1.1643836;
    let rgb = vec3<f32>(
        luma + 1.5960272 * chroma.y,
        luma - 0.3917623 * chroma.x - 0.8129683 * chroma.y,
        luma + 2.0172321 * chroma.x,
    );
    return vec4<f32>(rgb, 1.0);
}
"#;

pub(crate) struct NativeNv12Renderer {
    render_state: egui_wgpu::RenderState,
    #[cfg(target_os = "macos")]
    metal_cache: MacMetalTextureCache,
}

struct Nv12Resources {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    frame: Option<GpuFrame>,
}

struct GpuFrame {
    y_texture: wgpu::Texture,
    uv_texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    dimensions: [u32; 2],
    #[cfg(target_os = "macos")]
    _core_video_textures: Option<RetainedMetalPlanes>,
}

#[cfg(target_os = "macos")]
struct RetainedMetalPlanes {
    _planes: [CFRetained<CVMetalTexture>; 2],
}

// CVMetalTexture is a retained CoreVideo/Metal bridge object designed to keep
// IOSurface planes alive across GPU submission. It is never mutated here.
#[cfg(target_os = "macos")]
unsafe impl Send for RetainedMetalPlanes {}
#[cfg(target_os = "macos")]
unsafe impl Sync for RetainedMetalPlanes {}

#[cfg(target_os = "macos")]
struct MacMetalTextureCache {
    cache: CFRetained<CVMetalTextureCache>,
}

struct Nv12PaintCallback;

impl NativeNv12Renderer {
    pub(crate) fn new(context: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let render_state = context
            .wgpu_render_state
            .clone()
            .ok_or_else(|| "native NV12 renderer requires the wgpu backend".to_owned())?;
        let resources = Nv12Resources::new(&render_state.device, render_state.target_format);
        render_state
            .renderer
            .write()
            .callback_resources
            .insert(resources);
        #[cfg(target_os = "macos")]
        let metal_cache = MacMetalTextureCache::new(&render_state.device)?;
        Ok(Self {
            render_state,
            #[cfg(target_os = "macos")]
            metal_cache,
        })
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn upload(
        &self,
        frame: &DecodedFrame<openipc_video::MacOsVideoFrame>,
    ) -> Result<(), String> {
        if frame.surface.pixel_format() != PixelFormat::Nv12VideoRange {
            return Err(format!(
                "GPU presenter requires video-range NV12, received {:?}",
                frame.surface.pixel_format()
            ));
        }
        let dimensions = frame.dimensions();
        if frame.surface.is_io_surface_backed() {
            let mut renderer = self.render_state.renderer.write();
            let resources = renderer
                .callback_resources
                .get_mut::<Nv12Resources>()
                .ok_or_else(|| "NV12 GPU resources are unavailable".to_owned())?;
            return resources.import_macos(
                &self.render_state.device,
                &self.metal_cache,
                frame.surface.pixel_buffer(),
                [dimensions.width, dimensions.height],
            );
        }
        frame
            .surface
            .with_mapped_planes(|planes| {
                let [y, uv, ..] = planes else {
                    return Err("VideoToolbox NV12 frame did not expose two planes".to_owned());
                };
                self.upload_planes(
                    [dimensions.width, dimensions.height],
                    y.data(),
                    y.stride(),
                    uv.data(),
                    uv.stride(),
                )
            })
            .map_err(|error| error.to_string())?
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn upload(
        &self,
        frame: &DecodedFrame<openipc_video::LinuxVideoFrame>,
    ) -> Result<(), String> {
        if frame.surface.pixel_format() != PixelFormat::Nv12VideoRange {
            return Err(format!(
                "GPU presenter requires video-range NV12, received {:?}",
                frame.surface.pixel_format()
            ));
        }
        let dimensions = frame.dimensions();
        let pitches = frame.surface.plane_pitches();
        frame
            .surface
            .with_mapped_planes(|planes| {
                let [y, uv, ..] = planes else {
                    return Err("VA-API NV12 frame did not expose two planes".to_owned());
                };
                let [y_stride, uv_stride, ..] = pitches.as_slice() else {
                    return Err("VA-API NV12 frame did not expose two pitches".to_owned());
                };
                self.upload_planes(
                    [dimensions.width, dimensions.height],
                    y,
                    *y_stride,
                    uv,
                    *uv_stride,
                )
            })
            .map_err(|error| error.to_string())?
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn upload(
        &self,
        frame: &DecodedFrame<openipc_video::WindowsVideoFrame>,
    ) -> Result<(), String> {
        if frame.surface.pixel_format() != PixelFormat::Nv12VideoRange {
            return Err(format!(
                "GPU presenter requires video-range NV12, received {:?}",
                frame.surface.pixel_format()
            ));
        }
        let mapped = frame
            .surface
            .copy_nv12()
            .map_err(|error| error.to_string())?;
        let dimensions = mapped.dimensions();
        self.upload_planes(
            [dimensions.width, dimensions.height],
            mapped.y_plane(),
            mapped.stride(),
            mapped.uv_plane(),
            mapped.stride(),
        )
    }

    fn upload_planes(
        &self,
        dimensions: [u32; 2],
        y: &[u8],
        y_stride: usize,
        uv: &[u8],
        uv_stride: usize,
    ) -> Result<(), String> {
        let mut renderer = self.render_state.renderer.write();
        let resources = renderer
            .callback_resources
            .get_mut::<Nv12Resources>()
            .ok_or_else(|| "NV12 GPU resources are unavailable".to_owned())?;
        resources.upload(
            &self.render_state.device,
            &self.render_state.queue,
            dimensions,
            y,
            y_stride,
            uv,
            uv_stride,
        )
    }

    pub(crate) fn paint(&self, painter: &egui::Painter, rect: egui::Rect) {
        painter.add(egui_wgpu::Callback::new_paint_callback(
            rect,
            Nv12PaintCallback,
        ));
    }
}

impl Nv12Resources {
    fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nebulus-nv12-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_layout(1),
                texture_layout(2),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nebulus-nv12-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nebulus-nv12-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nebulus-nv12-pipeline"),
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
            label: Some("nebulus-nv12-sampler"),
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

    #[allow(clippy::too_many_arguments)]
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dimensions: [u32; 2],
        y: &[u8],
        y_stride: usize,
        uv: &[u8],
        uv_stride: usize,
    ) -> Result<(), String> {
        let [width, height] = dimensions;
        if let Some(frame) = self
            .frame
            .as_ref()
            .filter(|frame| frame.dimensions == dimensions)
        {
            write_plane(queue, &frame.y_texture, y, y_stride, width, height, 1)?;
            write_plane(
                queue,
                &frame.uv_texture,
                uv,
                uv_stride,
                width.div_ceil(2),
                height.div_ceil(2),
                2,
            )?;
            return Ok(());
        }
        let y_texture = create_texture(
            device,
            "nebulus-nv12-y",
            width,
            height,
            wgpu::TextureFormat::R8Unorm,
        );
        let uv_texture = create_texture(
            device,
            "nebulus-nv12-uv",
            width.div_ceil(2),
            height.div_ceil(2),
            wgpu::TextureFormat::Rg8Unorm,
        );
        write_plane(queue, &y_texture, y, y_stride, width, height, 1)?;
        write_plane(
            queue,
            &uv_texture,
            uv,
            uv_stride,
            width.div_ceil(2),
            height.div_ceil(2),
            2,
        )?;
        let y_view = y_texture.create_view(&Default::default());
        let uv_view = uv_texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nebulus-nv12-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&y_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&uv_view),
                },
            ],
        });
        self.frame = Some(GpuFrame {
            y_texture,
            uv_texture,
            bind_group,
            dimensions,
            #[cfg(target_os = "macos")]
            _core_video_textures: None,
        });
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn import_macos(
        &mut self,
        device: &wgpu::Device,
        cache: &MacMetalTextureCache,
        pixel_buffer: &objc2_core_video::CVPixelBuffer,
        dimensions: [u32; 2],
    ) -> Result<(), String> {
        let [width, height] = dimensions;
        let (y_texture, y_cv) = cache.import_plane(
            device,
            pixel_buffer,
            0,
            width,
            height,
            MTLPixelFormat::R8Unorm,
            wgpu::TextureFormat::R8Unorm,
        )?;
        let (uv_texture, uv_cv) = cache.import_plane(
            device,
            pixel_buffer,
            1,
            width.div_ceil(2),
            height.div_ceil(2),
            MTLPixelFormat::RG8Unorm,
            wgpu::TextureFormat::Rg8Unorm,
        )?;
        let y_view = y_texture.create_view(&Default::default());
        let uv_view = uv_texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nebulus-nv12-metal-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&y_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&uv_view),
                },
            ],
        });
        self.frame = Some(GpuFrame {
            y_texture,
            uv_texture,
            bind_group,
            dimensions,
            _core_video_textures: Some(RetainedMetalPlanes {
                _planes: [y_cv, uv_cv],
            }),
        });
        Ok(())
    }
}

#[cfg(target_os = "macos")]
impl MacMetalTextureCache {
    fn new(device: &wgpu::Device) -> Result<Self, String> {
        use std::{ptr, ptr::NonNull};

        // SAFETY: The cache borrows the raw Metal device only during creation
        // and retains the device internally for its own lifetime.
        let hal_device = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }
            .ok_or_else(|| "wgpu is not using the Metal backend".to_owned())?;
        let mut cache = ptr::null_mut();
        // SAFETY: `cache` is a writable out pointer and the Metal device comes
        // from this exact wgpu device.
        let status = unsafe {
            CVMetalTextureCache::create(
                None,
                None,
                hal_device.raw_device(),
                None,
                NonNull::from(&mut cache),
            )
        };
        if status != kCVReturnSuccess {
            return Err(format!("CVMetalTextureCacheCreate failed: {status}"));
        }
        let cache = NonNull::new(cache)
            .ok_or_else(|| "CVMetalTextureCacheCreate returned null".to_owned())?;
        // SAFETY: CoreVideo Create functions return a +1 retained object.
        Ok(Self {
            cache: unsafe { CFRetained::from_raw(cache) },
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn import_plane(
        &self,
        device: &wgpu::Device,
        pixel_buffer: &objc2_core_video::CVPixelBuffer,
        plane: usize,
        width: u32,
        height: u32,
        metal_format: MTLPixelFormat,
        wgpu_format: wgpu::TextureFormat,
    ) -> Result<(wgpu::Texture, CFRetained<CVMetalTexture>), String> {
        use std::{ptr, ptr::NonNull};

        let mut cv_texture = ptr::null_mut();
        // SAFETY: The cache and pixel buffer are live, dimensions match the
        // selected NV12 plane, and `cv_texture` is a writable out pointer.
        let status = unsafe {
            CVMetalTextureCache::create_texture_from_image(
                None,
                &self.cache,
                pixel_buffer,
                None,
                metal_format,
                width as usize,
                height as usize,
                plane,
                NonNull::from(&mut cv_texture),
            )
        };
        if status != kCVReturnSuccess {
            return Err(format!(
                "CVMetalTextureCacheCreateTextureFromImage plane {plane} failed: {status}"
            ));
        }
        let cv_texture = NonNull::new(cv_texture)
            .ok_or_else(|| format!("CoreVideo returned null for NV12 plane {plane}"))?;
        // SAFETY: CoreVideo Create functions return a +1 retained object.
        let cv_texture = unsafe { CFRetained::from_raw(cv_texture) };
        let raw_texture = CVMetalTextureGetTexture(&cv_texture)
            .ok_or_else(|| format!("CoreVideo returned no Metal texture for plane {plane}"))?;
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        // SAFETY: The Metal texture was created by a cache using this wgpu
        // device, is initialized by VideoToolbox, and matches this descriptor.
        let hal_texture = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                raw_texture,
                wgpu_format,
                MTLTextureType::Type2D,
                1,
                1,
                extent.into(),
            )
        };
        let descriptor = wgpu::TextureDescriptor {
            label: Some("nebulus-videotoolbox-plane"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        // SAFETY: `hal_texture` satisfies the conditions documented above and
        // remains backed by the retained CVMetalTexture stored with the frame.
        let texture = unsafe {
            device.create_texture_from_hal::<wgpu::hal::api::Metal>(hal_texture, &descriptor)
        };
        Ok((texture, cv_texture))
    }
}

impl egui_wgpu::CallbackTrait for Nv12PaintCallback {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(resources) = callback_resources.get::<Nv12Resources>() else {
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

fn create_texture(
    device: &wgpu::Device,
    label: &'static str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
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
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_plane(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    bytes: &[u8],
    stride: usize,
    width: u32,
    height: u32,
    bytes_per_texel: usize,
) -> Result<(), String> {
    let required_row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(bytes_per_texel))
        .ok_or_else(|| "video plane row size overflows usize".to_owned())?;
    let required_len = required_plane_len(stride, required_row_bytes, height)?;
    if bytes.len() < required_len {
        return Err(format!(
            "video plane is truncated: received {} bytes, need at least {required_len}",
            bytes.len()
        ));
    }
    let stride = u32::try_from(stride).map_err(|_| "video plane stride exceeds u32")?;
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
            bytes_per_row: Some(stride),
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

fn required_plane_len(
    stride: usize,
    required_row_bytes: usize,
    height: u32,
) -> Result<usize, String> {
    if height == 0 || required_row_bytes == 0 {
        return Err("video plane dimensions must be non-zero".to_owned());
    }
    if stride < required_row_bytes {
        return Err(format!(
            "video plane stride {stride} is smaller than its {required_row_bytes}-byte row"
        ));
    }
    usize::try_from(height - 1)
        .ok()
        .and_then(|rows| rows.checked_mul(stride))
        .and_then(|prefix| prefix.checked_add(required_row_bytes))
        .ok_or_else(|| "video plane byte length overflows usize".to_owned())
}

#[cfg(test)]
mod tests {
    use super::required_plane_len;

    #[test]
    fn plane_length_allows_padding_after_every_nonfinal_row() {
        assert_eq!(required_plane_len(8, 6, 3), Ok(22));
    }

    #[test]
    fn plane_length_rejects_short_stride() {
        assert!(required_plane_len(5, 6, 3).unwrap_err().contains("smaller"));
    }
}
