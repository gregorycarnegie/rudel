//! `spiral({gpu: true})` — the spiral drawn as a signed distance field instead
//! of tessellated polylines.
//!
//! The CPU painter strokes one polyline per hap ([`super::spiral`]), which is
//! what Strudel's canvas does. Adjacent haps then meet butt-end to butt-end with
//! ends that are not parallel — each end's normal comes from its own last
//! segment — so one side overlaps (and, since hap colours are translucent,
//! composites twice and reads bright) while the other leaves a sliver of
//! background. That seam is inherent to stroking, at any sampling rate.
//!
//! Here every band is evaluated per pixel instead. A pixel's polar coordinates
//! invert straight back to a spiral parameter, so "is this pixel inside band
//! *i*" is exact and the only edge softening is one pixel of deliberate
//! anti-aliasing. There is no geometry to seam.

use super::{
    spiral::SpiralBand,
    style::WidgetDrawColors,
};
use eframe::{egui, egui_wgpu, wgpu};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// Bytes per `Band` in the storage buffer: three floats and a pad, then an
/// `f32` colour that WGSL aligns to 16.
const BAND_SIZE: u64 = 32;
/// Bytes in the `Globals` uniform block.
const GLOBALS_SIZE: u64 = 32;

const SHADER: &str = r#"
struct Globals {
    size: vec2<f32>,
    margin: f32,
    rotate: f32,
    count: u32,
    srgb: u32,
};

struct Band {
    a0: f32,
    a1: f32,
    thickness: f32,
    _pad: f32,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var<storage, read> bands: array<Band>;

const TAU: f32 = 6.283185307179586;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    let x = f32(i32(index) / 2) * 4.0 - 1.0;
    let y = f32(i32(index) & 1) * 4.0 - 1.0;
    var out: VsOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}

// 0-1 linear from 0-1 sRGB gamma. egui's own shader carries this pair; a shader
// sharing its render pass has to land in the same space or its colours drift
// from every other widget's.
fn linear_from_gamma_rgb(srgb: vec3<f32>) -> vec3<f32> {
    let cutoff = srgb < vec3<f32>(0.04045);
    let lower = srgb / vec3<f32>(12.92);
    let higher = pow((srgb + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(higher, lower, cutoff);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Pixel offset from the widget centre, y down, the axes `spiral_point` uses.
    let p = (in.uv - vec2<f32>(0.5, 0.5)) * g.size;
    let r = length(p);
    // `spiral_point` puts angle `a` at radius `margin * a` and screen angle
    // `(a + rotate) * 360 - 90`, so a pixel's angle inverts to `a + rotate`.
    let psi = atan2(p.y, p.x) / TAU + 0.25;
    let base = psi - g.rotate;

    // Premultiplied, in gamma space -- exactly what egui composites in.
    var acc = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    for (var i = 0u; i < g.count; i = i + 1u) {
        let b = bands[i];
        // Every turn of the spiral crosses this pixel's angle; the one that can
        // cover the pixel is the turn nearest its radius.
        //
        // A band thicker than the spiral's pitch would also be reachable from
        // the neighbouring turn, which this single candidate misses. The default
        // thickness is half the pitch, so that needs `thickness > margin`.
        let a = base + round(r / g.margin - base);
        if (a < 0.0) {
            continue;
        }
        // Distance out of the band, in pixels: across its width, and along the
        // arc past either end. `max` of the two is the butt-capped stroke.
        let radial = abs(r - g.margin * a) - b.thickness * 0.5;
        let before = (b.a0 - a) * TAU * r;
        let after = (a - b.a1) * TAU * r;
        let d = max(radial, max(before, after));
        let coverage = 1.0 - smoothstep(-0.5, 0.5, d);
        if (coverage <= 0.0) {
            continue;
        }
        let alpha = b.color.a * coverage;
        acc = vec4<f32>(
            b.color.rgb * alpha + acc.rgb * (1.0 - alpha),
            alpha + acc.a * (1.0 - alpha),
        );
    }

    if (g.srgb == 1u) {
        return vec4<f32>(linear_from_gamma_rgb(acc.rgb), acc.a);
    }
    return acc;
}
"#;

/// Everything the shader needs that is not per-band.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Globals {
    size: [f32; 2],
    margin: f32,
    rotate: f32,
    count: u32,
    srgb: bool,
}

impl Globals {
    fn bytes(self) -> [u8; GLOBALS_SIZE as usize] {
        let mut out = [0u8; GLOBALS_SIZE as usize];
        let floats = [self.size[0], self.size[1], self.margin, self.rotate];
        for (slot, value) in floats.into_iter().enumerate() {
            out[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        out[16..20].copy_from_slice(&self.count.to_le_bytes());
        out[20..24].copy_from_slice(&u32::from(self.srgb).to_le_bytes());
        out
    }
}

/// One band packed for the storage buffer.
///
/// The angles are pre-multiplied by `stretch` here rather than in the shader,
/// because that is the form `spiral_point` actually draws in and it keeps the
/// per-pixel loop to the arithmetic it cannot avoid.
fn band_bytes(band: SpiralBand, stretch: f32, scale: f32, out: &mut Vec<u8>) {
    let [r, g, b, a] = band.color.to_srgba_unmultiplied();
    let floats = [
        band.from * stretch,
        band.to * stretch,
        band.thickness * scale,
        0.0,
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ];
    for value in floats {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

struct Surface {
    globals: wgpu::Buffer,
    bands: wgpu::Buffer,
    /// Bands the storage buffer has room for; it is only ever grown.
    capacity: u64,
    bind_group: wgpu::BindGroup,
    used: Instant,
}

/// Drop a widget's buffers once it has gone this long without painting. Editing
/// a spiral's source shifts its widget id, so ids do accumulate.
const IDLE_EVICTION: Duration = Duration::from_secs(30);

/// The pipeline and per-widget buffers, living in the wgpu renderer's callback
/// resources.
pub(crate) struct SpiralStore {
    format: wgpu::TextureFormat,
    pipeline: Option<(wgpu::RenderPipeline, wgpu::BindGroupLayout)>,
    surfaces: HashMap<String, Surface>,
}

impl SpiralStore {
    pub(crate) fn new(format: wgpu::TextureFormat) -> Self {
        Self {
            format,
            pipeline: None,
            surfaces: HashMap::new(),
        }
    }

    /// The pipeline, built on first use — `prepare` is the earliest point a
    /// callback is handed a device.
    fn pipeline(&mut self, device: &wgpu::Device) -> &(wgpu::RenderPipeline, wgpu::BindGroupLayout) {
        self.pipeline
            .get_or_insert_with(|| build_pipeline(device, self.format))
    }
}

fn build_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rudel-spiral-sdf"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rudel-spiral-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rudel-spiral-pipeline-layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rudel-spiral-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    (pipeline, layout)
}

struct SpiralCallback {
    id: String,
    globals: Globals,
    bands: Vec<u8>,
}

impl egui_wgpu::CallbackTrait for SpiralCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(store) = resources.get_mut::<SpiralStore>() else {
            return Vec::new();
        };
        let wanted = (self.globals.count as u64).max(1);
        let srgb = store.format.is_srgb();
        // A wgpu binding cannot be zero-sized, so an empty spiral still gets one
        // band's worth of buffer; `count` keeps the shader out of it.
        let layout = store.pipeline(device).1.clone();
        let surface = match store.surfaces.get(&self.id) {
            Some(surface) if surface.capacity >= wanted => None,
            _ => Some(create_surface(device, &layout, wanted)),
        };
        if let Some(surface) = surface {
            store.surfaces.insert(self.id.clone(), surface);
            let cutoff = Instant::now() - IDLE_EVICTION;
            store
                .surfaces
                .retain(|key, surface| key == &self.id || surface.used > cutoff);
        }
        let Some(surface) = store.surfaces.get_mut(&self.id) else {
            return Vec::new();
        };
        surface.used = Instant::now();
        let mut globals = self.globals;
        globals.srgb = srgb;
        queue.write_buffer(&surface.globals, 0, &globals.bytes());
        if !self.bands.is_empty() {
            queue.write_buffer(&surface.bands, 0, &self.bands);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        // egui has already set the viewport and scissor to this widget's rect.
        let Some(store) = resources.get::<SpiralStore>() else {
            return;
        };
        let (Some((pipeline, _)), Some(surface)) = (&store.pipeline, store.surfaces.get(&self.id))
        else {
            return;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &surface.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn create_surface(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    capacity: u64,
) -> Surface {
    let globals = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rudel-spiral-globals"),
        size: GLOBALS_SIZE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bands = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rudel-spiral-bands"),
        size: capacity * BAND_SIZE,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rudel-spiral-bind-group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: bands.as_entire_binding(),
            },
        ],
    });
    Surface {
        globals,
        bands,
        capacity,
        bind_group,
        used: Instant::now(),
    }
}

pub(super) fn paint_spiral_gpu(
    ui: &egui::Ui,
    rect: egui::Rect,
    id: &str,
    haps: &[&rudel_core::Hap],
    time: f64,
    colors: WidgetDrawColors,
    options: super::options::VisualWidgetOptions,
) {
    let (geometry, bands) = super::spiral::spiral_bands(haps, time, colors, options);
    let pixels_per_point = ui.ctx().pixels_per_point();
    let mut packed = Vec::with_capacity(bands.len() * BAND_SIZE as usize);
    for band in &bands {
        band_bytes(*band, geometry.stretch, pixels_per_point, &mut packed);
    }
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
        rect,
        SpiralCallback {
            id: id.to_string(),
            globals: Globals {
                size: [
                    rect.width() * pixels_per_point,
                    rect.height() * pixels_per_point,
                ],
                // The shader works in physical pixels, so the spiral's own
                // lengths have to be scaled to match.
                margin: geometry.margin * pixels_per_point,
                rotate: geometry.rotate * geometry.stretch,
                count: bands.len() as u32,
                // Filled in by `prepare`, which is where the target format is.
                srgb: false,
            },
            bands: packed,
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shader_is_valid_wgsl() {
        // Nothing else compiles it until a spiral with `gpu: true` is on screen
        // and a GPU is present, so a typo here would otherwise reach a user
        // before it reached a test.
        super::super::shader::check(SHADER).expect("the spiral shader compiles");
    }

    #[test]
    fn a_band_packs_into_its_storage_slot() {
        let band = SpiralBand {
            from: 1.0,
            to: 2.0,
            thickness: 8.0,
            color: egui::Color32::from_rgba_unmultiplied(255, 128, 0, 64),
        };
        let mut out = Vec::new();
        band_bytes(band, 2.0, 2.0, &mut out);
        assert_eq!(out.len(), BAND_SIZE as usize, "one band fills its slot");

        let at = |slot: usize| {
            let mut field = [0u8; 4];
            field.copy_from_slice(&out[slot * 4..slot * 4 + 4]);
            f32::from_le_bytes(field)
        };
        // `stretch` is applied here rather than per pixel.
        assert_eq!((at(0), at(1)), (2.0, 4.0));
        assert_eq!(at(2), 16.0, "thickness is scaled to physical pixels");
        // The colour arrives straight (not premultiplied), 0..1, at offset 16 --
        // where WGSL aligns the `vec4`.
        assert_eq!(at(4), 1.0);
        assert_eq!(at(7), 64.0 / 255.0);
    }

    #[test]
    fn the_globals_block_lays_out_as_the_shader_reads_it() {
        let bytes = Globals {
            size: [10.0, 20.0],
            margin: 30.0,
            rotate: 40.0,
            count: 7,
            srgb: true,
        }
        .bytes();
        assert_eq!(bytes.len(), GLOBALS_SIZE as usize);
        let float_at = |offset: usize| {
            let mut field = [0u8; 4];
            field.copy_from_slice(&bytes[offset..offset + 4]);
            f32::from_le_bytes(field)
        };
        let uint_at = |offset: usize| {
            let mut field = [0u8; 4];
            field.copy_from_slice(&bytes[offset..offset + 4]);
            u32::from_le_bytes(field)
        };
        assert_eq!((float_at(0), float_at(4)), (10.0, 20.0));
        assert_eq!(float_at(8), 30.0);
        assert_eq!(float_at(12), 40.0);
        assert_eq!(uint_at(16), 7);
        assert_eq!(uint_at(20), 1);
    }
}
