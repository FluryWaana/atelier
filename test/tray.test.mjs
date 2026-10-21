import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';
import { startTray } from '../lib/infrastructure/tray.mjs';

test('each desktop starts its helper with the server port and closes it before shutdown', async t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-tray-dispatch-'));
  t.after(() => { assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep)); rmSync(root, { recursive: true, force: true }); });
  mkdirSync(join(root, 'scripts'));
  for (const [platform, name] of [['win32', 'WindowsTray.exe'], ['darwin', 'MacTray'], ['linux', 'linux-tray.py']]) {
    writeFileSync(join(root, 'scripts', name), 'fixture');
    const env = { WAYLAND_DISPLAY: 'wayland-0', DBUS_SESSION_BUS_ADDRESS: 'unix:path=/test/bus', LOCAL_SETTING: 'preserved' };
    const child = new EventEmitter();
    child.stdin = new EventEmitter();
    let ended = false, stopped = false;
    child.stdin.end = () => { ended = true; };
    const stop = startTray(root, 54321, { platform, env, spawnProcess: (command, args, options) => {
      assert.equal(command, platform === 'linux' ? '/usr/bin/python3' : join(root, 'scripts', name));
      assert.deepEqual(args, platform === 'linux' ? ['-B', join(root, 'scripts', name)] : []);
      assert.deepEqual(options.env, { ...env, PORT: '54321' });
      assert.deepEqual(options.stdio, ['pipe', 'ignore', 'inherit']);
      assert.equal(options.cwd, root);
      assert.equal(options.shell, undefined);
      return child;
    } });
    const stopping = stop().then(() => { stopped = true; });
    assert.equal(ended, true);
    await Promise.resolve();
    assert.equal(stopped, false, 'Updates must wait until the helper releases its executable.');
    child.emit('close', 0);
    await stopping;
  }
  for (const options of [
    { platform: 'linux', env: {} },
    { platform: 'linux', env: { DISPLAY: ':0' } },
    { platform: 'linux', env: { DBUS_SESSION_BUS_ADDRESS: 'bus' } },
    { platform: 'unsupported', env: {} },
  ]) await startTray(root, 54321, { ...options, spawnProcess: () => assert.fail('No helper should start without a desktop session.') })();
  await startTray(join(root, 'missing'), 54321, { platform: 'darwin', spawnProcess: () => assert.fail('Source checkouts need no compiled helper.') })();
});
