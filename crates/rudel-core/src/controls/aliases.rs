use super::named::*;
use super::plain::*;
use crate::pattern::Pattern;
use crate::transforms::IntoPattern;

// Common aliases (Strudel exposes these via `registerControl(names, ...aliases)`).
macro_rules! control_aliases {
    ($($alias:ident => $target:ident),* $(,)?) => {
        $(
            #[doc = concat!("Alias for [`", stringify!($target), "`].")]
            pub fn $alias(pat: impl IntoPattern) -> Pattern {
                $target(pat)
            }
        )*
        impl Pattern {
            $(
                #[doc = concat!("Alias for [`", stringify!($target), "`](Self::", stringify!($target), ").")]
                pub fn $alias(&self, x: impl IntoPattern) -> Pattern {
                    self.$target(x)
                }
            )*
        }

        /// `(alias, builder)` pairs for the aliases above; used by
        /// [`control_name`](crate::controls::control_name) to resolve canonical
        /// control keys.
        pub(super) static ALIAS_CONTROL_BUILDERS: &[(&str, fn(Pattern) -> Pattern)] = &[
            $( (stringify!($alias), |p: Pattern| $target(p)) ),*
        ];
    };
}

control_aliases!(
    lpf => cutoff,
    lp => cutoff,
    ctf => cutoff,
    lpq => resonance,
    hpf => hcutoff,
    hp => hcutoff,
    hpq => hresonance,
    bpf => bandf,
    bp => bandf,
    bpq => bandq,
    vel => velocity,
    att => attack,
    rel => release,
    sus => sustain,
    dec => decay,
    delayt => delaytime,
    delayfb => delayfeedback,
    rlp => roomlp,
    rdim => roomdim,
    rfade => roomfade,
    o => orbit,
    // filter-envelope aliases
    lpe => lpenv,
    lpa => lpattack,
    lpd => lpdecay,
    lps => lpsustain,
    lpr => lprelease,
    hpe => hpenv,
    hpa => hpattack,
    hpd => hpdecay,
    hps => hpsustain,
    hpr => hprelease,
    bpe => bpenv,
    bpa => bpattack,
    bpd => bpdecay,
    bps => bpsustain,
    bpr => bprelease,
    // vibrato + pitch-envelope aliases
    vibrato => vib,
    vmod => vibmod,
    patt => pattack,
    pdec => pdecay,
    psus => psustain,
    prel => prelease,
    // voicing dictionary alias
    dict => dictionary,
    // sound / amplitude / sample-window aliases
    src => source,
    dur => duration,
    // `legato` is a Strudel alias of `clip` (`registerControl('clip', 'legato')`),
    // so `.legato(x)` writes the `clip` key and drives event clipping.
    legato => clip,
    gat => gate,
    // synth aliases
    det => detune,
    fme => fmenv,
    fmatt => fmattack,
    fmdec => fmdecay,
    fmsus => fmsustain,
    fmrel => fmrelease,
    // wavetable / warp envelope aliases
    wtatt => wtattack,
    wtdec => wtdecay,
    wtsus => wtsustain,
    wtrel => wtrelease,
    warpatt => warpattack,
    warpdec => warpdecay,
    warpsus => warpsustain,
    warprel => warprelease,
    // delay aliases
    dfb => delayfeedback,
    dt => delaytime,
    delays => delaysync,
    // tremolo aliases
    trem => tremolo,
    tremdepth => tremolodepth,
    tremskew => tremoloskew,
    tremphase => tremolophase,
    tremshape => tremoloshape,
    // phaser aliases
    ph => phaserrate,
    phs => phasersweep,
    phc => phasercenter,
    phd => phaserdepth,
    phasdp => phaserdepth,
    // ducking aliases (Strudel's canonical key is `duckorbit`)
    duck => duckorbit,
    duckons => duckonset,
    duckatt => duckattack,
    datt => duckattack,
    // channel / pulse-width aliases
    ch => channels,
    pwr => pwrate,
    pws => pwsweep,
    // tonal / spatial aliases
    oct => octave,
    bgain => busgain,
    // reverb / distortion aliases (Rudel's canonical reverb-size key is `size`)
    iresponse => ir,
    roomsize => size,
    sz => size,
    rsize => size,
    dist => distort,
    distvol => distortvol,
    disttype => distorttype,
    // filter LFO aliases
    lpdepthfreq => lpdepthfrequency,
    bpdepthfreq => bpdepthfrequency,
    hpdepthfreq => hpdepthfrequency,
    // vibrato / color aliases
    v => vib,
    colour => color,
    // byte-beat / FX-release aliases
    bbexpr => byte_beat_expression,
    bb => byte_beat_expression,
    bbst => byte_beat_start_time,
    fxr => fx_release,
);
