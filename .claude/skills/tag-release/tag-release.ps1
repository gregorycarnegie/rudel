#Requires -Version 7
<#
.SYNOPSIS
    Check that this commit is a releasable one, then tag it and push the tag.
.DESCRIPTION
    The `v*` tag is what triggers .github/workflows/release.yml, so the tag is
    the release. Everything here is a check that the tag will build the thing
    the changelog claims; -Push is what actually creates and pushes it.
#>
[CmdletBinding()]
param(
    # Without this the script only reports; with it, it tags and pushes.
    [switch]$Push
)

$ErrorActionPreference = 'Stop'
Set-Location (git rev-parse --show-toplevel)

$version = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).packages |
    Where-Object { $_.name -eq 'rudel-app' } |
    Select-Object -First 1 -ExpandProperty version
if (-not $version) { throw 'could not read the rudel-app version from cargo metadata' }
$tag = "v$version"

$problems = @()

if (git status --porcelain) {
    $problems += "the working tree is not clean:`n" + (git status --short | Out-String).TrimEnd()
}

$branch = git rev-parse --abbrev-ref HEAD
if ($branch -ne 'master') { $problems += "on branch $branch, not master" }

# A tag on a commit origin has never seen builds a release nobody can check out.
git fetch --quiet origin master
if ((git rev-parse HEAD) -ne (git rev-parse origin/master)) {
    $problems += 'HEAD is not what origin/master points at — push the release commit first'
}

if (git tag --list $tag) { $problems += "tag $tag already exists locally" }
if (git ls-remote --tags origin "refs/tags/$tag") { $problems += "tag $tag already exists on origin" }

# The changelog is the release notes; a version with no section of its own means
# the entries are still sitting under Unreleased.
$changelog = Get-Content CHANGELOG.md -Raw
if ($changelog -notmatch "(?m)^## \[$([regex]::Escape($version))\] — \d{4}-\d{2}-\d{2}") {
    $problems += "CHANGELOG.md has no dated '## [$version]' section"
}
$unreleased = [regex]::Match($changelog, '(?ms)^## \[Unreleased\]\r?\n(.*?)^## ')
if ($unreleased.Success -and $unreleased.Groups[1].Value -match '(?m)^- ') {
    $problems += 'CHANGELOG.md still has entries under [Unreleased] — roll them into the release section'
}

if ($problems) {
    Write-Host "not releasable as ${tag}:" -ForegroundColor Red
    $problems | ForEach-Object { Write-Host "  - $_" }
    exit 1
}

Write-Host "$tag is releasable: $(git rev-parse --short HEAD) on master, changelog dated, tag free"
if (-not $Push) {
    Write-Host 'dry run — re-run with -Push to tag and push it.'
    exit 0
}

git tag -a $tag -m "Release $tag"
git push origin $tag
Write-Host "pushed $tag — the release workflow builds it now (gh run watch)."
