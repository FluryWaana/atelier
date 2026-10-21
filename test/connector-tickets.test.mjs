import test from 'node:test';
import assert from 'node:assert/strict';
import { createConnectorTickets } from '../lib/application/connector-tickets.mjs';

function fixture() {
  const cards = new Map(), invalidated = [];
  let busy = false;
  const service = createConnectorTickets({ connector: 'example',
    store: { cards: () => [...cards.values()], card: id => cards.get(id), putCard: card => cards.set(card.id, card), transaction: run => run() },
    tasks: { create: input => ({ id: 'local-' + cards.size, status: 'todo', ...input }) },
    busy: () => busy, changed() {}, timestamp: () => 'now', invalidateReviews: id => invalidated.push(id),
  });
  return { service, cards, invalidated, setBusy: () => { busy = true; } };
}
const ticket = (host, extra = {}) => ({ id: '42', title: host, url: `https://${host}/tickets/42`, ...extra });

test('generic tickets distinguish instances and preserve local work when the same ticket returns', () => {
  const { service, cards, invalidated } = fixture();
  const first = service.upsert(ticket('a.example'));
  cards.set(first.id, { ...first, status: 'review', instructions: 'Travail local' });
  const second = service.upsert(ticket('b.example'));
  assert.notEqual(first.id, second.id);
  assert.equal(cards.get(first.id).title, 'a.example');
  const updated = service.upsert(ticket('a.example', { title: 'Nouveau titre', url: 'https://a.example/issues/42' }));
  assert.equal(cards.size, 2);
  assert.equal(updated.id, first.id);
  assert.equal(updated.status, 'review');
  assert.equal(updated.instructions, 'Travail local');
  assert.deepEqual(invalidated, [first.id]);
});

test('legacy tickets are reused only on their original origin, including attach', () => {
  const { service, cards, setBusy } = fixture();
  cards.set('legacy', { id: 'legacy', title: 'Ancien', ticket: { connector: 'example', ...ticket('a.example') } });
  assert.throws(() => service.attach('legacy', ticket('b.example')), /liaison/);
  assert.equal(service.upsert(ticket('b.example')).id, 'local-1');
  assert.equal(service.upsert(ticket('a.example')).id, 'legacy');
  assert.equal(cards.get('legacy').ticket.instance, 'https://a.example');
  assert.equal(service.attach('legacy', ticket('a.example')).id, 'legacy');
  setBusy();
  assert.throws(() => service.upsert(ticket('a.example')), /opération/);
});

test('explicit instance paths separate tenants and reject invalid scope without writes', () => {
  const { service, cards } = fixture();
  const first = service.upsert(ticket('shared.example', { instance: 'https://shared.example/client-a/' }));
  const second = service.upsert(ticket('shared.example', { instance: 'https://shared.example/client-b' }));
  assert.notEqual(first.id, second.id);
  assert.equal(service.upsert(ticket('shared.example', { instance: 'https://shared.example/client-a' })).id, first.id);
  assert.throws(() => service.attach(first.id, ticket('shared.example', { instance: 'https://shared.example/client-b' })), /liaison/);
  for (const instance of ['', 12, 'https://elsewhere.example', 'https://user:pass@shared.example', 'https://shared.example/?token=x', 'https://shared.example/#x']) {
    assert.throws(() => service.upsert(ticket('shared.example', { instance })), /instance/);
  }
  assert.equal(cards.size, 2);
});
