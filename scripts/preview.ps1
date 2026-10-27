param([switch]$Watch)
$ErrorActionPreference = 'Stop'
$watchLock = $null
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
  $assets = WorkspacePath '.artifacts/rust-ui-dev'
  $port = 4327
  if ($Watch) {
    $data = WorkspacePath '.artifacts/local-dev/data'
    $port = 4328
    $lockPath = WorkspacePath '.artifacts/local-dev/watch.lock'
    $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($lockPath))
    $watchLock = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
  }
  function Invoke-PreviewCommand([string]$file, [string[]]$arguments) {
    & $file @arguments
    if ($LASTEXITCODE -ne 0) { throw "Preview command failed with exit code $LASTEXITCODE : $file" }
  }
  Push-Location -LiteralPath $workspace
  try {
    function Stop-Preview {
      if (Test-Path -LiteralPath $data) {
        if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Previous preview data exists but its executable is missing. Restore the preview binary before rebuilding.' }
        $owners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object { $_.LocalPort -eq $port } | Select-Object -ExpandProperty OwningProcess -Unique)
        $previewOwners = @()
        foreach ($ownerId in $owners) {
          $owner = Get-Process -Id $ownerId -ErrorAction Stop
          if (-not $owner.Path -or -not [IO.Path]::GetFullPath($owner.Path).Equals($binary, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Port $port belongs to another executable. No process was stopped."
          }
          $previewOwners += $owner
        }
        # The CLI authenticates the local session and data identity, and waits for its data lock.
        Invoke-PreviewCommand $binary @('stop', '--data-dir', $data, '--port', "$port")
        foreach ($owner in $previewOwners) {
          if (-not $owner.WaitForExit(30000)) { throw 'The preview process has not exited. Rebuild refused; no process was killed.' }
        }
      }
    }
    function Build-Preview {
      Stop-Preview
      Write-Host 'Preparing the Rust preview. The first compilation can take a long time.'
      Invoke-PreviewCommand 'cargo' @('xtask', 'build-ui', '--dev')
      Invoke-PreviewCommand 'cargo' @('build', '--locked', '-p', 'atelier-app')
      $null = WorkspacePath 'target/debug/atelier.exe'
      $null = WorkspacePath '.artifacts/rust-ui-dev'
      $null = WorkspacePath $(if ($Watch) { '.artifacts/local-dev/data' } else { '.artifacts/local-preview/data' })
    }
    function Source-Stamp {
      $files = @(
        Get-ChildItem -LiteralPath (Join-Path $workspace 'crates'), (Join-Path $workspace 'xtask'), (Join-Path $workspace '.cargo') -File -Recurse
        Get-Item -LiteralPath (Join-Path $workspace 'Cargo.toml'), (Join-Path $workspace 'Cargo.lock'), (Join-Path $workspace 'rust-toolchain.toml')
      )
      return (($files | Sort-Object FullName | ForEach-Object { '{0}|{1}|{2}' -f $_.FullName, $_.Length, $_.LastWriteTimeUtc.Ticks }) -join "`n")
    }
    Invoke-PreviewCommand 'cargo' @('fetch', '--locked')
    if (-not $Watch) {
      Build-Preview
      Invoke-PreviewCommand $binary @('launch', '--data-dir', $data, '--assets', $assets, '--port', "$port")
      Write-Host 'Atelier Rust preview is open. Preview data is retained in .artifacts/local-preview/data.'
    } else {
      $previous = $null
      $opened = $false
      $child = $null
      Write-Host 'Mode developpement : surveillance des sources, Ctrl+C pour arreter. Donnees synthetiques : .artifacts/local-dev/data.'
      try {
        while ($true) {
          $stamp = Source-Stamp
          if ($stamp -ne $previous) {
            Start-Sleep -Milliseconds 700
            if ((Source-Stamp) -ne $stamp) { continue }
            $previous = $stamp
            try {
              Build-Preview
              $child = Start-Process -FilePath $binary -ArgumentList @('serve', '--dev', '--data-dir', ('"{0}"' -f $data), '--assets', ('"{0}"' -f $assets), '--port', "$port") -WindowStyle Hidden -PassThru
              $deadline = [DateTime]::UtcNow.AddSeconds(30)
              while ($true) {
                if ($child.HasExited) { throw 'Le serveur de developpement s est arrete au demarrage.' }
                try {
                  $response = Invoke-WebRequest -Uri "http://127.0.0.1:$port/" -UseBasicParsing -TimeoutSec 1
                  if ($response.StatusCode -eq 200) { break }
                } catch { }
                if ([DateTime]::UtcNow -ge $deadline) { throw 'Delai de demarrage depasse.' }
                Start-Sleep -Milliseconds 200
              }
              if (-not $opened) {
                Invoke-PreviewCommand $binary @('launch', '--data-dir', $data, '--assets', $assets, '--port', "$port")
                $opened = $true
              }
              Write-Host 'Version prete. En attente de modifications...'
            } catch {
              Write-Host ("Reconstruction echouee : {0}. Corrige puis sauvegarde pour reessayer." -f $_.Exception.Message) -ForegroundColor Red
            }
          }
          Start-Sleep -Milliseconds 500
        }
      } finally {
        if ($null -ne $child) { Stop-Preview }
      }
    }
  } finally { Pop-Location }
} catch {
  Write-Host ('Preview error: ' + $_.Exception.Message) -ForegroundColor Red
  exit 1
} finally {
  if ($null -ne $watchLock) { $watchLock.Dispose() }
}
