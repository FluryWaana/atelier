import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { DatabaseSync } from 'node:sqlite';
import { execFileSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { dataDirectory, installationMode, updateInstalledVersion } from '../lib/infrastructure/installation.mjs';
import { importPortableData } from '../lib/infrastructure/portable-import.mjs';
import { createPackageUpdater } from '../lib/infrastructure/package-updater.mjs';
import { openStore } from '../lib/store.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'atelier-install-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

test('portable copies keep their data, installed copies use user storage and Linux respects absolute XDG paths', t => {
  const root = fixture(t), home = join(root, 'home');
  assert.equal(installationMode(root), 'portable');
  assert.equal(dataDirectory(root), join(root, 'data'));
  writeFileSync(join(root, 'installation.json'), '{"mode":"user"}');
  assert.equal(dataDirectory(root, { platform: 'win32', home, env: {} }), join(home, 'AppData', 'Local', 'AtelierKanban', 'data'));
  assert.equal(dataDirectory(root, { platform: 'linux', home, env: { XDG_DATA_HOME: 'relative' } }), join(home, '.local', 'share', 'atelier-kanban'));
  assert.equal(dataDirectory(root, { platform: 'linux', home, env: { XDG_DATA_HOME: home } }), join(home, 'atelier-kanban'));
  writeFileSync(join(root, 'installation.json'), '{"mode":"system"}');
  assert.equal(createPackageUpdater({ root }), null);
  writeFileSync(join(root, 'installation.json'), '{"mode":"invalid"}');
  assert.throws(() => dataDirectory(root), /invalide/);
});

test('portable import snapshots committed SQLite WAL, preserves source and refuses to overwrite existing data', t => {
  const root = fixture(t), source = join(root, 'source'), destination = join(root, 'user', 'data');
  mkdirSync(source);
  const store = openStore(join(source, 'kanban.sqlite'));
  try {
    store.putCard({ id: 'preserved', title: 'À conserver', status: 'todo' });
    writeFileSync(join(source, 'attachment.txt'), 'document');
    for (const name of ['local-session', 'local-browser.html']) writeFileSync(join(source, name), 'expired session');
    importPortableData(source, destination);
    assert.equal(store.card('preserved').title, 'À conserver');
    assert.equal(readFileSync(join(destination, 'attachment.txt'), 'utf8'), 'document');
    for (const name of ['local-session', 'local-browser.html']) assert.equal(existsSync(join(destination, name)), false);
    const db = new DatabaseSync(join(destination, 'kanban.sqlite'), { readOnly: true });
    try { assert.match(db.prepare('SELECT data FROM cards WHERE id = ?').get('preserved').data, /À conserver/); }
    finally { db.close(); }
    assert.equal(readdirSync(join(destination, 'backups')).length, 1);
    assert.throws(() => importPortableData(source, destination), /écrasera pas/);
    assert.throws(() => importPortableData(source, source), /distinct/);
    assert.throws(() => importPortableData(source, join(source, 'nested')), /distinct/);
    assert.throws(() => importPortableData(source, join(source, '..nested')), /distinct/);
  } finally { store.db.close(); }
});

test('only native Windows installations synchronize their registered version, with literal arguments', t => {
  const root = fixture(t), calls = [];
  const execute = (...args) => calls.push(args);
  updateInstalledVersion(root, '1.2.3', { platform: 'win32', execute });
  writeFileSync(join(root, 'installation.json'), '{"mode":"user"}');
  updateInstalledVersion(root, '1.2.3', { platform: 'linux', execute });
  assert.equal(calls.length, 0);
  for (const version of ['1.2.3', '1.2.2']) updateInstalledVersion(root, version, { platform: 'win32', execute });
  assert.equal(calls.length, 2);
  for (const [index, [command, args, options]] of calls.entries()) {
    assert.match(command, /powershell\.exe$/);
    assert.deepEqual(args.slice(0, 3), ['-NoProfile', '-NonInteractive', '-Command']);
    assert.equal(options.env.ATELIER_INSTALL_ROOT, resolve(root));
    assert.equal(options.env.ATELIER_INSTALL_VERSION, index ? '1.2.2' : '1.2.3');
    assert.equal(options.windowsHide, true);
    const appId = readFileSync(new URL('../scripts/installers/windows.iss', import.meta.url), 'utf8').match(/AppId=\{(\{[^}]+\})/)[1];
    assert.ok(args[3].includes(appId + '_is1'));
    assert.ok(!args[3].includes(root));
  }
  assert.throws(() => updateInstalledVersion(root, '1.2.3', { platform: 'win32', execute: () => { throw new Error('Access denied'); } }), /Access denied/);
});

test('Windows version synchronization updates only the matching registration and supports rollback', { skip: process.platform !== 'win32' }, t => {
  const root = fixture(t);
  writeFileSync(join(root, 'installation.json'), '{"mode":"user"}');
  const key = 'Software\\AtelierKanbanTests\\' + randomUUID();
  const powershell = join(process.env.SystemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
  const run = script => execFileSync(powershell, ['-NoProfile', '-NonInteractive', '-Command',
    "$ErrorActionPreference = 'Stop'; $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryView]::Registry64); try { " + script + ' } finally { $base.Dispose() }'],
  { windowsHide: true, timeout: 10000, encoding: 'utf8', env: { ...process.env, ATELIER_TEST_KEY: key, ATELIER_TEST_ROOT: root } }).trim();
  t.after(() => run('$base.DeleteSubKeyTree($env:ATELIER_TEST_KEY, $false)'));
  const execute = (command, args, options) => {
    const script = args.at(-1).replace('Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{B1FD8B3A-76DE-4DF8-96B2-502E5F10C99F}_is1', key);
    assert.notEqual(script, args.at(-1), 'Registry writes must target only the disposable test key.');
    return execFileSync(command, [...args.slice(0, -1), script], options);
  };
  const version = () => run('$key = $base.OpenSubKey($env:ATELIER_TEST_KEY); if ($null -ne $key) { try { $key.GetValue("DisplayVersion") } finally { $key.Dispose() } }');
  updateInstalledVersion(root, '1.2.3', { execute });
  assert.equal(version(), '', 'Missing registrations must not be created.');
  run('$key = $base.CreateSubKey($env:ATELIER_TEST_KEY); try { $key.SetValue("InstallLocation", $env:ATELIER_TEST_ROOT + "-other"); $key.SetValue("DisplayVersion", "1.0.0") } finally { $key.Dispose() }');
  updateInstalledVersion(root, '1.2.3', { execute });
  assert.equal(version(), '1.0.0', 'Another installation must not be modified.');
  run('$key = $base.OpenSubKey($env:ATELIER_TEST_KEY, $true); try { $key.SetValue("InstallLocation", $env:ATELIER_TEST_ROOT.ToUpperInvariant() + "\\") } finally { $key.Dispose() }');
  updateInstalledVersion(root, '1.2.3', { execute });
  assert.equal(version(), '1.2.3');
  updateInstalledVersion(root, '1.0.0', { execute });
  assert.equal(version(), '1.0.0');
  run('$key = $base.OpenSubKey($env:ATELIER_TEST_KEY, $true); try { $key.DeleteValue("InstallLocation") } finally { $key.Dispose() }');
  updateInstalledVersion(root, '1.2.3', { execute });
  assert.equal(version(), '1.0.0');
});

test('invalid imports never leave a partial destination', t => {
  const root = fixture(t), source = join(root, 'source'), destination = join(root, 'data');
  mkdirSync(source); writeFileSync(join(source, 'kanban.sqlite'), 'corrupted');
  assert.throws(() => importPortableData(source, destination));
  assert.equal(existsSync(destination), false);
  assert.deepEqual(readdirSync(root), ['source']);
  rmSync(join(source, 'kanban.sqlite'));
  symlinkSync(root, join(source, 'kanban.sqlite'), process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => importPortableData(source, destination), /invalide/);
});
