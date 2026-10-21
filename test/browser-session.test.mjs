import test from 'node:test';
import assert from 'node:assert/strict';

test('browser preserves authentication across tabs, migrates old sessions and tolerates disabled storage', async t => {
  const names = ['location', 'history', 'localStorage', 'sessionStorage', 'fetch'];
  const previous = Object.fromEntries(names.map(name => [name, Object.getOwnPropertyDescriptor(globalThis, name)]));
  t.after(() => { for (const name of names) { if (previous[name]) Object.defineProperty(globalThis, name, previous[name]); else delete globalThis[name]; } });
  let token = 'a'.repeat(64);
  const persistent = new Map(), tab = new Map();
  const storage = values => ({ setItem: (key, value) => values.set(key, value), getItem: key => values.get(key) });
  const disabled = { setItem() { throw Error('disabled'); }, getItem() { throw Error('disabled'); } };
  let replaced, requested;
  for (const name of names) Object.defineProperty(globalThis, name, { value: undefined, writable: true, configurable: true });
  globalThis.history = { replaceState: (state, title, path) => { replaced = path; } };
  globalThis.fetch = async (path, options) => { requested = { path, options }; };
  for (const scenario of ['bootstrap', 'reload', 'reopen', 'legacy', 'renew', 'stale-tab', 'persistent-disabled', 'storage-disabled', 'anonymous']) {
    if (scenario === 'reopen') tab.clear();
    if (scenario === 'legacy') persistent.clear();
    if (scenario === 'renew') token = 'b'.repeat(64);
    if (scenario === 'stale-tab') tab.set('atelier-session', 'a'.repeat(64));
    if (scenario === 'anonymous') { persistent.clear(); tab.clear(); token = ''; }
    const supplied = ['bootstrap', 'renew', 'storage-disabled'].includes(scenario);
    globalThis.location = { hash: supplied ? '#session=' + token + '&view=board' : '', pathname: '/', search: '' };
    globalThis.localStorage = ['persistent-disabled', 'storage-disabled'].includes(scenario) ? disabled : storage(persistent);
    globalThis.sessionStorage = scenario === 'storage-disabled' ? disabled : storage(tab);
    const session = await import('../public/local-session.mjs?' + scenario);
    if (supplied) assert.equal(replaced, '/#view=board');
    await session.localFetch('/api/state', { headers: { 'X-Local-Request': '1' } });
    assert.equal(requested.options.headers.Authorization, 'Bearer ' + token);
    assert.equal(requested.options.headers['X-Local-Request'], '1');
    assert.equal(session.eventsUrl(), '/api/events?session=' + token);
    if (token && !['persistent-disabled', 'storage-disabled'].includes(scenario)) assert.equal(persistent.get('atelier-session'), token);
  }
});
