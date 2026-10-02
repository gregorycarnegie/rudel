# Re-test only the mutants a full run missed, e.g. after writing tests for them:
#   scripts/verify-missed.ps1 rudel-dsp
# Reads mutants-full/<pkg>/mutants.out/missed.txt and selects those mutants by
# name with --re, so a round costs minutes where re-checking whole files costs
# hours (dsp is ~4800 mutants; its misses were 139). Line and column are
# wildcarded because the tests being verified usually shift them; a pattern
# can therefore also catch a same-named mutation elsewhere in that function.
# Output lands in mutants-verify/<pkg>/.
param([Parameter(Mandatory)] [string] $Package)
$missed = Get-Content "mutants-full/$Package/mutants.out/missed.txt"
$alts = $missed | ForEach-Object {
    if ($_ -match '^(.+?):\d+:\d+: (.+)$') {
        [regex]::Escape($Matches[1]) + ':\d+:\d+: ' + [regex]::Escape($Matches[2])
    }
} | Sort-Object -Unique
$re = '^(' + ($alts -join '|') + ')$'
$scope = if ($Package -eq 'rudel-core') { @('--test-package','rudel-core','--test-package','rudel-mini') } else { @() }
if ($env:RUDEL_CSOUND_LIB -or (Test-Path 'C:/Program Files/Csound6_x64/bin/csound64.dll')) {
    $env:RUDEL_CSOUND_REQUIRED = '1'
}
New-Item -ItemType Directory -Force mutants-verify | Out-Null
cargo mutants --gitignore=true -j8 --build-timeout 900 --package $Package @scope --re $re `
    --output "mutants-verify/$Package" 2>&1 | Tee-Object "mutants-verify/$Package.log"
