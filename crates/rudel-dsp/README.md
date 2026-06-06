# rudel-dsp

Synthesis, drum, sampler, and post-effect voices for Rudel.

`rudel-dsp` is intentionally device-free: it renders voices sample by sample so
DSP behavior can be tested without an audio device. `rudel-audio` uses this
crate inside its real-time mixer.

## Public Surface

- `VoiceLike`: common interface for renderable stereo voices.
- `Voice`, `VoiceParams`, `Waveform`, `NoiseKind`, and `Adsr` for oscillator and
  noise synth voices.
- `DrumVoice`, `DrumParams`, and `DrumKind` for built-in synthesized drums:
  `bd`, `sd`, `rim`, `cp`, `hh`, `oh`, `lt`, `mt`, `ht`, `rd`, and `cr`.
- `Sample`, `SamplerParams`, and `SamplerVoice` for decoded sample playback.
- `FilterParams` for low-pass, high-pass, and band-pass filtering with envelope
  controls.
- `PostFx` and `PostFxVoice` for bitcrush, waveshaping, distortion, coarse
  sample-rate reduction, post-gain, and vowel formants.
- `VoiceSpec` for selecting synth, sampler, or drum rendering from scheduler
  events.

## Example

```rust
use rudel_core::Value;
use rudel_dsp::{Voice, VoiceParams};
use std::collections::BTreeMap;

let controls = BTreeMap::from([
    ("note".to_string(), Value::Str("a4".to_string())),
    ("s".to_string(), Value::Str("saw".to_string())),
]);

let params = VoiceParams::from_controls(&controls, 0.25);
let mut voice = Voice::new(params, 44_100.0);
let (left, right) = voice.tick();
```

## Tests

```bash
cargo test -p rudel-dsp
```
