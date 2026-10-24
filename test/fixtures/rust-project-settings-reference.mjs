import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { projectSettings } from '../../lib/project-settings.mjs';

const cases = [];
const add = (body, settings = {}) => cases.push({ body, settings });
for (const key of ['projectRules', 'baseBranch', 'frontCommand', 'backCommand', 'frontRepo', 'backRepo', 'frontDirectory', 'backDirectory', 'linkedProjectId', 'prTarget', 'testCommand']) {
  add({ [key]: ' \ud800🔧\udc00 ' });
  add({ [key]: '\ufeff\ud800\u00a0' });
}
add({ projectRules: '🔧'.repeat(6000) });
add({ projectRules: '🔧'.repeat(6001) });
add({ baseBranch: '\ud800'.repeat(180) });
add({ baseBranch: '\ud800'.repeat(181) });
for (const frontDirectory of ['\ud800/../safe', '\ud800/../../outside', '\ud800:\\folder', 'C:\\ud800', '\ud800\\..\\..', '\ud800\\..\\safe']) add({ frontDirectory });
add({ jiraMode: 'custom', jiraStatusMap: { review: ' \ud800 ' } });
add({ jiraStatusMap: { '\ud800': 'unknown status' } });
add({ frontPort: 4318, frontCommand: '\ud800 run {port}' });
add({ frontPort: 4318, frontCommand: 'run {po\ud800rt}' });
add({ agent: '\ud800' }, { agentProviders: [{ id: '\ud800', type: 'command', unknown: '\udc00' }] });
add({ agent: '\ud800' });
add({ agent: 'codex', unknown: { '\ud800': '\udc00' } }, { unknown: '\ud800' });
for (const body of [
  { agent: true, baseBranch: 3 }, { agent: 'missing', repositoryConnector: '\ud800' },
  { agent: 'missing', jiraMode: '\ud800' }, { agent: 'missing', baseBranch: '-bad' },
  { baseBranch: '-bad', repositoryConnector: '\ud800' }, { jiraMode: '\ud800', jiraStatusMap: [] },
  { autoFix: null, prTarget: false }, { frontPort: 1, frontCommand: '\ud800', projectRules: false },
  { backPort: 1, backCommand: false, automationEnabled: true },
  { frontDirectory: '../outside', projectRules: false }, { frontPort: '\ud800' },
  { automationEnabled: true, prTarget: '\ud800', testCommand: '\udc00' },
]) add(body);
for (const scenario of cases) {
  try { scenario.expected = { value: projectSettings(scenario.body, scenario.settings) }; }
  catch (error) { scenario.expected = { error: error.message }; }
}
const destination = new URL('./rust-project-settings-reference.json', import.meta.url);
if (process.argv.includes('--check')) assert.deepEqual(JSON.parse(readFileSync(destination, 'utf8')), cases, 'Les réglages de projet Node ont changé.');
else writeFileSync(destination, JSON.stringify(cases, null, 2) + '\n');
console.log(`${cases.length} réglages de projet UTF-16 historiques vérifiés.`);
