//! GPU-rendering. Varje utgång renderas till en offscreen-textur som visas
//! både i projektorfönstret och i editorns förhandsvisning (WYSIWYG).

use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use lm_core::{BlendMode, Mask, OutputId, Project, Shape, SourceId, Surface, SurfaceId, MASK_MAX_POINTS, MASK_MIN_POINTS, UNIT_QUAD};
use lm_geom::Homography;
use lm_media::FrameView;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use egui::TextureId;

pub use egui_wgpu;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const MSAA: u32 = 4;
const UNIFORM_SIZE: u64 = 48 + 48 + 16 + 16 + 16 * 16;
/// Antal trianglar per meshcell och riktning – tillräckligt för mjuka kurvor.
const MESH_SUBDIV: usize = 12;

#[repr(C)]
#[derive(Clone, Copy)]
struct SurfaceUniform {
    h: [[f32; 4]; 3],
    h2: [[f32; 4]; 3],
    params: [f32; 4],
    res: [f32; 4],
    mask: [[f32; 4]; 16],
}

/// Position på utgången + punkt som homografin avbildar på källan.
type Vertex = [f32; 4];
const VERTEX_SIZE: u64 = 16;

/// Ett ritanrop: vilken utgång, vilken textur (`None` = testbild), uniform och hörn.
struct Draw {
    output: OutputId,
    texture: Option<SourceId>,
    blend: BlendMode,
    uniform: SurfaceUniform,
    vertices: Range<u32>,
}

impl SurfaceUniform {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(UNIFORM_SIZE as usize);
        let floats = self
            .h
            .iter()
            .flatten()
            .chain(self.h2.iter().flatten())
            .chain(self.params.iter())
            .chain(self.res.iter())
            .chain(self.mask.iter().flatten());
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
    /// En pipeline per blandningsläge, i samma ordning som `BlendMode::ALL`.
    pipelines: [wgpu::RenderPipeline; 4],
    sampler: wgpu::Sampler,
    tex_layout: wgpu::BindGroupLayout,
    uni_layout: wgpu::BindGroupLayout,
    uniform_buf: wgpu::Buffer,
    uniform_bg: wgpu::BindGroup,
    uniform_stride: u64,
    uniform_capacity: u64,
    vertex_buf: wgpu::Buffer,
    vertex_capacity: u64,
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
        let pipelines = BlendMode::ALL.map(|mode| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(mode.label()),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX_SIZE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                })],
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
                    blend: Some(blend_state(mode)),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        }));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("source"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform_stride = uniform_stride(device.limits().min_uniform_buffer_offset_alignment as u64);
        let uniform_capacity = 64;
        let (uniform_buf, uniform_bg) = make_uniforms(device, &uni_layout, uniform_stride, uniform_capacity);
        let vertex_capacity = 4096;
        let vertex_buf = make_vertices(device, vertex_capacity);

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
            pipelines,
            sampler,
            tex_layout,
            uni_layout,
            uniform_buf,
            uniform_bg,
            uniform_stride,
            uniform_capacity,
            vertex_buf,
            vertex_capacity,
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

        // Samla alla ritanrop och deras hörn.
        let mut draws: Vec<Draw> = Vec::new();
        let mut vertices: Vec<Vertex> = Vec::new();
        for out in &project.outputs {
            if opts.blackout {
                continue;
            }
            if opts.output_test {
                let geo = quad_geometry(&UNIT_QUAD, &UNIT_QUAD, 1.0, false).unwrap();
                push_draw(&mut draws, &mut vertices, out.id, None, BlendMode::Normal, geo);
                continue;
            }
            for s in project.surfaces.iter().filter(|s| s.output == out.id && s.visible) {
                let test = opts.test_surfaces.contains(&s.id);
                let tex = if test { None } else { s.source };
                if !test && tex.is_none_or(|id| !self.sources.contains_key(&id)) {
                    continue;
                }
                if let Some(mut geo) = surface_geometry(s, test) {
                    apply_mask(&mut geo.0, s.mask.as_ref(), out.resolution);
                    push_draw(&mut draws, &mut vertices, out.id, tex, s.blend, geo);
                }
            }
        }

        if vertices.len() as u64 > self.vertex_capacity {
            self.vertex_capacity = (vertices.len() as u64).next_power_of_two();
            self.vertex_buf = make_vertices(device, self.vertex_capacity);
        }
        if !vertices.is_empty() {
            let bytes: Vec<u8> = vertices.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
            queue.write_buffer(&self.vertex_buf, 0, &bytes);
        }

        if draws.len() as u64 > self.uniform_capacity {
            self.uniform_capacity = (draws.len() as u64).next_power_of_two();
            let (b, g) = make_uniforms(device, &self.uni_layout, self.uniform_stride, self.uniform_capacity);
            self.uniform_buf = b;
            self.uniform_bg = g;
        }
        let mut data = vec![0u8; (self.uniform_stride * draws.len().max(1) as u64) as usize];
        for (i, d) in draws.iter().enumerate() {
            let o = i * self.uniform_stride as usize;
            data[o..o + UNIFORM_SIZE as usize].copy_from_slice(&d.uniform.bytes());
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
            pass.set_vertex_buffer(0, self.vertex_buf.slice(..));
            for (i, d) in draws.iter().enumerate() {
                if d.output != out.id {
                    continue;
                }
                let bg = match d.texture {
                    Some(id) => &self.sources[&id].bind_group,
                    None => &self.test.bind_group,
                };
                pass.set_pipeline(&self.pipelines[d.blend as usize]);
                pass.set_bind_group(0, &self.uniform_bg, &[(i as u64 * self.uniform_stride) as u32]);
                pass.set_bind_group(1, bg, &[]);
                pass.draw(d.vertices.clone(), 0..1);
            }
        }
        queue.submit([encoder.finish()]);
    }
}

/// Blandning för förmultiplicerad färg (s = källa·alfa, d = det som redan ritats).
fn blend_state(mode: BlendMode) -> wgpu::BlendState {
    use wgpu::BlendFactor as F;
    let (src_factor, dst_factor) = match mode {
        // s + d·(1 − a)
        BlendMode::Normal => (F::One, F::OneMinusSrcAlpha),
        // s + d
        BlendMode::Add => (F::One, F::One),
        // d·(s + 1 − a): ytan mörkar ner, genomskinliga delar lämnar d orörd.
        BlendMode::Multiply => (F::Dst, F::OneMinusSrcAlpha),
        // d + s·(1 − d)
        BlendMode::Screen => (F::OneMinusDst, F::One),
    };
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor,
            dst_factor,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent::OVER,
    }
}

/// Avstånd mellan uniformerna i bufferten: minst en uniform, och en multipel
/// av GPU:ns krav på justering för dynamiska offset.
fn uniform_stride(alignment: u64) -> u64 {
    UNIFORM_SIZE.next_multiple_of(alignment.max(1))
}

fn push_draw(
    draws: &mut Vec<Draw>,
    vertices: &mut Vec<Vertex>,
    output: OutputId,
    texture: Option<SourceId>,
    blend: BlendMode,
    (uniform, verts): (SurfaceUniform, Vec<Vertex>),
) {
    let start = vertices.len() as u32;
    vertices.extend(verts);
    draws.push(Draw {
        output,
        texture,
        blend,
        uniform,
        vertices: start..vertices.len() as u32,
    });
}

/// `h2`: vertexpunkt → ytans koordinater, `h`: ytans koordinater → källan.
fn surface_uniform(h2: Homography, h: Homography, opacity: f32) -> SurfaceUniform {
    SurfaceUniform {
        h: h.to_gpu(),
        h2: h2.to_gpu(),
        params: [opacity.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
        res: [1.0, 1.0, 0.0, 0.0],
        mask: [[0.0; 4]; 16],
    }
}

fn apply_mask(u: &mut SurfaceUniform, mask: Option<&Mask>, resolution: [u32; 2]) {
    u.res[0] = resolution[0].max(1) as f32;
    u.res[1] = resolution[1].max(1) as f32;
    let Some(m) = mask.filter(|m| m.points.len() >= MASK_MIN_POINTS) else { return };
    let pts = &m.points[..m.points.len().min(MASK_MAX_POINTS)];
    for (i, p) in pts.iter().enumerate() {
        u.mask[i / 2][(i % 2) * 2] = p[0];
        u.mask[i / 2][(i % 2) * 2 + 1] = p[1];
    }
    u.params[1] = pts.len() as f32;
    u.params[2] = m.feather.max(0.0);
    u.params[3] = if m.invert { 1.0 } else { 0.0 };
}

/// Uniform och trianglar för en yta. `test` = visa testbilden över hela ytan.
fn surface_geometry(s: &Surface, test: bool) -> Option<(SurfaceUniform, Vec<Vertex>)> {
    let src = if test { UNIT_QUAD } else { lm_geom::quad(&s.src_pts)? };
    match s.shape {
        Shape::Quad => quad_geometry(&lm_geom::quad(&s.dst_pts)?, &src, s.opacity, false),
        Shape::Ellipse => quad_geometry(&lm_geom::quad(&s.dst_pts)?, &src, s.opacity, true),
        Shape::Triangle => triangle_geometry(s.dst_pts.get(..3)?.try_into().ok()?, &src, s.opacity),
        Shape::Mesh { cols, rows } => mesh_geometry(&s.dst_pts, cols as usize, rows as usize, &src, s.opacity),
    }
}

/// Fyrhörn (eller ellips inskriven i den): två trianglar, texturen följer
/// homografin utgång → ytans koordinater → källa.
fn quad_geometry(
    dst: &[[f32; 2]; 4],
    src: &[[f32; 2]; 4],
    opacity: f32,
    ellipse: bool,
) -> Option<(SurfaceUniform, Vec<Vertex>)> {
    let h2 = Homography::from_points(dst, &UNIT_QUAD)?;
    let h = Homography::from_points(&UNIT_QUAD, src)?;
    let mut u = surface_uniform(h2, h, opacity);
    u.res[2] = if ellipse { 1.0 } else { 0.0 };
    let v = |i: usize| [dst[i][0], dst[i][1], dst[i][0], dst[i][1]];
    Some((u, [0, 1, 2, 0, 2, 3].map(v).to_vec()))
}

/// Triangel: hörnen hämtar motsvarande triangel ur källans utsnitt.
fn triangle_geometry(dst: &[[f32; 2]; 3], src: &[[f32; 2]; 4], opacity: f32) -> Option<(SurfaceUniform, Vec<Vertex>)> {
    let h = Homography::from_points(&UNIT_QUAD, src)?;
    let uv = lm_geom::TRIANGLE_UV;
    let verts = (0..3).map(|i| [dst[i][0], dst[i][1], uv[i][0], uv[i][1]]).collect();
    Some((surface_uniform(Homography::IDENTITY, h, opacity), verts))
}

/// Mesh: tätt rutnät av trianglar längs splineytan, texturen följer (u, v).
fn mesh_geometry(
    pts: &[[f32; 2]],
    cols: usize,
    rows: usize,
    src: &[[f32; 2]; 4],
    opacity: f32,
) -> Option<(SurfaceUniform, Vec<Vertex>)> {
    if cols < 2 || rows < 2 || pts.len() != cols * rows {
        return None;
    }
    let h = Homography::from_points(&UNIT_QUAD, src)?;
    let (nu, nv) = ((cols - 1) * MESH_SUBDIV + 1, (rows - 1) * MESH_SUBDIV + 1);
    let grid = lm_geom::mesh_grid(pts, cols, rows, nu, nv);
    let vertex = |i: usize, j: usize| {
        let p = grid[j * nu + i];
        [p[0], p[1], i as f32 / (nu - 1) as f32, j as f32 / (nv - 1) as f32]
    };
    let mut out = Vec::with_capacity((nu - 1) * (nv - 1) * 6);
    for j in 0..nv - 1 {
        for i in 0..nu - 1 {
            let (a, b, c, d) = (vertex(i, j), vertex(i + 1, j), vertex(i + 1, j + 1), vertex(i, j + 1));
            out.extend([a, b, c, a, c, d]);
        }
    }
    Some((surface_uniform(Homography::IDENTITY, h, opacity), out))
}

fn make_vertices(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("surface vertices"),
        size: capacity * VERTEX_SIZE,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Shadern valideras annars först när appen startar.
    #[test]
    fn shader_is_valid() {
        let module = naga::front::wgsl::parse_str(include_str!("../../../shaders/surface.wgsl"))
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string("surface.wgsl")));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
            .validate(&module)
            .unwrap();
    }

    /// `pipelines` indexeras med `blend as usize`.
    #[test]
    fn blend_pipeline_order() {
        for (i, mode) in BlendMode::ALL.iter().enumerate() {
            assert_eq!(*mode as usize, i);
        }
    }

    #[test]
    fn uniform_stride_is_aligned() {
        for align in [1, 64, 256] {
            let stride = uniform_stride(align);
            assert!(stride >= UNIFORM_SIZE);
            assert_eq!(stride % align, 0);
        }
    }

    /// Uniformens storlek i Rust måste stämma med WGSL-structen.
    #[test]
    fn uniform_layout_matches_shader() {
        let module = naga::front::wgsl::parse_str(include_str!("../../../shaders/surface.wgsl")).unwrap();
        let ty = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("SurfaceUniform")).unwrap().1;
        let size = ty.inner.size(module.to_ctx()) as u64;
        assert_eq!(size, UNIFORM_SIZE);
        let u = surface_uniform(Homography::IDENTITY, Homography::IDENTITY, 1.0);
        assert_eq!(u.bytes().len() as u64, UNIFORM_SIZE);
    }
}
