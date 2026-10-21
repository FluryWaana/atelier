import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { startWindowsTray } from '../lib/infrastructure/windows-tray.mjs';

test('Windows tray opens Atelier, requests graceful shutdown and exits when its server disconnects', { skip: process.platform !== 'win32', timeout: 30000 }, async t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-tray-'));
  const children = [];
  t.after(async () => {
    for (const child of children) if (child.exitCode === null) child.kill();
    assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep));
    rmSync(root, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
  });
  for (const name of ['scripts', 'runtime', 'public']) mkdirSync(join(root, name));
  copyFileSync(process.execPath, join(root, 'runtime', 'node.exe'));
  copyFileSync(new URL('../public/atelier-kanban.ico', import.meta.url), join(root, 'public', 'atelier-kanban.ico'));
  writeFileSync(join(root, 'launcher.mjs'), "import { appendFileSync } from 'node:fs'; appendFileSync(new URL('./actions', import.meta.url), (process.argv[2] || 'open') + ':' + process.env.PORT + '\\n');");
  const source = new URL('../scripts/WindowsTray.cs', import.meta.url);
  copyFileSync(source, join(root, 'scripts', 'WindowsTray.cs'));
  const compiler = join(process.env.SystemRoot, 'Microsoft.NET', 'Framework64', 'v4.0.30319', 'csc.exe');
  const binary = join(root, 'scripts', 'WindowsTray.exe');
  const compile = (output, extra = []) => execFileSync(compiler, ['/nologo', '/target:winexe', '/reference:System.Windows.Forms.dll', '/reference:System.Drawing.dll', '/out:' + output, join(root, 'scripts', 'WindowsTray.cs'), ...extra], { windowsHide: true });
  compile(binary);
  const launch = (file, args = []) => {
    const child = spawn(file, args, { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, PORT: '54321' } });
    children.push(child);
    child.stderr.on('data', chunk => t.diagnostic(chunk.toString()));
    const closed = new Promise((resolve, reject) => { child.once('error', reject); child.once('close', code => resolve(code)); });
    return { child, closed };
  };
  const { child, closed } = launch(binary);
  await delay(300);
  assert.equal(child.exitCode, null, 'The icon remains alive while its server owns stdin.');
  child.stdin.end();
  assert.equal(await closed, 0, 'EOF removes the icon even after an unexpected server exit.');

  const stop = startWindowsTray(root, 54321);
  await delay(300);
  await stop();
  await startWindowsTray(join(root, 'missing'), 54321)();

  const harness = join(root, 'scripts', 'TrayCheck.cs');
  writeFileSync(harness, `using System;
using System.IO;
using System.Reflection;
using System.Windows.Forms;
class TrayCheck {
  [STAThread] static void Main() {
    using(var tray = new WindowsTray()) {
      var menu = (ContextMenuStrip)typeof(WindowsTray).GetField("menu", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(tray);
      var icon = (NotifyIcon)typeof(WindowsTray).GetField("icon", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(tray);
      string actions = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "..", "actions");
      int step = 0;
      using(var timer = new Timer { Interval = 100 }) {
        timer.Tick += (sender, args) => {
          int count = File.Exists(actions) ? File.ReadAllLines(actions).Length : 0;
          if(!menu.Enabled || count != step) return;
          if(step == 0) menu.Items[0].PerformClick();
          else if(step == 1) typeof(NotifyIcon).GetMethod("OnDoubleClick", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(icon, new object[] { EventArgs.Empty });
          else if(step == 2) menu.Items[1].PerformClick();
          else { Application.ExitThread(); return; }
          step++;
        };
        timer.Start();
        Application.Run(tray);
      }
    }
  }
}`);
  const check = join(root, 'scripts', 'TrayCheck.exe');
  compile(check, ['/main:TrayCheck', harness]);
  assert.equal(await launch(check).closed, 0);
  assert.equal(readFileSync(join(root, 'actions'), 'utf8'), 'open:54321\nopen:54321\n--stop:54321\n');
});
