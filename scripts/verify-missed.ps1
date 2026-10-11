# Re-test only the mutants a full run missed, e.g. after writing tests for them:
#   scripts/verify-missed.ps1 rudel-dsp
# Reads mutants-full/<pkg>/mutants.out/missed.txt and selects those mutants by
# name, so a round costs minutes where re-checking whole files costs hours
# (dsp is ~4800 mutants; its misses were 139). Line and column are wildcarded
# because the tests being verified usually shift them; a pattern can therefore
# also catch a same-named mutation elsewhere in that function.
# Output lands in mutants-verify/<pkg>/. `-From mutants-verify` re-checks what
# the last verify left instead of the full run's misses.
param([Parameter(Mandatory)] [string] $Package, [string] $From = 'mutants-full')
$missed = Get-Content "$From/$Package/mutants.out/missed.txt"
$alts = $missed | ForEach-Object {
    if ($_ -match '^(.+?):\d+:\d+: (.+)$') {
        '^' + [regex]::Escape($Matches[1]) + ':\d+:\d+: ' + [regex]::Escape($Matches[2]) + '$'
    }
} | Sort-Object -Unique
New-Item -ItemType Directory -Force mutants-verify | Out-Null
# Hundreds of patterns overflow Windows' 32K command line as `--re`, so they go
# in a copy of the config as `examine_re` (TOML literal strings: no escaping).
$config = "mutants-verify/$Package.toml"
@(Get-Content .cargo/mutants.toml) + 'examine_re = [' +
    ($alts | ForEach-Object { "  '''$_'''," }) + ']' | Set-Content $config
$scope = if ($Package -eq 'rudel-core') { @('--test-package','rudel-core','--test-package','rudel-mini') } else { @() }
# The scratch copy has no target/, so point rudel-ffmpeg's build at the real one.
$env:RUDEL_FFMPEG_DIR = (Resolve-Path target/ffmpeg).Path
if ($env:RUDEL_CSOUND_LIB -or (Test-Path 'C:/Program Files/Csound6_x64/bin/csound64.dll')) {
    $env:RUDEL_CSOUND_REQUIRED = '1'
}
cargo mutants --config $config -j8 --build-timeout 900 --package $Package @scope `
    --output "mutants-verify/$Package" 2>&1 | Tee-Object "mutants-verify/$Package.log"
