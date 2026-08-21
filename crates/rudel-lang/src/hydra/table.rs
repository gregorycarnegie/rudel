//! hydra-synth's glsl function table, ported to WGSL.
//!
//! One entry per hydra function, pinned against
//! `tools/oracle/hydra_golden.json` (hydra-synth 1.3.29) by
//! `tests/hydra_parity.rs`: names, composition types, input names and defaults
//! all have to match, so a chain written for hydra means the same thing here.
//!
//! The bodies are transliterated from the GLSL, not reinvented. Four
//! differences are mechanical and apply throughout:
//!
//! - WGSL has no implicit scalar broadcast in `+`/`-` or scalar-over-vector
//!   division, so `st - 0.5` becomes `st - vec2<f32>(0.5)` and `1.0 / v`
//!   becomes `vec2<f32>(1.0) / v`.
//! - GLSL's `mod` floors; WGSL's `%` truncates, so they disagree on negative
//!   operands — and coordinates go negative as soon as a chain rotates or
//!   scrolls. [`MOD_HELPERS`] restores the GLSL behaviour rather than papering
//!   over it with `%`.
//! - `atan(y, x)` is `atan2(y, x)`. Note hydra's `shape` calls it as
//!   `atan(st.x, st.y)`, which is deliberate and is kept.
//! - Function parameters are immutable, so bodies that assign to `_st` take a
//!   `var` copy first.

use super::{FnType, Helper, HydraFn, Input};

const fn f(name: &'static str, default: f64) -> Input {
    Input { name, default }
}

/// GLSL's `mod`, which WGSL's `%` is not: floored rather than truncated, so it
/// agrees with the reference on negative coordinates.
pub(super) const MOD_HELPERS: &str = r#"
fn _mod(x: f32, y: f32) -> f32 { return x - y * floor(x / y); }
fn _mod2(x: vec2<f32>, y: f32) -> vec2<f32> { return x - y * floor(x / y); }
fn _mod3(x: vec3<f32>, y: f32) -> vec3<f32> { return x - y * floor(x / y); }
fn _mod4(x: vec4<f32>, y: f32) -> vec4<f32> { return x - y * floor(x / y); }
"#;

pub(super) const LUMINANCE: &str = r#"
fn _luminance(rgb: vec3<f32>) -> f32 {
    return dot(rgb, vec3<f32>(0.2125, 0.7154, 0.0721));
}
"#;

pub(super) const RGB_TO_HSV: &str = r#"
fn _rgbToHsv(c: vec3<f32>) -> vec3<f32> {
    let K = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4<f32>(c.bg, K.wz), vec4<f32>(c.gb, K.xy), step(c.b, c.g));
    let q = mix(vec4<f32>(p.xyw, c.r), vec4<f32>(c.r, p.yzx), step(p.x, c.r));
    let d = q.x - min(q.w, q.y);
    let e = 1.0e-10;
    return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}
"#;

pub(super) const HSV_TO_RGB: &str = r#"
fn _hsvToRgb(c: vec3<f32>) -> vec3<f32> {
    let K = vec4<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    let p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, vec3<f32>(0.0), vec3<f32>(1.0)), c.y);
}
"#;

/// Ian McEwan / Ashima Arts simplex 3D noise, as vendored in hydra's
/// `utility-functions.js`.
pub(super) const NOISE: &str = r#"
fn _permute4(x: vec4<f32>) -> vec4<f32> {
    return _mod4((x * 34.0 + vec4<f32>(1.0)) * x, 289.0);
}
fn _taylorInvSqrt4(r: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(1.79284291400159) - 0.85373472095314 * r;
}
fn _noise(v: vec3<f32>) -> f32 {
    let C = vec2<f32>(1.0 / 6.0, 1.0 / 3.0);
    let D = vec4<f32>(0.0, 0.5, 1.0, 2.0);

    var i = floor(v + vec3<f32>(dot(v, C.yyy)));
    let x0 = v - i + vec3<f32>(dot(i, C.xxx));

    let g = step(x0.yzx, x0.xyz);
    let l = vec3<f32>(1.0) - g;
    let i1 = min(g.xyz, l.zxy);
    let i2 = max(g.xyz, l.zxy);

    let x1 = x0 - i1 + 1.0 * C.xxx;
    let x2 = x0 - i2 + 2.0 * C.xxx;
    let x3 = x0 - vec3<f32>(1.0) + 3.0 * C.xxx;

    i = _mod3(i, 289.0);
    let p = _permute4(_permute4(_permute4(
        vec4<f32>(i.z) + vec4<f32>(0.0, i1.z, i2.z, 1.0))
        + vec4<f32>(i.y) + vec4<f32>(0.0, i1.y, i2.y, 1.0))
        + vec4<f32>(i.x) + vec4<f32>(0.0, i1.x, i2.x, 1.0));

    let n_ = 1.0 / 7.0;
    let ns = n_ * D.wyz - D.xzx;

    let j = p - 49.0 * floor(p * ns.z * ns.z);

    let x_ = floor(j * ns.z);
    let y_ = floor(j - 7.0 * x_);

    let x = x_ * ns.x + ns.yyyy;
    let y = y_ * ns.x + ns.yyyy;
    let h = vec4<f32>(1.0) - abs(x) - abs(y);

    let b0 = vec4<f32>(x.xy, y.xy);
    let b1 = vec4<f32>(x.zw, y.zw);

    let s0 = floor(b0) * 2.0 + vec4<f32>(1.0);
    let s1 = floor(b1) * 2.0 + vec4<f32>(1.0);
    let sh = -step(h, vec4<f32>(0.0));

    let a0 = b0.xzyw + s0.xzyw * sh.xxyy;
    let a1 = b1.xzyw + s1.xzyw * sh.zzww;

    var p0 = vec3<f32>(a0.xy, h.x);
    var p1 = vec3<f32>(a0.zw, h.y);
    var p2 = vec3<f32>(a1.xy, h.z);
    var p3 = vec3<f32>(a1.zw, h.w);

    let norm = _taylorInvSqrt4(vec4<f32>(dot(p0, p0), dot(p1, p1), dot(p2, p2), dot(p3, p3)));
    p0 = p0 * norm.x;
    p1 = p1 * norm.y;
    p2 = p2 * norm.z;
    p3 = p3 * norm.w;

    var m = max(vec4<f32>(0.6) - vec4<f32>(dot(x0, x0), dot(x1, x1), dot(x2, x2), dot(x3, x3)), vec4<f32>(0.0));
    m = m * m;
    return 42.0 * dot(m * m, vec4<f32>(dot(p0, x0), dot(p1, x1), dot(p2, x2), dot(p3, x3)));
}
"#;

/// Every hydra function Rudel implements, in the order hydra declares them.
///
/// `src` and `sum` are absent on purpose; see `hydra::UNIMPLEMENTED`.
pub(super) static FUNCTIONS: &[HydraFn] = &[
    // ---- src ---------------------------------------------------------------
    HydraFn {
        name: "noise",
        ty: FnType::Src,
        inputs: &[f("scale", 10.0), f("offset", 0.1)],
        helpers: &[Helper::Noise],
        wgsl: "return vec4<f32>(vec3<f32>(_noise(vec3<f32>(_st * scale, offset * hu.time))), 1.0);",
    },
    HydraFn {
        name: "voronoi",
        ty: FnType::Src,
        inputs: &[f("scale", 5.0), f("speed", 0.3), f("blending", 0.3)],
        helpers: &[],
        wgsl: r#"
    var color = vec3<f32>(0.0);
    let st = _st * scale;
    let i_st = floor(st);
    let f_st = fract(st);
    var m_dist = 10.0;
    var m_point = vec2<f32>(0.0);
    for (var j: i32 = -1; j <= 1; j = j + 1) {
        for (var i: i32 = -1; i <= 1; i = i + 1) {
            let neighbor = vec2<f32>(f32(i), f32(j));
            let p = i_st + neighbor;
            var point = fract(sin(vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)))) * 43758.5453);
            point = vec2<f32>(0.5) + 0.5 * sin(hu.time * speed + 6.2831 * point);
            let diff = neighbor + point - f_st;
            let dist = length(diff);
            if (dist < m_dist) {
                m_dist = dist;
                m_point = point;
            }
        }
    }
    color = color + vec3<f32>(dot(m_point, vec2<f32>(0.3, 0.6)));
    color = color * (1.0 - blending * m_dist);
    return vec4<f32>(color, 1.0);"#,
    },
    HydraFn {
        name: "osc",
        ty: FnType::Src,
        inputs: &[f("frequency", 60.0), f("sync", 0.1), f("offset", 0.0)],
        helpers: &[],
        wgsl: r#"
    let st = _st;
    let r = sin((st.x - offset / frequency + hu.time * sync) * frequency) * 0.5 + 0.5;
    let g = sin((st.x + hu.time * sync) * frequency) * 0.5 + 0.5;
    let b = sin((st.x + offset / frequency + hu.time * sync) * frequency) * 0.5 + 0.5;
    return vec4<f32>(r, g, b, 1.0);"#,
    },
    HydraFn {
        name: "shape",
        ty: FnType::Src,
        inputs: &[f("sides", 3.0), f("radius", 0.3), f("smoothing", 0.01)],
        helpers: &[],
        wgsl: r#"
    let st = _st * 2.0 - vec2<f32>(1.0);
    let a = atan2(st.x, st.y) + 3.1416;
    let r = (2.0 * 3.1416) / sides;
    let d = cos(floor(0.5 + a / r) * r - a) * length(st);
    return vec4<f32>(vec3<f32>(1.0 - smoothstep(radius, radius + smoothing + 0.0000001, d)), 1.0);"#,
    },
    HydraFn {
        name: "gradient",
        ty: FnType::Src,
        inputs: &[f("speed", 0.0)],
        helpers: &[],
        wgsl: "return vec4<f32>(_st, sin(hu.time * speed), 1.0);",
    },
    HydraFn {
        name: "prev",
        ty: FnType::Src,
        inputs: &[],
        helpers: &[],
        // Upstream reads `prevBuffer`, the output buffer this chain last drew
        // into. A Rudel hydra widget has exactly one such buffer, so this is
        // the widget's own previous frame.
        wgsl: "return textureSample(hPrevTex, hPrevSampler, fract(_st));",
    },
    HydraFn {
        name: "solid",
        ty: FnType::Src,
        inputs: &[f("r", 0.0), f("g", 0.0), f("b", 0.0), f("a", 1.0)],
        helpers: &[],
        wgsl: "return vec4<f32>(r, g, b, a);",
    },
    // ---- coord -------------------------------------------------------------
    HydraFn {
        name: "rotate",
        ty: FnType::Coord,
        inputs: &[f("angle", 10.0), f("speed", 0.0)],
        helpers: &[],
        wgsl: r#"
    var xy = _st - vec2<f32>(0.5);
    let ang = angle + speed * hu.time;
    xy = mat2x2<f32>(vec2<f32>(cos(ang), -sin(ang)), vec2<f32>(sin(ang), cos(ang))) * xy;
    xy = xy + vec2<f32>(0.5);
    return xy;"#,
    },
    HydraFn {
        name: "scale",
        ty: FnType::Coord,
        inputs: &[
            f("amount", 1.5),
            f("xMult", 1.0),
            f("yMult", 1.0),
            f("offsetX", 0.5),
            f("offsetY", 0.5),
        ],
        helpers: &[],
        wgsl: r#"
    var xy = _st - vec2<f32>(offsetX, offsetY);
    xy = xy * (vec2<f32>(1.0) / vec2<f32>(amount * xMult, amount * yMult));
    xy = xy + vec2<f32>(offsetX, offsetY);
    return xy;"#,
    },
    HydraFn {
        name: "pixelate",
        ty: FnType::Coord,
        inputs: &[f("pixelX", 20.0), f("pixelY", 20.0)],
        helpers: &[],
        wgsl: r#"
    let xy = vec2<f32>(pixelX, pixelY);
    return (floor(_st * xy) + vec2<f32>(0.5)) / xy;"#,
    },
    HydraFn {
        name: "repeat",
        ty: FnType::Coord,
        inputs: &[
            f("repeatX", 3.0),
            f("repeatY", 3.0),
            f("offsetX", 0.0),
            f("offsetY", 0.0),
        ],
        helpers: &[],
        wgsl: r#"
    var st = _st * vec2<f32>(repeatX, repeatY);
    st.x = st.x + step(1.0, _mod(st.y, 2.0)) * offsetX;
    st.y = st.y + step(1.0, _mod(st.x, 2.0)) * offsetY;
    return fract(st);"#,
    },
    HydraFn {
        name: "repeatX",
        ty: FnType::Coord,
        inputs: &[f("reps", 3.0), f("offset", 0.0)],
        helpers: &[],
        wgsl: r#"
    var st = _st * vec2<f32>(reps, 1.0);
    st.y = st.y + step(1.0, _mod(st.x, 2.0)) * offset;
    return fract(st);"#,
    },
    HydraFn {
        name: "repeatY",
        ty: FnType::Coord,
        inputs: &[f("reps", 3.0), f("offset", 0.0)],
        helpers: &[],
        wgsl: r#"
    var st = _st * vec2<f32>(1.0, reps);
    st.x = st.x + step(1.0, _mod(st.y, 2.0)) * offset;
    return fract(st);"#,
    },
    HydraFn {
        name: "kaleid",
        ty: FnType::Coord,
        inputs: &[f("nSides", 4.0)],
        helpers: &[],
        wgsl: r#"
    let st = _st - vec2<f32>(0.5);
    let r = length(st);
    var a = atan2(st.y, st.x);
    let pi = 2.0 * 3.1416;
    a = _mod(a, pi / nSides);
    a = abs(a - pi / nSides / 2.0);
    return r * vec2<f32>(cos(a), sin(a));"#,
    },
    HydraFn {
        name: "scroll",
        ty: FnType::Coord,
        inputs: &[
            f("scrollX", 0.5),
            f("scrollY", 0.5),
            f("speedX", 0.0),
            f("speedY", 0.0),
        ],
        helpers: &[],
        wgsl: r#"
    var st = _st;
    st.x = st.x + scrollX + hu.time * speedX;
    st.y = st.y + scrollY + hu.time * speedY;
    return fract(st);"#,
    },
    HydraFn {
        name: "scrollX",
        ty: FnType::Coord,
        inputs: &[f("scrollX", 0.5), f("speed", 0.0)],
        helpers: &[],
        wgsl: r#"
    var st = _st;
    st.x = st.x + scrollX + hu.time * speed;
    return fract(st);"#,
    },
    HydraFn {
        name: "scrollY",
        ty: FnType::Coord,
        inputs: &[f("scrollY", 0.5), f("speed", 0.0)],
        helpers: &[],
        wgsl: r#"
    var st = _st;
    st.y = st.y + scrollY + hu.time * speed;
    return fract(st);"#,
    },
    // ---- color -------------------------------------------------------------
    HydraFn {
        name: "posterize",
        ty: FnType::Color,
        inputs: &[f("bins", 3.0), f("gamma", 0.6)],
        helpers: &[],
        wgsl: r#"
    var c2 = pow(_c0, vec4<f32>(gamma));
    c2 = c2 * vec4<f32>(bins);
    c2 = floor(c2);
    c2 = c2 / vec4<f32>(bins);
    c2 = pow(c2, vec4<f32>(1.0 / gamma));
    return vec4<f32>(c2.xyz, _c0.a);"#,
    },
    HydraFn {
        name: "shift",
        ty: FnType::Color,
        inputs: &[f("r", 0.5), f("g", 0.0), f("b", 0.0), f("a", 0.0)],
        helpers: &[],
        wgsl: r#"
    var c2 = _c0;
    c2.r = fract(c2.r + r);
    c2.g = fract(c2.g + g);
    c2.b = fract(c2.b + b);
    c2.a = fract(c2.a + a);
    return c2;"#,
    },
    HydraFn {
        name: "invert",
        ty: FnType::Color,
        inputs: &[f("amount", 1.0)],
        helpers: &[],
        wgsl: "return vec4<f32>((vec3<f32>(1.0) - _c0.rgb) * amount + _c0.rgb * (1.0 - amount), _c0.a);",
    },
    HydraFn {
        name: "contrast",
        ty: FnType::Color,
        inputs: &[f("amount", 1.6)],
        helpers: &[],
        wgsl: r#"
    let c = (_c0 - vec4<f32>(0.5)) * vec4<f32>(amount) + vec4<f32>(0.5);
    return vec4<f32>(c.rgb, _c0.a);"#,
    },
    HydraFn {
        name: "brightness",
        ty: FnType::Color,
        inputs: &[f("amount", 0.4)],
        helpers: &[],
        wgsl: "return vec4<f32>(_c0.rgb + vec3<f32>(amount), _c0.a);",
    },
    HydraFn {
        name: "luma",
        ty: FnType::Color,
        inputs: &[f("threshold", 0.5), f("tolerance", 0.1)],
        helpers: &[Helper::Luminance],
        wgsl: r#"
    let a = smoothstep(threshold - (tolerance + 0.0000001), threshold + (tolerance + 0.0000001), _luminance(_c0.rgb));
    return vec4<f32>(_c0.rgb * a, a);"#,
    },
    HydraFn {
        name: "thresh",
        ty: FnType::Color,
        inputs: &[f("threshold", 0.5), f("tolerance", 0.04)],
        helpers: &[Helper::Luminance],
        wgsl: r#"
    return vec4<f32>(vec3<f32>(smoothstep(threshold - (tolerance + 0.0000001), threshold + (tolerance + 0.0000001), _luminance(_c0.rgb))), _c0.a);"#,
    },
    HydraFn {
        name: "color",
        ty: FnType::Color,
        inputs: &[f("r", 1.0), f("g", 1.0), f("b", 1.0), f("a", 1.0)],
        helpers: &[],
        wgsl: r#"
    let c = vec4<f32>(r, g, b, a);
    let pos = step(vec4<f32>(0.0), c);
    return mix((vec4<f32>(1.0) - _c0) * abs(c), c * _c0, pos);"#,
    },
    HydraFn {
        name: "saturate",
        ty: FnType::Color,
        inputs: &[f("amount", 2.0)],
        helpers: &[],
        wgsl: r#"
    let W = vec3<f32>(0.2125, 0.7154, 0.0721);
    let intensity = vec3<f32>(dot(_c0.rgb, W));
    return vec4<f32>(mix(intensity, _c0.rgb, amount), _c0.a);"#,
    },
    HydraFn {
        name: "hue",
        ty: FnType::Color,
        inputs: &[f("hue", 0.4)],
        helpers: &[Helper::RgbToHsv, Helper::HsvToRgb],
        wgsl: r#"
    var c = _rgbToHsv(_c0.rgb);
    c.r = c.r + hue;
    return vec4<f32>(_hsvToRgb(c), _c0.a);"#,
    },
    HydraFn {
        name: "colorama",
        ty: FnType::Color,
        inputs: &[f("amount", 0.005)],
        helpers: &[Helper::RgbToHsv, Helper::HsvToRgb],
        wgsl: r#"
    var c = _rgbToHsv(_c0.rgb);
    c = c + vec3<f32>(amount);
    c = _hsvToRgb(c);
    c = fract(c);
    return vec4<f32>(c, _c0.a);"#,
    },
    HydraFn {
        name: "r",
        ty: FnType::Color,
        inputs: &[f("scale", 1.0), f("offset", 0.0)],
        helpers: &[],
        wgsl: "return vec4<f32>(_c0.r * scale + offset);",
    },
    HydraFn {
        name: "g",
        ty: FnType::Color,
        inputs: &[f("scale", 1.0), f("offset", 0.0)],
        helpers: &[],
        wgsl: "return vec4<f32>(_c0.g * scale + offset);",
    },
    HydraFn {
        name: "b",
        ty: FnType::Color,
        inputs: &[f("scale", 1.0), f("offset", 0.0)],
        helpers: &[],
        wgsl: "return vec4<f32>(_c0.b * scale + offset);",
    },
    HydraFn {
        name: "a",
        ty: FnType::Color,
        inputs: &[f("scale", 1.0), f("offset", 0.0)],
        helpers: &[],
        wgsl: "return vec4<f32>(_c0.a * scale + offset);",
    },
    // ---- combine -----------------------------------------------------------
    HydraFn {
        name: "add",
        ty: FnType::Combine,
        inputs: &[f("amount", 1.0)],
        helpers: &[],
        wgsl: "return (_c0 + _c1) * amount + _c0 * (1.0 - amount);",
    },
    HydraFn {
        name: "sub",
        ty: FnType::Combine,
        inputs: &[f("amount", 1.0)],
        helpers: &[],
        wgsl: "return (_c0 - _c1) * amount + _c0 * (1.0 - amount);",
    },
    HydraFn {
        name: "layer",
        ty: FnType::Combine,
        inputs: &[],
        helpers: &[],
        wgsl: "return vec4<f32>(mix(_c0.rgb, _c1.rgb, _c1.a), clamp(_c0.a + _c1.a, 0.0, 1.0));",
    },
    HydraFn {
        name: "blend",
        ty: FnType::Combine,
        inputs: &[f("amount", 0.5)],
        helpers: &[],
        wgsl: "return _c0 * (1.0 - amount) + _c1 * amount;",
    },
    HydraFn {
        name: "mult",
        ty: FnType::Combine,
        inputs: &[f("amount", 1.0)],
        helpers: &[],
        wgsl: "return _c0 * (1.0 - amount) + (_c0 * _c1) * amount;",
    },
    HydraFn {
        name: "diff",
        ty: FnType::Combine,
        inputs: &[],
        helpers: &[],
        wgsl: "return vec4<f32>(abs(_c0.rgb - _c1.rgb), max(_c0.a, _c1.a));",
    },
    HydraFn {
        name: "mask",
        ty: FnType::Combine,
        inputs: &[],
        helpers: &[Helper::Luminance],
        wgsl: r#"
    let a = _luminance(_c1.rgb);
    return vec4<f32>(_c0.rgb * a, a * _c0.a);"#,
    },
    // ---- combineCoord ------------------------------------------------------
    HydraFn {
        name: "modulateRepeat",
        ty: FnType::CombineCoord,
        inputs: &[
            f("repeatX", 3.0),
            f("repeatY", 3.0),
            f("offsetX", 0.5),
            f("offsetY", 0.5),
        ],
        helpers: &[],
        wgsl: r#"
    var st = _st * vec2<f32>(repeatX, repeatY);
    st.x = st.x + step(1.0, _mod(st.y, 2.0)) + _c0.r * offsetX;
    st.y = st.y + step(1.0, _mod(st.x, 2.0)) + _c0.g * offsetY;
    return fract(st);"#,
    },
    HydraFn {
        name: "modulateRepeatX",
        ty: FnType::CombineCoord,
        inputs: &[f("reps", 3.0), f("offset", 0.5)],
        helpers: &[],
        wgsl: r#"
    var st = _st * vec2<f32>(reps, 1.0);
    st.y = st.y + step(1.0, _mod(st.x, 2.0)) + _c0.r * offset;
    return fract(st);"#,
    },
    HydraFn {
        name: "modulateRepeatY",
        ty: FnType::CombineCoord,
        inputs: &[f("reps", 3.0), f("offset", 0.5)],
        helpers: &[],
        wgsl: r#"
    var st = _st * vec2<f32>(reps, 1.0);
    st.x = st.x + step(1.0, _mod(st.y, 2.0)) + _c0.r * offset;
    return fract(st);"#,
    },
    HydraFn {
        name: "modulateKaleid",
        ty: FnType::CombineCoord,
        inputs: &[f("nSides", 4.0)],
        helpers: &[],
        wgsl: r#"
    let st = _st - vec2<f32>(0.5);
    let r = length(st);
    var a = atan2(st.y, st.x);
    let pi = 2.0 * 3.1416;
    a = _mod(a, pi / nSides);
    a = abs(a - pi / nSides / 2.0);
    return (_c0.r + r) * vec2<f32>(cos(a), sin(a));"#,
    },
    HydraFn {
        name: "modulateScrollX",
        ty: FnType::CombineCoord,
        inputs: &[f("scrollX", 0.5), f("speed", 0.0)],
        helpers: &[],
        wgsl: r#"
    var st = _st;
    st.x = st.x + _c0.r * scrollX + hu.time * speed;
    return fract(st);"#,
    },
    HydraFn {
        name: "modulateScrollY",
        ty: FnType::CombineCoord,
        inputs: &[f("scrollY", 0.5), f("speed", 0.0)],
        helpers: &[],
        wgsl: r#"
    var st = _st;
    st.y = st.y + _c0.r * scrollY + hu.time * speed;
    return fract(st);"#,
    },
    HydraFn {
        name: "modulate",
        ty: FnType::CombineCoord,
        inputs: &[f("amount", 0.1)],
        helpers: &[],
        wgsl: "return _st + _c0.xy * amount;",
    },
    HydraFn {
        name: "modulateScale",
        ty: FnType::CombineCoord,
        inputs: &[f("multiple", 1.0), f("offset", 1.0)],
        helpers: &[],
        wgsl: r#"
    var xy = _st - vec2<f32>(0.5);
    xy = xy * (vec2<f32>(1.0) / vec2<f32>(offset + multiple * _c0.r, offset + multiple * _c0.g));
    xy = xy + vec2<f32>(0.5);
    return xy;"#,
    },
    HydraFn {
        name: "modulatePixelate",
        ty: FnType::CombineCoord,
        inputs: &[f("multiple", 10.0), f("offset", 3.0)],
        helpers: &[],
        wgsl: r#"
    let xy = vec2<f32>(offset + _c0.x * multiple, offset + _c0.y * multiple);
    return (floor(_st * xy) + vec2<f32>(0.5)) / xy;"#,
    },
    HydraFn {
        name: "modulateRotate",
        ty: FnType::CombineCoord,
        inputs: &[f("multiple", 1.0), f("offset", 0.0)],
        helpers: &[],
        wgsl: r#"
    var xy = _st - vec2<f32>(0.5);
    let angle = offset + _c0.x * multiple;
    xy = mat2x2<f32>(vec2<f32>(cos(angle), -sin(angle)), vec2<f32>(sin(angle), cos(angle))) * xy;
    xy = xy + vec2<f32>(0.5);
    return xy;"#,
    },
    HydraFn {
        name: "modulateHue",
        ty: FnType::CombineCoord,
        inputs: &[f("amount", 1.0)],
        helpers: &[],
        wgsl: r#"
    return _st + (vec2<f32>(_c0.g - _c0.r, _c0.b - _c0.g) * amount * (vec2<f32>(1.0) / hu.res));"#,
    },
];
