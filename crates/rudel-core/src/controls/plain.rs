use super::base::control;
use crate::pattern::Pattern;
use crate::transforms::IntoPattern;

macro_rules! controls {
    ($($name:ident),* $(,)?) => {
        $(
            #[doc = concat!("The `", stringify!($name), "` control.")]
            pub fn $name(pat: impl IntoPattern) -> Pattern {
                control(stringify!($name), pat.into_pattern())
            }
        )*

        impl Pattern {
            $(
                #[doc = concat!("Set the `", stringify!($name), "` control, keeping this pattern's structure.")]
                pub fn $name(&self, x: impl IntoPattern) -> Pattern {
                    self.set($name(x))
                }
            )*
        }

        /// `(name, builder)` pairs for the plain controls above; used by
        /// [`control_name`](crate::controls::control_name) to resolve canonical
        /// control keys.
        pub(super) static PLAIN_CONTROL_BUILDERS: &[(&str, fn(Pattern) -> Pattern)] = &[
            $( (stringify!($name), |p: Pattern| $name(p)) ),*
        ];
    };
}

controls!(
    i,
    freq,
    mpe,
    note,
    n,
    gain,
    postgain,
    pan,
    speed,
    room,
    roomlp,
    roomdim,
    roomfade,
    size,
    shape,
    crush,
    cutoff,
    resonance,
    hcutoff,
    hresonance,
    bandf,
    bandq,
    ftype,
    // filter envelopes
    lpenv,
    lpattack,
    lpdecay,
    lpsustain,
    lprelease,
    hpenv,
    hpattack,
    hpdecay,
    hpsustain,
    hprelease,
    bpenv,
    bpattack,
    bpdecay,
    bpsustain,
    bprelease,
    fanchor,
    delay,
    delaytime,
    delayfeedback,
    dry,
    attack,
    decay,
    sustain,
    release,
    vowel,
    bank,
    cut,
    accelerate,
    coarse,
    orbit,
    velocity,
    begin,
    end,
    clip,
    unit,
    // synth: supersaw + FM + ADSR shortcuts
    unison,
    detune,
    spread,
    fm,
    fmh,
    fmi,
    fmwave,
    fmattack,
    fmdecay,
    fmsustain,
    fmrelease,
    pw,
    noise,
    pcurve,
    hold,
    // vibrato + pitch envelope
    vib,
    vibmod,
    penv,
    pattack,
    pdecay,
    psustain,
    prelease,
    panchor,
    // post-fx: tremolo + phaser
    tremolo,
    tremolodepth,
    phaser,
    phaserrate,
    phaserdepth,
    phasercenter,
    phasersweep,
    // OSC routing (read by the OSC back-end to pick a destination)
    oschost,
    oscport,
    // tonal / voicing controls
    mtranspose,
    ctranspose,
    dictionary,
    anchor,
    offset,
    octaves,
    // wavetable position + envelope
    wt,
    wtenv,
    wtattack,
    wtdecay,
    wtsustain,
    wtrelease,
    wtrate,
    wtsync,
    wtdepth,
    wtshape,
    wtdc,
    wtskew,
    wtphaserand,
    // wavetable warp + envelope
    warp,
    warpenv,
    warpattack,
    warpdecay,
    warpsustain,
    warprelease,
    warprate,
    warpsync,
    warpdepth,
    warpshape,
    warpdc,
    warpskew,
    warpmode,
    // sound / amplitude / sample-window extras
    source,
    amp,
    stretch,
    duration,
    gate,
    // filter LFO modulation
    lprate,
    lpsync,
    lpdepth,
    lpdepthfrequency,
    lpshape,
    lpdc,
    lpskew,
    bprate,
    bpsync,
    bpdepth,
    bpdepthfrequency,
    bpshape,
    bpdc,
    bpskew,
    hprate,
    hpsync,
    hpdepth,
    hpdepthfrequency,
    hpshape,
    hpdc,
    hpskew,
    // delay extras + DJ filter
    delayspeed,
    delaysync,
    djf,
    lock,
    // tremolo extras
    tremolosync,
    tremoloskew,
    tremolophase,
    tremoloshape,
    // fx: chorus / drive / ducking / channels / pulse-width LFO / leslie
    chorus,
    drive,
    duckorbit,
    duckdepth,
    duckonset,
    duckattack,
    channels,
    channel,
    pwrate,
    pwsweep,
    leslie,
    lrate,
    lsize,
    // tonal / spatial extras
    degree,
    harmonic,
    nudge,
    octave,
    bus,
    busgain,
    overgain,
    overshape,
    panspan,
    pansplay,
    panwidth,
    panorient,
    slide,
    semitone,
    voice,
    // impulse-response reverb + distortion + compressor
    ir,
    irspeed,
    irbegin,
    distort,
    distortvol,
    distorttype,
    compressor,
    // SuperDirt / SuperDough misc
    analyze,
    fft,
    squiz,
    waveloss,
    density,
    expression,
    sustainpedal,
    fshift,
    fshiftnote,
    fshiftphase,
    triode,
    krush,
    kcutoff,
    octer,
    octersub,
    octersubsub,
    ring,
    ringf,
    ringdf,
    freeze,
    xsdelay,
    tsdelay,
    real,
    imag,
    enhance,
    comb,
    smear,
    scram,
    binshift,
    hbrick,
    lbrick,
    frames,
    hours,
    minutes,
    seconds,
    uid,
    val,
    cps,
    // ZZFX
    zrand,
    curve,
    znoise,
    zmod,
    zcrush,
    zdelay,
    zzfx,
    // visuals / event metadata
    color,
    transient,
    // FM envelope ramp type
    fmenv,
    // MIDI controls
    midichan,
    midimap,
    midiport,
    midicmd,
    ccn,
    ccv,
    nrpnn,
    nrpv,
    sysexid,
    sysexdata,
    midibend,
    miditouch,
);

/// The `bendRange` control. The Rust function is snake_case while the emitted
/// control key matches Strudel's camelCase spelling.
pub fn bend_range(pat: impl IntoPattern) -> Pattern {
    control("bendRange", pat.into_pattern())
}

impl Pattern {
    /// Set the `bendRange` control, keeping this pattern's structure.
    pub fn bend_range(&self, x: impl IntoPattern) -> Pattern {
        self.set(bend_range(x))
    }
}
