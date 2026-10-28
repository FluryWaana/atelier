param(
  [switch]$NoBrowser,
  [ValidateRange(1, 65535)][int]$Port = 4328
)

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
  $target = WorkspacePath 'target'
  $env:CARGO_TARGET_DIR = $target
  $binary = WorkspacePath 'target/debug/atelier.exe'
  $data = WorkspacePath '.artifacts/local-dev/data'
  $assets = WorkspacePath '.artifacts/rust-ui-dev'
  $stampPath = WorkspacePath '.artifacts/local-dev/built-sources.txt'
  $fetchPath = WorkspacePath '.artifacts/local-dev/fetched-dependencies.txt'
  $lockPath = WorkspacePath '.artifacts/local-dev/watch.lock'
  $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($lockPath))
  $watchLock = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
  function Invoke-PreviewCommand([string]$file, [string[]]$arguments) {
    & $file @arguments
    if ($LASTEXITCODE -ne 0) { throw "Preview command failed with exit code $LASTEXITCODE : $file" }
  }
  Push-Location -LiteralPath $workspace
  try {
    function Stop-Preview {
      if (Test-Path -LiteralPath $data) {
        $owners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object { $_.LocalPort -eq $port } | Select-Object -ExpandProperty OwningProcess -Unique)
        if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
          if ($owners.Count -gt 0) { throw "Port $port is occupied and the preview executable is missing. No process was stopped." }
          return
        }
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
    function Build-Preview([bool]$buildUi = $true, [bool]$buildApp = $true) {
      if ($buildUi -or $buildApp) { Write-Host 'Compilation des sources modifiees. La premiere compilation peut etre longue.' }
      else { Write-Host 'Sources inchangees : reutilisation de la compilation precedente.' }
      if ($buildUi) { Invoke-PreviewCommand 'cargo' @('xtask', 'build-ui', '--dev') }
      Stop-Preview
      if ($buildApp) { Invoke-PreviewCommand 'cargo' @('build', '--locked', '-p', 'atelier-app') }
      $null = WorkspacePath 'target/debug/atelier.exe'
      $null = WorkspacePath '.artifacts/rust-ui-dev'
      $null = WorkspacePath '.artifacts/local-dev/data'
    }
    function Source-Files {
      $pending = [Collections.Generic.Stack[string]]::new()
      foreach ($root in @('crates', 'xtask', '.cargo')) { $pending.Push((WorkspacePath $root)) }
      while ($pending.Count -gt 0) {
        foreach ($entry in Get-ChildItem -LiteralPath $pending.Pop() -Force) {
          if ($entry.PSIsContainer -and $entry.Name -in @('target', 'data', '.artifacts', '.git')) { continue }
          if (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Linked source path refused: $($entry.FullName)" }
          if ($entry.PSIsContainer) { $pending.Push($entry.FullName) } else { $entry }
        }
      }
      foreach ($name in @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml')) { Get-Item -LiteralPath (WorkspacePath $name) }
    }
    function Source-Stamp {
      $files = @(Source-Files)
      return (($files | Sort-Object FullName | ForEach-Object { '{0}|{1}|{2}' -f $_.FullName, $_.Length, $_.LastWriteTimeUtc.Ticks }) -join "`n")
    }
    function Build-Scope([string]$before, [string]$after) {
      if (-not $before) { return @($true, $true) }
      $changed = @(Compare-Object ($before -split "`n") ($after -split "`n") | ForEach-Object { $_.InputObject })
      $uiOnly = [regex]::Escape([IO.Path]::GetFullPath((Join-Path $workspace 'crates/atelier-ui')) + [IO.Path]::DirectorySeparatorChar)
      $appOnly = [regex]::Escape([IO.Path]::GetFullPath((Join-Path $workspace 'crates/atelier-app')) + [IO.Path]::DirectorySeparatorChar)
      return @((@($changed | Where-Object { $_ -notmatch "^$appOnly" }).Count -gt 0),
               (@($changed | Where-Object { $_ -notmatch "^$uiOnly" }).Count -gt 0))
    }
    function Fetch-Dependencies {
      $cargoConfig = [IO.Path]::GetFullPath((Join-Path $workspace '.cargo')) + [IO.Path]::DirectorySeparatorChar
      $inputs = @(Source-Files | Where-Object { $_.Name -in @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml') -or $_.FullName.StartsWith($cargoConfig, [StringComparison]::OrdinalIgnoreCase) })
      $fingerprint = '{0}|{1}|{2}' -f (Get-Command cargo -ErrorAction Stop).Source, $env:CARGO_HOME, $env:RUSTUP_HOME
      $sha256 = [Security.Cryptography.SHA256]::Create()
      try {
        $fingerprint += "`n" + (($inputs | Sort-Object FullName | ForEach-Object {
          $stream = [IO.File]::OpenRead($_.FullName)
          try { $hash = [BitConverter]::ToString($sha256.ComputeHash($stream)) } finally { $stream.Dispose() }
          '{0}|{1}' -f $_.FullName, $hash
        }) -join "`n")
      } finally { $sha256.Dispose() }
      if (-not (Test-Path -LiteralPath $fetchPath) -or [IO.File]::ReadAllText($fetchPath) -ne $fingerprint) {
        Invoke-PreviewCommand 'cargo' @('fetch', '--locked')
        [IO.File]::WriteAllText($fetchPath, $fingerprint)
      }
    }
    function Ui-Ready {
      $names = @(Get-ChildItem -LiteralPath $assets -File -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name)
      return (@(@('index.html', 'atelier-loader.js', 'atelier-kanban.png') | Where-Object { $_ -notin $names }).Count -eq 0 -and
              @($names -match '^atelier-ui-.+\.wasm$').Count -gt 0 -and
              @($names -match '^atelier-ui-.+\.js$').Count -gt 0 -and
              @($names -match '^style-.+\.css$').Count -gt 0)
    }
      $previous = $null
      $built = $null
      if (Test-Path -LiteralPath $stampPath -PathType Leaf) { $built = [IO.File]::ReadAllText($stampPath) }
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
              $scope = Build-Scope $built $stamp
              $scope[0] = $scope[0] -or -not (Ui-Ready)
              $scope[1] = $scope[1] -or -not (Test-Path -LiteralPath $binary -PathType Leaf)
              if ($scope[0] -or $scope[1]) { Fetch-Dependencies }
              # A failed build may have partially replaced its outputs; never reuse its old stamp.
              if (Test-Path -LiteralPath $stampPath) { Remove-Item -LiteralPath $stampPath }
              Build-Preview $scope[0] $scope[1]
              $built = $stamp
              [IO.File]::WriteAllText($stampPath, $built)
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
                $launchArguments = @('launch', '--data-dir', $data, '--assets', $assets, '--port', "$port")
                if ($NoBrowser) { $launchArguments += '--no-browser' }
                Invoke-PreviewCommand $binary $launchArguments
                $opened = $true
              }
              Write-Host 'Version prete. En attente de modifications...'
            } catch {
              $built = $null
              Write-Host ("Reconstruction echouee : {0}. Corrige puis sauvegarde pour reessayer." -f $_.Exception.Message) -ForegroundColor Red
            }
          }
          Start-Sleep -Milliseconds 500
        }
      } finally {
        if ($null -ne $child) { Stop-Preview }
      }
  } finally { Pop-Location }
} catch {
  Write-Host ('Preview error: ' + $_.Exception.Message) -ForegroundColor Red
  exit 1
} finally {
  if ($null -ne $watchLock) { $watchLock.Dispose() }
}
