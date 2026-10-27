$ErrorActionPreference = 'Stop'
# Extract only the inventory function: never build, launch, or inspect real data.
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'preview.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors -join "`n") }
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Source-Stamp' }, $true)
Invoke-Expression $function.Extent.Text
$workspace = Join-Path ([IO.Path]::GetTempPath()) ('atelier-watch-' + [guid]::NewGuid())
try {
  $null = New-Item -ItemType Directory -Path $workspace
  foreach ($name in @('crates', 'xtask', '.cargo', 'data')) {
    $null = New-Item -ItemType Directory -Path (Join-Path $workspace $name)
  }
  foreach ($name in @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml')) {
    Set-Content -LiteralPath (Join-Path $workspace $name) -Value 'synthetic'
  }
  $initial = Source-Stamp
  $source = Join-Path $workspace 'crates/synthetic.rs'
  Set-Content -LiteralPath $source -Value 'synthetic source'
  $added = Source-Stamp
  if ($added -eq $initial) { throw 'Source addition was missed.' }
  Set-Content -LiteralPath $source -Value 'changed synthetic source'
  $changed = Source-Stamp
  if ($changed -eq $added) { throw 'Source modification was missed.' }
  Set-Content -LiteralPath (Join-Path $workspace 'data/untouched') -Value 'synthetic data'
  if ((Source-Stamp) -ne $changed) { throw 'Data must not be watched.' }
  Remove-Item -LiteralPath $source
  if ((Source-Stamp) -ne $initial) { throw 'Source deletion was missed.' }
} finally {
  $resolved = [IO.Path]::GetFullPath($workspace)
  $temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
  if (-not $resolved.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid synthetic test path.' }
  Remove-Item -LiteralPath $resolved -Recurse -Force
}
