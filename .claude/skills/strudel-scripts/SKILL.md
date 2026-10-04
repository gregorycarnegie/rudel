---
name: strudel-scripts
description: Write Strudel/rudel tune source correctly — which quotes make mini-notation and which make plain strings, and the traps inside .draw/.onPaint/animate callbacks. Use before writing any Strudel or rudel script: a test tune for run-rudel's -Eval, a script string inside a Rust test, a repro for a bug report, or an example for docs. Also use when a tune "does nothing", draws black or invisible shapes, or prints [object Object].
---

# Writing Strudel scripts

A script is JavaScript. Before it runs, a transpiler (upstream's
`plugin-mini`, rudel's `crates/rudel-lang/src/preprocess/mini.rs`) rewrites
some string literals into mini-notation patterns and leaves the rest alone.
**Which quote you use decides which you get.** rudel matches the strudel.cc
REPL exactly here, so a tune that misbehaves for a quoting reason in rudel
misbehaves the same way on strudel.cc. It is the tune, not a bug.

## The rule

| Literal | Becomes | Use for |
|---|---|---|
| `"bd sd"` | mini-notation pattern, `m("bd sd", offset)`, highlighted as it plays | anything patterned: sequences, `<a b>` alternation, `[a b]`, `*2`, `~` |
| `` `bd sd` `` (untagged backtick) | mini-notation, same as double quotes | multi-line patterns |
| `'tomato'` | plain JS string | colours, fonts, text, names, URLs, comparisons |
| `"key": …` (string followed by `:`) | plain object key | object literals |
| ``tag`...` `` (tagged template) | plain argument to the tag | `loadCsound`…` ` |

A single-quoted string passed to a control is **one literal value**: no
sequence, no alternation. `s('<rect ellipse>')` sets `s` to the text
`"<rect ellipse>"`. Upstream's `reify` only mini-parses plain strings after
`miniAllStrings()`, which the strudel.cc REPL never calls (only `@strudel/web`
does).

## The traps

Each of these came up in practice:

- **Alternation in single quotes does nothing.** `s('<rect ellipse>')` and
  `fill('<orange cyan>')` give a shape name that matches neither shape and a
  colour that does not parse, so `animate` draws nothing visible. Write
  `.s("<rect ellipse>").fill("<orange cyan>")`.
- **Double quotes inside a callback are patterns, not strings.** In `.draw`,
  `.onPaint`, `requestAnimationFrame` or `animate({callback})`:
  - `ctx.fillStyle = "tomato"` assigns a Pattern; the colour falls back to
    black. Write `'tomato'`.
  - `ctx.font = "48px sans-serif"` does not parse; text comes out at 10px.
  - `"haps " + haps.length` prints `[object Object]16`.
  - `hap.value.s === "hh"` is always false. Write `=== 'hh'`.
  Inside callbacks, **every** string is single-quoted.
- **A name with a space in single quotes is one name.** `cat('c minor')` is
  the scale `"c minor"`. In double quotes it would be two steps, `c` then
  `minor`. Real songs rely on the single-quoted form (eefano's
  `enjoythesilence.js`).

## Writing a test tune

- Patterned arguments get double quotes; everything inside JS code gets single
  quotes. Writing the whole callback in single quotes avoids the traps above.
- `d1`–`d9` and `p1`–`p9` are properties: `note("c").d1`, not `.d1()`.
- In a Rust test, a double-quoted script string needs `\"` escapes. Prefer
  single quotes for everything that is not a pattern, so the escapes stay few
  and the meaning stays clear.
- In PowerShell, put the tune in a single-quoted here-string (`@' … '@`) so
  neither quote style is touched, then pass it to run-rudel's
  `driver.ps1 -Eval`.
- **When a tune shows nothing,** check the quotes first. Then confirm the
  values headlessly before blaming the app: query the pattern
  (`crate::eval(src)?.query_arc(...)`) or step frames with
  `CanvasDriver::frame` in a test, and print what comes out.
