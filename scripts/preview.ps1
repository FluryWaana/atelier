$ErrorActionPreference = 'Stop'
try {
  $workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')).TrimEnd('\')
  function WorkspacePath([string]$relative) {
    $absolute = [IO.Path]::GetFullPath((Join-Path $workspace $relative))
    if (-not $absolute.StartsWith($workspace + '\', [StringComparison]::OrdinalIgnoreCase)) {
      throw 'Preview path must stay inside the workspace.'
    }
    $cursor = $absolute
    while ($cursor -and $cursor.StartsWith($workspace, [StringComparison]::OrdinalIgnoreCase)) {
      $entry = Get-Item -LiteralPath $cursor -Force -ErrorAction SilentlyContinue
      if ($null -ne $entry) {
        if (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
          throw "Preview refuses linked paths: $cursor"
        }
      }
      if ($cursor -eq $workspace) { break }
      $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    return $absolute
  }
  $env:CARGO_BUILD_JOBS = '1'
  $env:RUST_TEST_THREADS = '1'
  $previewShell = Get-Process -Id $PID
  $previewShell.PriorityClass = [Diagnostics.ProcessPriorityClass]::BelowNormal
  $allowedCpu = $previewShell.ProcessorAffinity.ToInt64()
  $selectedCpu = 0L
  $selectedCount = 0
  for ($bit = 0; $bit -lt 63 -and $selectedCount -lt 2; $bit++) {
    $mask = 1L -shl $bit
    if (($allowedCpu -band $mask) -ne 0) { $selectedCpu = $selectedCpu -bor $mask; $selectedCount++ }
  }
  if ($selectedCpu -ne 0) { $previewShell.ProcessorAffinity = [IntPtr]$selectedCpu }

  $localCargo = WorkspacePath '.artifacts/rust-tools/cargo'
  $localRustup = WorkspacePath '.artifacts/rust-tools/rustup'
  if (Test-Path -LiteralPath $localCargo -PathType Container) { $env:CARGO_HOME = $localCargo }
  if (Test-Path -LiteralPath $localRustup -PathType Container) { $env:RUSTUP_HOME = $localRustup }
  $toolPaths = @(
    (WorkspacePath '.artifacts/rust-tools/cargo/bin'),
    (WorkspacePath '.artifacts/rust-tools/trunk'),
    (WorkspacePath '.artifacts/rust-tools/wasm-bindgen-0.2.129-x86_64-pc-windows-msvc')
  ) | Where-Object { Test-Path -LiteralPath $_ -PathType Container }
  if ($toolPaths.Count -gt 0) { $env:PATH = ($toolPaths -join ';') + ';' + $env:PATH }
  $target = WorkspacePath 'target'
  $env:CARGO_TARGET_DIR = $target
  $binary = WorkspacePath 'target/debug/atelier.exe'
  $data = WorkspacePath '.artifacts/local-preview/data'
  $assets = WorkspacePath '.artifacts/rust-ui'
  $port = 4327
  function Invoke-PreviewCommand([string]$file, [string[]]$arguments) {
    & $file @arguments
    if ($LASTEXITCODE -ne 0) { throw "Preview command failed with exit code $LASTEXITCODE : $file" }
  }
  Push-Location -LiteralPath $workspace
  try {
    if (Test-Path -LiteralPath $data) {
      if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Previous preview data exists but its executable is missing. Restore the preview binary before rebuilding.' }
      $owners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object { $_.LocalPort -eq $port } | Select-Object -ExpandProperty OwningProcess -Unique)
      $previewOwners = @()
      foreach ($ownerId in $owners) {
        $owner = Get-Process -Id $ownerId -ErrorAction Stop
        if (-not $owner.Path -or -not [IO.Path]::GetFullPath($owner.Path).Equals($binary, [StringComparison]::OrdinalIgnoreCase)) {
          throw 'Port 4327 belongs to another executable. No process was stopped.'
        }
        $previewOwners += $owner
      }
      # The CLI authenticates the local session and data identity, and waits for its data lock.
      Invoke-PreviewCommand $binary @('stop', '--data-dir', $data, '--port', "$port")
      foreach ($owner in $previewOwners) {
        if (-not $owner.WaitForExit(30000)) { throw 'The preview process has not exited. Rebuild refused; no process was killed.' }
      }
    }
    Write-Host 'Preparing the Rust preview. The first compilation can take a long time.'
    Invoke-PreviewCommand 'cargo' @('fetch', '--locked')
    Invoke-PreviewCommand 'cargo' @('xtask', 'build-ui')
    Invoke-PreviewCommand 'cargo' @('build', '--locked', '-p', 'atelier-app')
    $null = WorkspacePath 'target/debug/atelier.exe'
    $null = WorkspacePath '.artifacts/rust-ui'
    $null = WorkspacePath '.artifacts/local-preview/data'
    Invoke-PreviewCommand $binary @('launch', '--data-dir', $data, '--assets', $assets, '--port', "$port")
    Write-Host 'Atelier Rust preview is open. Preview data is retained in .artifacts/local-preview/data.'
  } finally { Pop-Location }
} catch {
  Write-Host ('Preview error: ' + $_.Exception.Message) -ForegroundColor Red
  exit 1
}
