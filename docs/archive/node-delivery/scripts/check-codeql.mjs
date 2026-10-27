import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

export function checkCodeql(directory) {
  assert.ok(directory, 'Dossier des rapports CodeQL requis.');
  const files = readdirSync(directory).filter(name => name.endsWith('.sarif'));
  assert.ok(files.length, 'Aucun rapport CodeQL : publication refusée.');
  const findings = [];
  for (const file of files) {
    const report = JSON.parse(readFileSync(join(directory, file), 'utf8'));
    assert.equal(report.version, '2.1.0', 'Format SARIF CodeQL invalide.');
    assert.ok(Array.isArray(report.runs) && report.runs.length, 'Analyse CodeQL absente.');
    for (const run of report.runs) {
      assert.ok(Array.isArray(run.results), 'Résultats CodeQL absents.');
      assert.ok(!run.invocations?.some(invocation => invocation.executionSuccessful === false), 'Analyse CodeQL en échec.');
      for (const result of run.results) {
        if (result.suppressions?.some(suppression => suppression.kind === 'inSource' && (!suppression.status || suppression.status === 'accepted'))) continue;
        findings.push(`${file}: ${result.ruleId || 'alerte sans identifiant'}`);
      }
    }
  }
  assert.equal(findings.length, 0, `Alertes CodeQL à traiter avant publication :\n${findings.join('\n')}`);
}

if (import.meta.main) {
  checkCodeql(process.argv[2]);
  console.log('CodeQL : aucune alerte.');
}
