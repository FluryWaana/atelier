import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createSettings } from '../../lib/application/settings.mjs';
import { text } from '../../lib/domain/validation.mjs';

const reference = JSON.parse(readFileSync(new URL('./rust-settings-reference.json', import.meta.url), 'utf8'));
for (const scenario of reference.cases) {
  let saved = structuredClone(scenario.stored || {});
  let credentials = structuredClone(scenario.tokens || {});
  let writes = 0;
  const service = createSettings({
    store: { settings: () => saved, saveSettings(value) { saved = value; writes++; }, transaction: change => change() },
    vault: { save(value) { credentials = value; writes++; } }, initialTokens: credentials, platform: 'fixture',
    repository(value) {
      value = text(value, 2000);
      if (value === 'missing') throw new Error('Dépôt synthétique absent.');
      return 'resolved:' + value;
    },
    jiraBase(value) {
      if (value === 'invalid') throw new Error('Adresse synthétique invalide.');
      return value.replace(/\/$/, '');
    },
  });
  if (scenario.error) {
    assert.throws(() => service.update(scenario.body), error => error.message === scenario.error, scenario.name);
    assert.equal(writes, 0, scenario.name);
  } else {
    const actual = service.update(scenario.body);
    const expected = { ...reference.defaults, ...scenario.stored, ...scenario.settings };
    for (const key of scenario.omit || []) delete expected[key];
    // JSON persistence removes undefined taskQueue, matching the historical SQLite store.
    assert.deepEqual(JSON.parse(JSON.stringify(saved)), expected, scenario.name);
    assert.deepEqual(credentials, scenario.credentials || scenario.tokens || {}, scenario.name);
    assert.deepEqual(actual, { ...saved, platform: 'fixture', tokenSet: Boolean(credentials.jira), azureTokenSet: Boolean(credentials.azure), tempoTokenSet: Boolean(credentials.tempo) }, scenario.name);
    assert.equal(writes, 2, scenario.name);
  }
}
console.log(`${reference.cases.length} réglages globaux historiques vérifiés (résolveurs synthétiques).`);
