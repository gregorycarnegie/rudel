# Re-measure one file's mutants, e.g.
#   scripts/check-file.ps1 rudel-lang syntax.rs
# Output lands in mutants-check/<file stem>/ with a log beside it.
param(
    [Parameter(Mandatory)] [string] $Package,
    [Parameter(Mandatory)] [string] $File
)
$name = [IO.Path]::GetFileNameWithoutExtension($File)
New-Item -ItemType Directory -Force mutants-check | Out-Null
Remove-Item -Recurse -Force "mutants-check/$name" -EA SilentlyContinue
cargo mutants --gitignore=true -j8 --package $Package --file $File `
    --output "mutants-check/$name" 2>&1 | Tee-Object "mutants-check/$name.log"
