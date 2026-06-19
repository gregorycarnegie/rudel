// gen_tonal_oracle.mjs — dump golden tonal/xen outputs from the real Strudel
// engine for the rudel parity oracle (crates/rudel-mini/tests/tonal_parity.rs).
//
//   cd tools/oracle && node gen_tonal_oracle.mjs
//
// Each entry is a labelled Pattern built with @strudel/tonal + @strudel/xen; the
// Rust test reconstructs the same pattern with rudel-core and compares hap timing
// exactly and pitch values with a tolerance. Note-name values are normalised to
// MIDI numbers here so they compare against rudel's numeric note output (rudel
// intentionally emits MIDI numbers rather than enharmonic note names).
//
// Requires the @strudel/{core,mini,tonal,xen} symlinks in node_modules plus the
// @tonaljs/tonal + chord-voicings deps that @strudel/tonal pulls in:
//   npm install --no-save @tonaljs/tonal chord-voicings
// (re-create the strudel symlinks afterwards — npm install prunes them).

import { writeFileSync } from 'node:fs';
import { mini } from '@strudel/mini';
import { note, n, i, freq, noteToMidi } from '@strudel/core';
import { scale, transpose, scaleTranspose } from '@strudel/tonal';
import { xen, withBase, ftrans } from '@strudel/xen';
import { tune } from '@strudel/xen/tune.mjs';

// label -> Pattern. String scale/interval args are passed raw (reify -> pure),
// matching how rudel reconstructs them from Value::Str; only the *base* step
// pattern is mini-parsed (matching rudel's rudel_mini::parse).
const CASES = {
  // --- transpose / trans -----------------------------------------------------
  transpose_num: note(mini('c4 e4 g4')).transpose(7),
  transpose_interval: note(mini('c4 e4 g4')).transpose('3M'),
  transpose_interval_neg: note(mini('c3 c4')).transpose('-2M'),
  transpose_bare_num: mini('0 4 7').transpose(12),
  transpose_bare_interval: mini('0 4 7').transpose('5P'),
  transpose_pat: note(mini('c4 e4')).transpose(mini('<0 7>')),

  // --- scale (degree -> note, and note -> nearest scale note) ---------------
  scale_major: n(mini('0 1 2 3 4 5 6 7')).scale('C:major'),
  scale_minor: n(mini('0 2 4 7')).scale('A:minor'),
  scale_root_oct: n(mini('0 7 -1')).scale('C4:major'),
  scale_neg: n(mini('-1 -2 -7')).scale('C:major'),
  scale_sharps_flats: n(mini('0 1b 4#')).scale('C:major'),
  scale_pentatonic: n(mini('0 1 2 3 4 5')).scale('C:minor:pentatonic'),
  scale_chromatic: n(mini('0 1 2 3 11')).scale('C:chromatic'),
  scale_bebop: n(mini('0 1 2 3 4 5 6 7')).scale('C:bebop:major'),
  // quantisation: a chromatic note off the scale -> nearest scale note. These
  // exercise Strudel's preferHigher tie-break + the octave wrap candidate.
  scale_quantize_tie: note(mini('c#3 d#3 f#3')).scale('C:major'),
  scale_quantize_octave: note(mini('b3 b4')).scale('C:major:pentatonic'),
  scale_pat: n(mini('0 2 4')).scale(mini('<C:major A:minor>')),

  // --- scaleTranspose / scaleTrans / strans ---------------------------------
  strans_pos: n(mini('0 1 2')).scale('C:major').scaleTranspose(2),
  strans_neg: n(mini('0 2 4')).scale('C:major').scaleTranspose(-1),
  strans_pat: n(mini('0 2 4')).scale('C:major').scaleTranspose(mini('<0 -2>')),

  // --- xen (edo, ratio list, preset, tune name) ------------------------------
  xen_edo31: i(mini('0 8 18')).xen('31edo'),
  xen_edo12: i(mini('0 3 7 12')).xen('12edo'),
  xen_ratios: i(mini('0 1 2')).xen([1, 5 / 4, 3 / 2]),
  xen_ji: i(mini('0 1 2 3 4 5')).xen('12ji'),
  xen_tune_name: i(mini('0 1 2 3 4 5')).xen('hexany15'),
  xen_neg: i(mini('-1 -2')).xen([1, 5 / 4, 3 / 2]),
  xen_pat: i(mini('0 1 2')).xen(mini('<5edo 12edo>')),

  // --- withBase --------------------------------------------------------------
  withbase_num: i(mini('0 3 7')).xen('12edo').withBase(440),
  withbase_pair: i(mini('0 3')).xen('12edo').withBase([440, 110]),

  // --- ftrans / ftranspose ---------------------------------------------------
  ftrans_ctx: i(mini('0 8 18')).xen('31edo').ftrans(7),
  ftrans_explicit: freq(mini('200 300')).ftrans([7, 31]),
  ftrans_default12: freq(mini('200 400')).ftrans(7),
  ftrans_step_edo: i(mini('0 7')).xen('31edo').ftrans(mini('1:12')),
  ftrans_pat: i(mini('0 8')).xen('31edo').ftrans(mini('<8 -8>')),

  // --- tuning (registered on Pattern.prototype by importing @strudel/xen) -----
  tuning_ratios: mini('0 1 2 3').tuning([1, 5 / 4, 3 / 2]),

  // --- tune ------------------------------------------------------------------
  tune_name: i(mini('0 1 2 3 4 5')).tune('hexany15'),
  tune_array: i(mini('0 1 2')).tune([261.6255653006, 302.72962012827, 350.29154279212]),
};

const CYCLES = 2;

function fracStr(f) {
  return `${f.s < 0 ? '-' : ''}${f.n}/${f.d}`;
}

function toMidi(x) {
  return typeof x === 'number' ? x : noteToMidi(x);
}

// Reduce a hap value to a (kind, number) pitch descriptor.
function norm(v) {
  if (v && typeof v === 'object') {
    if (v.freq !== undefined) return ['freq', v.freq];
    if (v.note !== undefined) return ['note', toMidi(v.note)];
    if (v.n !== undefined) return ['note', toMidi(v.n)];
    return ['other', 0];
  }
  if (typeof v === 'number') return ['num', v];
  return ['note', toMidi(v)];
}

function dump(pat) {
  return pat.queryArc(0, CYCLES).map((h) => {
    const [k, x] = norm(h.value);
    return {
      pb: fracStr(h.part.begin),
      pe: fracStr(h.part.end),
      wb: h.whole ? fracStr(h.whole.begin) : null,
      we: h.whole ? fracStr(h.whole.end) : null,
      k,
      x,
    };
  });
}

const out = {};
for (const [label, pat] of Object.entries(CASES)) {
  out[label] = dump(pat);
}
writeFileSync(new URL('./tonal_golden.json', import.meta.url), JSON.stringify(out, null, 1));
console.error(`wrote tonal_golden.json (${Object.keys(out).length} cases)`);
