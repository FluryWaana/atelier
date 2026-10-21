import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync, spawnSync } from 'node:child_process';
import { createServer } from 'node:net';
import { setTimeout as delay } from 'node:timers/promises';
import { installerNames } from '../lib/domain/releases.mjs';
import { dataDirectory } from '../lib/infrastructure/installation.mjs';
import { readSession } from '../lib/infrastructure/local-session.mjs';
import { verifyChecksum } from '../lib/infrastructure/package-updater.mjs';

const version = JSON.parse(readFileSync(new URL('../package.json', import.meta.url))).version;
const names = installerNames(version, process.platform, process.arch);
// Installation modifies the package registry; only the explicit installer check opts in.
const enabled = process.env.ATELIER_REQUIRE_INSTALLERS === '1';
for (const name of names) test(`Native installer: ${name}`, { skip: !enabled, timeout: 180000 }, async t => {
  const archive = fileURLToPath(new URL('../dist/' + name, import.meta.url));
  verifyChecksum(readFileSync(archive), readFileSync(archive + '.sha256', 'utf8'), name);
  const root = mkdtempSync(join(tmpdir(), 'atelier-installer-'));
  const env = { ...process.env, LOCALAPPDATA: join(root, 'user'), XDG_DATA_HOME: join(root, 'user') };
  const socket = createServer(); await new Promise(resolve => socket.listen(0, '127.0.0.1', resolve));
  env.PORT = String(socket.address().port); await new Promise(resolve => socket.close(resolve));
  const request = path => fetch(`http://127.0.0.1:${env.PORT}${path}`, { headers: { Connection: 'close', Authorization: 'Bearer ' + readSession(dataDirectory(app, { env })) } });
  const app = process.platform === 'win32' ? join(root, 'app') : join(root, 'opt', 'atelier-kanban');
  const node = join(app, 'runtime', process.platform === 'win32' ? 'node.exe' : 'node');
  const run = (command, args, options = {}) => execFileSync(command, args, { env, windowsHide: true, timeout: 90000, ...options });
  let installed = false, pid, desktopShortcut;
  t.after(async () => {
    if (pid) { try { process.kill(pid); } catch {} await delay(1000); }
    if (installed && process.platform === 'win32' && existsSync(join(app, 'unins000.exe'))) run(join(app, 'unins000.exe'), ['/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART']);
    rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
  });
  if (process.platform === 'win32') {
    const existing = spawnSync('reg.exe', ['query', 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{B1FD8B3A-76DE-4DF8-96B2-502E5F10C99F}_is1'], { windowsHide: true });
    assert.equal(existing.status, 1, 'Une installation existe déjà pour cet utilisateur : utiliser un compte de test.');
    desktopShortcut = run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', "Join-Path ([Environment]::GetFolderPath('Desktop')) 'Atelier Kanban.lnk'"], { encoding: 'utf8' }).trim();
    assert.equal(existsSync(desktopShortcut), false, 'Un raccourci existe déjà : utiliser un compte de test.');
    run(archive, ['/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOICONS', '/TASKS=', '/DIR=' + app]); installed = true;
    const registration = JSON.parse(run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', "Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{B1FD8B3A-76DE-4DF8-96B2-502E5F10C99F}_is1' | Select-Object DisplayName, DisplayVersion | ConvertTo-Json -Compress"], { encoding: 'utf8' }));
    assert.equal(registration.DisplayName, 'Atelier Kanban');
    assert.equal(registration.DisplayVersion, version);
    assert.equal(existsSync(desktopShortcut), false, 'Le raccourci Bureau est facultatif.');
  } else if (name.endsWith('.deb')) {
    run('dpkg-deb', ['-x', archive, root]);
    run('dpkg-deb', ['-e', archive, join(root, 'control')]);
    for (const script of ['preinst', 'prerm']) assert.match(readFileSync(join(root, 'control', script), 'utf8'), /readlink[\s\S]*exit 1/);
  }
  else {
    run('rpm', ['--dbpath', join(root, 'rpmdb'), '--initdb']);
    const scripts = run('rpm', ['--dbpath', join(root, 'rpmdb'), '-qp', '--scripts', archive], { encoding: 'utf8' });
    assert.equal([...scripts.matchAll(/readlink[\s\S]*?exit 1/g)].length, 2, 'Les scripts RPM doivent refuser de remplacer une application en cours.');
    const bytes = run('rpm2cpio', [archive], { maxBuffer: 300 * 1024 * 1024 });
    run('cpio', ['-idmu', '--no-absolute-filenames'], { input: bytes, cwd: root });
  }
  assert.equal(existsSync(join(app, 'data')), false);
  assert.equal(JSON.parse(readFileSync(join(app, 'installation.json'))).mode, process.platform === 'win32' ? 'user' : 'system');
  const launch = () => process.platform === 'win32' ? run(join(app, 'AtelierKanban.exe'), ['--no-browser'], { stdio: 'ignore' }) : run(node, [join(app, 'launcher.mjs'), '--no-browser']);
  launch();
  const health = await (await request('/api/health')).json(); pid = health.pid;
  assert.equal(health.version, version);
  const response = await fetch(`http://127.0.0.1:${env.PORT}/api/cards`, { method: 'POST', headers: { Authorization: 'Bearer ' + readSession(dataDirectory(app, { env })), 'Content-Type': 'application/json', 'X-Local-Request': '1', Connection: 'close' }, body: JSON.stringify({ title: 'Carte conservée après réinstallation', status: 'todo' }) });
  assert.equal(response.ok, true, await response.text());
  const data = dataDirectory(app, { env });
  assert.equal(existsSync(join(data, 'kanban.sqlite')), true);
  assert.equal(existsSync(join(app, 'data')), false);
  const updates = await (await request('/api/updates')).json();
  assert.equal(updates.canInstall, process.platform === 'win32');
  run(node, [join(app, 'launcher.mjs'), '--stop']); pid = null;
  if (process.platform === 'win32') {
    run(archive, ['/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOICONS', '/TASKS=desktopicon', '/DIR=' + app]);
    assert.equal(existsSync(desktopShortcut), true);
    const shortcut = JSON.parse(run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', '$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($env:ATELIER_TEST_SHORTCUT); @{ target = $shortcut.TargetPath; directory = $shortcut.WorkingDirectory } | ConvertTo-Json -Compress'], { encoding: 'utf8', env: { ...env, ATELIER_TEST_SHORTCUT: desktopShortcut } }));
    // Windows runners can expose temporary directories through their short 8.3 names.
    assert.equal(realpathSync.native(shortcut.target).toLowerCase(), realpathSync.native(join(app, 'AtelierKanban.exe')).toLowerCase());
    assert.equal(realpathSync.native(shortcut.directory).toLowerCase(), realpathSync.native(app).toLowerCase());
    launch(); pid = (await (await request('/api/health')).json()).pid;
    const state = await (await request('/api/state')).json();
    assert.equal(state.cards[0].title, 'Carte conservée après réinstallation');
    run(node, [join(app, 'launcher.mjs'), '--stop']); pid = null;
    run(join(app, 'unins000.exe'), ['/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART']); installed = false;
    assert.equal(existsSync(node), false);
    assert.equal(existsSync(desktopShortcut), false);
    assert.equal(existsSync(join(data, 'kanban.sqlite')), true);
  } else {
    assert.match(readFileSync(join(root, 'usr/share/applications/atelier-kanban.desktop'), 'utf8'), /Exec=atelier-kanban/);
    assert.match(readFileSync(join(root, 'usr/bin/atelier-kanban'), 'utf8'), /\/opt\/atelier-kanban/);
    assert.deepEqual(readFileSync(join(root, 'usr/share/icons/hicolor/512x512/apps/atelier-kanban.png')), readFileSync(join(app, 'public/atelier-kanban.png')));
  }
});
