//! Färger till LED-slingor: en liten compute-pass läser utgångens bild där
//! lysdioderna ligger, och resultatet läses tillbaka utan att vänta på GPU:n.

use egui_wgpu::wgpu;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

pub(crate) struct LedPass {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

/// Läsning pågår / klar / misslyckad.
const WAITING: u8 = 0;
const DONE: u8 = 1;
const FAILED: u8 = 2;

pub(crate) struct LedState {
    points: wgpu::Buffer,
    colors: wgpu::Buffer,
    readback: wgpu::Buffer,
    params: wgpu::Buffer,
    cap: usize,
    /// Antal punkter i en läsning som pågår.
    pending: Option<usize>,
    status: Arc<AtomicU8>,
    pub result: Vec<[f32; 3]>,
}

impl LedPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("led"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../shaders/led.wgsl").into()),
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty,
            count: None,
        };
        let storage = |read_only| wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("led"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(1, storage(true)),
                entry(2, storage(false)),
                entry(
                    3,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("led"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("led"),
            layout: Some(&pl),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        LedPass { pipeline, layout }
    }

    pub fn make_state(&self, device: &wgpu::Device, cap: usize) -> LedState {
        let buffer = |label, size: usize, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage,
                mapped_at_creation: false,
            })
        };
        use wgpu::BufferUsages as U;
        LedState {
            points: buffer("led points", cap * 8, U::STORAGE | U::COPY_DST),
            colors: buffer("led colors", cap * 4, U::STORAGE | U::COPY_SRC),
            readback: buffer("led readback", cap * 4, U::MAP_READ | U::COPY_DST),
            params: buffer("led params", 16, U::UNIFORM | U::COPY_DST),
            cap,
            pending: None,
            status: Arc::new(AtomicU8::new(WAITING)),
            result: Vec::new(),
        }
    }

    /// Startar en ny läsning av `points` ur `view`, om förra läsningen är klar.
    pub fn sample(&self, device: &wgpu::Device, queue: &wgpu::Queue, state: &mut LedState, view: &wgpu::TextureView, points: &[[f32; 2]]) {
        if state.pending.is_some() || points.is_empty() {
            return;
        }
        let n = points.len().min(state.cap);
        let flat: Vec<f32> = points[..n].iter().flat_map(|p| [p[0], p[1]]).collect();
        queue.write_buffer(&state.points, 0, &bytes(&flat));
        queue.write_buffer(&state.params, 0, &[(n as u32).to_le_bytes(), [0; 4], [0; 4], [0; 4]].concat());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("led"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: state.points.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: state.colors.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: state.params.as_entire_binding() },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("led") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("led"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups((n as u32).div_ceil(64), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&state.colors, 0, &state.readback, 0, (n * 4) as u64);
        queue.submit([encoder.finish()]);
        state.status.store(WAITING, Ordering::Release);
        let status = state.status.clone();
        state.readback.slice(..(n * 4) as u64).map_async(wgpu::MapMode::Read, move |r| {
            status.store(if r.is_ok() { DONE } else { FAILED }, Ordering::Release);
        });
        state.pending = Some(n);
    }
}

impl LedState {
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Hämtar en färdig läsning, om någon. Väntar aldrig på GPU:n.
    pub fn collect(&mut self) {
        let Some(n) = self.pending else { return };
        match self.status.load(Ordering::Acquire) {
            DONE => {
                if let Ok(data) = self.readback.slice(..(n * 4) as u64).get_mapped_range() {
                    self.result = data.chunks_exact(4).map(|c| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]).collect();
                }
                self.readback.unmap();
                self.pending = None;
            }
            FAILED => self.pending = None,
            _ => {}
        }
    }
}

fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}
