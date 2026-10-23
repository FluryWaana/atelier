import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { text } from '../../lib/domain/validation.mjs';
import { checklist, checklistText } from '../../lib/domain/checklist.mjs';
import { blockers, dependencies, reviewRequirements } from '../../lib/domain/tasks.mjs';
import { agentProviders, agentProvider, agentOptions, resolveAgentOptions, providerSettings } from '../../lib/domain/agent-options.mjs';

const functions = { text, checklist, checklistText, blockers, dependencies, reviewRequirements, agentProviders, agentProvider, agentOptions, resolveAgentOptions, providerSettings };
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

const destination = fileURLToPath(new URL('./rust-domain-reference.json', import.meta.url));
const content = JSON.stringify(cases, null, 2) + '\n';
assert.ok(process.argv.slice(2).every(arg => arg === '--check'));
if (process.argv.includes('--check')) assert.equal(readFileSync(destination, 'utf8'), content, 'Les contrats de référence Node ont changé.');
else writeFileSync(destination, content);
console.log(`${cases.length} contrats de domaine Node ${process.argv.includes('--check') ? 'vérifiés' : 'capturés'}.`);
