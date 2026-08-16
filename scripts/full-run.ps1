# Full per-package cargo-mutants run. See memory: mutation-testing-setup.
#
# Pass -Packages to resume a run that died partway (a reboot, say) without
# discarding the packages already measured:
#   scripts/full-run.ps1 -Packages rudel-core,rudel-app
param(
    [string[]] $Packages = @('rudel-mini', 'rudel-lang', 'rudel-osc', 'rudel-midi',
        'rudel-core', 'rudel-audio', 'rudel-app', 'rudel-dsp')
)
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force mutants-full | Out-Null
$start = Get-Date
# Fresh log for a whole run, appended to for a resume: appending across two
# *full* runs makes two sets of numbers look like one.
$resuming = $Packages.Count -lt 8
"START $start  [$($Packages -join ', ')]" |
    Tee-Object mutants-full/run.log -Append:$resuming

foreach ($p in $Packages) {
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
