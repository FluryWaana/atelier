import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { msg, markupText, locale, setLocale, resolveLocale, languages, messageText, localizeModule, initializeLanguage, languageKey } from '../public/i18n.mjs';
import { validateConnector } from '../lib/domain/connectors.mjs';
import { createConnectors } from '../lib/application/connectors.mjs';
import { createSettingsView } from '../public/views/settings.mjs';
import { usageText } from '../public/agent-usage.mjs';
import jira from '../lib/connectors/jira.mjs';
import azure from '../lib/connectors/azure.mjs';
import github from '../lib/connectors/github.mjs';
import gitlab from '../lib/connectors/gitlab.mjs';
import tempo from '../lib/connectors/tempo.mjs';
import demo from '../examples/connectors/demo/index.mjs';

test('English US is the default and unsupported stored languages fall back safely', () => {
  assert.equal(locale, 'en-US');
  for (const value of [null, undefined, '', 'fr', 'xx-XX', '__proto__', 'toString']) assert.equal(resolveLocale(value), 'en-US');
  assert.equal(resolveLocale('fr-FR'), 'fr-FR');
  assert.equal(msg('Nouvelle tâche'), 'New task');
  assert.equal(msg('toString'), 'toString');
});

test('both languages interpolate without modifying task content or HTML escaping', () => {
  const title = '<img src=x onerror=alert(1)> Nouvelle tâche {1}';
  for (const code of ['en-US', 'fr-FR']) {
    setLocale(code);
    assert.equal(msg('Projet de {0}', title), (code === 'en-US' ? 'Project for ' : 'Projet de ') + title);
    const escaped = '&lt;img src=x&gt;';
    assert.equal(markupText('Projet de {0}', escaped), (code === 'en-US' ? 'Project for ' : 'Projet de ') + escaped);
    assert.equal(msg('Un texte personnel non répertorié'), 'Un texte personnel non répertorié');
  }
  setLocale('en-US');
});

test('adding a partial language catalog uses English for missing messages and keeps placeholders', () => {
  languages['es-ES'] = { label: 'Español', messages: { 'Nouvelle tâche': 'Nueva tarea', 'Projet de {0}': '{0}: proyecto' } };
  try {
    setLocale('es-ES');
    assert.equal(msg('Nouvelle tâche'), 'Nueva tarea');
    assert.equal(msg('Fermer'), 'Close');
    assert.equal(msg('Projet de {0}', 'Alpha'), 'Alpha: proyecto');
  } finally { delete languages['es-ES']; setLocale('en-US'); }
  const tokens = text => [...text.matchAll(/\{\d+\}/g)].map(match => match[0]).sort();
  for (const [source, translation] of Object.entries(languages['en-US'].messages)) assert.deepEqual(tokens(translation), tokens(source), source);
});

test('server messages translate without altering variables, unknown errors or persisted data', () => {
  setLocale('en-US');
  assert.equal(messageText('Carte introuvable.'), 'Card not found.');
  assert.equal(messageText('Le port 4317 est déjà attribué à un service Atelier.'), 'Port 4317 is already assigned to an Atelier service.');
  assert.equal(messageText('Connecteur demo : capacité tickets incomplète.'), 'Connector demo: incomplete tickets capability.');
  assert.equal(messageText('External error <script>'), 'External error <script>');
  setLocale('fr-FR');
  assert.equal(messageText('Carte introuvable.'), 'Carte introuvable.');
  setLocale('en-US');
});

test('number and currency formatting follow the selected locale', () => {
  const run = { status: 'completed', model: 'custom', usage: { totalTokens: 1234, inputTokens: 1200, outputTokens: 34, cacheReadTokens: 0, cacheWriteTokens: 0, costUsd: 1.2345 } };
  for (const code of ['en-US', 'fr-FR']) {
    setLocale(code);
    assert.ok(usageText(run).includes((1234).toLocaleString(code)));
    assert.ok(usageText(run).includes((1.2345).toLocaleString(code, { style: 'currency', currency: 'USD', minimumFractionDigits: 4, maximumFractionDigits: 6 })));
  }
  setLocale('en-US');
});

test('language selection populates from the registry, persists and survives blocked storage', async t => {
  const sourceText = { nodeValue: ' Nouvelle tâche ', parentElement: { closest: () => null } };
  const field = { attrs: { placeholder: 'Titre (obligatoire)' }, hasAttribute(key) { return key in this.attrs; }, getAttribute(key) { return this.attrs[key]; }, setAttribute(key, value) { this.attrs[key] = value; } };
  const select = { options: [], append(option) { this.options.push(option); } };
  const document = { documentElement: {}, body: {}, createTreeWalker() { let read = false; return { currentNode: sourceText, nextNode() { if (read) return false; read = true; return true; } }; }, querySelectorAll: () => [field], querySelector: () => select, createElement: () => ({}) };
  let saved, destination;
  const originalStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage'), originalLocation = Object.getOwnPropertyDescriptor(globalThis, 'location');
  t.after(() => { for (const [key, original] of [['localStorage', originalStorage], ['location', originalLocation]]) { if (original) Object.defineProperty(globalThis, key, original); else delete globalThis[key]; } setLocale('en-US'); });
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: { getItem: () => saved?.[1] ?? null, setItem: (key, value) => saved = [key, value] } });
  Object.defineProperty(globalThis, 'location', { configurable: true, value: { href: 'http://127.0.0.1:4317/#view=settings', assign: value => destination = value } });
  setLocale('en-US'); initializeLanguage(document);
  assert.equal(document.documentElement.lang, 'en-US');
  assert.equal(sourceText.nodeValue, ' New task ');
  assert.equal(field.attrs.placeholder, 'Title (required)');
  assert.deepEqual(select.options.map(option => option.value), Object.keys(languages));
  select.value = 'fr-FR'; select.onchange();
  assert.deepEqual(saved, [languageKey, 'fr-FR']);
  assert.equal(new URL(destination).searchParams.get('lang'), 'fr-FR');
  assert.equal(new URL(destination).hash, '#view=settings');
  assert.equal((await import('../public/i18n.mjs?stored-language')).locale, 'fr-FR');
  globalThis.localStorage.setItem = () => { throw new Error('blocked'); };
  assert.doesNotThrow(() => select.onchange());
  globalThis.location.href = destination;
  assert.equal((await import('../public/i18n.mjs?url-language')).locale, 'fr-FR');
});

test('built-in modules and extensions expose their catalogs without translating settings or IDs', () => {
  const registry = createConnectors({ modules: [jira, azure, github, gitlab, tempo, demo], settings: () => ({}), getSecret: () => '' });
  const catalog = registry.describe();
  assert.equal(catalog.errors.length, 0);
  for (const module of catalog.modules) {
    setLocale('en-US');
    const english = localizeModule(module);
    assert.equal(english.description, module.translations['en-US'][module.description]);
    assert.deepEqual(english.values, module.values);
    assert.deepEqual(english.fields.map(field => field.id), module.fields.map(field => field.id));
    setLocale('fr-FR');
    assert.equal(localizeModule(module).description, module.description);
  }
  setLocale('en-US');
  const translated = localizeModule(catalog.modules.find(module => module.id === 'jira'));
  assert.equal(translated.fields.find(field => field.id === 'status-todo').label, 'Jira status: To do');
  const example = localizeModule(catalog.modules.find(module => module.id === 'demo'));
  assert.equal(example.tabs[0].actions[0].label, 'Count imported tickets');
  assert.throws(() => validateConnector({ ...demo, translations: { 'en-US': { invalid: () => 'code' } } }), /Catalogue/);
  assert.throws(() => validateConnector({ ...demo, sourceLocale: '../bad' }), /Langue/);
});

test('translated module markup escapes labels and preserves user-entered values', async () => {
  setLocale('en-US');
  const host = { innerHTML: '', querySelector: () => ({}), querySelectorAll: () => [] }, navigation = {};
  const document = { querySelector: selector => selector === '#connector-settings' ? host : navigation, querySelectorAll: () => [] };
  const module = { id: 'example', name: 'Exemple', description: 'Description', sourceLocale: 'fr-FR', capabilities: [], fields: [{ id: 'title', label: 'Titre', type: 'text' }], tabs: [], values: { title: 'Nouvelle tâche' }, secrets: {}, translations: { 'en-US': { 'Exemple': '<script>Example</script>', 'Titre': 'Title' } } };
  const view = createSettingsView({ document, api: async () => ({ modules: [module], providers: {} }) });
  await view.open();
  assert.match(host.innerHTML, /&lt;script&gt;Example&lt;\/script&gt;/);
  assert.doesNotMatch(host.innerHTML, /<script>/i);
  assert.match(host.innerHTML, /value="Nouvelle tâche"/);
  assert.match(host.innerHTML, /Title<input/);
});

test('browser translation calls refer to registered catalog entries', () => {
  for (const file of ['app.js', 'ui-actions.mjs', 'views/card.mjs', 'views/time.mjs', 'views/brainstorm.mjs', 'views/task-editor.mjs', 'views/overview.mjs', 'views/workspace-workflows.mjs', 'ui-helpers.mjs', 'view-helpers.mjs', 'workflow.mjs', 'agent-usage.mjs', 'updates.mjs', 'views/board.mjs', 'views/projects.mjs', 'views/project-settings.mjs', 'views/processes.mjs', 'views/settings.mjs']) {
    const source = readFileSync(new URL('../public/' + file, import.meta.url), 'utf8');
    for (const match of source.matchAll(/(?:msg|markupText)\(("(?:[^"\\]|\\.)*")/g)) assert.ok(Object.hasOwn(languages['en-US'].messages, JSON.parse(match[1])), file + ': ' + match[1]);
  }
});
