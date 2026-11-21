//! Host-provided pixels blended into the SAME native presentation frame as
//! ProCraft, before `queue.present`. Unlike Android UI overlays, this uses
//! the camera the ProCraft GPU actually rendered in that very frame.
//!
//! Pixels are a provisional non-interleaved normal-alpha layer. They remain
//! transient; ProCraft's document and layer history never own these bytes.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use procraft_engine::geom::Affine;
use procraft_gpu::{Gpu, wgpu};

static NEXT_SERIAL: AtomicU64 = AtomicU64::new(1);
const MAX_PIXELS: usize = 4_000_000;

#[derive(Clone)]
pub(crate) struct ExternalOverlay {
    pixels: Arc<[u8]>,
    width: u32,
    height: u32,
    viewport: (u32, u32),
    camera: Affine,
    serial: u64,
}

impl ExternalOverlay {
    pub(crate) fn new(pixels: Vec<u8>, width: u32, height: u32, viewport: (u32, u32), matrix: &[f32]) -> Option<Self> {
        let count = (width as usize).checked_mul(height as usize)?;
        if count == 0
            || count > MAX_PIXELS
            || width > 4096
            || height > 4096
            || viewport.0 == 0
            || viewport.1 == 0
            || viewport.0 > 8192
            || viewport.1 > 8192
            || count.checked_mul(4)? != pixels.len()
            || matrix.len() != 6
            || matrix.iter().any(|v| !v.is_finite())
        {
            return None;
        }
        let camera = Affine { a: matrix[0], b: matrix[1], c: matrix[2], d: matrix[3], e: matrix[4], f: matrix[5] };
        camera.inverse()?;
        Some(Self {
            pixels: pixels.into(),
            width,
            height,
            viewport,
            camera,
            serial: NEXT_SERIAL.fetch_add(1, Ordering::Relaxed),
        })
    }

    /// Target frame pixel -> original render's normalized texture coordinates.
    /// The output device frame and the source VectorCraft frame are positioned
    /// with exactly the native document cameras from their respective renders.
    fn source_uv(&self, current: Affine) -> Option<[[f32; 4]; 2]> {
        let m = current.inverse()?.then(self.camera);
        let sx = self.viewport.0 as f32;
        let sy = self.viewport.1 as f32;
        Some([[m.a / sx, m.c / sx, m.e / sx, 0.0], [m.b / sy, m.d / sy, m.f / sy, 0.0]])
    }
}

/// Owned exclusively by the ProCraft native render thread. GPU texture only
/// updates on a new vector frame; camera updates just rewrite 32 uniform bytes.
pub(crate) struct OverlayGpu {
    format: Option<wgpu::TextureFormat>,
    pipeline: Option<wgpu::RenderPipeline>,
    layout: Option<wgpu::BindGroupLayout>,
    sampler: Option<wgpu::Sampler>,
    uniform: Option<wgpu::Buffer>,
    texture: Option<wgpu::Texture>,
    bind: Option<wgpu::BindGroup>,
    size: (u32, u32),
    serial: u64,
}

impl OverlayGpu {
    pub(crate) fn new() -> Self {
        Self {
            format: None,
            pipeline: None,
            layout: None,
            sampler: None,
            uniform: None,
            texture: None,
            bind: None,
            size: (0, 0),
            serial: 0,
        }
    }

    pub(crate) fn draw(
        &mut self,
        gpu: &Gpu,
        frame: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        current: Affine,
        source: &ExternalOverlay,
    ) {
        let Some(mapping) = source.source_uv(current) else { return };
        let device = &gpu.device;
        if self.format != Some(format) || self.pipeline.is_none() {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Diverge same-surface composite"),
                source: wgpu::ShaderSource::Wgsl(include_str!("external_overlay.wgsl").into()),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Diverge external raster binding"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Diverge external layer layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let blend = wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            };
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Diverge same-surface vector composite"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            self.pipeline = Some(pipeline);
            self.format = Some(format);
            self.layout = Some(layout);
            self.sampler = Some(device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Diverge vector filtered image"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }));
            self.uniform = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Diverge vector camera"),
                size: 32,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.bind = None;
        }
        if self.size != (source.width, source.height) || self.texture.is_none() {
            self.texture = Some(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Diverge native VectorCraft pixels"),
                size: wgpu::Extent3d { width: source.width, height: source.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }));
            self.size = (source.width, source.height);
            self.bind = None;
        }
        if self.bind.is_none() {
            let view = self.texture.as_ref().expect("texture created").create_view(&Default::default());
            self.bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Diverge vector frame binding"),
                layout: self.layout.as_ref().expect("binding layout created"),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(self.sampler.as_ref().expect("sampler created")),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.uniform.as_ref().expect("uniform created").as_entire_binding(),
                    },
                ],
            }));
        }
        if self.serial != source.serial {
            gpu.queue.write_texture(
                self.texture.as_ref().expect("texture created").as_image_copy(),
                source.pixels.as_ref(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(source.width * 4),
                    rows_per_image: Some(source.height),
                },
                wgpu::Extent3d { width: source.width, height: source.height, depth_or_array_layers: 1 },
            );
            self.serial = source.serial;
        }
        let mut camera_bytes = [0u8; 32];
        for (chunk, value) in camera_bytes.chunks_exact_mut(4).zip(mapping.into_iter().flatten()) {
            chunk.copy_from_slice(&value.to_ne_bytes());
        }
        gpu.queue.write_buffer(self.uniform.as_ref().expect("uniform created"), 0, &camera_bytes);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Diverge vector composite commands"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Diverge native vector atop Sketch"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(self.pipeline.as_ref().expect("pipeline created"));
            pass.set_bind_group(0, self.bind.as_ref().expect("bind group created"), &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shader_builds_and_blends_in_a_single_wgpu_surface_frame() {
        // Exercise WGSL/pipeline validity, not just the affine math.
        // CI without a GPU may legitimately skip this fixture.
        let gpu = match Gpu::headless() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("headless GPU unavailable: {e}");
                return;
            }
        };
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Diverge compositor test target"),
            size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Diverge test clear") });
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Diverge test clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        gpu.queue.submit([encoder.finish()]);
        let source = ExternalOverlay::new(vec![255; 32 * 32 * 4], 32, 32, (64, 64), &[1., 0., 0., 1., 0., 0.]).unwrap();
        let mut pass = OverlayGpu::new();
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        pass.draw(&gpu, &view, wgpu::TextureFormat::Rgba8Unorm, Affine::IDENTITY, &source);
        // Repeat on a different camera. This updates only the shader uniform,
        // reusing exactly the same RGBA texture and native render pass.
        pass.draw(&gpu, &view, wgpu::TextureFormat::Rgba8Unorm, Affine { e: 12., f: -6., ..Affine::IDENTITY }, &source);
        let error = pollster::block_on(scope.pop());
        assert!(error.is_none(), "WGPU overlay validation: {error:?}");
    }

    #[test]
    fn cached_frame_uses_current_native_camera_in_same_surface() {
        let rgba = vec![255u8; 100 * 50 * 4];
        let frame = ExternalOverlay::new(rgba, 100, 50, (200, 100), &[2., 0., 0., 2., 0., 0.]).unwrap();
        let identity = Affine::IDENTITY;
        let m = frame.source_uv(identity).unwrap();
        assert!((m[0][0] - 0.01).abs() < 1e-6);
        assert!((m[1][1] - 0.02).abs() < 1e-6);
        let shifted = Affine { e: 10., f: 20., ..identity };
        let shifted = frame.source_uv(shifted).unwrap();
        assert!((shifted[0][2] + 0.1).abs() < 1e-6);
        assert!((shifted[1][2] + 0.4).abs() < 1e-6);
        assert!(ExternalOverlay::new(vec![0; 8], 100, 50, (200, 100), &[1.; 6]).is_none());
    }
}
