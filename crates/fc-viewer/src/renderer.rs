//! The GPU pass — wgpu, surface-less: a flat-shaded render of the
//! assembled [`Scene`] into an offscreen texture, read back as RGBA.
//!
//! Everything structural (objects, triangles, colors, flags) is decided
//! on the CPU in [`crate::scene`]; this module only rasterizes. The
//! renderer is therefore thin: one vertex buffer (interleaved
//! position/normal/color), one uniform (view-projection + headlight),
//! one pipeline, one draw call.
//!
//! The interactive feature's window path reuses [`render_to_view`] with
//! a surface view instead of an offscreen texture — same shading, same
//! camera math.

use crate::camera::Camera;
use crate::scene::Scene;

use wgpu::util::DeviceExt as _;

/// A rendered frame: tightly-packed RGBA8 rows, top row first (matching
/// [`Camera::screen_pixel`](crate::camera::Camera::screen_pixel)'s
/// top-left-origin convention).
pub struct GpuFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// GPU device context — one per process is enough; adapters are
/// requested on demand (the interactive example adds a surface to the
/// same instance).
pub struct GpuContext {
    pub instance: wgpu::Instance,
    #[allow(dead_code)] // kept for surface capability queries in examples
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Acquire an adapter + device (blocking; `pollster` bridges wgpu's
/// async setup — no async runtime is pulled into the workspace).
pub fn create_context() -> Result<GpuContext, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        compatible_surface: None,
        ..Default::default()
    }))
    .map_err(|_| {
        "no suitable GPU adapter found — rendering needs Vulkan/Metal/D3D \
         (or a software adapter such as lavapipe; see the CI viewer job)"
            .to_string()
    })?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("fc-viewer"),
        ..Default::default()
    }))
    .map_err(|e| format!("GPU device setup failed: {e}"))?;
    Ok(GpuContext {
        instance,
        adapter,
        device,
        queue,
    })
}

/// Interleaved vertex soup: `[position(3f), normal(3f), color(4f)]` per
/// vertex — 10 floats, 40 bytes.
fn scene_vertices(scene: &Scene) -> Vec<f32> {
    let mut data = Vec::with_capacity(scene.triangle_count() * 3 * 10);
    for object in &scene.objects {
        for (p, n) in object.positions.iter().zip(object.normals.iter()) {
            data.extend_from_slice(&[p[0] as f32, p[1] as f32, p[2] as f32]);
            data.extend_from_slice(&[n[0] as f32, n[1] as f32, n[2] as f32]);
            data.extend_from_slice(&object.color);
        }
    }
    data
}

/// `f32` slice → little-endian bytes (upload form for GPU buffers).
fn f32_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for v in values {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// Uniform block: view-projection (column-major, as WGSL expects) +
/// world-space headlight (w unused).
fn uniform_bytes(camera: &Camera, aspect: f64) -> Vec<u8> {
    let a = camera.view_projection(aspect);
    let mut bytes = vec![0u8; 80];
    for c in 0..4 {
        for r in 0..4 {
            let value = a[r][c] as f32;
            bytes[c * 16 + r * 4..c * 16 + r * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    let light = camera.headlight_world();
    for i in 0..3 {
        bytes[64 + i * 4..68 + i * 4].copy_from_slice(&(light[i] as f32).to_le_bytes());
    }
    bytes
}

const SHADER: &str = r"
struct Uniforms {
    view_proj: mat4x4<f32>,
    light: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
};
@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.pos = u.view_proj * vec4<f32>(in.pos, 1.0);
    out.normal = in.normal;
    out.color = in.color;
    return out;
}
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    // svg.rs's headlight shading: lambert + floor, so unlit faces stay
    // readable against the black background.
    let lambert = max(dot(n, normalize(u.light.xyz)), 0.0);
    let shade = 0.35 + 0.65 * lambert;
    return vec4<f32>(in.color.rgb * shade, in.color.a);
}
";

/// Record one full render of `scene` into `color` (with `depth`), and
/// submit it. Used by the offscreen path (textures) and the interactive
/// example (surface view) alike. Buffers and pipeline are rebuilt per
/// call — the viewer is not a game loop; simplicity beats caching here.
pub fn render_to_view(
    scene: &Scene,
    camera: &Camera,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    ctx: &GpuContext,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let device = &ctx.device;

    let vertices = scene_vertices(scene);
    if vertices.is_empty() {
        // Nothing to draw: still clear the target (an honest empty
        // frame, not a stale one).
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fc-viewer empty"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        ctx.queue.submit([encoder.finish()]);
        return Ok(());
    }

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("fc-viewer vertices"),
        contents: &f32_bytes(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("fc-viewer uniforms"),
        contents: &uniform_bytes(camera, width as f64 / height as f64),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fc-viewer shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fc-viewer bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(80),
            },
            count: None,
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fc-viewer bg"),
        layout: &bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fc-viewer layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("fc-viewer pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 40,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 12,
                        shader_location: 1,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 24,
                        shader_location: 2,
                    },
                ],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24Plus,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fc-viewer pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.draw(0..(vertices.len() as u32 / 10), 0..1);
    }
    ctx.queue.submit([encoder.finish()]);
    Ok(())
}

/// Render `scene` offscreen (texture targets) and read the frame back.
pub fn render_offscreen(
    scene: &Scene,
    camera: &Camera,
    width: u32,
    height: u32,
) -> Result<GpuFrame, String> {
    if width == 0 || height == 0 {
        return Err(format!(
            "render target must be non-empty, got {width}x{height}"
        ));
    }
    let ctx = create_context()?;
    let device = &ctx.device;

    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fc-viewer color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fc-viewer depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24Plus,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

    render_to_view(scene, camera, &color_view, &depth_view, &ctx, width, height)?;

    // Read back: rows padded to the 256-byte COPY_BYTES_PER_ROW_ALIGNMENT.
    let bytes_per_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fc-viewer readback"),
        size: u64::from(bytes_per_row) * u64::from(height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &color,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    ctx.queue.submit([encoder.finish()]);

    // Map and unpad.
    let (tx, rx) = std::sync::mpsc::channel();
    readback.map_async(wgpu::MapMode::Read, .., move |result| {
        let _ = tx.send(result);
    });
    ctx.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|e| format!("GPU wait failed: {e:?}"))?;
    rx.recv_timeout(std::time::Duration::from_secs(30))
        .map_err(|_| "GPU readback timed out".to_string())?
        .map_err(|e| format!("GPU readback failed: {e:?}"))?;

    let padded = readback
        .slice(..)
        .get_mapped_range()
        .map_err(|e| format!("GPU map failed: {e:?}"))?;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let start = row * bytes_per_row as usize;
        rgba.extend_from_slice(&padded[start..start + (width * 4) as usize]);
    }
    drop(padded);
    readback.unmap();

    Ok(GpuFrame {
        width,
        height,
        rgba,
    })
}
