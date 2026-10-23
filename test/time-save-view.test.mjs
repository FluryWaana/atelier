import './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { createTimeView } from '../public/views/time.mjs';

test('saving time or starting a timer navigates and confirms without the removed card dialog', async t => {
  const fields = { cardId: 'task', startTime: '09:00', minutes: '30', description: 'Review', date: '2026-10-23' };
  t.mock.method(globalThis, 'FormData', function (form) { return new Map(Object.entries(form.values)); });
  for (const start of [false, true]) {
    const events = [], button = { disabled: false };
    const form = { values: fields, elements: { cardId: { value: 'task' } }, reportValidity: () => true, querySelectorAll: () => [button] };
    const nodes = new Map([['#time-form', form], ['#time-dialog', { close: () => events.push('close') }]]);
    const view = createTimeView({
      document: { querySelector: selector => nodes.get(selector) || null },
      api: async (path, method, body) => {
        assert.equal(button.disabled, true);
        assert.equal(path, '/api/time'); assert.equal(method, 'POST');
        assert.equal(body.start, start); assert.equal(body.seconds, 1800); assert.equal(body.startTime, '09:00:00');
        events.push('saved');
      },
      setView: name => events.push(name), refresh: async () => events.push('refresh'), toast: text => events.push(text),
    });
    await view.saveTime(start);
    assert.deepEqual(events.slice(0, 4), ['saved', 'close', 'time', 'refresh']);
    assert.match(events[4], start ? /Chronomètre démarré/ : /Temps enregistré/);
    assert.equal(button.disabled, false);
  }
});
