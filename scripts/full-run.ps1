# Full per-package cargo-mutants run. See memory: mutation-testing-setup.
#
# Pass -Packages to resume a run that died partway (a reboot, say) without
# discarding the packages already measured:
#   scripts/full-run.ps1 -Packages rudel-core,rudel-app
#
# One comma-separated string, not a string[]: `pwsh -File` passes arguments
# through as literal text, so an array parameter would bind the whole list as a
# single package name and silently test nothing.
param(
    [string] $Packages = 'rudel-mini,rudel-lang,rudel-osc,rudel-midi,rudel-core,rudel-audio,rudel-app,rudel-dsp'
)
$list = $Packages -split ','
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force mutants-full | Out-Null
$start = Get-Date
# Fresh log for a whole run, appended to for a resume: appending across two
# *full* runs makes two sets of numbers look like one.
$resuming = $list.Count -lt 8
"START $start  [$($list -join ', ')]" |
    Tee-Object mutants-full/run.log -Append:$resuming

foreach ($p in $list) {
    $scope = if ($p -eq 'rudel-core') { @('--test-package','rudel-core','--test-package','rudel-mini') } else { @() }
    $t0 = Get-Date
    "=== $p  $t0" | Tee-Object -Append mutants-full/run.log
    cargo mutants --gitignore=true -j8 --package $p @scope --output "mutants-full/$p" 2>&1 |
        Tee-Object -Append "mutants-full/$p.log" | Select-Object -Last 0
    "--- $p done in $((Get-Date) - $t0)" | Tee-Object -Append mutants-full/run.log
}

"END $(Get-Date), total $((Get-Date) - $start)" | Tee-Object -Append mutants-full/run.log
foreach ($p in Get-ChildItem mutants-full -Directory) {
    $m = @(Get-Content "$($p.FullName)/mutants.out/missed.txt" -EA SilentlyContinue).Count
    $c = @(Get-Content "$($p.FullName)/mutants.out/caught.txt" -EA SilentlyContinue).Count
    $t = @(Get-Content "$($p.FullName)/mutants.out/timeout.txt" -EA SilentlyContinue).Count
    $pct = if ($c + $m + $t) { [math]::Round(100 * $c / ($c + $m + $t), 1) } else { 0 }
    "$($p.Name): $c caught / $m missed / $t timeout = $pct%" | Tee-Object -Append mutants-full/run.log
}
