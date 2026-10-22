import { existsSync, readFileSync } from 'node:fs';
import { isAbsolute, join, resolve } from 'node:path';
import { homedir } from 'node:os';
import { execFileSync } from 'node:child_process';

export function installationMode(root) {
  const file = join(root, 'installation.json');
  if (!existsSync(file)) return 'portable';
  const { mode } = JSON.parse(readFileSync(file, 'utf8'));
  if (!['user', 'system'].includes(mode)) throw new Error('Mode d’installation invalide.');
  return mode;
}

export function dataDirectory(root, { platform = process.platform, env = process.env, home = homedir() } = {}) {
  if (installationMode(root) === 'portable') return join(root, 'data');
  if (platform === 'win32') return join(env.LOCALAPPDATA || join(home, 'AppData', 'Local'), 'AtelierKanban', 'data');
  if (platform === 'linux') return join(env.XDG_DATA_HOME && isAbsolute(env.XDG_DATA_HOME) ? env.XDG_DATA_HOME : join(home, '.local', 'share'), 'atelier-kanban');
  throw new Error('Installation native non prise en charge sur ce système.');
}

export function updateInstalledVersion(root, version, { platform = process.platform, execute = execFileSync } = {}) {
  if (platform !== 'win32' || installationMode(root) !== 'user') return;
  const script = String.raw`
$ErrorActionPreference = 'Stop'
$registry = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryView]::Registry64)
$key = $null
try {
  $key = $registry.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\{B1FD8B3A-76DE-4DF8-96B2-502E5F10C99F}_is1', $true)
  if ($null -eq $key) { return }
  $location = [string]$key.GetValue('InstallLocation')
  if (!$location) { return }
  $installed = [IO.Path]::GetFullPath($location).TrimEnd('\')
  $running = [IO.Path]::GetFullPath($env:ATELIER_INSTALL_ROOT).TrimEnd('\')
  if (![string]::Equals($installed, $running, [StringComparison]::OrdinalIgnoreCase)) { return }
  $key.SetValue('DisplayVersion', $env:ATELIER_INSTALL_VERSION, [Microsoft.Win32.RegistryValueKind]::String)
} finally {
  if ($null -ne $key) { $key.Dispose() }
  $registry.Dispose()
}`;
  execute(join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe'),
    ['-NoProfile', '-NonInteractive', '-Command', script], {
      windowsHide: true, timeout: 10000, stdio: 'pipe',
      env: { ...process.env, ATELIER_INSTALL_ROOT: resolve(root), ATELIER_INSTALL_VERSION: version },
    });
}
