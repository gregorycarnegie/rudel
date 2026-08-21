//! The `_hydra` widget: up to four chains, each with its own output buffer.
//!
//! `_shader` draws straight into egui's render pass, which is enough for a
//! chain that only looks at the current frame. Hydra's feedback needs the last
//! frame back as a texture, so a hydra widget renders offscreen first and then
//! blits one buffer into its rect.
//!
//! Each output owns two textures and alternates between them, so a chain writes
//! one while every buffer read — its own via `prev()`, another output's via
//! `src(o1)` — comes from the set written last frame. That is upstream's rule
//! too: `format-arguments.js` binds `output.getTexture()`, the fbo that is
//! *not* currently being drawn into. It also means no texture is ever sampled
//! while it is a render target, which wgpu would reject outright.
//!
//! Buffers are the window's own format, so a value survives the round trip and
//! a chain looks the same here as it would drawn directly. Only outputs a
//! script actually gave a chain are allocated; the rest bind a shared 1x1
//! texture, which reads as the empty buffer it is.

use super::style::WidgetDrawColors;
use crate::editor::decorations::WidgetDecoration;
use eframe::{egui, egui_wgpu, wgpu};
use rudel_core::Hap;
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    time::{Duration, Instant},
};

/// Copies the finished output buffer into the widget rect.
const BLIT: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv);
}
"#;

/// All four buffers at once, as hydra's `render()` with no argument draws them.
///
/// Ported from hydra-synth's `renderAll` fragment shader. Its quadrant map is
/// column-major — `quad = floor(st).x * 2 + floor(st).y`, so o0 is top-left, o1
/// bottom-left, o2 top-right, o3 bottom-right — and its `st.x +=` / `st.y +=`
/// lines are dropped here because they only ever add whole numbers immediately
/// before a `fract`, which cannot change the result.
const BLIT_GRID: &str = r#"
@group(0) @binding(0) var t0: texture_2d<f32>;
@group(0) @binding(1) var t1: texture_2d<f32>;
@group(0) @binding(2) var t2: texture_2d<f32>;
@group(0) @binding(3) var t3: texture_2d<f32>;
@group(0) @binding(4) var samp: sampler;

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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let st = in.uv * 2.0;
    let cell = vec2<i32>(floor(st));
    let uv = fract(st);
    let c0 = textureSample(t0, samp, uv);
    let c1 = textureSample(t1, samp, uv);
    let c2 = textureSample(t2, samp, uv);
    let c3 = textureSample(t3, samp, uv);
    let left = select(c0, c1, cell.y == 1);
    let right = select(c2, c3, cell.y == 1);
    return select(left, right, cell.x == 1);
}
"#;

const UNIFORM_SIZE: u64 = 32;

/// The uniform block hydra's generated preamble declares.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Uniforms {
    res: [f32; 2],
    time: f32,
    gain: f32,
    note: f32,
    voices: f32,
}

impl Uniforms {
    fn bytes(self) -> [u8; UNIFORM_SIZE as usize] {
        let fields = [
            self.res[0],
            self.res[1],
            self.time,
            self.gain,
            self.note,
            self.voices,
            0.0,
            0.0,
        ];
        let mut out = [0u8; UNIFORM_SIZE as usize];
        for (slot, value) in fields.into_iter().enumerate() {
            out[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        out
    }
}

/// What the widget displays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Render {
    /// One buffer, filling the rect.
    One(usize),
    /// All four, tiled — hydra's `render()` with no argument.
    Grid,
}

/// One output buffer with a chain bound to it.
struct Output {
    pipeline: wgpu::RenderPipeline,
    /// The two textures this output alternates between.
    views: [wgpu::TextureView; 2],
    /// `blit_bind[i]` samples `views[i]`.
    blit_bind: [wgpu::BindGroup; 2],
}

struct Surface {
    /// The chains and render target this was built for; any change rebuilds.
    hash: u64,
    size: (u32, u32),
    outputs: [Option<Output>; 4],
    render: Render,
    /// Only built when the widget is showing the grid.
    grid_bind: Option<[wgpu::BindGroup; 2]>,
    uniforms: wgpu::Buffer,
    /// `chain_bind[i]` binds every output's `views[1 - i]` — the set written
    /// last frame. One per parity, shared by all four chains, because they all
    /// read the same previous-frame set.
    chain_bind: [wgpu::BindGroup; 2],
    write: usize,
    used: Instant,
}

const IDLE_EVICTION: Duration = Duration::from_secs(30);

/// Pipelines and output buffers, living in the wgpu renderer's callback
/// resources.
pub(crate) struct HydraStore {
    format: wgpu::TextureFormat,
    chain_layout: Option<wgpu::BindGroupLayout>,
    blit: Option<(wgpu::RenderPipeline, wgpu::BindGroupLayout)>,
    grid: Option<(wgpu::RenderPipeline, wgpu::BindGroupLayout)>,
    sampler: Option<wgpu::Sampler>,
    /// Stands in for an output nothing was bound to. Every chain binds all four
    /// buffers whether or not it reads them, and an unused one should read as
    /// empty rather than cost a full-size texture.
    empty: Option<wgpu::TextureView>,
    surfaces: HashMap<String, Surface>,
}

impl HydraStore {
    pub(crate) fn new(format: wgpu::TextureFormat) -> Self {
        Self {
            format,
            chain_layout: None,
            blit: None,
            grid: None,
            sampler: None,
            empty: None,
            surfaces: HashMap::new(),
        }
    }

    /// Feedback reads the frame before, so the sampler clamps rather than
    /// repeats: a chain that scrolls its own output should smear off the edge,
    /// not wrap round to the other side.
    fn sampler(&mut self, device: &wgpu::Device) -> wgpu::Sampler {
        self.sampler
            .get_or_insert_with(|| {
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("rudel-hydra-sampler"),
                    address_mode_u: wgpu::AddressMode::ClampToEdge,
                    address_mode_v: wgpu::AddressMode::ClampToEdge,
                    address_mode_w: wgpu::AddressMode::ClampToEdge,
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    ..Default::default()
                })
            })
            .clone()
    }

    fn empty(&mut self, device: &wgpu::Device) -> wgpu::TextureView {
        let format = self.format;
        self.empty
            .get_or_insert_with(|| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("rudel-hydra-empty"),
                        size: wgpu::Extent3d {
                            width: 1,
                            height: 1,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            })
            .clone()
    }

    fn chain_layout(&mut self, device: &wgpu::Device) -> wgpu::BindGroupLayout {
        self.chain_layout
            .get_or_insert_with(|| {
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("rudel-hydra-chain-layout"),
                    entries: &[
                        uniform_entry(0),
                        texture_entry(1, wgpu::ShaderStages::FRAGMENT),
                        texture_entry(2, wgpu::ShaderStages::FRAGMENT),
                        texture_entry(3, wgpu::ShaderStages::FRAGMENT),
                        texture_entry(4, wgpu::ShaderStages::FRAGMENT),
                        sampler_entry(5, wgpu::ShaderStages::FRAGMENT),
                    ],
                })
            })
            .clone()
    }

    fn blit(&mut self, device: &wgpu::Device) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
        let format = self.format;
        self.blit
            .get_or_insert_with(|| {
                let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("rudel-hydra-blit-layout"),
                    entries: &[
                        texture_entry(0, wgpu::ShaderStages::FRAGMENT),
                        sampler_entry(1, wgpu::ShaderStages::FRAGMENT),
                    ],
                });
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("rudel-hydra-blit"),
                    source: wgpu::ShaderSource::Wgsl(BLIT.into()),
                });
                let pipeline = render_pipeline(device, "rudel-hydra-blit", &module, &layout, format);
                (pipeline, layout)
            })
            .clone()
    }
}

impl HydraStore {
    fn grid(&mut self, device: &wgpu::Device) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
        let format = self.format;
        self.grid
            .get_or_insert_with(|| {
                let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("rudel-hydra-grid-layout"),
                    entries: &[
                        texture_entry(0, wgpu::ShaderStages::FRAGMENT),
                        texture_entry(1, wgpu::ShaderStages::FRAGMENT),
                        texture_entry(2, wgpu::ShaderStages::FRAGMENT),
                        texture_entry(3, wgpu::ShaderStages::FRAGMENT),
                        sampler_entry(4, wgpu::ShaderStages::FRAGMENT),
                    ],
                });
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("rudel-hydra-grid"),
                    source: wgpu::ShaderSource::Wgsl(BLIT_GRID.into()),
                });
                let pipeline = render_pipeline(device, "rudel-hydra-grid", &module, &layout, format);
                (pipeline, layout)
            })
            .clone()
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn render_pipeline(
    device: &wgpu::Device,
    label: &str,
    module: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module,
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
    })
}

fn build_surface(
    store: &mut HydraStore,
    device: &wgpu::Device,
    call: &HydraCallback,
) -> Surface {
    let format = store.format;
    let layout = store.chain_layout(device);
    let sampler = store.sampler(device);
    let empty = store.empty(device);
    let (_, blit_layout) = store.blit(device);

    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rudel-hydra-uniforms"),
        size: UNIFORM_SIZE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let make_view = || {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("rudel-hydra-output"),
                size: wgpu::Extent3d {
                    width: call.size.0,
                    height: call.size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };

    let outputs: [Option<Output>; 4] = std::array::from_fn(|index| {
        let source = call.sources[index].as_deref()?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rudel-hydra-chain"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = render_pipeline(device, "rudel-hydra-chain", &module, &layout, format);
        let views = [make_view(), make_view()];
        let blit_bind = std::array::from_fn(|parity| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rudel-hydra-blit-bind"),
                layout: &blit_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&views[parity]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            })
        });
        Some(Output {
            pipeline,
            views,
            blit_bind,
        })
    });

    // While writing parity `w`, every buffer read comes from `1 - w`: the set
    // the previous frame wrote. One bind group serves all four chains.
    let chain_bind = std::array::from_fn(|parity| {
        let read = 1 - parity;
        let view = |index: usize| {
            outputs[index]
                .as_ref()
                .map_or(&empty, |output| &output.views[read])
        };
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rudel-hydra-chain-bind"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view(0)),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(view(1)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(view(2)),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(view(3)),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        })
    });

    // The grid samples this frame's set, so it pairs with the write parity the
    // way the single-output blit does.
    let grid_bind = (call.render == Render::Grid).then(|| {
        let (_, grid_layout) = store.grid(device);
        std::array::from_fn(|parity| {
            let view = |index: usize| {
                outputs[index]
                    .as_ref()
                    .map_or(&empty, |output| &output.views[parity])
            };
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rudel-hydra-grid-bind"),
                layout: &grid_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view(0)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(view(1)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(view(2)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(view(3)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            })
        })
    });

    Surface {
        hash: call.hash,
        size: call.size,
        outputs,
        render: call.render,
        grid_bind,
        uniforms,
        chain_bind,
        write: 0,
        used: Instant::now(),
    }
}

struct HydraCallback {
    id: String,
    sources: [Option<String>; 4],
    render: Render,
    hash: u64,
    size: (u32, u32),
    uniforms: Uniforms,
}

impl egui_wgpu::CallbackTrait for HydraCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(store) = resources.get_mut::<HydraStore>() else {
            return Vec::new();
        };
        let stale = store
            .surfaces
            .get(&self.id)
            .is_none_or(|s| s.hash != self.hash || s.size != self.size);
        if stale {
            let surface = build_surface(store, device, self);
            store.surfaces.insert(self.id.clone(), surface);
            let cutoff = Instant::now() - IDLE_EVICTION;
            store
                .surfaces
                .retain(|key, s| key == &self.id || s.used > cutoff);
        }
        let Some(surface) = store.surfaces.get_mut(&self.id) else {
            return Vec::new();
        };
        surface.used = Instant::now();
        // Alternate before drawing, so this frame writes the buffer the last
        // one was reading and `prev()` sees the last frame rather than this one.
        surface.write = 1 - surface.write;
        queue.write_buffer(&surface.uniforms, 0, &self.uniforms.bytes());

        // egui has not begun its own pass yet, so the chains' passes can go
        // straight into its encoder rather than into a separate submission.
        // One pass per output; they are independent, since every read is of
        // last frame's set.
        for output in surface.outputs.iter().flatten() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rudel-hydra-output"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output.views[surface.write],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&output.pipeline);
            pass.set_bind_group(0, &surface.chain_bind[surface.write], &[]);
            pass.draw(0..3, 0..1);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(store) = resources.get::<HydraStore>() else {
            return;
        };
        let Some(surface) = store.surfaces.get(&self.id) else {
            return;
        };
        match surface.render {
            Render::Grid => {
                let (Some((grid, _)), Some(binds)) = (&store.grid, &surface.grid_bind) else {
                    return;
                };
                pass.set_pipeline(grid);
                pass.set_bind_group(0, &binds[surface.write], &[]);
            }
            Render::One(index) => {
                let (Some((blit, _)), Some(shown)) =
                    (&store.blit, surface.outputs[index].as_ref())
                else {
                    return;
                };
                pass.set_pipeline(blit);
                pass.set_bind_group(0, &shown.blit_bind[surface.write], &[]);
            }
        }
        pass.draw(0..3, 0..1);
    }
}

/// The chains a widget declared, by output, and which output it displays.
///
/// `chain` is `o0` under its single-output name, so the one-chain form keeps
/// working unchanged.
fn chains(widget: &WidgetDecoration) -> ([Option<String>; 4], Render) {
    let read = |key: &str| super::options::option_str(&widget.options, key).map(str::to_string);
    let sources = [
        read("o0").or_else(|| read("chain")),
        read("o1"),
        read("o2"),
        read("o3"),
    ];
    // `render: 'all'` is hydra's `render()` with no argument; a number names
    // one buffer, as `render(o2)` does.
    let render = match read("render").as_deref() {
        Some("all") => Render::Grid,
        _ => Render::One(
            super::options::option_f32(&widget.options, "render")
                .unwrap_or(0.0)
                .clamp(0.0, 3.0) as usize,
        ),
    };
    (sources, render)
}

pub(super) fn paint_hydra_gpu(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget: &WidgetDecoration,
    haps: &[&Hap],
    time: f64,
    colors: WidgetDrawColors,
) {
    let (sources, render) = chains(widget);
    if sources.iter().all(Option::is_none) {
        super::shader::paint_error(
            ui,
            rect,
            "hydra: no chain
.hydra({ chain: Hydra.osc() })",
            colors,
        );
        return;
    }
    // A widget showing an output nothing was bound to would just be black, and
    // silently so; say which one instead. The grid shows whatever is there.
    if let Render::One(index) = render
        && sources[index].is_none()
    {
        super::shader::paint_error(
            ui,
            rect,
            &format!("hydra: render: {index} has no chain"),
            colors,
        );
        return;
    }

    let mut hasher = DefaultHasher::new();
    sources.hash(&mut hasher);
    render.hash(&mut hasher);
    let hash = hasher.finish();

    let pixels_per_point = ui.ctx().pixels_per_point();
    let size = (
        ((rect.width() * pixels_per_point).round() as u32).max(1),
        ((rect.height() * pixels_per_point).round() as u32).max(1),
    );

    let gain: f32 = haps.iter().map(|hap| super::style::event_alpha(hap)).sum();
    let note = haps
        .iter()
        .max_by(|a, b| super::style::event_alpha(a).total_cmp(&super::style::event_alpha(b)))
        .and_then(|hap| super::pitchwheel::hap_frequency(hap))
        .map(|freq| rudel_core::freq_to_midi(freq) as f32)
        .unwrap_or(-1.0);

    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
        rect,
        HydraCallback {
            id: widget.id.clone(),
            sources,
            render,
            hash,
            size,
            uniforms: Uniforms {
                res: [size.0 as f32, size.1 as f32],
                time: time as f32,
                gain: gain.min(4.0),
                note,
                voices: haps.len() as f32,
            },
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blit_shaders_are_valid_wgsl() {
        // Nothing compiles these until a hydra widget is on screen with a GPU
        // present, so a typo would otherwise reach a user before a test.
        super::super::shader::check(BLIT).expect("the blit shader compiles");
        super::super::shader::check(BLIT_GRID).expect("the grid shader compiles");
    }

    #[test]
    fn the_uniform_block_matches_the_generated_preamble() {
        // `rudel_lang::hydra` writes the struct this fills; six floats padded to
        // a whole 16-byte row.
        let bytes = Uniforms {
            res: [2.0, 4.0],
            time: 8.0,
            gain: 16.0,
            note: 32.0,
            voices: 64.0,
        }
        .bytes();
        for (slot, expected) in [2.0f32, 4.0, 8.0, 16.0, 32.0, 64.0, 0.0, 0.0]
            .into_iter()
            .enumerate()
        {
            let mut field = [0u8; 4];
            field.copy_from_slice(&bytes[slot * 4..slot * 4 + 4]);
            assert_eq!(f32::from_le_bytes(field), expected, "field {slot}");
        }
    }
}
