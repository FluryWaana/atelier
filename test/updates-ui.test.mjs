import test from 'node:test';
import assert from 'node:assert/strict';
import { initUpdates } from '../public/updates.mjs';

function fixture(t, api, confirm = async () => true) {
  const nodes = Object.fromEntries(['update-banner', 'update-message', 'check-updates', 'install-update', 'update-notes', 'dismiss-update'].map(id => ['#' + id, { focus() {} }]));
  const previous = globalThis.document;
  globalThis.document = { hidden: false, querySelector: selector => nodes[selector] };
  t.after(() => { if (previous === undefined) delete globalThis.document; else globalThis.document = previous; });
  t.mock.method(globalThis, 'setInterval', () => 0);
  initUpdates({ api, confirm });
  return nodes;
}
test('update banner links to the release and reports a rejected installation without disabling controls permanently', async t => {
  const calls = [];
  const nodes = fixture(t, async (path, method, body) => {
    calls.push({ path, method, body });
    if (path.endsWith('/install')) throw new Error('Un agent travaille.');
    return { configured: true, canInstall: true, available: { version: '1.1.0', notesUrl: 'https://github.com/owner/repo/releases/tag/v1.1.0' } };
  });
  await nodes['#check-updates'].onclick();
  assert.equal(nodes['#update-banner'].hidden, false);
  assert.match(nodes['#update-message'].textContent, /1.1.0/);
  assert.match(nodes['#update-notes'].href, /releases\/tag/);
  await nodes['#install-update'].onclick();
  assert.equal(calls.at(-1).body.version, '1.1.0');
  assert.equal(nodes['#update-message'].textContent, 'Un agent travaille.');
  assert.equal(nodes['#install-update'].disabled, false);
});
test('source copies explain availability without offering automatic installation', async t => {
  const nodes = fixture(t, async () => ({ configured: false, canInstall: false }));
  await nodes['#check-updates'].onclick();
  assert.equal(nodes['#install-update'].hidden, true);
  assert.equal(nodes['#update-notes'].hidden, true);
  assert.match(nodes['#update-message'].textContent, /version distribuée/);
});

function installation(t, result) {
  t.mock.method(globalThis, 'setTimeout', callback => { callback(); return 0; });
  const previous = globalThis.location;
  let reloads = 0, polls = 0;
  globalThis.location = { reload() { reloads++; } };
  t.after(() => { if (previous === undefined) delete globalThis.location; else globalThis.location = previous; });
  t.mock.method(globalThis, 'fetch', async () => {
    polls++;
    if (polls === 1) throw new TypeError('Server restarting');
    if (polls === 2) return { ok: true, json: async () => ({ phase: 'installing' }) };
    return { ok: true, json: async () => ({ phase: 'idle', result: { version: '1.1.0', ...result, date: new Date().toISOString() } }) };
  });
  return { reloads: () => reloads, polls: () => polls };
}

for (const success of [true, false]) test(`installation survives server disconnection and ${success ? 'reloads the updated app' : 'reports rollback'}`, async t => {
  const progress = installation(t, { success, message: 'Démarrage impossible.' });
  const nodes = fixture(t, async path => {
    if (path.endsWith('/install')) throw new TypeError('Connection closed');
    return { configured: true, canInstall: true, available: { version: '1.1.0' } };
  });
  await nodes['#check-updates'].onclick();
  await nodes['#install-update'].onclick();
  assert.equal(progress.polls(), 3);
  assert.equal(progress.reloads(), success ? 1 : 0);
  if (!success) assert.match(nodes['#update-message'].textContent, /mise à jour a échoué.*Démarrage impossible/);
  assert.equal(nodes['#install-update'].disabled, false);
  assert.equal(nodes['#check-updates'].disabled, false);
});

test('dismissed notices stay hidden until a manual check or a different release', async t => {
  let version = '1.1.0';
  const nodes = fixture(t, async () => ({ configured: true, canInstall: true, available: { version } }));
  const periodic = globalThis.setInterval.mock.calls[0].arguments[0];
  await nodes['#check-updates'].onclick();
  nodes['#dismiss-update'].onclick();
  assert.equal(nodes['#update-banner'].hidden, true);
  periodic(); await new Promise(resolve => setImmediate(resolve));
  assert.equal(nodes['#update-banner'].hidden, true);
  await nodes['#check-updates'].onclick();
  assert.equal(nodes['#update-banner'].hidden, false);
  nodes['#dismiss-update'].onclick();
  version = '1.2.0'; periodic(); await new Promise(resolve => setImmediate(resolve));
  assert.equal(nodes['#update-banner'].hidden, false);
});

test('opening the page during installation resumes monitoring without installing again', async t => {
  const progress = installation(t, { success: true });
  const nodes = fixture(t, async path => {
    assert.equal(path, '/api/updates/check');
    return { configured: true, phase: 'installing', available: { version: '1.1.0' } };
  });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(progress.reloads(), 1);
  assert.equal(nodes['#update-banner'].hidden, false);
});

test('a late check cannot overwrite installation progress or enable controls', async t => {
  let finishCheck, finishInstall, holdCheck = false;
  const nodes = fixture(t, async path => {
    if (path.endsWith('/install')) return new Promise((resolve, reject) => { finishInstall = reject; });
    if (holdCheck) return new Promise(resolve => { finishCheck = resolve; });
    return { configured: true, canInstall: true, available: { version: '1.1.0' } };
  });
  await nodes['#check-updates'].onclick();
  holdCheck = true;
  const check = nodes['#check-updates'].onclick();
  const install = nodes['#install-update'].onclick();
  await new Promise(resolve => setImmediate(resolve));
  finishCheck({ configured: true, currentVersion: '1.0.0' }); await check;
  assert.match(nodes['#update-message'].textContent, /Téléchargement/);
  assert.equal(nodes['#check-updates'].disabled, true);
  nodes['#dismiss-update'].onclick(); assert.equal(nodes['#update-banner'].hidden, false);
  finishInstall(new Error('Échec de préparation')); await install;
  assert.equal(nodes['#dismiss-update'].disabled, false);
});

test('monitoring ignores receipts for another version and reports preparation failure', async t => {
  t.mock.method(globalThis, 'setTimeout', callback => { callback(); return 0; });
  let polls = 0;
  t.mock.method(globalThis, 'fetch', async () => ({ ok: true, json: async () => ++polls === 1
    ? { phase: 'idle', result: { version: '1.0.1', success: true, date: new Date().toISOString() } }
    : { phase: 'idle', error: 'Empreinte invalide.' } }));
  const nodes = fixture(t, async path => {
    if (path.endsWith('/install')) throw new TypeError('Connection closed');
    return { configured: true, canInstall: true, available: { version: '1.1.0' } };
  });
  await nodes['#check-updates'].onclick(); await nodes['#install-update'].onclick();
  assert.equal(polls, 2);
  assert.equal(nodes['#update-message'].textContent, 'Empreinte invalide.');
  assert.equal(nodes['#install-update'].disabled, false);
});

test('concurrent confirmations install only the version the user confirmed, once', async t => {
  installation(t, { success: true });
  const confirmations = [], versions = [];
  let availableVersion = '1.1.0';
  const nodes = fixture(t, async (path, method, body) => {
    if (path.endsWith('/install')) { versions.push(body.version); return {}; }
    return { configured: true, canInstall: true, available: { version: availableVersion } };
  }, () => new Promise(resolve => confirmations.push(resolve)));
  await nodes['#check-updates'].onclick();
  const first = nodes['#install-update'].onclick(), second = nodes['#install-update'].onclick();
  availableVersion = '1.2.0';
  await nodes['#check-updates'].onclick();
  confirmations.forEach(resolve => resolve(true));
  await Promise.all([first, second]);
  assert.deepEqual(versions, ['1.1.0']);
});

test('cancelling the confirmation never sends an installation request', async t => {
  let installs = 0;
  const nodes = fixture(t, async path => {
    if (path.endsWith('/install')) installs++;
    return { configured: true, canInstall: true, available: { version: '1.1.0' } };
  }, async () => false);
  await nodes['#check-updates'].onclick();
  await nodes['#install-update'].onclick();
  assert.equal(installs, 0);
});
