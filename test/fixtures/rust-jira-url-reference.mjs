import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { jiraBase } from '../../lib/jira.mjs';

const cases = JSON.parse(readFileSync(new URL('./rust-jira-url-reference.json', import.meta.url), 'utf8'));
for (const { input, value, error } of cases) {
  if (error) assert.throws(() => jiraBase(input), actual => actual.message === error, input);
  else assert.equal(jiraBase(input), value);
}
console.log(`${cases.length} adresses Jira historiques vérifiées sans requête distante.`);
