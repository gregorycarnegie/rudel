import { writeFileSync } from 'node:fs';

export function writeJson(filename, data, space) {
  writeFileSync(new URL(filename, import.meta.url), JSON.stringify(data, null, space));
}

export function fracStr(f) {
  return `${f.s < 0 ? '-' : ''}${f.n}/${f.d}`;
}

export function normValue(v) {
  if (v === null || v === undefined) return null;
  if (Array.isArray(v)) return v.map(normValue);
  if (typeof v === 'object') {
    const o = {};
    for (const k of Object.keys(v).sort()) o[k] = normValue(v[k]);
    return o;
  }
  return v;
}
