import test from 'node:test';
import assert from 'node:assert/strict';
import { systemProtect } from '../lib/token-vault.mjs';
import { sameRepository } from '../lib/domain/paths.mjs';
import { packageName } from '../lib/domain/releases.mjs';
import { inspectProcesses, descendantPids } from '../lib/process-inspection.mjs';
import { chooseFolder } from '../lib/folders.mjs';

test('repository identity respects case on POSIX and Windows separators', () => {
  assert.equal(sameRepository('C:\\App', 'c:/app', 'win32'), true);
  for (const platform of ['linux', 'darwin']) assert.equal(sameRepository('/work/App', '/work/app', platform), false);
  assert.equal(packageName('1.2.3', 'linux', 'arm64'), 'atelier-kanban-1.2.3-linux-arm64.tar.gz');
  assert.equal(packageName('1.2.3', 'darwin', 'x64'), 'atelier-kanban-1.2.3-macos-x64.tar.gz');
  assert.throws(() => packageName('1.2.3', 'linux', '../evil'));
});

for (const platform of ['linux', 'darwin']) test(`${platform} vault encrypts, authenticates and refuses missing keyrings`, () => {
  let stored = '', locked = false;
  const execute = (command, args, options) => {
    if (locked) return { error: new Error('unavailable') };
    if (args[0] === 'store' || args[0] === '-i') {
      stored = platform === 'darwin' ? options.input.match(/-w ([a-f0-9]{64})/)[1] : options.input;
      assert.equal(args.join(' ').includes(stored), false);
      return { status: 0, stdout: '' };
    }
    return { status: stored ? 0 : platform === 'darwin' ? 44 : 1, stdout: stored };
  };
  const protect = systemProtect({ platform, execute });
  const encoded = protect('secret-token');
  assert.equal(encoded.includes('secret-token'), false);
  assert.equal(protect(encoded, true), 'secret-token');
  assert.notEqual(encoded, protect('secret-token'));
  const changed = JSON.parse(encoded); changed.data = Buffer.from('tampered').toString('base64');
  assert.throws(() => protect(JSON.stringify(changed), true));
  locked = true; assert.throws(() => protect('new-token')); assert.throws(() => protect(encoded, true));
});

test('POSIX inspection associates IPv4 and IPv6 listeners with their process tree', async () => {
  const snapshot = await inspectProcesses('linux', async command => ({ stdout: command === 'ps' ? ' 10 1\n 11 10\n 20 1\n' : 'p11\nn127.0.0.1:4317\np20\nn[::1]:8000\nn*:9000\nninvalid:0\n' }));
  assert.deepEqual(descendantPids(10, snapshot.processes), [10, 11]);
  assert.deepEqual(snapshot.listeners, [{ pid: 11, address: '127.0.0.1', port: 4317 }, { pid: 20, address: '::1', port: 8000 }, { pid: 20, address: '0.0.0.0', port: 9000 }]);
});

test('POSIX folder chooser handles selection, cancellation and absent desktop tools', async () => {
  assert.equal(await chooseFolder('/a space', (command, args, options, done) => {
    assert.equal(command, 'zenity'); assert.ok(args.includes('--filename=/a space/')); done(null, '/chosen\n');
  }, 'linux'), '/chosen');
  assert.equal(await chooseFolder('', (command, args, options, done) => done({ code: 1 }, ''), 'linux'), null);
  await assert.rejects(chooseFolder('', (command, args, options, done) => done({ code: 'ENOENT' }, ''), 'linux'), /Saisissez/);
  assert.equal(await chooseFolder('', (command, args, options, done) => { assert.equal(command, 'osascript'); done(null, ''); }, 'darwin'), null);
});
