import test from 'node:test';
import assert from 'node:assert/strict';

test('browser removes the bootstrap secret and authenticates requests, including without storage', async t => {
  const names = ['location', 'history', 'sessionStorage', 'fetch'];
  const previous = Object.fromEntries(names.map(name => [name, Object.getOwnPropertyDescriptor(globalThis, name)]));
  t.after(() => { for (const name of names) { if (previous[name]) Object.defineProperty(globalThis, name, previous[name]); else delete globalThis[name]; } });
  const token = 'a'.repeat(64), values = new Map();
  let replaced, requested;
  for (const name of names) Object.defineProperty(globalThis, name, { value: undefined, writable: true, configurable: true });
  globalThis.history = { replaceState: (state, title, path) => { replaced = path; } };
  globalThis.fetch = async (path, options) => { requested = { path, options }; };
  for (const scenario of ['bootstrap', 'reload', 'storage-disabled']) {
    globalThis.location = { hash: scenario === 'reload' ? '' : '#session=' + token + '&view=board', pathname: '/', search: '' };
    globalThis.sessionStorage = scenario === 'storage-disabled'
      ? { setItem() { throw Error('disabled'); }, getItem() { throw Error('disabled'); } }
      : { setItem: (key, value) => values.set(key, value), getItem: key => values.get(key) };
    const session = await import('../public/local-session.mjs?' + scenario);
    if (scenario !== 'reload') assert.equal(replaced, '/#view=board');
    await session.localFetch('/api/state', { headers: { 'X-Local-Request': '1' } });
    assert.equal(requested.options.headers.Authorization, 'Bearer ' + token);
    assert.equal(requested.options.headers['X-Local-Request'], '1');
    assert.equal(session.eventsUrl(), '/api/events?session=' + token);
  }
});
