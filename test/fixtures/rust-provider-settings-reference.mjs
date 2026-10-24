import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { providerSettings } from '../../lib/domain/agent-options.mjs';

const reference = JSON.parse(readFileSync(new URL('./rust-provider-settings-reference.json', import.meta.url), 'utf8'));
for (const scenario of reference.cases) {
  const body = { ...reference.body, ...scenario.body };
  if (scenario.error) assert.throws(() => providerSettings(body), error => error.message === scenario.error);
  else assert.deepEqual(providerSettings(body), { ...reference.defaults, ...scenario.settings });
}
console.log(`${reference.cases.length} profils UTF-16 historiques vérifiés.`);
