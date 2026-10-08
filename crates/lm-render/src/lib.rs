//! GPU-rendering. Varje utgång renderas till en offscreen-textur som visas
//! både i projektorfönstret och i editorns förhandsvisning (WYSIWYG).

use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use lm_core::{OutputId, Project, SourceId, SurfaceId, UNIT_QUAD};
use lm_geom::Homography;
use lm_media::FrameView;
use std::collections::{HashMap, HashSet};
use egui::TextureId;

pub use egui_wgpu;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const MSAA: u32 = 4;
const UNIFORM_SIZE: u64 = 96;

#[repr(C)]
#[derive(Clone, Copy)]
struct SurfaceUniform {
    h: [[f32; 4]; 3],
    dst: [[f32; 2]; 4],
    params: [f32; 4],
}

impl SurfaceUniform {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(UNIFORM_SIZE as usize);
        let floats = self
            .h
            .iter()
            .flatten()
            .chain(self.dst.iter().flatten())
            .chain(self.params.iter());
        for f in floats {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out
    }
}

struct GpuTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    size: [u32; 2],
    egui_id: TextureId,
}

struct GpuOutput {
    resolve_view: wgpu::TextureView,
    msaa_view: wgpu::TextureView,
    size: [u32; 2],
    egui_id: TextureId,
}

/// Vad som ska visas utöver projektet självt.
#[derive(Default)]
pub struct RenderOptions {
    /// Ytor som tillfälligt visar testbild i stället för sin källa.
    pub test_surfaces: HashSet<SurfaceId>,
    /// Testbild över hela utgången (för att rikta in projektorn).
    pub output_test: bool,
    /// Svart utgång (blackout).
    pub blackout: bool,
}

pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    tex_layout: wgpu::BindGroupLayout,
    uni_layout: wgpu::BindGroupLayout,
    uniform_buf: wgpu::Buffer,
    uniform_bg: wgpu::BindGroup,
    uniform_stride: u64,
    uniform_capacity: u64,
    sources: HashMap<SourceId, GpuTexture>,
    test: GpuTexture,
    outputs: HashMap<OutputId, GpuOutput>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, egui: &mut egui_wgpu::Renderer) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("surface"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../shaders/surface.wgsl").into()),
        });
        let uni_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("surface uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE),
                },
                count: None,
            }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("source texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("surface"),
            bind_group_layouts: &[Some(&uni_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("surface"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: MSAA,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("source"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform_stride = (device.limits().min_uniform_buffer_offset_alignment as u64).max(UNIFORM_SIZE);
        let uniform_capacity = 64;
        let (uniform_buf, uniform_bg) = make_uniforms(device, &uni_layout, uniform_stride, uniform_capacity);

        let (tw, th) = (1024, 1024);
        let pattern = lm_media::test_pattern(tw, th);
        let test = make_texture(device, &tex_layout, &sampler, egui, [tw, th], None);
        write_frame(
            queue,
            &test.texture,
            &FrameView {
                width: tw,
                height: th,
                stride: tw * 4,
                data: &pattern,
            },
        );

        Renderer {
            pipeline,
            sampler,
            tex_layout,
            uni_layout,
            uniform_buf,
            uniform_bg,
            uniform_stride,
            uniform_capacity,
            sources: HashMap::new(),
            test,
            outputs: HashMap::new(),
        }
    }

    /// Laddar upp en ny bildruta för en källa (skapar/ändrar storlek på texturen vid behov).
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        egui: &mut egui_wgpu::Renderer,
        id: SourceId,
        frame: &FrameView,
    ) {
        let size = [frame.width, frame.height];
        if self.sources.get(&id).map(|t| t.size) != Some(size) {
            let reuse = self.sources.remove(&id).map(|t| t.egui_id);
            let tex = make_texture(device, &self.tex_layout, &self.sampler, egui, size, reuse);
            self.sources.insert(id, tex);
        }
        write_frame(queue, &self.sources[&id].texture, frame);
    }

    pub fn remove_source(&mut self, egui: &mut egui_wgpu::Renderer, id: SourceId) {
        if let Some(t) = self.sources.remove(&id) {
            egui.free_texture(&t.egui_id);
        }
    }

    pub fn source_texture(&self, id: SourceId) -> Option<(TextureId, [u32; 2])> {
        self.sources.get(&id).map(|t| (t.egui_id, t.size))
    }

    pub fn test_texture(&self) -> TextureId {
        self.test.egui_id
    }

    pub fn output_texture(&self, id: OutputId) -> Option<TextureId> {
        self.outputs.get(&id).map(|o| o.egui_id)
    }

    /// Renderar alla utgångar i projektet till sina offscreen-texturer.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        egui: &mut egui_wgpu::Renderer,
        project: &Project,
        opts: &RenderOptions,
    ) {
        for out in &project.outputs {
            let size = [out.resolution[0].max(16), out.resolution[1].max(16)];
            if self.outputs.get(&out.id).map(|o| o.size) != Some(size) {
                let reuse = self.outputs.remove(&out.id).map(|o| o.egui_id);
                self.outputs.insert(out.id, make_output(device, egui, size, reuse));
            }
        }
        let live: HashSet<_> = project.outputs.iter().map(|o| o.id).collect();
        self.outputs.retain(|id, o| {
            let keep = live.contains(id);
            if !keep {
                egui.free_texture(&o.egui_id);
            }
            keep
        });

        // Samla alla ritanrop: (utgång, textur, uniform).
        let mut draws: Vec<(OutputId, Option<SourceId>, SurfaceUniform)> = Vec::new();
        for out in &project.outputs {
            if opts.blackout {
                continue;
            }
            if opts.output_test {
                draws.push((out.id, None, uniform(&UNIT_QUAD, &UNIT_QUAD, 1.0).unwrap()));
                continue;
            }
            for s in project.surfaces.iter().filter(|s| s.output == out.id && s.visible) {
                let test = opts.test_surfaces.contains(&s.id);
                let tex = if test { None } else { s.source };
                if !test && tex.is_none_or(|id| !self.sources.contains_key(&id)) {
                    continue;
                }
                let (Some(dst), Some(src)) = (lm_geom::quad(&s.dst_pts), lm_geom::quad(&s.src_pts)) else {
                    continue;
                };
                let src = if test { UNIT_QUAD } else { src };
                if let Some(u) = uniform(&dst, &src, s.opacity) {
                    draws.push((out.id, tex, u));
                }
            }
        }

        if draws.len() as u64 > self.uniform_capacity {
            self.uniform_capacity = (draws.len() as u64).next_power_of_two();
            let (b, g) = make_uniforms(device, &self.uni_layout, self.uniform_stride, self.uniform_capacity);
            self.uniform_buf = b;
            self.uniform_bg = g;
        }
        let mut data = vec![0u8; (self.uniform_stride * draws.len().max(1) as u64) as usize];
        for (i, (_, _, u)) in draws.iter().enumerate() {
            let o = i * self.uniform_stride as usize;
            data[o..o + UNIFORM_SIZE as usize].copy_from_slice(&u.bytes());
        }
        queue.write_buffer(&self.uniform_buf, 0, &data);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("lumamap") });
        for out in &project.outputs {
            let target = &self.outputs[&out.id];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("output"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.msaa_view,
                    depth_slice: None,
                    resolve_target: Some(&target.resolve_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            for (i, (oid, tex, _)) in draws.iter().enumerate() {
                if *oid != out.id {
                    continue;
                }
                let bg = match tex {
                    Some(id) => &self.sources[id].bind_group,
                    None => &self.test.bind_group,
                };
                pass.set_bind_group(0, &self.uniform_bg, &[(i as u64 * self.uniform_stride) as u32]);
                pass.set_bind_group(1, bg, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
    }
}

fn uniform(dst: &[[f32; 2]; 4], src: &[[f32; 2]; 4], opacity: f32) -> Option<SurfaceUniform> {
    let h = Homography::from_points(dst, src)?;
    Some(SurfaceUniform {
        h: h.to_gpu(),
        dst: *dst,
        params: [opacity.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
    })
}

fn make_uniforms(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    stride: u64,
    capacity: u64,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("surface uniforms"),
        contents: &vec![0u8; (stride * capacity) as usize],
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("surface uniforms"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buf,
                offset: 0,
                size: wgpu::BufferSize::new(UNIFORM_SIZE),
            }),
        }],
    });
    (buf, bg)
}

fn make_texture(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    egui: &mut egui_wgpu::Renderer,
    size: [u32; 2],
    reuse: Option<TextureId>,
) -> GpuTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("source"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("source"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    let egui_id = register(device, egui, &view, reuse);
    GpuTexture {
        texture,
        bind_group,
        size,
        egui_id,
    }
}

fn make_output(
    device: &wgpu::Device,
    egui: &mut egui_wgpu::Renderer,
    size: [u32; 2],
    reuse: Option<TextureId>,
) -> GpuOutput {
    let desc = |samples, usage| wgpu::TextureDescriptor {
        label: Some("output"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage,
        view_formats: &[],
    };
    let msaa = device.create_texture(&desc(MSAA, wgpu::TextureUsages::RENDER_ATTACHMENT));
    let resolve = device.create_texture(&desc(
        1,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    ));
    let resolve_view = resolve.create_view(&Default::default());
    let egui_id = register(device, egui, &resolve_view, reuse);
    GpuOutput {
        msaa_view: msaa.create_view(&Default::default()),
        resolve_view,
        size,
        egui_id,
    }
}

fn register(
    device: &wgpu::Device,
    egui: &mut egui_wgpu::Renderer,
    view: &wgpu::TextureView,
    reuse: Option<TextureId>,
) -> TextureId {
    match reuse {
        Some(id) => {
            egui.update_egui_texture_from_wgpu_texture(device, view, wgpu::FilterMode::Linear, id);
            id
        }
        None => egui.register_native_texture(device, view, wgpu::FilterMode::Linear),
    }
}

fn write_frame(queue: &wgpu::Queue, texture: &wgpu::Texture, frame: &FrameView) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        frame.data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(frame.stride),
            rows_per_image: Some(frame.height),
        },
        wgpu::Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        },
    );
}
