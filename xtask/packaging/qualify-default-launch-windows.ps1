param([Parameter(Mandatory = $true)][string]$Binary)
$ErrorActionPreference = 'Stop'
if ($env:CI -ne 'true' -or $env:ATELIER_RUST_FINAL_CAMPAIGN -ne '1' -or -not $env:RUNNER_TEMP -or [Environment]::OSVersion.Platform -ne 'Win32NT') { throw 'Disposable Windows final CI campaign required.' }
if (-not [IO.Path]::IsPathFullyQualified($Binary)) { throw 'An absolute installed binary is required.' }
$runnerRoot = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd([char[]]@('\', '/'))
$Binary = [IO.Path]::GetFullPath($Binary)
if (-not $Binary.StartsWith($runnerRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'The installed binary must be strictly inside RUNNER_TEMP.' }
$ownedPath = $Binary
while ($true) {
  $item = Get-Item -LiteralPath $ownedPath -Force
  if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked qualification paths are refused.' }
  if ([string]::Equals($ownedPath, $runnerRoot, [StringComparison]::OrdinalIgnoreCase)) { break }
  $ownedPath = [IO.Path]::GetDirectoryName($ownedPath)
}
if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) { throw 'Installed binary missing.' }
$app = [IO.Path]::GetDirectoryName($Binary)
$failureLog = Join-Path ([IO.Path]::GetDirectoryName($app)) 'default-launch-qualification-error.log'
$qualificationPhase = 'shortcut and installed executable preflight'
$probeFailure = ''
trap {
  $details = if ($probeFailure) { $probeFailure } else { "Phase: $qualificationPhase`n" + ($_ | Out-String) }
  $details.Substring(0, [Math]::Min($details.Length, 32768)) | Set-Content -LiteralPath $failureLog -Encoding UTF8
  exit 1
}
$shortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) 'Atelier Kanban/Atelier Kanban.lnk'
if (-not (Test-Path -LiteralPath $shortcutPath -PathType Leaf)) { throw 'Installed Start Menu shortcut missing.' }
$shell = New-Object -ComObject WScript.Shell
$shortcut = $null
try {
  $shortcut = $shell.CreateShortcut($shortcutPath)
  $shortcutTarget = [IO.Path]::GetFullPath($shortcut.TargetPath)
  $shortcutDirectory = [IO.Path]::GetFullPath($shortcut.WorkingDirectory)
  if (-not [string]::Equals($shortcutTarget, $Binary, [StringComparison]::OrdinalIgnoreCase) -or $shortcut.Arguments -ne '' -or -not [string]::Equals($shortcutDirectory, $app, [StringComparison]::OrdinalIgnoreCase)) { throw 'Installed shortcut target, arguments or working directory changed.' }
} finally {
  if ($null -ne $shortcut) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shortcut) }
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shell)
}
$current = Get-Content -LiteralPath (Join-Path $app 'current.json') -Raw | ConvertFrom-Json
$version = [Diagnostics.FileVersionInfo]::GetVersionInfo($Binary)
if ($version.ProductName -cne 'Atelier Kanban' -or $version.FileDescription -cne 'Atelier Kanban' -or $version.FileVersion -cne $current.active.version -or $version.ProductVersion -cne $current.active.version) { throw 'Installed executable branding or version metadata mismatch.' }
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$visualStudio = @(& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if ($LASTEXITCODE -ne 0 -or $visualStudio.Count -ne 1) { throw 'MSVC required to inspect the actual installed executable.' }
$toolset = (Get-Content -LiteralPath (Join-Path $visualStudio[0] 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt') -Raw).Trim()
if ($toolset -notmatch '^\d+\.\d+\.\d+$') { throw 'Invalid default MSVC toolset version.' }
$dumpbin = Join-Path $visualStudio[0] "VC/Tools/MSVC/$toolset/bin/Hostx64/x64/dumpbin.exe"
if (-not (Test-Path -LiteralPath $dumpbin -PathType Leaf)) { throw 'MSVC dumpbin required to inspect the actual installed executable.' }
$imports = & $dumpbin /DEPENDENTS $Binary
if ($LASTEXITCODE -ne 0 -or ($imports -join "`n") -match '(?im)^\s*(vcruntime|msvcp|ucrtbase|api-ms-win-crt-)[^\r\n]*\.dll\s*$') { throw 'Installed executable requires a separately installed C runtime.' }
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 4317)
try { $listener.Start() } finally { $listener.Stop() }
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

function Get-OwnedLaunchError([int]$ProcessId) {
  $condition = [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@(
    [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty, $ProcessId),
    [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ClassNameProperty, '#32770'),
    [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Atelier Kanban')
  ))
  $dialogs = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, $condition)
  "Owned launcher PID ${ProcessId}: $($dialogs.Count) dialogs."
  for ($index = 0; $index -lt [Math]::Min($dialogs.Count, 4); $index++) {
    $texts = $dialogs[$index].FindAll([Windows.Automation.TreeScope]::Descendants,
      [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Text))
    for ($textIndex = 0; $textIndex -lt [Math]::Min($texts.Count, 8); $textIndex++) {
      $text = $texts[$textIndex].Current.Name
      $text.Substring(0, [Math]::Min($text.Length, 1024))
    }
  }
}

function Confirm-OwnedWorkspace([Diagnostics.Process]$Parent, [string]$Answer) {
  $deadline = [Diagnostics.Stopwatch]::StartNew()
  while ($deadline.ElapsedMilliseconds -lt 20000) {
    if ($Parent.HasExited) { throw 'Default launch exited before its confirmation.' }
    $children = @(Get-CimInstance -ClassName Win32_Process -Filter "ParentProcessId = $($Parent.Id)" -Property ProcessId, ParentProcessId, ExecutablePath -OperationTimeoutSec 2 | Where-Object { [string]::Equals($_.ExecutablePath, $Binary, [StringComparison]::OrdinalIgnoreCase) })
    if ($children.Count -gt 1) { throw 'Ambiguous owned confirmation process.' }
    if ($children.Count -eq 1) {
      $childId = [int]$children[0].ProcessId
      $condition = [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@(
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty, $childId),
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ClassNameProperty, '#32770'),
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Atelier Kanban')
      ))
      $dialogs = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, $condition)
      if ($dialogs.Count -gt 1) { throw 'Ambiguous owned confirmation dialog.' }
      if ($dialogs.Count -eq 1) {
        $buttonId = if ($Answer -eq 'No') { '7' } elseif ($Answer -eq 'Yes') { '6' } else { throw 'Unknown confirmation answer.' }
        $buttons = $dialogs[0].FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@(
          [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::AutomationIdProperty, $buttonId),
          [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Button)
        )))
        if ($buttons.Count -ne 1 -or -not $buttons[0].Current.IsEnabled) { throw 'Owned confirmation button missing or ambiguous.' }
        $stillOwned = @(Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $childId AND ParentProcessId = $($Parent.Id)" -Property ProcessId, ParentProcessId, ExecutablePath -OperationTimeoutSec 2 | Where-Object { [string]::Equals($_.ExecutablePath, $Binary, [StringComparison]::OrdinalIgnoreCase) })
        if ($Parent.HasExited -or $stillOwned.Count -ne 1) { throw 'Confirmation process identity changed.' }
        $buttons[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
        return
      }
    }
    Start-Sleep -Milliseconds 100
  }
  throw 'Owned workspace confirmation timed out.'
}

function Invoke-DefaultLaunchProbe([string]$Profile, [string]$Action = '', [string]$Answer = '') {
  $script:qualificationPhase = "default launch scenario=$scenario action='$Action' answer='$Answer'"
  $start = [Diagnostics.ProcessStartInfo]::new()
  $start.FileName = $shortcutTarget
  $start.WorkingDirectory = $shortcutDirectory
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  $start.Environment['USERPROFILE'] = $Profile
  $start.Environment['HOME'] = $Profile
  $start.Environment['LOCALAPPDATA'] = Join-Path $Profile 'AppData/Local'
  $start.Environment['APPDATA'] = Join-Path $Profile 'AppData/Roaming'
  if ($Action) {
    if ($Action -ne 'stop') { throw 'Unknown default launch action.' }
    $start.ArgumentList.Add($Action)
  }
  $process = [Diagnostics.Process]::new()
  $process.StartInfo = $start
  $started = $false
  $deadline = [Diagnostics.Stopwatch]::StartNew()
  try {
    if (-not $process.Start()) { throw 'Default launch process did not start.' }
    $started = $true
    $stdout = $process.StandardOutput.BaseStream.CopyToAsync([IO.Stream]::Null)
    $stderr = $process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
    if ($Answer) { Confirm-OwnedWorkspace $process $Answer }
    $remaining = [int][Math]::Max(0, 30000 - $deadline.ElapsedMilliseconds)
    if (-not $process.WaitForExit($remaining)) { throw 'Default launch process timed out.' }
    # Detached servers can retain unused inherited pipe handles after the launcher has exited.
    # The default Stop probe below verifies their lifecycle without waiting for pipe EOF here.
    if ($process.ExitCode -ne 0) { throw "Default launch failed with exit $($process.ExitCode)." }
  } catch {
    if (-not $script:probeFailure) {
      $script:probeFailure = "Phase: $qualificationPhase`n" + ($_ | Out-String)
      $probeFailure.Substring(0, [Math]::Min($probeFailure.Length, 32768)) | Set-Content -LiteralPath $failureLog -Encoding UTF8
      if ($started) {
        try { $script:probeFailure += "`n" + (Get-OwnedLaunchError $process.Id | Out-String) }
        catch { $script:probeFailure += "`nOwned launcher dialog evidence unavailable." }
      }
      $probeFailure.Substring(0, [Math]::Min($probeFailure.Length, 32768)) | Set-Content -LiteralPath $failureLog -Encoding UTF8
    }
    throw
  } finally {
    try {
      if ($started -and -not $process.HasExited) {
        $process.Kill($true)
        if (-not $process.WaitForExit(5000)) { throw 'Owned default launch process did not stop.' }
      }
    } finally { $process.Dispose() }
  }
}

function Assert-DefaultWorkspace([string]$Workspace) {
  foreach ($name in @('.atelier-rust-prototype', 'kanban.sqlite')) {
    if (-not (Test-Path -LiteralPath (Join-Path $Workspace $name) -PathType Leaf)) { throw 'Default workspace was not initialized.' }
  }
}

function Assert-PreviousData([string]$Previous) {
  if ([IO.File]::ReadAllText((Join-Path $Previous 'human-note')) -cne 'preserve previous-format data' -or @(Get-ChildItem -LiteralPath $Previous -Force).Count -ne 1) { throw 'Previous-format data was modified.' }
}

$fixture = Join-Path $runnerRoot ('atelier-default-launch-' + [Guid]::NewGuid().ToString('N'))
foreach ($scenario in @('fresh', 'previous-format')) {
  $profile = Join-Path $fixture $scenario
  $local = Join-Path $profile 'AppData/Local'
  New-Item -ItemType Directory -Path $local, (Join-Path $profile 'AppData/Roaming') | Out-Null
  $workspace = Join-Path $local 'AtelierKanban/workspace'
  $previous = Join-Path $local 'AtelierKanban/data'
  $needsStop = $false
  try {
    if ($scenario -eq 'previous-format') {
      New-Item -ItemType Directory -Path $previous | Out-Null
      [IO.File]::WriteAllText((Join-Path $previous 'human-note'), 'preserve previous-format data')
      $needsStop = $true
      Invoke-DefaultLaunchProbe $profile '' 'No'
      if (Test-Path -LiteralPath $workspace) { throw 'Declining a fresh workspace created files.' }
      Assert-PreviousData $previous
      $needsStop = $false
    }
    $needsStop = $true
    if ($scenario -eq 'previous-format') { Invoke-DefaultLaunchProbe $profile '' 'Yes' } else { Invoke-DefaultLaunchProbe $profile }
    Assert-DefaultWorkspace $workspace
    if ($scenario -eq 'previous-format') { Assert-PreviousData $previous }
    Invoke-DefaultLaunchProbe $profile 'stop'
    $needsStop = $false
  } finally {
    if ($needsStop -and (Test-Path -LiteralPath (Join-Path $workspace '.atelier-rust-prototype') -PathType Leaf)) { Invoke-DefaultLaunchProbe $profile 'stop' }
  }
}
Write-Output 'Installed Start Menu default launch, workspace confirmation and default stop qualified.'
