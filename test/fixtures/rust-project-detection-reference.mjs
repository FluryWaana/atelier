import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { createProjects } from '../../lib/application/projects.mjs';
import { text } from '../../lib/domain/validation.mjs';

export function verifyProjectDetection() {
  const fixture = JSON.parse(readFileSync(new URL('./rust-project-detection-reference.json', import.meta.url), 'utf8'));
  for (const scenario of fixture.cases) {
    const project = structuredClone(Object.hasOwn(scenario, 'project') ? scenario.project : fixture.project);
    const before = structuredClone(project), events = [];
    const service = createProjects({
      store: { projects: () => project ? [project] : [] },
      repository: value => { events.push({ repository: value }); return 'checked:' + text(value, 2000); },
      detectLaunch: repo => {
        events.push({ scan: repo });
        if (scenario.failScan) throw new Error('Synthetic scan failure');
        return structuredClone(fixture.detected);
      },
    });
    let actual;
    try { actual = { value: service.detect('project', scenario.body, Boolean(scenario.useDraft)), events }; }
    catch (error) { actual = { error: error.message, events }; }
    assert.deepEqual(actual, scenario.expected, scenario.name);
    assert.deepEqual(project, before, 'La détection ne doit pas modifier le projet.');
  }
  return fixture.cases.length;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) console.log(`${verifyProjectDetection()} contrats de détection de projets historiques vérifiés.`);
