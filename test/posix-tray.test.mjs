import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

test('Linux indicator handles menu actions, refused shutdown and EOF on a virtual desktop', { skip: process.platform !== 'linux', timeout: 30000 }, () => {
  execFileSync('dbus-run-session', ['--', 'xvfb-run', '-a', '/usr/bin/python3', '-B', fileURLToPath(new URL('./linux-tray-check.py', import.meta.url))], { timeout: 25000, stdio: 'pipe' });
});

test('macOS menu bar helper compiles and exits when its server disconnects', { skip: process.platform !== 'darwin', timeout: 60000 }, async t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-mac-tray-'));
  let child;
  t.after(() => {
    if (child?.exitCode === null) child.kill();
    assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep));
    rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  });
  mkdirSync(join(root, 'scripts')); mkdirSync(join(root, 'public'));
  copyFileSync(new URL('../public/atelier-kanban.png', import.meta.url), join(root, 'public', 'atelier-kanban.png'));
  const binary = join(root, 'scripts', 'MacTray');
  execFileSync('xcrun', ['swiftc', '-swift-version', '5', '-framework', 'AppKit', '-o', binary, fileURLToPath(new URL('../scripts/MacTray.swift', import.meta.url))], { timeout: 45000 });
  child = spawn(binary, [], { stdio: ['pipe', 'ignore', 'pipe'] });
  let error = '';
  child.stderr.on('data', chunk => { error += chunk; });
  const closed = new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
  await delay(1000);
  assert.equal(child.exitCode, null, error);
  child.stdin.end();
  assert.equal(await closed, 0, error);
});
