//! YUV-video till RGBA på GPU:n (se `shaders/yuv.wgsl`). Planen laddas upp
//! som de kommer från avkodaren och räknas om till källans RGBA-textur.

use egui_wgpu::wgpu;
use lm_media::{FrameView, PixelFormat, YuvColor};

const UNIFORM_SIZE: u64 = 80;

/// GPU-texturer för en källas plan. Skapas om när storlek eller format ändras.
pub(crate) struct YuvPlanes {
    y: wgpu::Texture,
    a: wgpu::Texture,
    b: wgpu::Texture,
    size: [u32; 2],
    i420: bool,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

pub(crate) struct YuvConverter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl YuvConverter {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yuv"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../shaders/yuv.wgsl").into()),
        });
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yuv"),
            entries: &[
                texture(0),
                texture(1),
                texture(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yuv"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yuv"),
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
        // Linjär filtrering skalar upp färgplanen mjukt; luminansen läses i
        // exakt sin upplösning och påverkas inte.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("yuv"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        YuvConverter { pipeline, layout, sampler }
    }

    /// Laddar upp bildrutans plan och räknar om dem till `target` (RGBA).
    /// Gör inget för RGBA-bildrutor.
    pub fn convert(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        planes: &mut Option<YuvPlanes>,
        frame: &FrameView,
        target: &wgpu::Texture,
    ) {
        let (i420, color, a, a_stride, b) = match &frame.format {
            PixelFormat::Rgba => return,
            PixelFormat::Nv12 { uv, uv_stride, color } => (false, *color, *uv, *uv_stride, None),
            PixelFormat::I420 { u, u_stride, v, v_stride, color } => (true, *color, *u, *u_stride, Some((*v, *v_stride))),
        };
        let size = [frame.width, frame.height];
        let chroma = [size[0].div_ceil(2), size[1].div_ceil(2)];
        if planes.as_ref().is_none_or(|p| p.size != size || p.i420 != i420) {
            *planes = Some(self.make_planes(device, size, chroma, i420));
        }
        let p = planes.as_ref().unwrap();
        write_plane(queue, &p.y, frame.data, frame.stride, size, 1);
        write_plane(queue, &p.a, a, a_stride, chroma, if i420 { 1 } else { 2 });
        if let Some((v, v_stride)) = b {
            write_plane(queue, &p.b, v, v_stride, chroma, 1);
        }
        queue.write_buffer(&p.uniform, 0, &uniform_bytes(color, i420));

        let view = target.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("yuv") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yuv"),
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
            pass.set_bind_group(0, &p.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }

    fn make_planes(&self, device: &wgpu::Device, size: [u32; 2], chroma: [u32; 2], i420: bool) -> YuvPlanes {
        let tex = |label, size: [u32; 2], format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let y = tex("plane y", size, wgpu::TextureFormat::R8Unorm);
        let (a, b) = if i420 {
            (tex("plane u", chroma, wgpu::TextureFormat::R8Unorm), tex("plane v", chroma, wgpu::TextureFormat::R8Unorm))
        } else {
            // NV12 har bara två plan; det tredje är en tom platshållare.
            (tex("plane uv", chroma, wgpu::TextureFormat::Rg8Unorm), tex("unused", [1, 1], wgpu::TextureFormat::R8Unorm))
        };
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuv"),
            size: UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let views = [&y, &a, &b].map(|t| t.create_view(&Default::default()));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yuv"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[0]) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[1]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[2]) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: uniform.as_entire_binding() },
            ],
        });
        YuvPlanes {
            y,
            a,
            b,
            size,
            i420,
            uniform,
            bind_group,
        }
    }
}

fn write_plane(queue: &wgpu::Queue, texture: &wgpu::Texture, data: &[u8], stride: u32, size: [u32; 2], bytes_per_pixel: u32) {
    // Sista raden kan sakna utfyllnad; skriv bara det som finns.
    let needed = (stride * (size[1] - 1) + size[0] * bytes_per_pixel) as usize;
    if data.len() < needed {
        log::warn!("YUV-plan för kort: {} < {needed}", data.len());
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(stride),
            rows_per_image: Some(size[1]),
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
}

fn uniform_bytes(color: YuvColor, i420: bool) -> Vec<u8> {
    let (m, offset) = color.to_rgb();
    let floats = [
        m[0][0], m[0][1], m[0][2], 0.0,
        m[1][0], m[1][1], m[1][2], 0.0,
        m[2][0], m[2][1], m[2][2], 0.0,
        offset[0], offset[1], offset[2], 0.0,
        if i420 { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0,
    ];
    floats.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_matches_shader_size() {
        let c = YuvColor { matrix: lm_media::YuvMatrix::Bt709, full_range: false };
        assert_eq!(uniform_bytes(c, false).len() as u64, UNIFORM_SIZE);
    }
}
