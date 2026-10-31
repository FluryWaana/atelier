param(
  [string]$Binary = 'target/debug/atelier.exe',
  [string]$Assets = '.artifacts/rust-ui-dev',
  [string]$Driver = 'msedgedriver.exe'
)
$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Set-Location $workspace
$run = Join-Path $workspace ('.artifacts/project-clone-browser/' + [Guid]::NewGuid().ToString('N'))
$data = Join-Path $run 'store'
$null = New-Item -ItemType Directory -Force $run
$report = [Collections.Generic.List[object]]::new()
$server = $null
$driverProcess = $null
$session = $null
$originalGitGlobal = $env:GIT_CONFIG_GLOBAL
$originalGitSystem = $env:GIT_CONFIG_SYSTEM
function Free-Port {
  $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
  $listener.Start()
  try { $listener.LocalEndpoint.Port } finally { $listener.Stop() }
}
function WD([string]$Method, [string]$Path, $Body = $null) {
  $arguments = @{ Uri = "$endpoint$Path"; Method = $Method; TimeoutSec = 45 }
  if ($null -ne $Body) { $arguments.Body = ConvertTo-Json -InputObject $Body -Depth 20 -Compress; $arguments.ContentType = 'application/json' }
  (Invoke-RestMethod @arguments).value
}
function JS([string]$Source, [array]$Arguments = @()) { WD POST "/session/$session/execute/sync" @{ script = $Source; args = $Arguments } }
function Wait-Dom([string]$Source) {
  for ($attempt = 0; $attempt -lt 150; $attempt++) {
    if (JS "return ($Source)") { return }
    Start-Sleep -Milliseconds 100
  }
  throw "Condition absente : $Source"
}
function Click([string]$Selector) {
  $element = WD POST "/session/$session/element" @{ using = 'css selector'; value = $Selector }
  WD POST "/session/$session/element/$($element.'element-6066-11e4-a52e-4f735466cecf')/click" @{} | Out-Null
}
function Input-Text([string]$Selector, [string]$Value) {
  JS 'const el=document.querySelector(arguments[0]);el.value=arguments[1];el.dispatchEvent(new Event("input",{bubbles:true}));' @($Selector, $Value) | Out-Null
}
function Require([string]$Name, [string]$Source) {
  if (-not (JS "return ($Source)")) { throw "Vérification échouée : $Name" }
  $report.Add([pscustomobject]@{ width = $width; check = $Name; result = 'passed' })
}
try {
  $env:GIT_CONFIG_GLOBAL = 'NUL'
  $env:GIT_CONFIG_SYSTEM = 'NUL'
  $port = Free-Port
  $base = "http://127.0.0.1:$port"
  $server = Start-Process -FilePath (Resolve-Path $Binary).Path -ArgumentList @('serve', '--data-dir', ('"{0}"' -f $data), '--assets', ('"{0}"' -f (Resolve-Path $Assets).Path), '--port', $port) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'server.out') -RedirectStandardError (Join-Path $run 'server.err')
  for ($attempt = 0; $attempt -lt 150 -and -not (Test-Path (Join-Path $data 'local-session')); $attempt++) { Start-Sleep -Milliseconds 100 }
  Add-Type -AssemblyName System.Security
  $stored = [IO.File]::ReadAllText((Join-Path $data 'local-session'))
  $encrypted = [byte[]]@(for ($i = 0; $i -lt $stored.Length; $i += 2) { [Convert]::ToByte($stored.Substring($i, 2), 16) })
  $token = [Text.Encoding]::Unicode.GetString([Security.Cryptography.ProtectedData]::Unprotect($encrypted, $null, [Security.Cryptography.DataProtectionScope]::CurrentUser))
  $headers = @{ Authorization = "Bearer $token"; 'X-Local-Request' = '1'; Origin = $base }
  $parent = Join-Path $data 'repositories'
  foreach ($size in @(1440, 768, 390)) {
    $repo = Join-Path $parent "cloned-$size"
    $null = New-Item -ItemType Directory -Force $repo
    & git -c core.hooksPath=NUL -c init.templateDir= init -b main $repo | Out-Null
    if ($LASTEXITCODE) { throw 'Initialisation du dépôt synthétique impossible.' }
  }
  $driverPort = Free-Port
  $endpoint = "http://127.0.0.1:$driverPort"
  $driverProcess = Start-Process -FilePath (Get-Command $Driver -ErrorAction Stop).Source -ArgumentList "--port=$driverPort" -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'driver.out') -RedirectStandardError (Join-Path $run 'driver.err')
  for ($attempt = 0; $attempt -lt 100; $attempt++) { try { WD GET '/status' | Out-Null; break } catch { Start-Sleep -Milliseconds 100 } }
  $created = WD POST '/session' @{ capabilities = @{ alwaysMatch = @{ browserName = 'MicrosoftEdge'; 'ms:edgeOptions' = @{ args = @('--headless=new', '--disable-gpu', '--no-first-run', "--user-data-dir=$(Join-Path $run 'profile')") } } } }
  $session = $created.sessionId
  $init = @'
sessionStorage.setItem('atelier-session',TOKEN);
const realFetch=window.fetch.bind(window);
window.cloneMode='error';window.cloneRequests=[];window.createRequests=0;
window.fetch=async(input,init)=>{
  const url=new URL(typeof input==='string'?input:input.url,location.href);
  if(url.pathname.endsWith('/diagnose'))return Promise.resolve(new Response(JSON.stringify({error:'synthetic diagnostic disabled'}),{status:409}));
  if(url.pathname==='/api/repositories/clone'){
    window.cloneRequests.push(input instanceof Request ? await input.clone().json() : JSON.parse(init.body));
    if(window.cloneMode==='cancel')return new Promise(resolve=>{window.finishClone=()=>resolve(new Response(JSON.stringify({error:'message.clone_interrupted'}),{status:400}));});
    if(window.cloneMode==='success'||window.cloneMode==='save-error')return Promise.resolve(new Response(JSON.stringify({path:window.clonePath,authorized:true}),{status:200}));
    return Promise.resolve(new Response(JSON.stringify({error:'message.clone_https_failed'}),{status:400}));
  }
  if(url.pathname.includes('/clone/')&&url.pathname.endsWith('/cancel')){window.finishClone();return Promise.resolve(new Response('{}',{status:200}));}
  if(url.pathname==='/api/projects'&&(init?.method||input.method)==='POST'){
    window.createRequests++;
    if(window.cloneMode==='save-error')return Promise.resolve(new Response(JSON.stringify({error:'message.the_project_could_not_be_updated'}),{status:400}));
  }
  return realFetch(input,init);
};
'@
  $init = $init.Replace('TOKEN', (ConvertTo-Json -InputObject $token -Compress))
  WD POST "/session/$session/ms/cdp/execute" @{ cmd = 'Page.addScriptToEvaluateOnNewDocument'; params = @{ source = $init } } | Out-Null
  foreach ($width in @(1440, 768, 390)) {
    WD POST "/session/$session/window/rect" @{ width = $width; height = 1000 } | Out-Null
    WD POST "/session/$session/ms/cdp/execute" @{ cmd = 'Emulation.setDeviceMetricsOverride'; params = @{ width = $width; height = 1000; deviceScaleFactor = 1; mobile = $false } } | Out-Null
    WD POST "/session/$session/url" @{ url = "$base/projects" } | Out-Null
    Wait-Dom "document.querySelector('#add-project')&&!document.querySelector('#add-project').disabled"
    Require 'requested viewport is real' "innerWidth===$width"
    Click '#add-project'
    Wait-Dom "document.querySelector('#project-create-dialog')?.open"
    Require 'local creation remains available' "!!document.querySelector('#project-directory')"
    JS 'const el=document.querySelector("#project-source");el.value="clone";el.dispatchEvent(new Event("change",{bubbles:true}));' | Out-Null
    Wait-Dom "!!document.querySelector('#project-clone-url')"
    Input-Text '#project-clone-url' 'https://forge.example.test/team/synthetic.git'
    Input-Text '#project-clone-parent' $parent
    Require 'repository name suggested' "document.querySelector('#project-clone-directory').value==='synthetic'&&document.querySelector('#project-create-name').value==='synthetic'"
    JS 'document.querySelector("#project-form").requestSubmit()' | Out-Null
    Wait-Dom "Array.from(document.querySelectorAll('#project-form .error[role=alert]')).some(el=>el.textContent.includes('HTTPS'))"
    Require 'failure preserves draft and skips project creation' "document.querySelector('#project-clone-url').value.includes('synthetic.git')&&window.createRequests===0"
    JS 'window.cloneMode="cancel";document.querySelector("#project-form").requestSubmit()' | Out-Null
    Wait-Dom "!!document.querySelector('#cancel-project-clone')"
    Require 'form is protected during cloning' "document.querySelector('#project-clone-url').matches(':disabled')&&document.querySelector('#close-project-create').disabled"
    Click '#cancel-project-clone'
    Wait-Dom "!document.querySelector('#cancel-project-clone')"
    Require 'cancel preserves the draft' "document.querySelector('#project-clone-url').value.includes('synthetic.git')&&window.createRequests===0"
    Input-Text '#project-clone-url' 'git@clone.example.test:team/ssh-project.git'
    Require 'SSH uses the same form without forge credentials' "!document.querySelector('#project-clone-connection')&&document.querySelector('#project-clone-url').value.startsWith('git@')"
    [IO.File]::WriteAllBytes((Join-Path $run "clone-$width.png"), [Convert]::FromBase64String((WD GET "/session/$session/screenshot")))
    Require 'no horizontal overflow' "document.documentElement.scrollWidth<=innerWidth&&document.querySelector('#project-create-dialog').scrollWidth<=document.querySelector('#project-create-dialog').clientWidth"
    $clonePath = Join-Path $parent "cloned-$width"
    if ($width -eq 1440) {
      JS 'window.cloneMode="save-error";window.clonePath=arguments[0];document.querySelector("#project-form").requestSubmit()' @($clonePath) | Out-Null
      Wait-Dom "!!document.querySelector('#project-directory')&&!document.querySelector('#project-directory').matches(':disabled')&&window.createRequests===1"
      Require 'completed clone survives failed project creation' "document.querySelector('#project-directory').value===window.clonePath&&!document.querySelector('#project-clone-url')&&document.querySelector('#project-form').textContent.includes('sans le recloner')"
      JS 'window.cloneMode="success";document.querySelector("#project-form").requestSubmit()' | Out-Null
      Wait-Dom "!document.querySelector('#project-create-dialog')"
      Require 'retry adds the clone without cloning again' "window.createRequests===2&&window.cloneRequests.length===3"
      $savedState = Invoke-RestMethod -Uri "$base/api/state" -Headers $headers
      $savedProject = $savedState.projects | Where-Object repo -eq $clonePath
      Click "#project-settings-$($savedProject.id)"
      Wait-Dom "document.querySelector('#project-name')?.value==='ssh-project'"
    } else {
      JS 'window.cloneMode="success";window.clonePath=arguments[0];document.querySelector("#project-form").requestSubmit()' @($clonePath) | Out-Null
      Wait-Dom "!document.querySelector('#project-create-dialog')&&document.querySelector('#project-name')?.value==='ssh-project'"
    }
    Require 'success opens the registered project' "window.cloneRequests.at(-1).connection===null&&document.querySelector('#project-repo')?.value===window.clonePath"
    $state = Invoke-RestMethod -Uri "$base/api/state" -Headers $headers
    if (-not ($state.projects | Where-Object repo -eq $clonePath)) { throw 'Projet absent de la base synthétique.' }
  }
  $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $run 'results.json')
  Write-Output "Parcours clonage réussis : $($report.Count) contrôles. Résultats : $run"
} catch {
  if ($session) {
    try { [IO.File]::WriteAllBytes((Join-Path $run 'failure.png'), [Convert]::FromBase64String((WD GET "/session/$session/screenshot"))); JS 'return document.body.innerText' | Set-Content (Join-Path $run 'failure.txt') } catch { }
  }
  throw
} finally {
  if ($session) { try { WD DELETE "/session/$session" | Out-Null } catch { } }
  if ($server) { try { Invoke-RestMethod -Method Post -Uri "$base/api/shutdown" -Headers $headers -ContentType 'application/json' -Body '{}' | Out-Null; $server.WaitForExit(10000) | Out-Null } catch { }; if (-not $server.HasExited) { Stop-Process -Id $server.Id } }
  if ($driverProcess -and -not $driverProcess.HasExited) { Stop-Process -Id $driverProcess.Id }
  $env:GIT_CONFIG_GLOBAL = $originalGitGlobal
  $env:GIT_CONFIG_SYSTEM = $originalGitSystem
}
