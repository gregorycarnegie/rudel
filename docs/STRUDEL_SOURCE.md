# Pinned Strudel source of truth

Rudel is a parity port of [Strudel](https://codeberg.org/uzu/strudel). All
parity work in [`FULL_STRUDEL.md`](../FULL_STRUDEL.md), the oracle generators in
[`tools/oracle`](../tools/oracle), and the vendored sources under `strudel/`
(git-ignored by this repo — it is a local working checkout, not committed) are
pinned to one upstream revision so the comparison is reproducible.

## Pinned revision

| Field | Value |
| --- | --- |
| Upstream remote | `https://codeberg.org/uzu/strudel.git` |
| Commit | `0c61cd767031cb377d63b4f290a0645c45e457c5` |
| Commit date | 2026-05-27 |
| `git describe` | `@strudel/codemirror@1.3.0-288-g0c61cd76` |
| HEAD subject | `Merge pull request 'Don't force SSL for mqtt websocket connections' (#2058)` |

Strudel is a Lerna monorepo with independently versioned packages
(`lerna.json` → `"version": "independent"`). Key package versions at this commit:

| Package | Version |
| --- | --- |
| root (`strudel`) | 0.5.0 |
| `@strudel/core` | 1.2.6 |
| `@strudel/codemirror` | 1.3.0 |
| `@strudel/reference` | 1.2.2 |

## Local patches

**None.** The vendored checkout is clean (`git -C strudel status` reports no
changes) — Rudel does not modify Strudel sources in place. Behavioral
differences are implemented on the Rudel side and documented as "intentionally
different" in `FULL_STRUDEL.md` / [`UNSUPPORTED.md`](UNSUPPORTED.md).

## Refreshing the pin

To re-vendor a newer Strudel:

```sh
cd strudel
git fetch origin && git checkout <new-commit>
```

Then regenerate the parity oracles (see [`tools/oracle/README.md`](../tools/oracle/README.md)),
update the table above with the new `git rev-parse HEAD` /
`git describe --tags --always`, and re-run `cargo test --workspace`. The
reference-surface guard (`crates/rudel-lang/tests/reference_parity.rs`) will fail
if the upstream name surface changed, pointing at exactly what to reconcile.
