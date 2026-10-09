//! Kantblandningspasset: multiplicerar varje utgångs bild med en ramp mot
//! kanterna (se `shaders/edge_blend.wgsl`). Ritas sist i utgångens renderpass.

use egui_wgpu::wgpu;
use lm_core::{EdgeBlend, OutputId};
use std::collections::HashMap;

const UNIFORM_SIZE: u64 = 32;

pub(crate) struct EdgePass {
    pipeline: wgpu::RenderPipeline,
    /// Additiv svartnivåkompensation.
    black: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    stride: u64,
    capacity: u64,
}

impl EdgePass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, samples: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("edge blend"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../shaders/edge_blend.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edge blend"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("edge blend"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // Multiplicera det som redan ritats: färg = d · f, alfa orörd.
        let multiply = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::Src,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let make = |entry: &str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("edge blend"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make("fs_main", multiply);
        // Lägg till: färg = d + s, alfa orörd.
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let black = make("fs_black", add);
        let stride = UNIFORM_SIZE.next_multiple_of((device.limits().min_uniform_buffer_offset_alignment as u64).max(1));
        let capacity = 4;
        let (buf, bind_group) = make_buffer(device, &layout, stride, capacity);
        EdgePass {
            pipeline,
            black,
            layout,
            buf,
            bind_group,
            stride,
            capacity,
        }
    }

    /// Laddar upp inställningarna för utgångar med kantblandning. Ger offset
    /// i uniformbufferten per utgång; utgångar utan blandning saknas.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        blends: impl Iterator<Item = (OutputId, EdgeBlend)>,
    ) -> HashMap<OutputId, u32> {
        let active: Vec<_> = blends.filter(|(_, b)| b.is_active()).collect();
        if active.is_empty() {
            return HashMap::new();
        }
        if active.len() as u64 > self.capacity {
            self.capacity = (active.len() as u64).next_power_of_two();
            (self.buf, self.bind_group) = make_buffer(device, &self.layout, self.stride, self.capacity);
        }
        let mut data = vec![0u8; (self.stride * active.len() as u64) as usize];
        let mut offsets = HashMap::new();
        for (i, (id, b)) in active.iter().enumerate() {
            let o = i * self.stride as usize;
            let floats = [b.left, b.right, b.top, b.bottom, b.gamma, b.black_level, 0.0, 0.0];
            for (k, f) in floats.iter().enumerate() {
                data[o + k * 4..o + k * 4 + 4].copy_from_slice(&f.to_le_bytes());
            }
            offsets.insert(*id, o as u32);
        }
        queue.write_buffer(&self.buf, 0, &data);
        offsets
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass, offset: u32, black_level: f32) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[offset]);
        pass.draw(0..3, 0..1);
        if black_level > 0.0 {
            pass.set_pipeline(&self.black);
            pass.draw(0..3, 0..1);
        }
    }
}

fn make_buffer(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, stride: u64, capacity: u64) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("edge blend"),
        size: stride * capacity,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("edge blend"),
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
    (buf, bind_group)
}
