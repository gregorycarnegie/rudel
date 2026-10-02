# Re-measure one or more files' mutants, e.g.
#   scripts/check-file.ps1 rudel-lang syntax.rs
#   scripts/check-file.ps1 rudel-core euclid.rs choice.rs timing.rs
# One invocation per package, however many files: the tree copy and baseline
# build are paid once, so ten files together cost minutes where ten separate
# runs cost hours. --file matches on the *basename*.
# Output lands in mutants-check/<first file stem>/ with a log beside it.
param(
    [Parameter(Mandatory)] [string] $Package,
    [Parameter(Mandatory, ValueFromRemainingArguments)] [string[]] $File
)
$name = [IO.Path]::GetFileNameWithoutExtension($File[0])
# rudel-core's parity tests live in rudel-mini; without mini in scope the miss
# count reads about a fifth too high. See memory: mutation-testing-setup.
$scope = if ($Package -eq 'rudel-core') { @('--test-package','rudel-core','--test-package','rudel-mini') } else { @() }
$files = $File | ForEach-Object { '--file'; $_ }
New-Item -ItemType Directory -Force mutants-check | Out-Null
Remove-Item -Recurse -Force "mutants-check/$name" -EA SilentlyContinue
# Csound's tests skip where the library is missing, so a mutant that breaks
# starting it would read as caught-by-nothing. Where it is installed, a skip
# has to be a failure.
if ($env:RUDEL_CSOUND_LIB -or (Test-Path 'C:/Program Files/Csound6_x64/bin/csound64.dll')) {
    $env:RUDEL_CSOUND_REQUIRED = '1'
}
cargo mutants --gitignore=true -j8 --build-timeout 900 --package $Package @files @scope `
    --output "mutants-check/$name" 2>&1 | Tee-Object "mutants-check/$name.log"
