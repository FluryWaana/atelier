$ErrorActionPreference = 'Stop'
# Extract launcher functions: use synthetic files and replace external commands.
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'atelier.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors -join "`n") }
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Source-Stamp' }, $true)
Invoke-Expression $function.Extent.Text
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Source-Files' }, $true)
Invoke-Expression $function.Extent.Text
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Ui-Ready' }, $true)
Invoke-Expression $function.Extent.Text
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Stop-Preview' }, $true)
Invoke-Expression $function.Extent.Text
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'WorkspacePath' }, $true)
Invoke-Expression $function.Extent.Text
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Fetch-Dependencies' }, $true)
Invoke-Expression $function.Extent.Text
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Build-Scope' }, $true)
Invoke-Expression $function.Extent.Text
$workspace = Join-Path ([IO.Path]::GetTempPath()) ('atelier-watch-' + [guid]::NewGuid())
$previousCargoHome = $env:CARGO_HOME
try {
  $null = New-Item -ItemType Directory -Path $workspace
  foreach ($name in @('crates', 'xtask', '.cargo', 'data')) {
    $null = New-Item -ItemType Directory -Path (Join-Path $workspace $name)
  }
  foreach ($name in @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml')) {
    Set-Content -LiteralPath (Join-Path $workspace $name) -Value 'synthetic'
  }
  $escaped = $false
  try { $null = WorkspacePath '../outside' } catch { $escaped = $true }
  if (-not $escaped) { throw 'Workspace escape must be refused.' }
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
  foreach ($relative in @('crates/atelier-ui/.artifacts', 'crates/atelier-ui/target', 'xtask/data', 'xtask/.git')) {
    $ignored = Join-Path $workspace $relative
    $null = New-Item -ItemType Directory -Path $ignored -Force
    Set-Content -LiteralPath (Join-Path $ignored 'Cargo.toml') -Value 'ignored synthetic output'
  }
  if ((Source-Stamp) -ne $changed) { throw 'Nested outputs and data must not be watched.' }
  Remove-Item -LiteralPath $source
  if ((Source-Stamp) -ne $initial) { throw 'Source deletion was missed.' }
  $ui = [IO.Path]::GetFullPath((Join-Path $workspace 'crates/atelier-ui/src.rs'))
  $app = [IO.Path]::GetFullPath((Join-Path $workspace 'crates/atelier-app/src.rs'))
  $core = [IO.Path]::GetFullPath((Join-Path $workspace 'crates/atelier-core/src.rs'))
  $scope = Build-Scope "$ui|1|1" "$ui|1|1"
  if ($scope[0] -or $scope[1]) { throw 'Unchanged sources must reuse both targets.' }
  $scope = Build-Scope '' "$ui|1|1"
  if (-not $scope[0] -or -not $scope[1]) { throw 'Initial build must include both targets.' }
  $scope = Build-Scope "$ui|1|1" "$ui|2|2"
  if (-not $scope[0] -or $scope[1]) { throw 'UI edits must build only the UI.' }
  $scope = Build-Scope "$app|1|1" "$app|2|2"
  if ($scope[0] -or -not $scope[1]) { throw 'App edits must build only the app.' }
  $scope = Build-Scope "$core|1|1" "$core|2|2"
  if (-not $scope[0] -or -not $scope[1]) { throw 'Shared edits must build both targets.' }
  $scope = Build-Scope "$ui|1|1`n$app|1|1" "$ui|2|2`n$app|2|2"
  if ($scope.Count -ne 2 -or -not $scope[0] -or -not $scope[1]) { throw 'Mixed edits must return two build flags.' }
  $fetchPath = Join-Path $workspace 'fetch.txt'
  $script:fetches = 0
  function Invoke-PreviewCommand { $script:fetches++ }
  Fetch-Dependencies
  Fetch-Dependencies
  if ($script:fetches -ne 1) { throw 'Unchanged dependencies must not be fetched again.' }
  Add-Content -LiteralPath (Join-Path $workspace 'Cargo.lock') -Value 'changed'
  Fetch-Dependencies
  if ($script:fetches -ne 2) { throw 'Dependency changes must trigger a new fetch.' }
  $env:CARGO_HOME = Join-Path $workspace 'different-cargo-cache'
  Fetch-Dependencies
  if ($script:fetches -ne 3) { throw 'Switching Cargo caches must trigger a new fetch.' }
  $env:CARGO_HOME = $previousCargoHome
  $savedFingerprint = [IO.File]::ReadAllText($fetchPath)
  Add-Content -LiteralPath (Join-Path $workspace 'Cargo.toml') -Value 'changed'
  function Invoke-PreviewCommand { throw 'Synthetic fetch failure' }
  $failed = $false
  try { Fetch-Dependencies } catch { $failed = $true }
  if (-not $failed -or [IO.File]::ReadAllText($fetchPath) -ne $savedFingerprint) { throw 'Failed fetch must not update the cache.' }
  $data = Join-Path $workspace 'data'
  $binary = Join-Path $workspace 'synthetic.exe'
  Set-Content -LiteralPath $binary -Value 'never executed'
  $port = 4328
  function Get-NetTCPConnection { [pscustomobject]@{ LocalPort = $port; OwningProcess = $PID } }
  $script:stopCalls = 0
  function Invoke-PreviewCommand { $script:stopCalls++ }
  $refused = $false
  try { Stop-Preview } catch { $refused = $_.Exception.Message -match 'another executable' }
  if (-not $refused -or $script:stopCalls -ne 0) { throw 'An unrelated port owner must never be stopped.' }
  Remove-Item -LiteralPath $binary
  function Get-NetTCPConnection { }
  Stop-Preview
  if ($script:stopCalls -ne 0) { throw 'A missing binary on a free port must allow rebuilding.' }
  function Get-NetTCPConnection { [pscustomobject]@{ LocalPort = $port; OwningProcess = $PID } }
  $refused = $false
  try { Stop-Preview } catch { $refused = $_.Exception.Message -match 'executable is missing' }
  if (-not $refused) { throw 'A missing binary on an occupied port must be refused.' }
  $assets = Join-Path $workspace 'assets'
  $null = New-Item -ItemType Directory -Path $assets
  if (Ui-Ready) { throw 'Empty assets must be rebuilt.' }
  $assetNames = @('index.html', 'atelier-loader.js', 'atelier-kanban.png', 'atelier-ui-synthetic.js', 'atelier-ui-synthetic.wasm', 'style-synthetic.css')
  foreach ($name in $assetNames) { Set-Content -LiteralPath (Join-Path $assets $name) -Value 'synthetic' }
  if (-not (Ui-Ready)) { throw 'Complete assets must be reusable.' }
  foreach ($name in $assetNames) {
    Remove-Item -LiteralPath (Join-Path $assets $name)
    if (Ui-Ready) { throw "Missing asset must trigger rebuilding: $name" }
    Set-Content -LiteralPath (Join-Path $assets $name) -Value 'synthetic'
  }
  Write-Host 'OK : surveillance ajout/modification/suppression, exclusion data, perimetres UI/app/partage, cache et echec des dependances, refus de sortie du workspace et du port tiers.'
} finally {
  $env:CARGO_HOME = $previousCargoHome
  $resolved = [IO.Path]::GetFullPath($workspace)
  $temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
  if (-not $resolved.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid synthetic test path.' }
  Remove-Item -LiteralPath $resolved -Recurse -Force
}
