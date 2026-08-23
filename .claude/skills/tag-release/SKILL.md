---
name: tag-release
description: Cut a rudel release — bump the workspace version, roll the changelog's Unreleased section into a dated one, commit, and push the `v*` tag that triggers the binary builds. Use when asked to release, cut a release, bump the version, or tag a version of rudel.
---

# Releasing rudel

A pushed `v*` tag is the release: `.github/workflows/release.yml` builds Linux,
Windows and macOS binaries from it and uploads them. Nothing else publishes
anything — rudel is not on crates.io.

`.claude/skills/tag-release/tag-release.ps1` is the last step. It reports by
default and only tags with `-Push`:

```powershell
pwsh -File .claude/skills/tag-release/tag-release.ps1          # check only
pwsh -File .claude/skills/tag-release/tag-release.ps1 -Push    # tag and push
```

It refuses unless the tree is clean, HEAD is `origin/master`, the tag is free
locally and on origin, `CHANGELOG.md` has a dated `## [<version>]` section, and
nothing is left under `## [Unreleased]`.

## The release commit

Run the check first — it names whatever is missing. The commit it wants looks
exactly like `git show b515a09` (Release 0.18.1):

1. Pick the number from what is under `## [Unreleased]`. Pre-1.0 here, the
   minor carries breaking changes: new features or breaking ones bump the minor
   (`0.18.1` → `0.19.0`), fixes alone bump the patch (`0.18.1` → `0.18.2`).
2. `version` in `[workspace.package]` of the root `Cargo.toml` — the only place
   it is written; every crate inherits it.
3. `cargo check --workspace` to write the new version through `Cargo.lock`.
4. In `CHANGELOG.md`, rename `## [Unreleased]` to `## [<version>] — <today>`
   and leave a fresh empty `## [Unreleased]` above it.
5. Commit those three files as `Release <version>`, and push master.

Then `-Push` the tag.

## After

`gh run watch` follows the build; `gh release view v<version>` shows what came
out of it. A failed run is fixed by fixing the commit and moving the tag —
delete it locally and on origin (`git push origin :refs/tags/v<version>`) and
run this again, rather than leaving a half-built release in place.
