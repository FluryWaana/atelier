param(
  [Parameter(Mandatory)][string]$DriverBinary,
  [string]$ServerBinary = 'target/debug/atelier.exe',
  [string]$FixtureBinary = 'target/debug/native-fixture.exe',
  [string]$Assets = '.artifacts/agent-badge-ui'
)
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot
$root = Join-Path (Join-Path $workspace '.artifacts') ('badge-synthetic-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $root
$server = $null
$driver = $null
$sessionId = $null
$diagnosticClient = $null
function Free-Port { $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0); $listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop(); return $port }
function Driver-Request($method,$path,$body) {
  $args = @{Method=$method;Uri="$driverAddress$path";TimeoutSec=30}
  if ($null -ne $body) { $args.ContentType='application/json'; $args.Body=ConvertTo-Json -Depth 30 -Compress -InputObject $body }
  return (Invoke-RestMethod @args).value
}
function Save-Profile($profile) {
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do {
    try { $null=Invoke-RestMethod -Method Post -Uri "$address/api/agent-providers" -Headers $headers -ContentType application/json -Body ($profile|ConvertTo-Json -Depth 20); return }
    catch { if ([int]$_.Exception.Response.StatusCode -ne 409 -or [DateTime]::UtcNow -gt $deadline) { throw }; Start-Sleep -Milliseconds 200 }
  } while ($true)
}
try {
  $port = Free-Port
  $driverPort = Free-Port
  $address = "http://127.0.0.1:$port"
  $driverAddress = "http://127.0.0.1:$driverPort"
  $fixture = Join-Path $root 'claude.exe'
  Copy-Item -LiteralPath (Join-Path $workspace $FixtureBinary) -Destination $fixture
  [IO.File]::WriteAllText((Join-Path $root '.slow-diagnostic'), 'atelier-synthetic-held')
  $serverCopy = Join-Path $root 'synthetic-atelier.exe'
  Copy-Item -LiteralPath (Join-Path $workspace $ServerBinary) -Destination $serverCopy
  $data = Join-Path $root 'synthetic-data'
  $config = Join-Path $root 'synthetic-config'
  $null = New-Item -ItemType Directory -Path $config
  $start = [Diagnostics.ProcessStartInfo]::new($serverCopy)
  $start.UseShellExecute=$false; $start.CreateNoWindow=$true
  foreach ($arg in @('serve','--data-dir',$data,'--assets',(Join-Path $workspace $Assets),'--port',"$port")) { $start.ArgumentList.Add($arg) }
  $start.Environment.Clear()
  foreach ($key in @('SystemRoot','WINDIR','TEMP','TMP')) { $start.Environment[$key]=[Environment]::GetEnvironmentVariable($key) }
  foreach ($key in @('HOME','USERPROFILE','LOCALAPPDATA','APPDATA')) { $start.Environment[$key]=$root }
  $start.Environment['PATH']=$root
  $server=[Diagnostics.Process]::Start($start)
  $server.PriorityClass='BelowNormal'
  $deadline=[DateTime]::UtcNow.AddSeconds(30)
  while (!(Test-Path (Join-Path $data 'local-session'))) { if ($server.HasExited -or [DateTime]::UtcNow -gt $deadline) { throw 'Synthetic server failed' }; Start-Sleep -Milliseconds 200 }
  $stored=[IO.File]::ReadAllText((Join-Path $data 'local-session'))
  $token=[Text.Encoding]::Unicode.GetString([Security.Cryptography.ProtectedData]::Unprotect([Convert]::FromHexString($stored),$null,[Security.Cryptography.DataProtectionScope]::CurrentUser))
  $headers=@{Authorization="Bearer $token";'x-local-request'='1'}
  $deadline=[DateTime]::UtcNow.AddSeconds(30)
  do {
    try { $null=Invoke-WebRequest -Uri "$address/" -TimeoutSec 1; break } catch {
      if ($server.HasExited -or [DateTime]::UtcNow -gt $deadline) { throw 'Synthetic HTTP server did not become ready' }
      Start-Sleep -Milliseconds 200
    }
  } while ($true)
  $body=@{id='claude-extra';name='Claude synthetic extra';type='claude';auth='cli';command=$fixture;configDir=$config;expected=$null} | ConvertTo-Json
  $null=Invoke-RestMethod -Method Post -Uri "$address/api/agent-providers" -Headers $headers -ContentType application/json -Body $body
  $profiles=Invoke-RestMethod -Uri "$address/api/agent-providers" -Headers $headers
  $handler=[Net.Http.HttpClientHandler]::new(); $handler.UseProxy=$false
  $diagnosticClient=[Net.Http.HttpClient]::new($handler)
  $pendingDiagnostics=@()
  foreach ($id in @('claude','claude-extra')) {
    $profile=@($profiles | Where-Object id -eq $id)[0]
    $request=[Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Post,"$address/api/agent-providers/$id/diagnose")
    $request.Headers.Authorization=[Net.Http.Headers.AuthenticationHeaderValue]::new('Bearer',$token)
    $request.Headers.Add('x-local-request','1')
    $request.Content=[Net.Http.StringContent]::new((@{expected=$profile;statusOnly=$true}|ConvertTo-Json -Depth 20),[Text.Encoding]::UTF8,'application/json')
    $request.Content.Headers.ContentType.CharSet=$null
    $pendingDiagnostics+=,$diagnosticClient.SendAsync($request)
  }
  $deadline=[DateTime]::UtcNow.AddSeconds(10)
  while (@(Get-ChildItem -LiteralPath $root -Filter '.version-started.*').Count -lt 2) {
    if ([DateTime]::UtcNow -gt $deadline) { throw 'Two held diagnostics did not start' }
    Start-Sleep -Milliseconds 100
  }
  $driver=Start-Process -FilePath (Join-Path $workspace $DriverBinary) -ArgumentList "--port=$driverPort" -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $root 'driver-out') -RedirectStandardError (Join-Path $root 'driver-err')
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do {
    try { $null=Driver-Request GET '/status' $null; break } catch {
      if ($driver.HasExited -or [DateTime]::UtcNow -gt $deadline) { throw 'Synthetic WebDriver did not become ready' }
      Start-Sleep -Milliseconds 200
    }
  } while ($true)
  $session=Driver-Request POST '/session' @{capabilities=@{alwaysMatch=@{browserName='MicrosoftEdge';'ms:edgeOptions'=@{args=@('--headless=new','--disable-gpu','--no-first-run','--window-size=1280,900')}}}}
  $sessionId=$session.sessionId
  $null=Driver-Request POST "/session/$sessionId/url" @{url="$address/#session=$token"}
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  while (!(Test-Path (Join-Path $root '.version-started'))) {
    if ([DateTime]::UtcNow -gt $deadline) { throw 'Slow diagnostic fixture did not start' }
    Start-Sleep -Milliseconds 100
  }
  foreach ($reload in 1..3) { $null=Driver-Request POST "/session/$sessionId/refresh" @{} }
  Start-Sleep -Seconds 1
  [IO.File]::WriteAllText((Join-Path $root '.version-release'), 'synthetic-only')
  Remove-Item -LiteralPath (Join-Path $root '.slow-diagnostic')
  foreach ($pendingDiagnostic in $pendingDiagnostics) {
    if (!$pendingDiagnostic.Wait(15000) -or !$pendingDiagnostic.Result.IsSuccessStatusCode) { throw 'Held diagnostic did not finish cleanly' }
    $pendingDiagnostic.Result.Dispose()
  }
  $deadline=[DateTime]::UtcNow.AddSeconds(60)
  do {
    $result=Driver-Request POST "/session/$sessionId/execute/sync" @{script="return Array.from(document.querySelectorAll('.agent-row')).filter(e=>e.textContent.includes('Claude')).map(e=>{const dot=e.querySelector('.online-dot'),name=e.querySelector('strong');return {green:dot.classList.contains('agent-status-online'),color:getComputedStyle(dot).backgroundColor,before:!!(dot.compareDocumentPosition(name)&Node.DOCUMENT_POSITION_FOLLOWING),aligned:Math.abs((dot.getBoundingClientRect().top+dot.getBoundingClientRect().height/2)-(name.getBoundingClientRect().top+name.getBoundingClientRect().height/2))<3};});";args=@()}
    if (@($result).Count -eq 2 -and @($result | Where-Object { !$_.green }).Count -eq 0) { break }
    Start-Sleep -Milliseconds 300
  } while ([DateTime]::UtcNow -lt $deadline)
  if (@($result).Count -ne 2) { throw 'Both Claude profiles were not rendered' }
  foreach ($row in $result) { if (!$row.green -or !$row.before -or !$row.aligned -or $row.color -ne 'rgb(140, 203, 176)') { throw ('Badge assertion failed: '+($row|ConvertTo-Json -Compress)) } }
  $profiles=Invoke-RestMethod -Uri "$address/api/agent-providers" -Headers $headers
  $profile=@($profiles | Where-Object id -eq 'claude-extra')[0]
  $change=@{id=$profile.id;name=$profile.name;type='claude';auth='cli';command=(Join-Path $root 'missing.exe');configDir=$config;expected=$profile}
  Save-Profile $change
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do {
    $gray=Driver-Request POST "/session/$sessionId/execute/sync" @{script="const row=Array.from(document.querySelectorAll('.agent-row')).find(e=>e.textContent.includes('Claude synthetic extra'));return !!row&&!row.querySelector('.agent-status-online');";args=@()}
    if ($gray) { break }; Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $deadline)
  if (!$gray) { throw 'Changed profile retained a stale green badge' }
  $profiles=Invoke-RestMethod -Uri "$address/api/agent-providers" -Headers $headers
  $change.expected=@($profiles | Where-Object id -eq 'claude-extra')[0]
  $change.command=$fixture
  Save-Profile $change
  $null=Driver-Request POST "/session/$sessionId/execute/sync" @{script="const originalFetch=window.fetch.bind(window),originalTimeout=AbortSignal.timeout.bind(AbortSignal);window.syntheticDiagnosticRequests=[];AbortSignal.timeout=()=>originalTimeout(2000);window.fetch=async function(input,init){const request=new Request(input,init);if(request.url.endsWith('/diagnose')){const body=await request.clone().json();window.syntheticDiagnosticRequests.push(body);if(!body.statusOnly)return new Promise((resolve,reject)=>{if(request.signal.aborted)reject(new DOMException('Aborted','AbortError'));else request.signal.addEventListener('abort',()=>reject(new DOMException('Aborted','AbortError')),{once:true});});}return originalFetch(request);};return true;";args=@()}
  $null=Driver-Request POST "/session/$sessionId/execute/sync" @{script="document.querySelector('#manage-agents').click();return true;";args=@()}
  $deadline=[DateTime]::UtcNow.AddSeconds(30)
  do {
    $clicked=Driver-Request POST "/session/$sessionId/execute/sync" @{script="const b=document.querySelector('[data-profile=claude-extra]');if(!b||b.disabled)return false;b.click();return true;";args=@()}
    if ($clicked) { break }; Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $deadline)
  if (!$clicked) { throw 'Synthetic profile could not be opened' }
  $deadline=[DateTime]::UtcNow.AddSeconds(30)
  do {
    $green=Driver-Request POST "/session/$sessionId/execute/sync" @{script="const row=Array.from(document.querySelectorAll('.agent-row')).find(e=>e.textContent.includes('Claude synthetic extra'));return !!row?.querySelector('.agent-status-online')&&document.querySelector('#agent-diagnostic-version')?.textContent==='0.0.7';";args=@()}
    if ($green) { break }; Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $deadline)
  if (!$green) { throw 'Profile diagnostic did not update the sidebar badge' }
  $deadline=[DateTime]::UtcNow.AddSeconds(10)
  do {
    $bounded=Driver-Request POST "/session/$sessionId/execute/sync" @{script="return document.querySelector('#diagnose-profile')?.disabled===false&&document.querySelector('#agent-diagnostic-error')?.textContent.includes('minute')&&!!Array.from(document.querySelectorAll('.agent-row')).find(e=>e.textContent.includes('Claude synthetic extra'))?.querySelector('.agent-status-online')&&!document.querySelector('#profile-model')&&!document.querySelector('#profile-effort')&&window.syntheticDiagnosticRequests.every(body=>!Object.hasOwn(body,'probeModel'));";args=@()}
    if ($bounded) { break }; Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $deadline)
  if (!$bounded) { throw 'Stalled optional details did not preserve a usable connected state without model probes' }
  $null=Driver-Request POST "/session/$sessionId/execute/sync" @{script="document.querySelector('#close-agents').click();return true;";args=@()}
  $screenshot=Driver-Request GET "/session/$sessionId/screenshot" $null
  [IO.File]::WriteAllBytes((Join-Path $root 'badges.png'),[Convert]::FromBase64String($screenshot))
  Write-Output 'PASS: reloads during slow diagnostics recover automatically, two Claude profiles green, actual CSS color, alignment, stale-state invalidation, profile updates sidebar, stalled details bounded, no model probes.'
} finally {
  if ($diagnosticClient) { $diagnosticClient.Dispose() }
  if ($sessionId) { try { $null=Driver-Request DELETE "/session/$sessionId" $null } catch {} }
  foreach ($process in @($driver,$server)) { if ($null -ne $process -and !$process.HasExited) { $process.Kill(); $process.WaitForExit() } }
}
