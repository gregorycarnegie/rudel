//! The `_shader` widget: a user WGSL fragment body painted into the widget rect
//! on the GPU.
//!
//! Rudel already renders through eframe's wgpu backend, so a shader goes into
//! the *same* render pass as the rest of the frame via an
//! [`egui_wgpu::CallbackTrait`] — no second surface, no offscreen target, and
//! egui itself sets the viewport and scissor from the widget's rect before
//! calling us, so a shader cannot paint outside its own surface.
//!
//! The body is written in single quotes: double-quoted strings are
//! mini-notation, and a WGSL blob is not a pattern.
//!
//! ```koto
//! s("bd*4").shader({ code: '
//!   let d = length(uv - vec2<f32>(0.5, 0.5));
//!   return vec4<f32>(u.gain * (1.0 - d), 0.1, d, 1.0);
//! ' })
//! ```

use super::{
    options::option_str,
    pitchwheel::hap_frequency,
    style::{WidgetDrawColors, event_alpha},
};
use crate::editor::decorations::WidgetDecoration;
use eframe::{egui, egui_wgpu, wgpu};
use rudel_core::Hap;
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    time::{Duration, Instant},
};

/// Declarations wrapped around the user's body. `uv` spans the widget with y
/// pointing down, matching the canvas convention the other visualisers draw in.
const PRELUDE: &str = r#"
struct Uniforms {
    res: vec2<f32>,
    time: f32,
    gain: f32,
    note: f32,
    voices: f32,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

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
    return rudel_main(in.uv);
}

fn rudel_main(uv: vec2<f32>) -> vec4<f32> {
"#;

const EPILOGUE: &str = "\n}\n";

/// What `.shader({})` draws with no `code` of its own: a ring pulsing on the
/// pattern's gain, so an empty shader widget still shows it is alive.
const DEFAULT_BODY: &str = "
    let d = length(uv - vec2<f32>(0.5, 0.5));
    let wave = sin(d * 40.0 - u.time * 6.28318) * 0.5 + 0.5;
    let glow = wave * (0.15 + u.gain);
    return vec4<f32>(glow * 0.7, glow * 0.25, glow, 1.0);
";

/// The user's body wrapped in the declarations it is written against.
pub(super) fn assemble(body: &str) -> String {
    let body = if body.trim().is_empty() {
        DEFAULT_BODY
    } else {
        body
    };
    format!("{PRELUDE}{body}{EPILOGUE}")
}

/// Compile-check WGSL on the CPU, so a typo draws its error in the widget
/// instead of reaching wgpu — where a validation failure would take the whole
/// app down mid-performance.
pub(super) fn check(source: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|err| describe(&err.emit_to_string(source), source))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map(|_| ())
    .map_err(|err| describe(&err.emit_to_string(source), source))
}

/// naga reports against the *assembled* source and frames the offending line in
/// box-drawing glyphs the editor font has no room for. Keep its message and the
/// offending line, drop the frame, and move the line number back onto the body
/// the user wrote — a diagnostic pointing 30 lines into a prelude they never saw
/// is worse than no diagnostic at all.
fn describe(rendered: &str, source: &str) -> String {
    let message = rendered.lines().next().unwrap_or("shader error").trim();
    let Some(location) = rendered
        .split("wgsl:")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
    else {
        return message.to_string();
    };
    let mut parts = location.split(':');
    let Some(assembled) = parts.next().and_then(|line| line.parse::<usize>().ok()) else {
        return message.to_string();
    };
    let column = parts.next().unwrap_or("1");
    let line = assembled.saturating_sub(PRELUDE.matches('\n').count());
    // naga's lines are 1-based and count the assembled source, prelude included.
    let offending = source
        .lines()
        .nth(assembled.saturating_sub(1))
        .unwrap_or_default()
        .trim();
    format!("{message}\nline {line}, column {column}\n{offending}")
}

fn hash_of(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Uniforms {
    res: [f32; 2],
    time: f32,
    gain: f32,
    note: f32,
    voices: f32,
}

impl Uniforms {
    /// The WGSL `Uniforms` layout: six floats, padded to 32 bytes so the buffer
    /// is a whole number of 16-byte rows.
    fn bytes(self) -> [u8; 32] {
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
        let mut out = [0u8; 32];
        for (slot, value) in fields.into_iter().enumerate() {
            out[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        out
    }
}

struct Program {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    buffer: wgpu::Buffer,
}

struct Entry {
    hash: u64,
    used: Instant,
    program: Program,
}

/// Evict a widget's pipeline once it has gone this long without painting —
/// editing a shader's source shifts the widget id, so ids do accumulate.
const IDLE_EVICTION: Duration = Duration::from_secs(30);

/// The compiled pipelines, one per live shader widget, living in the wgpu
/// renderer's callback resources.
pub(crate) struct ShaderStore {
    format: wgpu::TextureFormat,
    programs: HashMap<String, Entry>,
}

impl ShaderStore {
    pub(crate) fn new(format: wgpu::TextureFormat) -> Self {
        Self {
            format,
            programs: HashMap::new(),
        }
    }

    fn program(&mut self, device: &wgpu::Device, id: &str, hash: u64, source: &str) -> &Program {
        let stale = self.programs.get(id).is_none_or(|entry| entry.hash != hash);
        if stale {
            let program = build(device, self.format, source);
            self.programs.insert(
                id.to_string(),
                Entry {
                    hash,
                    used: Instant::now(),
                    program,
                },
            );
            let cutoff = Instant::now() - IDLE_EVICTION;
            self.programs
                .retain(|key, entry| key == id || entry.used > cutoff);
        }
        let entry = self.programs.get_mut(id).expect("just inserted or present");
        entry.used = Instant::now();
        &entry.program
    }
}

fn build(device: &wgpu::Device, format: wgpu::TextureFormat, source: &str) -> Program {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rudel-shader-widget"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rudel-shader-uniforms"),
        size: 32,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rudel-shader-layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rudel-shader-bind-group"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rudel-shader-pipeline-layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rudel-shader-pipeline"),
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
                // egui's own pass is premultiplied; matching it lets a shader
                // return alpha and blend over the widget background.
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
    Program {
        pipeline,
        bind_group,
        buffer,
    }
}

struct ShaderCallback {
    id: String,
    source: String,
    hash: u64,
    uniforms: Uniforms,
}

impl egui_wgpu::CallbackTrait for ShaderCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(store) = resources.get_mut::<ShaderStore>() {
            let program = store.program(device, &self.id, self.hash, &self.source);
            queue.write_buffer(&program.buffer, 0, &self.uniforms.bytes());
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
        let Some(program) = resources
            .get::<ShaderStore>()
            .and_then(|store| store.programs.get(&self.id))
            .map(|entry| &entry.program)
        else {
            return;
        };
        pass.set_pipeline(&program.pipeline);
        pass.set_bind_group(0, &program.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// The WGSL check result for one source, kept in egui's temp store so a valid
/// shader is not re-parsed every frame.
#[derive(Clone)]
struct CachedCheck {
    hash: u64,
    error: Option<String>,
}

pub(super) fn paint_shader(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget: &WidgetDecoration,
    haps: &[&Hap],
    time: f64,
    colors: WidgetDrawColors,
) {
    // Read straight off the decoration rather than through
    // `VisualWidgetOptions`: the body is a `String`, and that struct is `Copy`
    // so every painter can pass it around by value.
    let source = assemble(option_str(&widget.options, "code").unwrap_or_default());
    paint_wgsl(ui, rect, &widget.id, source, haps, time, colors);
}

/// The `_hydra` widget: a hydra chain, already compiled to WGSL.
///
/// The chain is folded into a shader by `rudel_lang::hydra` during evaluation
/// and arrives here as a string option, so nothing about hydra reaches the draw
/// path — this is the same renderer `_shader` uses, handed a generated module
/// instead of a hand-written body.
pub(super) fn paint_hydra(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget: &WidgetDecoration,
    haps: &[&Hap],
    time: f64,
    colors: WidgetDrawColors,
) {
    let Some(source) = option_str(&widget.options, "chain") else {
        // `.hydra()` with nothing in it, or an option that was not a chain.
        paint_error(ui, rect, "hydra: no chain
.hydra({ chain: osc() })", colors);
        return;
    };
    super::hydra_gpu::paint_hydra_gpu(ui, rect, widget, source.to_string(), haps, time, colors);
}

/// Render a finished WGSL module into the widget rect.
///
/// Split out from [`paint_shader`] because a hydra chain compiles to a whole
/// module rather than to a body, so it cannot go through [`assemble`] — but
/// everything downstream of that (the compile check, the uniforms, the pipeline
/// cache) is the same for both.
pub(super) fn paint_wgsl(
    ui: &egui::Ui,
    rect: egui::Rect,
    id: &str,
    source: String,
    haps: &[&Hap],
    time: f64,
    colors: WidgetDrawColors,
) {
    let hash = hash_of(&source);
    let cache_id = egui::Id::new(("rudel-shader-check", id));
    let checked = || CachedCheck {
        hash,
        error: check(&source).err(),
    };
    let error = ui.ctx().data_mut(|d| {
        let cached = d.get_temp_mut_or_insert_with(cache_id, checked);
        if cached.hash != hash {
            *cached = checked();
        }
        cached.error.clone()
    });

    if let Some(error) = error {
        paint_error(ui, rect, &error, colors);
        return;
    }

    // The loudest sounding hap drives `note`; `gain` sums the lot, so a chord
    // pushes harder than one voice.
    let gain: f32 = haps.iter().map(|hap| event_alpha(hap)).sum();
    let note = haps
        .iter()
        .max_by(|a, b| event_alpha(a).total_cmp(&event_alpha(b)))
        .and_then(|hap| hap_frequency(hap))
        .map(|freq| rudel_core::freq_to_midi(freq) as f32)
        .unwrap_or(-1.0);
    let pixels_per_point = ui.ctx().pixels_per_point();

    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
        rect,
        ShaderCallback {
            id: id.to_string(),
            source,
            hash,
            uniforms: Uniforms {
                res: [
                    rect.width() * pixels_per_point,
                    rect.height() * pixels_per_point,
                ],
                time: time as f32,
                gain: gain.min(4.0),
                note,
                voices: haps.len() as f32,
            },
        },
    ));
}

fn paint_error(ui: &egui::Ui, rect: egui::Rect, error: &str, colors: WidgetDrawColors) {
    ui.painter().text(
        rect.left_top() + egui::vec2(8.0, 6.0),
        egui::Align2::LEFT_TOP,
        error,
        egui::TextStyle::Small.resolve(ui.style()),
        colors.foreground,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_compiles_against_the_uniforms_it_is_promised() {
        // Every name the doc comment advertises has to resolve, or the widget
        // is documenting a shader nobody can write.
        let body = "
            let d = length(uv - vec2<f32>(0.5, 0.5)) / max(u.res.x, u.res.y);
            let n = u.note * u.voices + u.gain + u.time;
            return vec4<f32>(d, n, 0.0, 1.0);
        ";
        check(&assemble(body)).expect("the advertised uniforms exist");
    }

    #[test]
    fn the_default_body_is_valid_wgsl() {
        check(&assemble("")).expect("an empty shader widget still draws");
        check(&assemble("   \n  ")).expect("whitespace counts as empty");
    }

    #[test]
    fn a_broken_body_is_reported_rather_than_reaching_wgpu() {
        // Handing this to `create_shader_module` would take the app down, so the
        // check has to catch it first — and say something about where.
        let error = check(&assemble("return vec4<f32>(nope, 0.0, 0.0, 1.0);"))
            .expect_err("an undefined identifier is an error");
        assert!(error.contains("nope"), "unhelpful diagnostic: {error}");

        assert!(check(&assemble("this is not wgsl at all")).is_err());
        // A body that type-checks as a statement but returns the wrong type is
        // a validation error rather than a parse error; both must be caught.
        assert!(check(&assemble("return 1.0;")).is_err());
    }

    #[test]
    fn an_error_points_at_the_line_the_user_wrote() {
        // Three lines of body, broken on the third. The prelude is ~30 lines of
        // assembled source in front of it and must not show up in the count.
        let error = check(&assemble("\nlet a = 1.0;\nreturn vec4<f32>(a, nope, 0.0, 1.0);\n"))
            .expect_err("`nope` is undefined");
        assert!(error.contains("line 3,"), "wrong line: {error}");
        // Only the message and the location survive — naga frames the offending
        // line in box-drawing glyphs the editor font renders as tofu.
        assert_eq!(error.lines().count(), 3, "undecorated: {error}");
        assert!(error.contains("return vec4<f32>(a, nope"), "no source line: {error}");
        assert!(
            error.is_ascii(),
            "the editor font has no box-drawing glyphs: {error}"
        );
    }

    #[test]
    fn crlf_line_endings_compile_and_count_as_one_line_each() {
        // The editor is a Windows text buffer, so a pasted body arrives CRLF.
        let body = "
let a = 1.0;
return vec4<f32>(a, nope, 0.0, 1.0);
";
        let error = check(&assemble(body)).expect_err("`nope` is still undefined");
        assert!(error.contains("nope"), "wrong error: {error}");
        assert!(error.contains("line 3,"), "CRLF shifted the line: {error}");
    }

    #[test]
    fn the_uniform_block_is_a_whole_number_of_rows() {
        let bytes = Uniforms {
            res: [2.0, 4.0],
            time: 8.0,
            gain: 16.0,
            note: 32.0,
            voices: 64.0,
        }
        .bytes();
        // Six floats in WGSL declaration order, then padding to 32.
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
