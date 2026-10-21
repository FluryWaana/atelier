import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { openBrowser } from '../lib/browser.mjs';

test('Windows opens the default browser with the session URL passed as data and waits for success', async () => {
  const address = 'http://127.0.0.1:4317/#session=test&literal="$(test)"';
  const child = new EventEmitter();
  let finished = false;
  const opening = openBrowser(address, 'win32', (command, args, options) => {
    assert.match(command, /[\\/]WindowsPowerShell[\\/]v1\.0[\\/]powershell\.exe$/);
    assert.ok(args.includes('-NoProfile'));
    assert.match(args.at(-1), /Start-Process -FilePath \$env:ATELIER_BROWSER_URL/);
    assert.match(args.at(-1), /\$ErrorActionPreference = "Stop"/);
    assert.ok(!args.join(' ').includes(address));
    assert.equal(options.env.ATELIER_BROWSER_URL, address);
    assert.equal(options.windowsHide, true);
    assert.equal(options.detached, false);
    assert.equal(options.timeout, 15000);
    assert.equal(options.shell, undefined);
    return child;
  }).then(() => { finished = true; });
  child.emit('spawn');
  await Promise.resolve();
  assert.equal(finished, false);
  child.emit('close', 0);
  await opening;
});

for (const event of ['error', 'failure', 'timeout']) test(`browser ${event} is reported without exposing the session`, async () => {
  const child = new EventEmitter();
  const opening = openBrowser('http://127.0.0.1:4317/#session=secret', 'win32', () => child);
  if (event === 'error') child.emit('error', new Error('secret'));
  else child.emit('close', event === 'failure' ? 1 : null);
  await assert.rejects(opening, error => {
    assert.match(error.message, /Impossible d’ouvrir le navigateur/);
    assert.ok(!error.message.includes('secret'));
    return true;
  });
});

for (const [platform, command] of [['darwin', 'open'], ['linux', 'xdg-open']]) test(`${platform} keeps its detached browser opener`, async () => {
  const address = 'file:///private/session%20file.html';
  const child = new EventEmitter();
  let unreferenced = false;
  child.unref = () => { unreferenced = true; };
  const opening = openBrowser(address, platform, (binary, args, options) => {
    assert.equal(binary, command);
    assert.deepEqual(args, [address]);
    assert.equal(options.detached, true);
    assert.equal(options.env, undefined);
    return child;
  });
  child.emit('spawn');
  await opening;
  assert.equal(unreferenced, true);
});
