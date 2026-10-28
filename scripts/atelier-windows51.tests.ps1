$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$binary = Join-Path $workspace 'target/debug/atelier.exe'
$data = Join-Path $workspace '.artifacts/local-dev/data'
$source = Join-Path $workspace 'crates/atelier-ui/index.html'
$results = Join-Path $workspace '.artifacts/launcher-verification'
$null = [IO.Directory]::CreateDirectory($results)
$original = [IO.File]::ReadAllBytes($source)
$edited = [byte[]]($original + [Text.Encoding]::UTF8.GetBytes("`n<!-- synthetic-windows51-rebuild -->`n"))
$changed = $false
$completed = $false
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()
$address = "http://127.0.0.1:$port/"
function Read-Page {
  try { return (Invoke-WebRequest -Uri $address -UseBasicParsing -TimeoutSec 1).Content } catch { return '' }
}
function Wait-Page([scriptblock]$condition) {
  $deadline = [DateTime]::UtcNow.AddSeconds(60)
  while (-not (& $condition)) {
    if ($legacy.HasExited) { throw 'Windows PowerShell 5.1 a quitte.' }
    if ([DateTime]::UtcNow -ge $deadline) { throw 'Reconstruction Windows PowerShell 5.1 non confirmee.' }
    Start-Sleep -Milliseconds 200
  }
}
function Restore-Html {
  if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($source)) -ne [Convert]::ToBase64String($edited)) {
    throw 'Modification concurrente du HTML, restauration refusee.'
  }
  [IO.File]::WriteAllBytes($source, $original)
  $script:changed = $false
}
$timer = [Diagnostics.Stopwatch]::StartNew()
$legacy = Start-Process -FilePath "$env:SystemRoot/System32/WindowsPowerShell/v1.0/powershell.exe" -ArgumentList @('-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"{0}"' -f (Join-Path $PSScriptRoot 'atelier.ps1')), '-NoBrowser', '-Port', "$port") -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $results 'windows51-rebuild.stdout.txt') -RedirectStandardError (Join-Path $results 'windows51-rebuild.stderr.txt')
try {
  Wait-Page { (Read-Page).Contains('<!--atelier-dev:') }
  $startup = [Math]::Round($timer.Elapsed.TotalSeconds, 2)
  $backup = Join-Path $results 'originals/windows51-index.html'
  $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($backup))
  [IO.File]::WriteAllBytes($backup, $original)
  $timer.Restart()
  [IO.File]::WriteAllBytes($source, $edited)
  $changed = $true
  Wait-Page { (Read-Page).Contains('synthetic-windows51-rebuild') }
  $elapsed = [Math]::Round($timer.Elapsed.TotalSeconds, 2)
  Restore-Html
  Wait-Page { $page = Read-Page; $page.Contains('<!--atelier-dev:') -and -not $page.Contains('synthetic-windows51-rebuild') }
  $completed = $true
  Write-Host "OK : lancement Windows PowerShell 5.1 ($startup s), reconstruction HTML ($elapsed s), sources et assets restaures."
} finally {
  try {
    & $binary stop --data-dir $data --port $port
    if ($LASTEXITCODE -ne 0) { $completed = $false; throw 'Arret authentifie non confirme.' }
  } finally {
    if (-not $legacy.HasExited) { Stop-Process -Id $legacy.Id; $legacy.WaitForExit() }
    try { if ($changed) { Restore-Html } } catch { $completed = $false; throw } finally {
      $status = if ($completed) { 'passed' } else { 'failed' }
      [pscustomobject]@{ scenario = 'lancement et reconstruction HTML sous Windows PowerShell 5.1'; startupSeconds = $startup; rebuildSeconds = $elapsed; result = $status } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $results 'windows51-rebuild.json')
    }
  }
}
