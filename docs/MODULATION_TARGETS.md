# Modulation targets

What each control does as an `lfo`/`env`/`bmod` target, matched to superdough.

A modulator names a `control` (and optionally a `subControl`). superdough looks
the pair up in `CONTROL_TARGETS` (superdoughdata.mjs): `${control}_${subControl}`
first, then the control alone, after stripping any `_<id>` suffix. That gives a
node key and an AudioParam name. The modulator's output is then *added* to that
param, scaled by `depthabs`, or by `depth × the param's current value` (with 0
read as 1). A `frequency` param whose current value is at least 30 has the sum
clamped to 20 Hz..24 kHz.

rudel matches every row of the table below; `every_modulation_target_does_what_superdough_does`
(crates/rudel-audio/src/mixer/tests.rs) renders each one with and without its
modulator.

Upstream has three outcomes, and rudel reproduces each:

- **works**: the param exists and is modulated.
- **skipped**: no such control, or the node is not in the graph (the effect is
  not active, or the node is never registered). Logged, and the hap's other
  modulators still run.
- **throws**: the node exists but has no such param (`targetParams[0].value` of
  `undefined`). That aborts the rest of the hap's modulator setup, so every
  modulator after it in order is skipped too. The note still plays.

A node exists only while its effect is active: `lpf` once `cutoff` is set,
`tremolo` once `tremolo`/`tremolosync` is, and so on. Modulators run in order:
each `FX` stage's (`fxi`) and then the main controls, `lfo` before `env` before
`bmod`, and within each in `__ids` order. So an envelope can drive an LFO, but
an LFO never finds an envelope: it has not been created yet.

| Control | Node · param | Upstream | Notes |
| --- | --- | --- | --- |
| `gain` | `gain` · gain | works | the voice's gain stage, `gain × velocity` after the gain curve |
| `postgain` | `post` · gain | works | main only |
| `pan` | `pan` · pan | works | needs `pan`; the param is bipolar, `2·pan − 1` |
| `stretch` | `stretch` · pitchFactor | works | needs `stretch`; the phase vocoder's pitch factor |
| `transient` | `transient` · attack | throws | registered as a bare node, not a list |
| `tremolo`, `tremolosync` | `tremolo` · frequency | works | needs tremolo; the tremolo LFO's rate |
| `tremolodepth` | `tremolo_gain` · gain | works | the AM gain's base, `1 − depth` |
| `tremoloskew` | `tremolo` · skew | works | |
| `tremoloshape` | `tremolo` · shape | works | a non-integer shape index throws inside the worklet, which then stays silent |
| `tremolophase` | `tremolo` · phase | throws | the LFO's param is `phaseoffset` |
| `cutoff` | `lpf` · frequency | works | `ladder` too (its param is also `frequency`) |
| `resonance` | `lpf` · Q | works | throws on `ftype('ladder')`, whose param is `q` |
| `lpdepth`, `lpdepthfrequency` | `lpf_lfo` · depth | works | needs the filter LFO; throws if `lpf` has none |
| `lpshape` | `lpf_lfo` · shape | works | as `tremoloshape` |
| `lpdc` | `lpf_lfo` · dcoffset | works | |
| `lpskew` | `lpf_lfo` · skew | works | |
| `lprate`, `lpsync` | `lpf_lfo` · rate / sync | throws | the LFO has neither param |
| `hcutoff`, `hresonance`, `hp…` | `hpf`, `hpf_lfo` | as `lpf` | |
| `bandf`, `bandq`, `bp…` | `bpf`, `bpf_lfo` | as `lpf` | |
| `vowel` | `vowel` · frequency | works | every formant filter, offset by the same Hz |
| `coarse`, `crush` | `coarse`, `crush` | works | |
| `shape`, `shapevol` | `shape` · shape / postgain | works | |
| `distort`, `distortvol` | `distort` · distort / postgain | works | |
| `distorttype` | `distort` · distort | works | the distortion *amount*, not the type |
| `compressor` | `compressor` · threshold | works | needs `compressor` |
| `compressorRatio`, `…Knee`, `…Attack`, `…Release` | `compressor` · ratio, knee, attack, release | works | |
| `phaserrate` | `phaser_lfo` · frequency | works | needs `phaserrate` and `phaserdepth > 0` |
| `phasersweep` | `phaser_lfo` · depth | works | |
| `phasercenter` | `phaser` · frequency | works | the notch's centre |
| `phaserdepth` | `phaser` · Q | works | the notch's Q, `2 − clamp(2·depth, 0, 1.9)` |
| `delay` | `delay_mix` · gain | works | the voice's send to the orbit delay; in an `FX` stage (`fxi`), the stage's own delay's wet level |
| `delaytime`, `delaysync` | `delay` · delayTime | works | the orbit's shared delay line, or a stage's own |
| `delayfeedback` | `delay` · feedback | works | the orbit's shared feedback, or a stage's own |
| `room` | `room_mix` · gain | works | the voice's send to the orbit reverb; in an `FX` stage, the stage's own reverb's wet level |
| `djf` | `djf` · value | works | the orbit's DJ filter |
| `dry` | `dry` · gain | skipped | the dry gain is never registered |
| `busgain` | `bus` · gain | skipped | the bus send is never registered |
| `s`, `freq`, `note` | `source` · frequency | works | oscillators and synth worklets; a buffer source (sample, noise, soundfont, ZZFX) has no `frequency`, so its `detune` (cents) is modulated instead; `s("one")` and `s("bus")` throw |
| `detune` | `source` · freqspread | works | supersaw and wavetable; throws on other sources |
| `spread` | `source` · panspread | works | supersaw and wavetable; throws on other sources |
| `wt`, `warp` | `source` · position / warp | works | wavetable; throws on other sources |
| `wtrate`, `wtsync`, `wtdepth`, `wtskew` | `wt_lfo` · frequency, depth, skew | works | needs the position LFO |
| `wtdc` | `wt_lfo` · dc | throws | the LFO's param is `dcoffset` |
| `warprate`, `warpsync`, `warpdepth`, `warpskew`, `warpdc` | `warp_lfo` | as `wt…` | |
| `pw` | `source` · pulsewidth | works | `pulse`; throws on other sources |
| `pwrate`, `pwsweep` | `pw_lfo` · frequency, depth | works | `pulse` with `pwrate`; throws on a pulse without it |
| `fmi` … `fmi8` | `fm_N_gain` · gain | works | operator N's modulation index |
| `fmh` … `fmh8` | `fm_N` · frequency | works | operator N's oscillator, in Hz; its deviation scale stays as built |
| `vib` | `vib` · frequency | works | needs `vib > 0` |
| `vibmod` | `vib_gain` · gain | works | in cents, `vibmod × 100` |
| `byteBeatStartTime` | `source` · byteBeatStartTime | throws | the worklet has no such param |
| `lfo`, `lfo_rate`, `lfo_sync` | `lfo_<id>` · frequency | works | another LFO, named by id (`lfo_0`); a bare `lfo` is skipped |
| `lfo_depth`, `lfo_depthabs` | `lfo_<id>` · depth | works | |
| `lfo_skew`, `lfo_curve`, `lfo_dcoffset` | `lfo_<id>` · skew, curve, dcoffset | works | |
| `env`, `env_attack`, … `env_release` | `env_<id>` · depth, attack, decay, sustain, release | works | another envelope, named by id |
| `bmod`, `bmod_depth`, `bmod_depthabs` | `bmod` · depth | skipped | bus modulators are never registered |

A modulator can only reach one created before it. Within each 128-frame block,
the LFO and envelope worklets read their params once, so a modulated LFO rate
changes per block, not per sample.
