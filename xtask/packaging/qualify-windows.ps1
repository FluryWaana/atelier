param([Parameter(Mandatory = $true)][string]$Delivery)
$ErrorActionPreference = 'Stop'
if ($env:CI -ne 'true' -or $env:ATELIER_RUST_FINAL_CAMPAIGN -ne '1' -or -not $env:RUNNER_TEMP) { throw 'Disposable final CI campaign required.' }
$deliveryRoot = [IO.Path]::GetFullPath($Delivery)
$setups = @(Get-ChildItem -LiteralPath (Join-Path $deliveryRoot 'installers') -Filter '*-setup.exe' -File)
if ($setups.Count -ne 1) { throw 'Exactly one native Setup is required.' }
$registration = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{1E47A40D-620A-4815-AED7-B4CB019B4BB9}_is1'
if (Test-Path -LiteralPath $registration) { throw 'Existing Rust registration preserved; runner is not disposable.' }
$fixtureBase = [IO.Path]::GetFullPath($env:RUNNER_TEMP)
$fixture = Join-Path $fixtureBase ('atelier-setup-qualification-' + [Guid]::NewGuid().ToString('N'))
$app = Join-Path $fixture 'installed'
$data = Join-Path $fixture 'synthetic-data'
New-Item -ItemType Directory -Path $fixture | Out-Null
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()
$launched = $false
function Invoke-Owned([string]$File, [string[]]$Arguments, [bool]$Success = $true) {
  $start = [Diagnostics.ProcessStartInfo]::new()
  $start.FileName = $File
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
  foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
  $process = [Diagnostics.Process]::Start($start)
  try {
    if (-not $process.WaitForExit(120000)) {
      $process.Kill($true)
      $process.WaitForExit()
      throw 'Owned qualification command timed out.'
    }
    if ($Success -ne ($process.ExitCode -eq 0)) { throw "Unexpected qualification exit: $($process.ExitCode) ($File)" }
  } finally { $process.Dispose() }
}
$silent = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-')
try {
  # A partial/foreign root must survive a refused installation.
  $partial = Join-Path $fixture 'partial-installation'
  New-Item -ItemType Directory -Path $partial | Out-Null
  $partialNote = Join-Path $partial 'human-note.txt'
  [IO.File]::WriteAllText($partialNote, 'preserve-partial-installation')
  Invoke-Owned $setups[0].FullName ($silent + @("/DIR=$partial", "/LOG=$(Join-Path $deliveryRoot 'setup-partial-refusal.log')")) $false
  if ([IO.File]::ReadAllText($partialNote) -ne 'preserve-partial-installation') { throw 'Partial installation was modified.' }
  Invoke-Owned $setups[0].FullName ($silent + @("/DIR=$app", "/LOG=$(Join-Path $deliveryRoot 'setup-install.log')"))
  $binary = Join-Path $app 'atelier.exe'
  Invoke-Owned $binary @('verify-installation')
  Invoke-Owned $binary @('launch', '--data-dir', $data, '--port', "$port", '--no-browser')
  $launched = $true
  $uninstallers = @(Get-ChildItem -LiteralPath $app -Filter 'unins*.exe' -File)
  if ($uninstallers.Count -ne 1) { throw 'Native uninstaller missing or ambiguous.' }
  Invoke-Owned $uninstallers[0].FullName ($silent + @("/LOG=$(Join-Path $deliveryRoot 'uninstall-running-refusal.log')")) $false
  Invoke-Owned $setups[0].FullName ($silent + @("/DIR=$app", "/LOG=$(Join-Path $deliveryRoot 'setup-running-refusal.log')")) $false
  $nativePowerShell = Join-Path ([Environment]::GetFolderPath('System')) 'WindowsPowerShell/v1.0/powershell.exe'
  Invoke-Owned $nativePowerShell @('-NoLogo', '-NoProfile', '-NonInteractive', '-STA', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'qualify-tray-windows.ps1'), '-LockFile', (Join-Path $data '.atelier-data-lock.sqlite'), '-Port', "$port")
  $launched = $false
  # Reuse the signed successor and existing updater on the actual Setup installation.
  # The helper reads the synthetic DPAPI session internally; no token enters PowerShell.
  Invoke-Owned 'cargo' @('xtask', 'qualify-installed-update', $app, $data, "$port", $deliveryRoot)
  $upgradeReport = Get-Content -LiteralPath (Join-Path $deliveryRoot 'native-installed-upgrade-qualification.json') -Raw | ConvertFrom-Json
  if (-not $upgradeReport.runtimeVersionObserved -or -not $upgradeReport.conversationPreserved -or -not $upgradeReport.previousSlotRetained -or -not $upgradeReport.stopped) { throw 'Installed upgrade qualification incomplete.' }
  $database = Join-Path $data 'kanban.sqlite'
  if (-not (Test-Path -LiteralPath $database)) { throw 'Synthetic database missing after native launch.' }
  $databaseHash = (Get-FileHash -LiteralPath $database -Algorithm SHA256).Hash
  $current = Get-Content -LiteralPath (Join-Path $app 'current.json') -Raw | ConvertFrom-Json
  if ($current.active.version -ne $upgradeReport.toVersion -or $current.previous.version -ne $upgradeReport.fromVersion) { throw 'Installed upgrade history changed before uninstall.' }
  $upgradedSlot = Join-Path (Join-Path $app 'versions') $current.active.id
  $upgradedReceipt = Join-Path (Join-Path $app 'receipts') ($current.active.id + '.json')
  if (-not (Test-Path -LiteralPath $upgradedReceipt)) { throw 'Upgraded signed receipt missing.' }
  $signedReadme = Join-Path (Join-Path (Join-Path $app 'versions') $current.active.id) 'README.txt'
  $original = [IO.File]::ReadAllBytes($signedReadme)
  [IO.File]::AppendAllText($signedReadme, 'synthetic human modification')
  Invoke-Owned $uninstallers[0].FullName ($silent + @("/LOG=$(Join-Path $deliveryRoot 'uninstall-modified-refusal.log')")) $false
  if (-not [IO.File]::ReadAllText($signedReadme).Contains('synthetic human modification')) { throw 'Human edit was removed.' }
  [IO.File]::WriteAllBytes($signedReadme, $original)
  $humanNote = Join-Path $app 'human-untracked-note.txt'
  [IO.File]::WriteAllText($humanNote, 'preserve-untracked')
  Invoke-Owned $uninstallers[0].FullName ($silent + @("/LOG=$(Join-Path $deliveryRoot 'uninstall.log')"))
  if (Test-Path -LiteralPath $binary) { throw 'Owned bootstrap remained after native uninstall.' }
  if ([IO.File]::ReadAllText($humanNote) -ne 'preserve-untracked') { throw 'Untracked file was removed.' }
  if ((Get-FileHash -LiteralPath $database -Algorithm SHA256).Hash -ne $databaseHash) { throw 'Uninstall changed the synthetic database.' }
  if (Test-Path -LiteralPath $registration) { throw 'Native uninstall registration remained.' }
  # Inno never owned slots created by the Rust updater; retain their inactive signed code.
  if (-not (Test-Path -LiteralPath $upgradedSlot) -or -not (Test-Path -LiteralPath $upgradedReceipt)) { throw 'Updater-created signed history was removed unexpectedly.' }
  # The retained human root remains protected; reinstall into a fresh explicit destination.
  $reinstalled = Join-Path $fixture 'reinstalled'
  Invoke-Owned $setups[0].FullName ($silent + @("/DIR=$reinstalled", "/LOG=$(Join-Path $deliveryRoot 'setup-reinstall.log')"))
  Invoke-Owned (Join-Path $reinstalled 'atelier.exe') @('verify-installation')
  $secondUninstaller = @(Get-ChildItem -LiteralPath $reinstalled -Filter 'unins*.exe' -File)
  if ($secondUninstaller.Count -ne 1) { throw 'Reinstalled native uninstaller missing.' }
  Invoke-Owned $secondUninstaller[0].FullName $silent
  $report = [ordered]@{format='atelier-rust-native-installer-qualification/1';platform='windows-x64';setupInstalled=$true;launcherExecuted=$true;activeServerRemovalRefused=$true;activeServerInstallRefused=$true;nativeTrayMenuOpened=$true;nativeTrayOpenCommandCompleted=$true;nativeTrayBrowserSessionPrepared=$true;nativeTrayQuitInvoked=$true;nativeTrayRemoved=$true;dataLockReleasedBeforeRestart=$true;installedUpgradeExecuted=$true;newRuntimeVersionObserved=$true;conversationPreserved=$true;updaterCreatedCodeRetainedInactive=$true;modifiedCodePreserved=$true;partialRootPreserved=$true;uninstallExecuted=$true;untrackedFilePreserved=$true;databasePreserved=$true;reinstallExecuted=$true;fixture=$fixture;published=$false}
  [IO.File]::WriteAllText((Join-Path $deliveryRoot 'native-installer-qualification.json'), ($report | ConvertTo-Json -Depth 5))
} finally {
  if ($launched) {
    try { Invoke-Owned (Join-Path $app 'atelier.exe') @('stop', '--data-dir', $data, '--port', "$port") } catch { Write-Warning $_ }
  }
  # Disposable runner cleanup owns registrations; retained fixture files remain available for examination.
}
