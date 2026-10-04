// Generate rudel's editor theme table from Strudel's CodeMirror themes.
//
// The names a script passes to `theme(...)` come from the registry in
// strudel/packages/codemirror/themes.mjs. Each theme module is evaluated with
// stand-ins for `@lezer/highlight`'s tags and the theme helper, so the colours
// they compute (palette arrays, constants) come out as the browser sees them:
// the exported `settings` are what `@strudel/draw` paints with, the `styles`
// the syntax colours.
//
//   node tools/generate_themes.mjs
// writes crates/rudel-app/src/editor/themes_generated.rs.

import { readFileSync, writeFileSync } from 'node:fs';
import { colorMap } from '../strudel/packages/draw/color.mjs';

const root = new URL('../', import.meta.url);
const cm = new URL('strudel/packages/codemirror/', root);
const out = new URL('crates/rudel-app/src/editor/themes_generated.rs', root);

const named = { white: [255, 255, 255, 255], black: [0, 0, 0, 255], transparent: [0, 0, 0, 0] };
function color(css) {
  if (css === undefined || css === null) return null;
  css = String(css).trim().toLowerCase();
  if (css in named) return named[css];
  if (css in colorMap) return color(colorMap[css]);
  if (css.startsWith('#')) {
    let h = css.slice(1);
    if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join('');
    if (h.length === 6) h += 'ff';
    if (!/^[0-9a-f]{8}$/.test(h)) throw new Error(`bad hex colour ${css}`);
    return [0, 2, 4, 6].map((i) => parseInt(h.slice(i, i + 2), 16));
  }
  const m = /^rgba?\(([^)]*)\)$/.exec(css);
  if (m) {
    const p = m[1].split(',').map((x) => x.trim());
    return [...p.slice(0, 3).map((x) => Math.round(Number(x))), p[3] === undefined ? 255 : Math.round(Number(p[3]) * 255)];
  }
  const hsl = /^hsla?\(([^)]*)\)$/.exec(css);
  if (hsl) {
    const [h, s, l, a = 1] = hsl[1].split(',').map((x) => parseFloat(x));
    const [S, L] = [s / 100, l / 100];
    const k = (n) => (n + h / 30) % 12;
    const f = (n) => L - S * Math.min(L, 1 - L) * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1));
    return [f(0), f(8), f(4)].map((v) => Math.round(v * 255)).concat(Math.round(a * 255));
  }
  if (css === 'inherit' || css === '' || css === 'none') return null;
  throw new Error(`unknown colour ${css}`);
}

// `t.keyword` is a plain tag; `t.special(t.variableName)` a modified one,
// which is not what the palette means by "variable name".
const t = new Proxy(
  {},
  {
    get: (_, name) => {
      const modifier = (inner) => ({ modifier: name, inner });
      modifier.plain = name;
      return modifier;
    },
  },
);

function evaluate(file) {
  const text = readFileSync(new URL(`themes/${file}.mjs`, cm), 'utf8')
    .replace(/^import .*$/gm, '')
    .replace(/export default /, '__default = ')
    .replace(/export const /g, 'const ');
  const run = new Function('t', 'createTheme', `let __default;\n${text}\n;return { settings, theme: __default };`);
  return run(t, (spec) => spec);
}

// The colour each plain tag gets: the first style naming it wins, as in
// CodeMirror, where earlier rules take precedence.
function styleMap(styles = []) {
  const map = {};
  for (const style of styles) {
    for (const tag of [style.tag].flat()) {
      if (typeof tag === 'function' && !(tag.plain in map) && style.color !== undefined) map[tag.plain] = style.color;
    }
  }
  return map;
}

const registry = readFileSync(new URL('themes.mjs', cm), 'utf8');
const modules = Object.fromEntries(
  [...registry.matchAll(/import (\w+),[^;]*from '\.\/themes\/([\w-]+)\.mjs'/g)].map((m) => [m[1], m[2]]),
);
const listed = registry.slice(registry.indexOf('export const themes = {'));
const names = [...listed.slice(0, listed.indexOf('};')).matchAll(/^\s+(\w+),$/gm)].map((m) => m[1]);

const rgba = (c) => `[${c.join(', ')}]`;
const rows = names.map((name) => {
  const { settings: s, theme } = evaluate(modules[name]);
  const styles = styleMap(theme?.styles);
  const fg = color(s.foreground);
  const bg = color(s.background);
  const setting = (key, fallback) => color(s[key]) ?? fallback;
  const style = (tags, fallback = fg) => {
    for (const tag of tags) {
      const c = color(styles[tag]);
      if (c) return c;
    }
    return fallback;
  };
  const muted = setting('muted', [...fg.slice(0, 3), 0x50]);
  const string = style(['string']);
  const light = s.light === true || theme?.theme === 'light';
  return `    ThemeData {
        name: "${name}",
        light: ${light},
        background: ${rgba(bg)},
        line_background: ${rgba(setting('lineBackground', [...bg.slice(0, 3), 0x99]))},
        foreground: ${rgba(fg)},
        muted: ${rgba(muted)},
        caret: ${rgba(setting('caret', fg))},
        selection: ${rgba(setting('selection', [...fg.slice(0, 3), 0x40]))},
        selection_match: ${rgba(setting('selectionMatch', [...fg.slice(0, 3), 0x26]))},
        line_highlight: ${rgba(setting('lineHighlight', [...fg.slice(0, 3), 0x1a]))},
        gutter_background: ${rgba(setting('gutterBackground', [0, 0, 0, 0]))},
        gutter_foreground: ${rgba(setting('gutterForeground', muted))},
        keyword: ${rgba(style(['keyword']))},
        method: ${rgba(style(['propertyName', 'variableName']))},
        string: ${rgba(string)},
        number: ${rgba(style(['number']))},
        comment: ${rgba(style(['comment'], muted))},
        mini_op: ${rgba(style(['punctuation', 'operator', 'keyword']))},
        mini_word: ${rgba(string)},
    },`;
});

writeFileSync(
  out,
  `// Generated by tools/generate_themes.mjs from Strudel's CodeMirror themes
// (strudel/packages/codemirror/themes.mjs and themes/*.mjs). Do not edit.

use super::settings::ThemeData;

pub(crate) static THEMES: [ThemeData; ${rows.length}] = [
${rows.join('\n')}
];
`,
);
console.log(`${rows.length} themes -> crates/rudel-app/src/editor/themes_generated.rs`);
