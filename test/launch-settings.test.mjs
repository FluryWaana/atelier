import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import childProcess from 'node:child_process';
import { EventEmitter } from 'node:events';
import { syncBuiltinESMExports } from 'node:module';
import { createDevServers, serviceDirectory } from '../lib/dev-servers.mjs';
import { projectSettings } from '../lib/project-settings.mjs';

test('launch settings preserve legacy commands and validate service directories', () => {
  const legacy = projectSettings({ frontCommand: 'cd front && npm run dev' });
  assert.equal(legacy.frontDirectory, '');
  assert.equal(legacy.backDirectory, '');
  assert.equal(legacy.frontCommand, 'cd front && npm run dev');
  const settings = projectSettings({ frontDirectory: ' front ', backDirectory: 'src/Api' });
  assert.equal(settings.frontDirectory, 'front');
  assert.equal(settings.backDirectory, 'src/Api');
  for (const value of [42, [], 'x'.repeat(2001)]) assert.throws(() => projectSettings({ backDirectory: value }), /invalide/);
  for (const value of ['../back', '..\\back', '/back', 'C:\\back', 'C:back', '\\\\server\\share']) assert.throws(() => projectSettings({ frontDirectory: value }), /relatif/);
});

test('front and back use their worktree subdirectories and validate both before spawning', async t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-launch-settings-'));
  const calls = [], servers = createDevServers(() => {});
  const spawn = t.mock.method(childProcess, 'spawn', (...args) => {
    calls.push(args);
    const child = new EventEmitter();
    child.stdout = new EventEmitter(); child.stderr = new EventEmitter();
    child.stdout.setEncoding = child.stderr.setEncoding = () => {};
    queueMicrotask(() => child.emit('close', 0));
    return child;
  });
  syncBuiltinESMExports();
  try {
    mkdirSync(join(root, 'front')); mkdirSync(join(root, 'back'));
    writeFileSync(join(root, 'file'), 'not a directory');
    const card = { id: 'card', worktree: { path: root } };
    const project = { id: 'project', ...projectSettings({ frontDirectory: 'front', frontCommand: 'npm run dev', backDirectory: 'back', backCommand: 'dotnet run' }) };
    assert.equal(serviceDirectory(root), serviceDirectory(root, '.'));
    for (const directory of ['..', root, 'C:\\elsewhere', 'file', 'missing']) assert.throws(() => serviceDirectory(root, directory));
    assert.throws(() => servers.start(card, { ...project, backDirectory: 'missing' }));
    assert.equal(calls.length, 0, 'invalid back must not leave the front running');
    assert.deepEqual(servers.state(), []);
    const session = servers.start(card, project);
    assert.deepEqual(session.services.map(service => service.repo), [serviceDirectory(root, 'front'), serviceDirectory(root, 'back')]);
    assert.deepEqual(calls.map(([, , options]) => options.cwd), session.services.map(service => service.repo));
    assert.ok(calls[0][1].some(argument => argument.includes('npm run dev')));
    assert.ok(calls[1][1].some(argument => argument.includes('dotnet run')));
    await new Promise(resolve => setImmediate(resolve));
  } finally {
    spawn.mock.restore(); syncBuiltinESMExports();
    await servers.close(); rmSync(root, { recursive: true, force: true });
  }
});
