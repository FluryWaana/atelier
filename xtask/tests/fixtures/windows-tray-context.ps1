param([string]$Helper, [string]$Fixture)
$ErrorActionPreference = 'Stop'
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($Helper, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors -join ', ') }
$trap = $ast.Find({ param($node) $node -is [Management.Automation.Language.TrapStatementAst] }, $true)
$format = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Format-TrayControlContext' }, $true)
. ([scriptblock]::Create($format.Extent.Text))
$metadata = [pscustomobject]@{ ProcessId=42; ControlType=[pscustomobject]@{ProgrammaticName='ControlType.Button'}; IsOffscreen=$true; Name=('n'*400); ClassName=('c'*400); AutomationId=('a'*400) }
$record = Format-TrayControlContext $metadata
if ($record.Name.Length -ne 256 -or $record.ClassName.Length -ne 256 -or $record.AutomationId.Length -ne 256 -or $record.ProcessId -ne 42 -or -not $record.Offscreen) { throw 'Native UI record contract failed.' }
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$constructor = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'New-TrayControlCondition' }, $true)
. ([scriptblock]::Create($constructor.Extent.Text))
function Assert-TrayCondition($Name, $Type, $AlternativeName, [string[]]$ExpectedNames, [string[]]$RejectedNames) {
  $parts = (New-TrayControlCondition $Name $Type $AlternativeName).GetConditions()
  if ($parts.Count -ne 2 -or $parts[1].Property -ne [System.Windows.Automation.AutomationElement]::ControlTypeProperty -or $parts[1].Value -ne $Type.Id) { throw 'Tray control condition lost its exact control type.' }
  $names = @(if ($parts[0] -is [System.Windows.Automation.OrCondition]) { $parts[0].GetConditions() } else { $parts[0] })
  if ($names.Count -ne $ExpectedNames.Count) { throw 'Unexpected tray label aliases.' }
  foreach ($condition in $names) {
    if ($condition.Property -ne [System.Windows.Automation.AutomationElement]::NameProperty -or $condition.Flags -ne [System.Windows.Automation.PropertyConditionFlags]::IgnoreCase -or $ExpectedNames -notcontains $condition.Value) { throw 'Tray label condition lost its exact case-insensitive match.' }
  }
  foreach ($label in @($ExpectedNames + $RejectedNames)) {
    $matched = @($names | Where-Object { [StringComparer]::OrdinalIgnoreCase.Equals([string]$_.Value, $label) }).Count -gt 0
    if ($matched -ne ($ExpectedNames -contains $label)) { throw "Tray label match failed: $label" }
  }
}
$buttonType = [System.Windows.Automation.ControlType]::Button
$menuType = [System.Windows.Automation.ControlType]::MenuItem
$ready = 'Atelier Kanban : Pr' + [char]0x00ea + 't'
$compositeReady = 'Atelier Kanban Atelier Kanban : Pr' + [char]0x00ea + 't'
$rejectedReady = @(($ready + ' other'), ($compositeReady + ' other'), ('Other ' + $ready), ('Atelier Kanban ' + $compositeReady))
Assert-TrayCondition $ready $buttonType '' @($ready, $compositeReady) $rejectedReady
Assert-TrayCondition 'Atelier Kanban' $buttonType '' @('Atelier Kanban', 'Atelier Kanban Atelier Kanban') @($ready, 'Atelier Kanban other')
Assert-TrayCondition 'Atelier Kanban' $buttonType $ready @('Atelier Kanban', 'Atelier Kanban Atelier Kanban', $ready, $compositeReady) $rejectedReady
Assert-TrayCondition 'Show hidden icons' $buttonType '' @('Show Hidden Icons') @('Show Hidden Icons Hide', 'Show Hidden Icons other', 'Atelier Kanban Show hidden icons')
Assert-TrayCondition 'Ouvrir Atelier' $menuType '' @('Ouvrir Atelier') @('Atelier Kanban Ouvrir Atelier')
Assert-TrayCondition $ready $menuType '' @($ready) @($compositeReady)
Assert-TrayCondition 'Atelier Kanban other' $buttonType '' @('Atelier Kanban other') @('Atelier Kanban Atelier Kanban other')
$wait = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-TrayButton' }, $true)
. ([scriptblock]::Create($wait.Extent.Text))
function Find-OwnedTrayControl {
  param($Name, $Type, $AlternativeName)
  if ($Name -eq $script:targetName) {
    $script:poll++
    if ($script:poll -gt 12) { throw 'Synthetic notification panel never recovered.' }
    if ($script:scenario -in @('dismissed', 'dismissed-before-poll') -and $script:poll -eq 2) { $script:panelVisible = $false }
    if ($script:scenario -eq 'pending' -and $script:poll -eq 3) { $script:panelVisible = $true }
    if ($script:panelVisible -and $script:poll -ge $script:readyAt) { return [pscustomobject]@{Name=$Name; Current=[pscustomobject]@{IsOffscreen=$false}} }
  } elseif ($Name -eq 'Atelier Kanban' -and ($script:panelVisible -or $script:scenario -eq 'offscreen')) {
    if ($script:scenario -eq 'renamed' -and $AlternativeName -ne $script:targetName) { throw 'Visible ready label was lost during its transition.' }
    return [pscustomobject]@{Name=$Name; Current=[pscustomobject]@{IsOffscreen=(-not $script:panelVisible)}}
  } elseif ($Name -eq 'Show hidden icons') {
    return [pscustomobject]@{Name='Show Hidden Icons'}
  }
  return $null
}
function Invoke-TrayControl {
  param($Control)
  if ($Control.Name -ne 'Show Hidden Icons') { throw 'Synthetic action escaped the notification panel.' }
  $script:opens++
  if ($script:scenario -ne 'pending') { $script:panelVisible = $true }
}
function Start-Sleep { param($Milliseconds) }
foreach ($scenario in @('immediate', 'closed', 'dismissed', 'dismissed-before-poll', 'pending', 'offscreen', 'renamed')) {
  $script:scenario=$scenario; $script:poll=0; $script:opens=0
  $script:panelVisible=$scenario -in @('immediate', 'dismissed', 'renamed')
  $script:readyAt=if ($scenario -eq 'immediate') { 1 } elseif ($scenario -in @('dismissed', 'dismissed-before-poll', 'pending')) { 4 } else { 3 }
  $script:targetName='Atelier Kanban : synthetic ready'
  $button=Wait-TrayButton $script:targetName
  $expectedOpens=if ($scenario -in @('immediate', 'renamed')) { 0 } elseif ($scenario -eq 'dismissed-before-poll') { 2 } else { 1 }
  if ($button.Name -ne $script:targetName -or $script:opens -ne $expectedOpens) { throw "Notification panel state failed: $scenario" }
}
$prelude = @'
param($Fixture, $Scenario)
$fixtureBase = $Fixture + [IO.Path]::DirectorySeparatorChar
$failureLog = Join-Path $Fixture 'error.log'
function Get-CimInstance {
  param($ClassName, $Filter, $OperationTimeoutSec, $ErrorAction)
  if ($Scenario -eq 'unavailable') { throw 'Synthetic query failure.' }
  [pscustomobject]@{ProcessId=1; ParentProcessId=0; SessionId=1; Name='explorer.exe'; ExecutablePath='C:\Windows\explorer.exe'}
  [pscustomobject]@{ProcessId=2; ParentProcessId=0; SessionId=2; Name='atelier.exe'; ExecutablePath='C:\foreign\private-process.exe'}
  1..40 | ForEach-Object { [pscustomobject]@{ProcessId=(100+$_); ParentProcessId=100; SessionId=1; Name='atelier.exe'; ExecutablePath=(Join-Path $Fixture 'installed\atelier.exe')} }
}
function Get-TrayControlContext {
  param($ExplorerId)
  if (-not [IO.File]::Exists($failureLog) -or -not [IO.File]::ReadAllText($failureLog).Contains('Synthetic original failure')) { throw 'Original error was not persisted before the native UI query.' }
  if ($Scenario -eq 'ui-unavailable') { throw 'Synthetic UI query failure.' }
  if ($ExplorerId -ne 1) { throw 'UI query escaped the Explorer scope.' }
  [pscustomobject]@{Name='Atelier synthetic control'; Offscreen=$true}
}
$_ = 'Synthetic original failure'
if ($Scenario -eq 'long') { $_ += 'x' * 40000 }
'@
$body = $trap.Body.Extent.Text
$child = Join-Path $Fixture 'context-child.ps1'
[IO.File]::WriteAllText($child, $prelude + "`n" + $body.Substring(1, $body.Length-2))
$native = Join-Path ([Environment]::GetFolderPath('System')) 'WindowsPowerShell/v1.0/powershell.exe'
foreach ($scenario in @('owned', 'unavailable', 'ui-unavailable', 'long')) {
  $scope = Join-Path $Fixture $scenario
  [void][IO.Directory]::CreateDirectory($scope)
  & $native -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $child $scope $scenario
  if ($LASTEXITCODE -ne 1) { throw 'Original failure exit lost.' }
  $saved = [IO.File]::ReadAllText((Join-Path $scope 'error.log'))
  if (-not $saved.StartsWith('Synthetic original failure') -or $saved.Length -gt 32770 -or $saved.Contains('private-process')) { throw 'Error context scope or bound failed.' }
  if ($scenario -eq 'owned') {
    $processes = ($saved -split 'Native Explorer controls:')[0]
    if ([regex]::Matches($processes, '(?m)^ProcessId\s*:').Count -ne 32 -or -not $saved.Contains('Atelier synthetic control')) { throw 'Native context count or UI record failed.' }
  }
  if ($scenario -eq 'unavailable' -and -not $saved.Contains('Native process context unavailable')) { throw 'CIM failure masked original error.' }
  if ($scenario -eq 'ui-unavailable' -and (-not $saved.Contains('Native Explorer controls unavailable') -or -not $saved.Contains('explorer.exe'))) { throw 'UI failure masked process evidence.' }
}
& {
  $quit = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Confirm-TrayQuit' }, $true)
  . ([scriptblock]::Create($quit.Extent.Text))
  function Wait-TrayButton { return 'synthetic button' }
  function Wait-TrayControl { param($Name) return $Name }
  function Find-OwnedTrayControl { return $null }
  function Invoke-TrayControl {
    param($Control)
    if ($Control -eq 'Quitter Atelier') { $listener.Stop() }
  }
  $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
  $listener.Start()
  $port = $listener.LocalEndpoint.Port
  $lock = Join-Path $Fixture 'released-synthetic.lock'
  [IO.File]::WriteAllText($lock, 'synthetic')
  try {
    Confirm-TrayQuit $lock $port
    if (-not [IO.File]::Exists($lock)) { throw 'Quit qualification removed the persistent lock file.' }
  } finally { $listener.Stop() }
}
Write-Output 'Native tray evidence contract passed with synthetic processes and UI records.'
