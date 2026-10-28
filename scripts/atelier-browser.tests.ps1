param([string]$Driver = 'msedgedriver.exe')

$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$binary = Join-Path $workspace 'target/debug/atelier.exe'
$data = Join-Path $workspace '.artifacts/local-dev/data'
$results = Join-Path $workspace '.artifacts/launcher-verification'
$null = [IO.Directory]::CreateDirectory($results)
$report = [Collections.Generic.List[object]]::new()
$watcher = $null
$driverProcess = $null
$session = $null
$originals = @{}
$written = @{}
$completed = $false

function Free-Port {
  $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
  $listener.Start()
  try { return $listener.LocalEndpoint.Port } finally { $listener.Stop() }
}
function Wait-Until([scriptblock]$Condition, [string]$Description, [int]$Seconds = 120) {
  $timer = [Diagnostics.Stopwatch]::StartNew()
  while ($timer.Elapsed.TotalSeconds -lt $Seconds) {
    if (& $Condition) { return }
    Start-Sleep -Milliseconds 200
  }
  throw "Delai depasse : $Description"
}
function Record([string]$Name, [Diagnostics.Stopwatch]$Timer) {
  $entry = [pscustomobject]@{ scenario = $Name; seconds = [Math]::Round($Timer.Elapsed.TotalSeconds, 2); result = 'passed' }
  $report.Add($entry)
  Write-Host ("OK : {0} ({1}s)" -f $entry.scenario, $entry.seconds)
}
function WebDriver([string]$Method, [string]$Path, $Body = $null) {
  $arguments = @{ Uri = "$endpoint$Path"; Method = $Method; TimeoutSec = 15; UseBasicParsing = $true }
  if ($null -ne $Body) { $arguments.Body = ConvertTo-Json -InputObject $Body -Depth 15 -Compress; $arguments.ContentType = 'application/json' }
  return (Invoke-RestMethod @arguments).value
}
function Script([string]$Code) {
  return WebDriver POST "/session/$session/execute/sync" @{ script = $Code; args = @() }
}
function Watcher-Log {
  # Index live streams: enumerating an open PSDataCollection waits for its completion.
  $lines = @(for ($i = 0; $i -lt $watcher.Streams.Information.Count; $i++) { $watcher.Streams.Information[$i].MessageData.ToString() })
  $lines += @(for ($i = 0; $i -lt $watcherOutput.Count; $i++) { [string]$watcherOutput[$i] })
  return $lines -join "`n"
}
function Html {
  try { return (Invoke-WebRequest -Uri $address -UseBasicParsing -TimeoutSec 2).Content } catch { return '' }
}
function Revision {
  $match = [regex]::Match((Html), '<!--atelier-dev:([a-f0-9]+)-->')
  return $match.Groups[1].Value
}
function Session-Token {
  $stored = [IO.File]::ReadAllText((Join-Path $data 'local-session'))
  $encrypted = [byte[]]@(for ($i = 0; $i -lt $stored.Length; $i += 2) { [Convert]::ToByte($stored.Substring($i, 2), 16) })
  return [Text.Encoding]::Unicode.GetString([Security.Cryptography.ProtectedData]::Unprotect($encrypted, $null, [Security.Cryptography.DataProtectionScope]::CurrentUser))
}
function Start-Watcher {
  $script:watcher = [PowerShell]::Create()
  $null = $watcher.AddCommand((Join-Path $workspace 'scripts/atelier.ps1')).AddParameter('NoBrowser').AddParameter('Port', $port)
  $script:watcherOutput = [Management.Automation.PSDataCollection[psobject]]::new()
  $script:watcherTask = $watcher.BeginInvoke[psobject, psobject]($null, $watcherOutput)
}
function Stop-Watcher {
  if ($null -ne $watcher) {
    $watcher.Stop()
    Watcher-Log | Set-Content -LiteralPath (Join-Path $results 'watcher-last.txt')
    for ($i = 0; $i -lt $watcher.Streams.Error.Count; $i++) {
      [string]$watcher.Streams.Error[$i] | Add-Content -LiteralPath (Join-Path $results 'watcher-last.txt')
    }
    $watcher.Dispose()
    $script:watcher = $null
  }
}
function Edit-Source([string]$Relative, [string]$Suffix) {
  $path = Join-Path $workspace $Relative
  if (-not $originals.ContainsKey($path)) {
    $originals[$path] = [IO.File]::ReadAllBytes($path)
    $backup = Join-Path $results "originals/$Relative"
    $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($backup))
    [IO.File]::WriteAllBytes($backup, $originals[$path])
  }
  $bytes = [byte[]]($originals[$path] + [Text.Encoding]::UTF8.GetBytes($Suffix))
  [IO.File]::WriteAllBytes($path, $bytes)
  $written[$path] = [Convert]::ToBase64String($bytes)
}
function Restore-Source([string]$Path) {
  if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($Path)) -ne $written[$Path]) {
    throw "Modification concurrente detectee, restauration automatique refusee : $Path"
  }
  [IO.File]::WriteAllBytes($Path, $originals[$Path])
  $written.Remove($Path)
}

try {
  if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Compiler atelier-app avant ce parcours.' }
  # Take the same exclusive lock before editing any sources or starting a browser.
  $lockPath = Join-Path $workspace '.artifacts/local-dev/watch.lock'
  $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($lockPath))
  $guard = [IO.File]::Open($lockPath, 'OpenOrCreate', 'ReadWrite', 'None')
  $guard.Dispose()
  $port = Free-Port
  $address = "http://127.0.0.1:$port/"
  $driverPort = Free-Port
  $endpoint = "http://127.0.0.1:$driverPort"
  $driverProcess = Start-Process -FilePath $Driver -ArgumentList "--port=$driverPort" -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $results 'driver.stdout.txt') -RedirectStandardError (Join-Path $results 'driver.stderr.txt')
  Wait-Until { try { $null = WebDriver GET '/status'; return $true } catch { return $false } } 'WebDriver' 20
  $created = WebDriver POST '/session' @{ capabilities = @{ alwaysMatch = @{ browserName = 'MicrosoftEdge'; 'ms:edgeOptions' = @{ args = @('--headless=new', '--disable-gpu', '--no-first-run', '--window-size=1280,900') } } } }
  $session = $created.sessionId
  $browserVersion = $created.capabilities.browserVersion
  $driverVersion = (& $Driver --version) -replace '^Microsoft Edge WebDriver ([0-9.]+).*', '$1'
  if (($browserVersion.Split('.')[0..2] -join '.') -ne ($driverVersion.Split('.')[0..2] -join '.')) {
    throw 'Les trois premiers numeros des versions Edge et WebDriver doivent correspondre.'
  }
  $null = WebDriver POST "/session/$session/timeouts" @{ implicit = 0; pageLoad = 30000; script = 10000 }
  $timer = [Diagnostics.Stopwatch]::StartNew()
  Start-Watcher
  Wait-Until { (Revision) -ne '' } 'premier demarrage' 600
  Record 'demarrage avec verification des compilations' $timer
  $revision = Revision
  Add-Type -AssemblyName System.Security
  $token = Session-Token
  $null = WebDriver POST "/session/$session/url" @{ url = "${address}#session=$token" }
  Wait-Until { (Script 'return !!document.getElementById("main-content")') } 'interface WASM et session' 30
  $timer = [Diagnostics.Stopwatch]::StartNew()
  Stop-Watcher
  Wait-Until { (Revision) -eq '' } 'arret supervise' 15
  Record 'arret supervise et liberation du serveur' $timer
  $timer.Restart()
  Start-Watcher
  Wait-Until { (Revision) -ne '' -and (Revision) -ne $revision } 'relance sans modification' 30
  Wait-Until { (Watcher-Log) -match 'Version prete' } 'lanceur pret' 15
  if ((Watcher-Log) -match 'Compiling|Starting trunk') { throw 'Une relance sans modification a recompile.' }
  if ((Watcher-Log) -notmatch 'Sources inchangees') { throw 'La compilation precedente n a pas ete reutilisee.' }
  Record 'relance sans recompilation' $timer
  $timer.Restart()
  $powershell = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
  $duplicate = Start-Process -FilePath $powershell -ArgumentList @('-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"{0}"' -f (Join-Path $workspace 'scripts/atelier.ps1')), '-NoBrowser', '-Port', "$port") -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $results 'duplicate.stdout.txt') -RedirectStandardError (Join-Path $results 'duplicate.stderr.txt')
  if (-not $duplicate.WaitForExit(10000)) { $duplicate.Kill(); throw 'Un second lanceur doit refuser le verrou immediatement.' }
  if ($duplicate.ExitCode -ne 1 -or (Revision) -eq '') { throw 'Le second lanceur doit echouer sans arreter le premier.' }
  Record 'double lancement refuse sous Windows PowerShell 5.1' $timer
  $null = WebDriver POST "/session/$session/url" @{ url = $address }
  Wait-Until { (Script 'return !!document.getElementById("main-content")') } 'session conservee' 30
  $revision = Revision
  $timer.Restart()
  Edit-Source 'crates/atelier-ui/index.html' "`n<!-- synthetic-launcher-html -->`n"
  Wait-Until { (Revision) -ne '' -and (Revision) -ne $revision } 'reconstruction HTML' 90
  Wait-Until { Script "return new XMLSerializer().serializeToString(document).includes('synthetic-launcher-html')" } 'rechargement HTML automatique' 30
  Record 'sauvegarde HTML vers affichage automatique' $timer
  $revision = Revision
  $timer.Restart()
  Edit-Source 'crates/atelier-ui/style.css' "`n:root { --synthetic-launcher-test: ready; }`n"
  Wait-Until { (Revision) -ne '' -and (Revision) -ne $revision } 'reconstruction CSS' 90
  Wait-Until { (Script "return getComputedStyle(document.documentElement).getPropertyValue('--synthetic-launcher-test').trim()") -eq 'ready' } 'rechargement CSS automatique' 30
  Record 'sauvegarde CSS vers affichage automatique' $timer
  # A synthetic textarea exercises the real capture listener without changing application data.
  Wait-Until { Script 'return !!document.getElementById("main-content")' } 'interface prete avant saisie' 30
  $null = Script "const field=document.createElement('textarea');field.id='synthetic-draft';field.value='brouillon';document.body.append(field);field.dispatchEvent(new Event('input',{bubbles:true}));"
  $revision = Revision
  $timer.Restart()
  Edit-Source 'crates/atelier-ui/index.html' "`n<!-- synthetic-launcher-draft -->`n"
  Wait-Until { (Revision) -ne '' -and (Revision) -ne $revision } 'nouvelle version avec brouillon' 90
  Wait-Until { try { return (WebDriver GET "/session/$session/alert/text") -match 'saisies' } catch { return $false } } 'confirmation du rechargement' 20
  $null = WebDriver POST "/session/$session/alert/dismiss" @{}
  if ((Script "return document.getElementById('synthetic-draft').value") -ne 'brouillon') { throw 'Brouillon perdu apres annulation.' }
  Record 'annulation du rechargement conserve le brouillon' $timer
  $null = WebDriver POST "/session/$session/refresh" @{}
  Wait-Until { Script "return new XMLSerializer().serializeToString(document).includes('synthetic-launcher-draft')" } 'actualisation manuelle' 30
  Wait-Until { Script 'return !!document.getElementById("main-content")' } 'interface prete apres actualisation manuelle' 30
  $null = Script "const field=document.createElement('textarea');field.value='brouillon';document.body.append(field);field.dispatchEvent(new Event('input',{bubbles:true}));"
  $revision = Revision
  $timer.Restart()
  Edit-Source 'crates/atelier-app/src/main.rs' "`n// synthetic-launcher-app`n"
  Wait-Until { (Revision) -ne '' -and (Revision) -ne $revision } 'reconstruction serveur seul' 90
  Wait-Until { try { return (WebDriver GET "/session/$session/alert/text") -match 'saisies' } catch { return $false } } 'confirmation apres reconstruction serveur' 20
  $null = WebDriver POST "/session/$session/alert/accept" @{}
  Wait-Until { (Script 'return !!document.getElementById("main-content")') } 'confirmation accepte le rechargement' 30
  Record 'serveur seul et acceptation du rechargement' $timer
  $revision = Revision
  $timer.Restart()
  Edit-Source 'crates/atelier-ui/index.html' "`n<link data-trunk rel=`"css`" href=`"synthetic-missing.css`">`n"
  Wait-Until { (Watcher-Log) -match 'Reconstruction echouee' } 'erreur de compilation visible' 90
  if ((Revision) -ne $revision) { throw 'Une compilation UI en erreur a remplace le serveur valide.' }
  Edit-Source 'crates/atelier-ui/index.html' "`n<!-- synthetic-launcher-recovered -->`n"
  Wait-Until { (Revision) -ne '' -and (Revision) -ne $revision } 'reprise apres erreur' 90
  Wait-Until { Script "return new XMLSerializer().serializeToString(document).includes('synthetic-launcher-recovered')" } 'affichage apres correction' 30
  Record 'erreur UI sans interruption puis reprise' $timer
  if ((Session-Token) -cne $token) { throw 'La session a change pendant les redemarrages.' }
  $authenticated = Invoke-WebRequest -Uri "${address}api/state" -Headers @{ Authorization = "Bearer $token" } -UseBasicParsing -TimeoutSec 5
  if ($authenticated.StatusCode -ne 200) { throw 'La session initiale ne donne plus acces au tableau.' }
  $timer.Restart()
  Stop-Watcher
  foreach ($path in @($written.Keys)) { Restore-Source $path }
  Start-Watcher
  Wait-Until { (Revision) -ne '' -and (Revision) -ne $revision } 'reconstruction des sources initiales' 120
  Wait-Until { (Watcher-Log) -match 'Version prete' } 'sources initiales pretes' 15
  if ((Html) -match 'synthetic-launcher') { throw 'Une modification de test subsiste dans les assets.' }
  Record 'sources initiales restaurees et reconstruites' $timer
  [pscustomobject]@{ browser = $browserVersion; driver = $driverVersion; rust = (& rustc --version); cargo = (Get-Command cargo).Source } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $results 'environment.json')
  $completed = $true
} finally {
  try {
    Stop-Watcher
    foreach ($path in @($written.Keys)) { Restore-Source $path }
  } catch { $completed = $false; throw } finally {
    if ($session) { try { $null = WebDriver DELETE "/session/$session" } catch { } }
    if ($driverProcess -and -not $driverProcess.HasExited) { Stop-Process -Id $driverProcess.Id; $driverProcess.WaitForExit() }
    $status = if ($completed) { 'passed' } else { 'failed' }
    $report.Add([pscustomobject]@{ scenario = 'parcours complet'; result = $status })
    $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $results 'results.json')
  }
}
