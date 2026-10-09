//! Mönsterkällor: ritas av GPU:n varje bildruta till källans textur
//! (se `shaders/generator.wgsl`), så att de fungerar som vilken källa som helst.

use egui_wgpu::wgpu;

const UNIFORM_SIZE: u64 = 48;

pub(crate) struct GeneratorPass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

/// Uniformbuffert och bindning för en mönsterkälla.
pub(crate) struct GeneratorState {
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl GeneratorPass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("generator"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../shaders/generator.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("generator"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("generator"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("generator"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        GeneratorPass { pipeline, layout }
    }

    pub fn make_state(&self, device: &wgpu::Device) -> GeneratorState {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("generator"),
            size: UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("generator"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        GeneratorState { uniform, bind_group }
    }

    /// Ritar mönstret till `target`. `time` = taktslag × fart.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        state: &GeneratorState,
        target: &wgpu::Texture,
        pattern: u32,
        time: f32,
        colors: [[f32; 3]; 2],
    ) {
        let size = target.size();
        let aspect = size.width as f32 / size.height.max(1) as f32;
        let [a, b] = colors;
        let floats = [pattern as f32, time, aspect, 1.0, a[0], a[1], a[2], 1.0, b[0], b[1], b[2], 1.0];
        let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        queue.write_buffer(&state.uniform, 0, &bytes);
        let view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("generator"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &state.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
