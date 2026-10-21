import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve, sep } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { checkCodeql } from '../scripts/check-codeql.mjs';

test('the security gate refuses findings, failed analyses and missing or malformed reports', t => {
  const directory = mkdtempSync(join(tmpdir(), 'atelier-security-'));
  t.after(() => {
    assert.ok(resolve(directory).startsWith(resolve(tmpdir()) + sep));
    rmSync(directory, { recursive: true, force: true });
  });
  const write = (runs, version = '2.1.0') => writeFileSync(join(directory, 'javascript.sarif'), JSON.stringify({ version, runs }));
  assert.throws(() => checkCodeql(), /requis/);
  assert.throws(() => checkCodeql(directory), /Aucun rapport/);
  write([{ results: [] }]);
  assert.doesNotThrow(() => checkCodeql(directory));
  const accepted = spawnSync(process.execPath, ['scripts/check-codeql.mjs', directory], { encoding: 'utf8', windowsHide: true });
  assert.equal(accepted.status, 0, accepted.stderr);
  write([{ results: [] }, { results: [{ ruleId: 'js/command-line-injection', level: 'warning' }] }]);
  assert.throws(() => checkCodeql(directory), /js\/command-line-injection/);
  const blocked = spawnSync(process.execPath, ['scripts/check-codeql.mjs', directory], { encoding: 'utf8', windowsHide: true });
  assert.equal(blocked.status, 1);
  write([{ results: [{ ruleId: 'js/http-to-file-access', suppressions: [{ kind: 'inSource' }] }] }]);
  assert.doesNotThrow(() => checkCodeql(directory));
  for (const suppression of [{ kind: 'external' }, { kind: 'inSource', status: 'rejected' }, { kind: 'inSource', status: 'underReview' }]) {
    write([{ results: [{ ruleId: 'js/http-to-file-access', suppressions: [suppression] }] }]);
    assert.throws(() => checkCodeql(directory), /js\/http-to-file-access/);
  }
  write([{ results: [{ ruleId: 'js/http-to-file-access', suppressions: [{ kind: 'inSource' }] }, { ruleId: 'js/xss-through-dom' }] }]);
  assert.throws(() => checkCodeql(directory), /js\/xss-through-dom/);
  write([{ results: [], invocations: [{ executionSuccessful: false }] }]);
  assert.throws(() => checkCodeql(directory), /en échec/);
  write([]);
  assert.throws(() => checkCodeql(directory), /Analyse CodeQL absente/);
  write([{}]);
  assert.throws(() => checkCodeql(directory), /Résultats CodeQL absents/);
  write([{ results: [] }], 'invalid');
  assert.throws(() => checkCodeql(directory), /Format SARIF/);
  write([{ results: [] }]);
  writeFileSync(join(directory, 'actions.sarif'), JSON.stringify({ version: '2.1.0', runs: [{ results: [{ ruleId: 'actions/code-injection/critical' }] }] }));
  assert.throws(() => checkCodeql(directory), /actions\/code-injection/);
  writeFileSync(join(directory, 'actions.sarif'), '{');
  assert.throws(() => checkCodeql(directory), SyntaxError);
});
