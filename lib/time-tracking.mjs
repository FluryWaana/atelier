import { createTempoExport } from './tempo-export.mjs';
import { randomUUID } from 'node:crypto';
import { tempoRequest, findTempoEntry } from './tempo.mjs';
import { createTimeTracking as createUseCases } from './application/time-tracking.mjs';
import { timeRoute } from './http/time-routes.mjs';
export { timeFields } from './domain/time.mjs';

export function createTimeTracking(options) {
  const tempo = createTempoExport({ requestTempo: tempoRequest, findTempoEntry, ...options });
  const service = createUseCases({ newId: randomUUID, ...options, exporting: tempo.busy });
  const clock = setInterval(service.tick, 5000); clock.unref();
  const execute = (action, id, body) => ['export', 'reconcile'].includes(action) ? tempo.execute(action, id, body) : service.execute(action, id, body);
  return { execute, busy: tempo.busy, close() { clearInterval(clock); service.close(); }, route: (request, path, body) => timeRoute({ execute }, request.method, path, body) };
}
