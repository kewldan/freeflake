//! wgpu renderer for freeflake.
//!
//! One [`Renderer`] owns the device and pipelines; every overlay window gets
//! its own [`Target`] that covers a part of the desktop. Geometry is uploaded
//! once per frame with [`Renderer::upload`] and then drawn into each target.

mod draw_list;

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use bytemuck::{Pod, Zeroable};
pub use draw_list::{Disc, DrawList, MeshVertex};
use freeflake_core::Rect;
pub use wgpu;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewportUniform {
    origin: [f32; 2],
    size: [f32; 2],
    scale: f32,
    _pad: [f32; 3],
}

/// GPU device shared by all windows, including the settings UI.
#[derive(Clone)]
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Picks a low-power adapter that can present to `surface`.
    pub fn new(instance: wgpu::Instance, surface: &wgpu::Surface<'_>) -> Result<Self> {
        Self::create(instance, Some(surface))
    }

    /// A device without any window, for offscreen rendering.
    pub fn headless() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        Self::create(instance, None)
    }

    fn create(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<Self> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))
        .context("no suitable GPU adapter")?;
        log::info!("GPU adapter: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("freeflake"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .context("failed to create GPU device")?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }
}

/// One window's surface covering `viewport` of the desktop.
pub struct Target {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    viewport: Rect,
    scale: f32,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    dirty: bool,
    /// Whether the last presented frame was empty; lets idle frames be skipped.
    blank: bool,
}

impl Target {
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    /// Updates the covered desktop area and the pixel size of the surface.
    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        if self.viewport != viewport || self.scale != scale {
            self.viewport = viewport;
            self.scale = scale;
            self.dirty = true;
        }
    }
}

pub struct Renderer {
    gpu: Gpu,
    format: wgpu::TextureFormat,
    mesh_pipeline: wgpu::RenderPipeline,
    disc_pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    mesh_buffer: GrowableBuffer,
    disc_buffer: GrowableBuffer,
    mesh_count: u32,
    mesh_split: u32,
    disc_count: u32,
    scratch: Vec<MeshVertex>,
}

impl Renderer {
    /// Creates the renderer for surfaces compatible with `first`.
    pub fn new(gpu: Gpu, first: &wgpu::Surface<'_>) -> Result<Self> {
        let caps = first.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| anyhow!("surface reports no formats"))?;
        Ok(Self::with_format(gpu, format))
    }

    /// Creates a renderer drawing into textures of `format`.
    pub fn with_format(gpu: Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let target = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let pipeline = |label, vs, fs, buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &target,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mesh_pipeline = pipeline(
            "mesh",
            "mesh_vs",
            "mesh_fs",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<MeshVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
            })],
        );
        let disc_pipeline = pipeline(
            "disc",
            "disc_vs",
            "disc_fs",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Disc>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32x4, 4 => Float32
                ],
            })],
        );
        Self {
            mesh_buffer: GrowableBuffer::new(device, "mesh", 64 * 1024),
            disc_buffer: GrowableBuffer::new(device, "discs", 64 * 1024),
            gpu,
            format,
            mesh_pipeline,
            disc_pipeline,
            bind_layout,
            mesh_count: 0,
            mesh_split: 0,
            disc_count: 0,
            scratch: Vec::new(),
        }
    }

    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Wraps a surface that will show `viewport` of the desktop.
    pub fn create_target(
        &self,
        surface: wgpu::Surface<'static>,
        viewport: Rect,
        scale: f32,
    ) -> Result<Target> {
        let caps = surface.get_capabilities(&self.gpu.adapter);
        let alpha_mode = [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ]
        .into_iter()
        .find(|m| caps.alpha_modes.contains(m))
        .unwrap_or(caps.alpha_modes[0]);
        if alpha_mode == wgpu::CompositeAlphaMode::Opaque {
            log::warn!("surface does not support transparency");
        }
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::AutoVsync
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.format,
            width: 1,
            height: 1,
            present_mode,
            desired_maximum_frame_latency: 1,
            alpha_mode,
            view_formats: vec![],
            ..surface
                .get_default_config(&self.gpu.adapter, 1, 1)
                .context("surface is not supported by the adapter")?
        };
        let uniform = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport"),
            size: size_of::<ViewportUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("viewport"),
                layout: &self.bind_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }],
            });
        Ok(Target {
            surface,
            config,
            viewport,
            scale,
            uniform,
            bind_group,
            dirty: true,
            blank: false,
        })
    }

    /// Uploads this frame's geometry, shared by all targets.
    pub fn upload(&mut self, list: &DrawList) {
        let gpu = &self.gpu;
        self.scratch.clear();
        self.scratch.extend_from_slice(&list.mesh);
        self.scratch.extend_from_slice(&list.mesh_top);
        self.mesh_buffer
            .write(gpu, bytemuck::cast_slice(&self.scratch));
        self.disc_buffer
            .write(gpu, bytemuck::cast_slice(&list.discs));
        self.mesh_count = self.scratch.len() as u32;
        self.mesh_split = list.mesh.len() as u32;
        self.disc_count = list.discs.len() as u32;
    }

    /// Draws the uploaded geometry into `target` and presents it.
    pub fn render(&self, target: &mut Target) {
        let empty = self.mesh_count == 0 && self.disc_count == 0;
        if empty && target.blank && !target.dirty {
            return;
        }
        if target.dirty {
            let width = (target.viewport.w * target.scale).round().max(1.0) as u32;
            let height = (target.viewport.h * target.scale).round().max(1.0) as u32;
            target.config.width = width;
            target.config.height = height;
            target.surface.configure(&self.gpu.device, &target.config);
            let uniform = ViewportUniform {
                origin: [target.viewport.x, target.viewport.y],
                size: [target.viewport.w, target.viewport.h],
                scale: target.scale,
                _pad: [0.0; 3],
            };
            self.gpu
                .queue
                .write_buffer(&target.uniform, 0, bytemuck::bytes_of(&uniform));
            target.dirty = false;
        }
        let frame = match target.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                target.dirty = true;
                return;
            }
            other => {
                log::debug!("skipping frame: {other:?}");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let commands = self.encode(&view, &target.bind_group, wgpu::Color::TRANSPARENT);
        self.gpu.queue.submit([commands]);
        self.gpu.queue.present(frame);
        target.blank = empty;
    }

    /// Records drawing the uploaded geometry into `view`.
    fn encode(
        &self,
        view: &wgpu::TextureView,
        bind_group: &wgpu::BindGroup,
        clear: wgpu::Color,
    ) -> wgpu::CommandBuffer {
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("overlay"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, bind_group, &[]);
            let mesh_len = self.mesh_count as u64 * size_of::<MeshVertex>() as u64;
            if self.mesh_split > 0 {
                pass.set_pipeline(&self.mesh_pipeline);
                pass.set_vertex_buffer(0, self.mesh_buffer.slice(mesh_len));
                pass.draw(0..self.mesh_split, 0..1);
            }
            if self.disc_count > 0 {
                let len = self.disc_count as u64 * size_of::<Disc>() as u64;
                pass.set_pipeline(&self.disc_pipeline);
                pass.set_vertex_buffer(0, self.disc_buffer.slice(len));
                pass.draw(0..6, 0..self.disc_count);
            }
            if self.mesh_count > self.mesh_split {
                pass.set_pipeline(&self.mesh_pipeline);
                pass.set_vertex_buffer(0, self.mesh_buffer.slice(mesh_len));
                pass.draw(self.mesh_split..self.mesh_count, 0..1);
            }
        }
        encoder.finish()
    }

    fn viewport_bind_group(&self, viewport: Rect, scale: f32) -> (wgpu::Buffer, wgpu::BindGroup) {
        let uniform = ViewportUniform {
            origin: [viewport.x, viewport.y],
            size: [viewport.w, viewport.h],
            scale,
            _pad: [0.0; 3],
        };
        let buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport"),
            size: size_of::<ViewportUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.gpu
            .queue
            .write_buffer(&buffer, 0, bytemuck::bytes_of(&uniform));
        let bind_group = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("viewport"),
                layout: &self.bind_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
        (buffer, bind_group)
    }

    /// Renders the uploaded geometry over `background` into an RGBA8 image.
    /// Used for snapshot tests and screenshots without a display.
    pub fn render_offscreen(
        &self,
        viewport: Rect,
        scale: f32,
        background: [f64; 3],
    ) -> Result<Image> {
        let width = (viewport.w * scale).round() as u32;
        let height = (viewport.h * scale).round() as u32;
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let (_uniform, bind_group) = self.viewport_bind_group(viewport, scale);
        let [r, g, b] = background;
        let draw = self.encode(&view, &bind_group, wgpu::Color { r, g, b, a: 1.0 });

        let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.gpu.queue.submit([draw, encoder.finish()]);
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
        let data = slice.get_mapped_range().map_err(|e| anyhow!("{e:?}"))?;
        let bgra = matches!(
            self.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            let line = &data[y * row as usize..][..width as usize * 4];
            for px in line.as_chunks::<4>().0 {
                if bgra {
                    pixels.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                } else {
                    pixels.extend_from_slice(px);
                }
            }
        }
        Ok(Image {
            width,
            height,
            pixels,
        })
    }
}

/// An RGBA8 image read back from the GPU.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// A vertex buffer that grows to fit its contents.
struct GrowableBuffer {
    label: &'static str,
    buffer: Arc<wgpu::Buffer>,
}

impl GrowableBuffer {
    fn new(device: &wgpu::Device, label: &'static str, size: u64) -> Self {
        Self {
            label,
            buffer: Arc::new(Self::create(device, label, size)),
        }
    }

    fn create(device: &wgpu::Device, label: &'static str, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn write(&mut self, gpu: &Gpu, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        if data.len() as u64 > self.buffer.size() {
            let size = (data.len() as u64).next_power_of_two();
            self.buffer = Arc::new(Self::create(&gpu.device, self.label, size));
        }
        gpu.queue.write_buffer(&self.buffer, 0, data);
    }

    fn slice(&self, len: u64) -> wgpu::BufferSlice<'_> {
        self.buffer.slice(..len)
    }
}
