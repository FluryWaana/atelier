import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { text } from '../../lib/domain/validation.mjs';
import { checklist, checklistText } from '../../lib/domain/checklist.mjs';
import { blockers, dependencies, reviewRequirements } from '../../lib/domain/tasks.mjs';
import { agentProviders, agentProvider, agentOptions, resolveAgentOptions, providerSettings } from '../../lib/domain/agent-options.mjs';
import { projectSettings, settingsForProject } from '../../lib/project-settings.mjs';
import { sameRepository } from '../../lib/domain/paths.mjs';
import { timeFields } from '../../lib/domain/time.mjs';

const functions = { text, checklist, checklistText, blockers, dependencies, reviewRequirements, agentProviders, agentProvider, agentOptions, resolveAgentOptions, providerSettings, projectSettings, settingsForProject, sameRepository, timeFields };
const cases = [];
function add(operation, ...args) {
  let expected;
  try { expected = { value: functions[operation](...args) }; }
  catch (error) { expected = { error: error.message }; }
  cases.push({ operation, args, expected });
}

for (const value of ['', '  é 🔧 ', '\ufeffx\u00a0', '\u0085x\u0085', null, 12, '🔧'.repeat(151)]) add('text', value, 300);
for (const value of [null, {}, [], [{ text: '  Critère 🔧 ', done: false, extra: 'discard' }], [{ text: 'OK', done: true }], [{ text: '', done: false }], [{ text: '  ', done: true }], [{ text: 'x', done: 'true' }], [{ text: null, done: false }], [null], Array(21).fill({ text: 'x', done: false }), [{ text: '🔧'.repeat(250), done: true }], [{ text: '🔧'.repeat(251), done: true }]]) add('checklist', value);
for (const value of [[], [{ text: 'à faire', done: false }, { text: 'fini', done: true }]]) add('checklistText', value);
for (const value of [{}, { title: 'titre', description: null, instructions: '', checklist: [{ text: 'C1', done: true }, { text: 'C2', done: false }] }]) add('reviewRequirements', value);
const cards = [{ id: 'a', status: 'todo' }, { id: 'b', status: 'todo', archived: true, blockedBy: ['a'] }, { id: 'c', status: 'done' }, { id: 'd', status: 'review', blockedBy: ['b'] }];
for (const blockedBy of [[], ['b', 'missing', 'c'], ['c'], ['b', 'b']]) add('blockers', { blockedBy }, cards);
for (const value of [[], ['a'], ['b'], ['d'], ['missing'], ['c', 'c'], [3], null, 'c', ['c']]) add('dependencies', 'a', value, cards);
add('dependencies', 'b', ['a'], cards);
add('dependencies', 'new', ['x'], [{ id: 'x', blockedBy: ['y'] }, { id: 'y', blockedBy: ['x'] }]);
add('dependencies', 'new', ['x'], [{ id: 'x', blockedBy: ['deleted'] }]);
add('dependencies', 'new', Array.from({ length: 21 }, (_, i) => String(i)), Array.from({ length: 21 }, (_, i) => ({ id: String(i) })));

const settings = { codexModel: 'codex-default', codexEffort: 'medium', claudeModel: 'sonnet', claudeEffort: 'low' };
for (const settings of [{}, { agentProviders: null }, { agentProviders: [] }, { agentProviders: [{ id: 'work', type: 'claude', unknown: true }] }, { codexModel: 'default', claudeEffort: 'max' }]) add('agentProviders', settings);
for (const agent of ['codex', 'claude', 'missing']) add('agentProvider', settings, agent);
for (const agent of ['codex', 'claude', 'command', 'unknown']) {
  add('agentOptions', agent, {});
  for (const model of [' custom/model:v1 ', '--unsafe', 'a\nb', 'a"b', 42, null, 'é', '🔧'.repeat(91)]) add('agentOptions', agent, { model });
  for (const effort of ['', 'minimal', 'max', 'deep_mode-1', ' low ', null, 'x'.repeat(41)]) add('agentOptions', agent, { effort });
}
add('agentOptions', 'work', { model: 'opus', effort: 'max' }, { agentProviders: [{ id: 'work', type: 'claude' }] });
add('agentOptions', 'work', {}, { agentProviders: [] });
const card = { agent: 'codex', model: 'task-model', effort: 'high' };
for (const body of [{ agent: 'codex' }, { agent: 'claude' }, { agent: 'codex', model: 'one-run', effort: '' }, { agent: 'codex', model: null, effort: null }, { agent: 'codex', model: false }, { agent: 'missing' }]) add('resolveAgentOptions', settings, card, body);
add('resolveAgentOptions', settings, {}, { agent: 'codex' });
const provider = { id: 'work', name: 'Travail', type: 'claude', model: 'opus', effort: 'max' };
add('providerSettings', provider);
for (const patch of [{ id: '../escape' }, { id: 'constructor' }, { id: 'prototype' }, { id: '__proto__' }, { id: 'A' }, { id: 'x'.repeat(65) }, { name: '' }, { name: 'a\nb' }, { type: 'unknown' }, { auth: 'api' }, { auth: null }, { auth: 1 }, { configDir: '../relative' }, { command: './relative' }, { command: '-command' }, { command: null }, { effort: 'minimal' }]) add('providerSettings', { ...provider, ...patch });
for (const path of ['\\\\server\\share', '//server/share', '/\\server', '\\?\\C:\\x', 'C:relative', 'C:/é space', '/home/é space', '\\rooted']) {
  add('providerSettings', { ...provider, command: path });
  add('providerSettings', { ...provider, configDir: path });
}
const custom = { id: 'custom', name: 'Custom', type: 'command', command: 'runner', effort: 'deep' };
add('providerSettings', custom);
add('providerSettings', { ...custom, versionArgs: [null], statusArgs: 'invalid' });
for (const patch of [{ analyzeArgs: '--unsafe' }, { editArgs: [null] }, { statusArgs: ['\0'] }, { versionArgs: null }, { versionArgs: [] }, { command: '' }, { analyzeArgs: ['literal $() ; é', '\n', ''] }, { statusArgs: Array(101).fill('x') }, { npmPackage: '../private' }, { npmPackage: '@scope/package.name' }, { npmPackage: 'Upper' }, { npmPackage: '@scope/nested/path' }]) add('providerSettings', { ...custom, ...patch });
add('providerSettings', { id: 'api', name: 'API', type: 'codex', auth: 'api', model: 'custom/model', effort: 'high' });
for (const value of [null, [], 'text']) add('providerSettings', value);

add('projectSettings', {});
for (const patch of [{ projectRules: '  Règle\nseconde 🔧  ' }, { projectRules: 42 }, { projectRules: 'x'.repeat(12001) }, { agent: 'claude' }, { agent: false }, { agent: 'missing' }, { baseBranch: '-danger' }, { repositoryConnector: '../git' }, { repositoryConnector: 'azure-devops' }, { jiraMode: 'other' }, { jiraMode: 'custom' }, { jiraMode: 'custom', jiraStatusMap: { review: ' À revoir ' } }, { jiraStatusMap: [] }, { jiraStatusMap: { unknown: 'x' } }, { jiraStatusMap: { todo: null } }, { automationEnabled: null }, { autoFix: 'false' }, { automationEnabled: true }, { automationEnabled: true, prTarget: 'main', testCommand: 'cargo test' }, { prTarget: '-unsafe' }, { linkedProjectId: null }]) add('projectSettings', patch);
for (const value of ['', ' front ', 'src/Api', 'front/../back', '.', 'front/..', 'a//b/../../c', '../back', '..\\back', 'a/../../outside', '/back', 'C:\\back', 'C:back', '\\\\server\\share', '\\rooted', 'a/./../..', null, 42]) add('projectSettings', { frontDirectory: value });
for (const value of ['', '0', 0, '004317', 65535, 65536, -1, 1.5, ' 80 ', '+80', '8e1', '٠٨', null, true]) add('projectSettings', { frontPort: value, frontCommand: 'run --port {port}' });
add('projectSettings', { frontPort: 4317, frontCommand: 'run' });
add('projectSettings', { backPort: '4318', backCommand: 'run --port {port}', autoFix: false });
add('projectSettings', { agent: 'work' }, { agentProviders: [{ id: 'work', type: 'claude' }] });
const projects = [{ repo: 'first', projectRules: 'Règle A', agent: 'claude', baseBranch: 'develop', jiraMode: 'off' }, { repo: 'second', projectRules: 'Règle B', automationEnabled: true, autoFix: false, jiraMode: 'custom', jiraStatusMap: { todo: 'Ready' } }, { repo: 'legacy' }];
for (const repo of ['first', 'second', 'legacy', 'unknown', '']) add('settingsForProject', { repo: 'first', agent: 'codex', baseBranch: 'main', jiraAutoTransition: true, jiraStatusMap: { todo: 'Open' }, extra: { untouched: true } }, projects, repo);
for (const platform of ['win32', 'linux', 'darwin']) {
  add('sameRepository', 'C:\\É Space\\Repo', 'c:/é space/repo', platform);
  add('sameRepository', '/repo/', '/repo', platform);
  add('sameRepository', '', '', platform);
}

const time = { seconds: 60, date: '2026-10-24', startTime: '09:00:00', description: ' Travail 🔧 ' };
for (const seconds of [1, 86400, 0, -1, 86401, 1.5, null, true, false, '', ' ', ' 60 ', '1e2', '0x10', '0b11', '0o10', '+1', '+0x10', 'Infinity', [], [60], [['60']], [true], [null], [1, 2], {}, { toString: null }, [{ toString: 1 }]]) add('timeFields', { ...time, seconds });
add('timeFields', {});
for (const date of ['0000-02-29', '1900-02-29', '2000-02-29', '2024-02-29', '2026-02-29', '2026-04-31', '2026-00-01', '2026-13-01', '2026-01-00', '2026-01-32', '9999-12-31', '2026-1-01', null]) add('timeFields', { ...time, date });
for (const description of ['', ' \ufeff\u00a0 ', '\u0085', null, true, '🔧'.repeat(2000), '🔧'.repeat(2001)]) add('timeFields', { ...time, description });
for (const startTime of ['00:00:00', '23:59:59', '24:00:00', '12:60:00', '12:00:60', '9:00:00', '09:00', '09:00:00 ', null, 9]) add('timeFields', { ...time, startTime });

const destination = fileURLToPath(new URL('./rust-domain-reference.json', import.meta.url));
const content = JSON.stringify(cases, null, 2) + '\n';
assert.ok(process.argv.slice(2).every(arg => arg === '--check'));
if (process.argv.includes('--check')) assert.deepEqual(JSON.parse(readFileSync(destination, 'utf8')), cases, 'Les contrats de référence Node ont changé.');
else writeFileSync(destination, content);
console.log(`${cases.length} contrats de domaine Node ${process.argv.includes('--check') ? 'vérifiés' : 'capturés'}.`);
