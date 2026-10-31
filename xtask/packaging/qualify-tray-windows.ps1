param([Parameter(Mandatory = $true)][string]$LockFile, [Parameter(Mandatory = $true)][int]$Port)
$ErrorActionPreference = 'Stop'
if ($env:CI -ne 'true' -or $env:ATELIER_RUST_FINAL_CAMPAIGN -ne '1' -or -not $env:RUNNER_TEMP) { throw 'Disposable native desktop final campaign required.' }
$fixtureBase = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
if (-not [IO.Path]::IsPathRooted($LockFile) -or $Port -lt 1 -or $Port -gt 65535 -or -not [IO.Path]::GetFullPath($LockFile).StartsWith($fixtureBase, [StringComparison]::OrdinalIgnoreCase)) { throw 'Synthetic tray scope refused.' }
$failureLog = Join-Path ([IO.Path]::GetDirectoryName($LockFile)) 'tray-qualification-error.log'
trap {
  $details = $_ | Out-String
  $details.Substring(0, [Math]::Min($details.Length, 32768)) | Set-Content -LiteralPath $failureLog -Encoding UTF8
  try {
    $processes = Get-CimInstance Win32_Process -Filter "Name='explorer.exe' OR Name='atelier.exe'" -OperationTimeoutSec 3 -ErrorAction Stop |
      Where-Object { $_.Name -eq 'explorer.exe' -or ($_.ExecutablePath -and $_.ExecutablePath.StartsWith($fixtureBase, [StringComparison]::OrdinalIgnoreCase)) } |
      Select-Object -First 32 ProcessId, ParentProcessId, SessionId, Name, ExecutablePath
    $details += "`nNative process context (qualification PID $PID):`n" + ($processes | Format-List | Out-String)
    $details.Substring(0, [Math]::Min($details.Length, 32768)) | Set-Content -LiteralPath $failureLog -Encoding UTF8
    try {
      $controls = @($processes | Where-Object { $_.Name -eq 'explorer.exe' } | Select-Object -First 4 | ForEach-Object { Get-TrayControlContext $_.ProcessId })
      $details += "`nNative Explorer controls:`n" + ($controls | Format-List | Out-String)
    } catch {
      $details += "`nNative Explorer controls unavailable.`n"
    }
  } catch {
    $details += "`nNative process context unavailable.`n"
  }
  $details.Substring(0, [Math]::Min($details.Length, 32768)) | Set-Content -LiteralPath $failureLog -Encoding UTF8
  exit 1
}
# ASCII source keeps the Windows PowerShell 5.1 helper independent of the runner code page.
$readyName = 'Atelier Kanban : Pr' + [char]0x00ea + 't / Ready'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
function Format-TrayControlContext($Current) {
  $record = [ordered]@{ ProcessId = $Current.ProcessId; Type = $Current.ControlType.ProgrammaticName; Offscreen = $Current.IsOffscreen }
  foreach ($field in @('Name', 'ClassName', 'AutomationId')) {
    $text = [string]$Current.$field
    $record[$field] = $text.Substring(0, [Math]::Min($text.Length, 256))
  }
  return [pscustomobject]$record
}
function Get-TrayControlContext([int]$ExplorerId) {
  $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $ExplorerId)
  $elements = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
  $records = for ($index = 0; $index -lt [Math]::Min($elements.Count, 512); $index++) {
    Format-TrayControlContext ($elements[$index].Current)
  }
  'Explorer PID {0}: {1} controls found, {2} inspected.' -f $ExplorerId, $elements.Count, [Math]::Min($elements.Count, 512)
  $records | Sort-Object @{ Expression = { $_.Name -like 'Atelier*' }; Descending = $true } | Select-Object -First 128
}
function Find-OwnedTrayControl([string]$Name, [System.Windows.Automation.ControlType]$Type, [string]$AlternativeName) {
  $condition = [System.Windows.Automation.AndCondition]::new(
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, $Name, [System.Windows.Automation.PropertyConditionFlags]::IgnoreCase),
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, $Type))
  if ($AlternativeName) {
    $condition = [System.Windows.Automation.AndCondition]::new(
      [System.Windows.Automation.OrCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, $Name, [System.Windows.Automation.PropertyConditionFlags]::IgnoreCase),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, $AlternativeName, [System.Windows.Automation.PropertyConditionFlags]::IgnoreCase)),
      [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, $Type))
  }
  $found = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
  if ($found.Count -gt 1) { throw 'Ambiguous native tray control; no action sent.' }
  if ($found.Count -eq 1) { return $found[0] }
  return $null
}
function Wait-TrayControl([string]$Name, [System.Windows.Automation.ControlType]$Type) {
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  do {
    $control = Find-OwnedTrayControl $Name $Type
    if ($null -ne $control) { return $control }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Native tray control not observed: $Name"
}
function Invoke-TrayControl($Control) {
  $pattern = $Control.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
  ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
}
function Wait-TrayButton([string]$Name) {
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  $pendingPanelPolls = 0
  do {
    $button = Find-OwnedTrayControl $Name ([System.Windows.Automation.ControlType]::Button)
    if ($null -ne $button -and -not $button.Current.IsOffscreen) { return $button }
    # Opening the browser dismisses the notification overflow while the action completes.
    $initial = Find-OwnedTrayControl 'Atelier Kanban' ([System.Windows.Automation.ControlType]::Button) $Name
    if ($null -ne $initial -and -not $initial.Current.IsOffscreen) {
      $pendingPanelPolls = 0
    } elseif ($pendingPanelPolls -gt 0) {
      $pendingPanelPolls--
    } else {
      $overflow = Find-OwnedTrayControl 'Show hidden icons' ([System.Windows.Automation.ControlType]::Button)
      # Allow the flyout to appear, then recover if focus dismissed it before observation.
      if ($null -ne $overflow) { Invoke-TrayControl $overflow; $pendingPanelPolls = 5 }
    }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Native tray button not observed: $Name"
}
function Confirm-TrayQuit([string]$LockFile, [int]$Port) {
  # This guarded disposable runner has one installed Atelier instance; ambiguity refuses any click.
  $button = Wait-TrayButton 'Atelier Kanban'
  Invoke-TrayControl $button
  $open = Wait-TrayControl 'Ouvrir Atelier / Open Atelier' ([System.Windows.Automation.ControlType]::MenuItem)
  Invoke-TrayControl $open
  # Windows opens the authenticated URL directly; only Unix creates local-browser.html.
  $openedConnection = [Net.Sockets.TcpClient]::new()
  try {
    if (-not $openedConnection.ConnectAsync([Net.IPAddress]::Loopback, $Port).Wait(5000) -or -not $openedConnection.Connected) { throw 'Native Open lost the local server.' }
  } finally { $openedConnection.Dispose() }
  $button = Wait-TrayButton $readyName
  Invoke-TrayControl $button
  $quit = Wait-TrayControl 'Quitter Atelier / Quit Atelier' ([System.Windows.Automation.ControlType]::MenuItem)
  Invoke-TrayControl $quit
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  $closed = $false
  do {
    $connection = [Net.Sockets.TcpClient]::new()
    try {
      $connected = $connection.ConnectAsync([Net.IPAddress]::Loopback, $Port)
      if ($connected.Wait(1000)) { $connected.GetAwaiter().GetResult() }
    } catch {
      $failure = $_.Exception.GetBaseException()
      if ($failure -isnot [Net.Sockets.SocketException] -or $failure.SocketErrorCode -ne [Net.Sockets.SocketError]::ConnectionRefused) { throw }
      $closed = $true
    } finally { $connection.Dispose() }
    if ($closed) {
      try {
        $released = [IO.File]::Open($LockFile, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        $released.Dispose()
        break
      } catch [IO.IOException] { $closed = $false }
    }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  if (-not $closed) { throw 'The native Quit menu did not close HTTP and release the data lock.' }
  $deadline = [DateTime]::UtcNow.AddSeconds(10)
  while ($null -ne (Find-OwnedTrayControl 'Atelier Kanban' ([System.Windows.Automation.ControlType]::Button)) -or $null -ne (Find-OwnedTrayControl $readyName ([System.Windows.Automation.ControlType]::Button))) {
    if ([DateTime]::UtcNow -ge $deadline) { throw 'Native tray icon remained after Quit.' }
    Start-Sleep -Milliseconds 100
  }
}

Confirm-TrayQuit $LockFile $Port
