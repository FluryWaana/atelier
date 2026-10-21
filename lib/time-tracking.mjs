import { randomUUID } from 'node:crypto';
import { tempoRequest, findTempoEntry } from './tempo.mjs';
import { createTimeTracking as createUseCases } from './application/time-tracking.mjs';
import { timeRoute } from './http/time-routes.mjs';
export { timeFields } from './domain/time.mjs';

export function createTimeTracking(options) {
  const service = createUseCases({ newId: randomUUID, requestTempo: tempoRequest, findTempoEntry, ...options });
  return { ...service, route: (request, path, body) => timeRoute(service, request.method, path, body) };
}
