// gen_gm_table.mjs — the General MIDI instrument table for soundfont playback.
//
//   cd tools/oracle && node gen_gm_table.mjs
//
// Reads strudel/packages/soundfonts/gm.mjs (the `gm_*` sound name -> list of
// WebAudioFont preset files map) and writes it as a sorted JSON table that
// crates/rudel-audio embeds. Pure data, so it is generated rather than
// hand-copied — the same treatment as the Tune.js scale archive.

import { writeJson } from './lib.mjs';
import gm from '../../strudel/packages/soundfonts/gm.mjs';

const names = Object.keys(gm).sort();
const table = Object.fromEntries(names.map((n) => [n, gm[n]]));
const files = new Set(names.flatMap((n) => gm[n]));

writeJson('./gm_table.json', table, 1);
console.error(`wrote gm_table.json (${names.length} names, ${files.size} distinct preset files)`);
