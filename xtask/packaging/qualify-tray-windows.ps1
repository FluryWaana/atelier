param([Parameter(Mandatory = $true)][string]$LockFile, [Parameter(Mandatory = $true)][int]$Port)
$ErrorActionPreference = 'Stop'
if ($env:CI -ne 'true' -or $env:ATELIER_RUST_FINAL_CAMPAIGN -ne '1' -or -not $env:RUNNER_TEMP) { throw 'Disposable native desktop final campaign required.' }
$fixtureBase = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
if (-not [IO.Path]::IsPathRooted($LockFile) -or $Port -lt 1 -or $Port -gt 65535 -or -not [IO.Path]::GetFullPath($LockFile).StartsWith($fixtureBase, [StringComparison]::OrdinalIgnoreCase)) { throw 'Synthetic tray scope refused.' }
# ASCII source keeps the Windows PowerShell 5.1 helper independent of the runner code page.
$readyName = 'Atelier Kanban ' + [char]0x2014 + ' Pr' + [char]0x00ea + 't / Ready'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
function Find-OwnedTrayControl([string]$Name, [System.Windows.Automation.ControlType]$Type) {
  $condition = [System.Windows.Automation.AndCondition]::new(
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, $Name),
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, $Type))
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
function Confirm-TrayQuit([string]$LockFile, [int]$Port) {
  # This guarded disposable runner has one installed Atelier instance; ambiguity refuses any click.
  $button = Find-OwnedTrayControl 'Atelier Kanban' ([System.Windows.Automation.ControlType]::Button)
  if ($null -eq $button) {
    $overflow = Find-OwnedTrayControl 'Show hidden icons' ([System.Windows.Automation.ControlType]::Button)
    if ($null -ne $overflow) { Invoke-TrayControl $overflow }
    $button = Wait-TrayControl 'Atelier Kanban' ([System.Windows.Automation.ControlType]::Button)
  }
  Invoke-TrayControl $button
  $open = Wait-TrayControl 'Ouvrir Atelier / Open Atelier' ([System.Windows.Automation.ControlType]::MenuItem)
  Invoke-TrayControl $open
  $button = Wait-TrayControl $readyName ([System.Windows.Automation.ControlType]::Button)
  # Windows opens the authenticated URL directly; only Unix creates local-browser.html.
  $openedConnection = [Net.Sockets.TcpClient]::new()
  try {
    if (-not $openedConnection.ConnectAsync([Net.IPAddress]::Loopback, $Port).Wait(5000) -or -not $openedConnection.Connected) { throw 'Native Open lost the local server.' }
  } finally { $openedConnection.Dispose() }
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
