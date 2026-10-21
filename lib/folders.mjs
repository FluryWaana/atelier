import { execFile } from 'node:child_process';
import { join } from 'node:path';

// The browser cannot return an absolute folder path; use the local desktop dialog.
export function chooseFolder(initial = '', execute = execFile, platform = process.platform, title = 'Choisir le répertoire du projet') {
  if (platform !== 'win32') {
    const command = platform === 'darwin' ? 'osascript' : 'zenity';
    const args = platform === 'darwin'
      ? ['-e', 'try', '-e', 'POSIX path of (choose folder with prompt "Choisir le répertoire du projet")', '-e', 'on error number -128', '-e', 'return ""', '-e', 'end try']
      : ['--file-selection', '--directory', '--title=' + title, ...(initial ? ['--filename=' + initial + '/'] : [])];
    return new Promise((resolve, reject) => execute(command, args, { encoding: 'utf8', timeout: 300000, maxBuffer: 65536 }, (error, stdout) => {
      if (error && platform === 'linux' && error.code === 1) return resolve(null);
      if (error) return reject(new Error('Sélecteur indisponible. Saisissez le chemin du dossier (Linux : installez Zenity pour le sélecteur).'));
      resolve(stdout.trim() || null);
    }));
  }
  const script = `
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class AtelierFolderFocus {
  [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint from, uint to, bool attach);
  [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr window);
  public static void Activate(IntPtr window) {
    uint process;
    uint foreground = GetWindowThreadProcessId(GetForegroundWindow(), out process);
    uint current = GetCurrentThreadId();
    bool attached = foreground != 0 && foreground != current && AttachThreadInput(current, foreground, true);
    try {
      BringWindowToTop(window);
      SetForegroundWindow(window);
    } finally {
      if (attached) AttachThreadInput(current, foreground, false);
    }
  }
}
'@
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$owner = New-Object System.Windows.Forms.Form
try {
  $dialog.Description = $env:ATELIER_FOLDER_TITLE
  $dialog.ShowNewFolderButton = $false
  if (Test-Path -LiteralPath $env:ATELIER_FOLDER_INITIAL -PathType Container) { $dialog.SelectedPath = $env:ATELIER_FOLDER_INITIAL }
  $owner.TopMost = $true
  $owner.ShowInTaskbar = $false
  $owner.Opacity = 0
  $owner.Show()
  $owner.BringToFront()
  $owner.Activate()
  [AtelierFolderFocus]::Activate($owner.Handle)
  if ($dialog.ShowDialog($owner) -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::Write($dialog.SelectedPath) }
} finally { $dialog.Dispose(); $owner.Dispose() }
`;
  return new Promise((resolve, reject) => {
    execute(join(process.env.SystemRoot || 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe'),
      ['-NoProfile', '-STA', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')],
      { windowsHide: true, shell: false, timeout: 300000, maxBuffer: 65536, encoding: 'utf8', env: { ...process.env, ATELIER_FOLDER_INITIAL: initial, ATELIER_FOLDER_TITLE: title } },
      (error, stdout) => error ? reject(new Error('Le sélecteur a expiré ou n’a pas pu s’ouvrir. Réessayez ou saisissez le chemin.')) : resolve(stdout.trim() || null));
  });
}
