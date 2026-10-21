import { sameRepository } from '../lib/domain/paths.mjs';
import { createProcessesView } from '../public/views/processes.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('../public/views/project-settings.mjs', import.meta.url), 'utf8') + '\n' + readFileSync(new URL('../public/app.js', import.meta.url), 'utf8');
const esc = value => String(value ?? '').replace(/[&<>"']/g, character => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character]);
function view(state) {
  const context = { state, launchWorktrees: {} };
  Object.assign(context, createProcessesView({ getState: () => context.state, getSelection: () => context.launchWorktrees,
    api: (...args) => context.api(...args), refresh: () => context.refresh(), refreshProcesses: () => context.refreshProcesses(),
  }));
  return context;
}
test('process view includes companion projects, real socket owners, safe logs and archived stop controls', () => {
  const projects = [{ id: 'front', name: 'TOTO', repo: 'front', frontCommand: 'npm run dev', linkedProjectId: 'back' }, { id: 'back', name: 'TOTO2', repo: 'back' }];
  const card = { id: 'a', title: 'Test <front>', repo: 'front', worktree: { path: 'work' }, archived: true };
  const session = { cardId: 'a', projectIds: ['front', 'back'], log: '<script>unsafe</script>', services: [{ id: 'front:front', name: 'TOTO · Front', repo: 'work', status: 'running', pid: 123, processIds: [123, 456], endpoints: [{ address: '127.0.0.1', port: 5173, pid: 456, url: 'http://127.0.0.1:5173/' }] }] };
  const context = view({ projects, cards: [card], devServers: [session] });
  assert.equal(context.processCards(projects[1]).length, 1, 'the companion project filter retains the originating card');
  assert.equal(context.processCards({ id: 'other' }).length, 0);
  const html = context.processesMarkup(projects[1]);
  assert.match(html, /Test &lt;front&gt;/);
  assert.match(html, /&lt;script&gt;unsafe&lt;\/script&gt;/);
  assert.match(html, /PID lanceur : <code>123<\/code>/);
  assert.match(html, /<td>5173<\/td><td>456<\/td>/);
  assert.match(html, /href="http:\/\/127\.0\.0\.1:5173\/"/);
  assert.match(html, /data-process-action="start"[^>]+data-service-id="front:front"[^>]+disabled/);
  assert.doesNotMatch(html, /data-process-action="stop"[^>]+data-service-id="front:front"[^>]+disabled/);
});
test('project launcher offers a worktree per project and hides unused card details', () => {
  const projects = [{ id: 'p', repo: 'repo', frontCommand: 'npm run dev' }];
  const context = view({ projects, cards: [{ id: 'a', title: 'Ready', repo: 'repo', worktree: { path: 'work' } }], devServers: [] });
  assert.match(context.processesMarkup(), /data-launch-project="p"/);
  assert.match(context.processesMarkup(), /data-launch-settings="p">Configurer le lancement/);
  assert.match(context.processesMarkup(), /<option value="a"/);
  assert.doesNotMatch(context.processesMarkup(), /data-process-card="a"/);
  context.launchWorktrees.p = 'a';
  assert.match(context.processesMarkup(), /value="a" selected/);
  assert.match(context.processesMarkup(), /data-launch-one="p" >Lancer/);
  context.state.cards[0].archived = true;
  assert.doesNotMatch(context.processesMarkup(), /<option value="a"/);
  assert.equal(context.launchWorktrees.p, undefined);
  assert.match(view({ projects: [], cards: [] }).processesMarkup(), /Aucun service lancé/);
});

test('launch configuration edits only startup fields and keeps the dialog open on save failure', async () => {
  const elements = Object.fromEntries(['frontCommand', 'backCommand', 'frontDirectory', 'backDirectory', 'frontPort', 'backPort', 'frontRepo', 'backRepo'].map(name => [name, { value: '' }]));
  const button = {}, form = { elements, dataset: {}, querySelector: () => button, querySelectorAll: () => [] }, nodes = {
    '#launch-settings-form': form, '#launch-settings-title': {}, '#launch-settings-fields': {}, '#launch-settings-status': {},
    '#launch-settings-dialog': { showModal() { this.open = true; }, close() { this.open = false; } },
  };
  const calls = [];
  let pending;
  const context = contextFor({
    $: selector => nodes[selector], state: { projects: [{ id: 'p', name: 'Projet <test>', frontDirectory: 'web', frontCommand: 'npm start' }], devServers: [] },
    devRunning: () => false, action: fn => { pending = fn(); }, encodeURIComponent,
    FormData: class { constructor(form) { return Object.entries(form.elements).map(([name, field]) => [name, field.value]); } },
    api: async (...args) => calls.push(args), async refresh() {}, toast() {},
  });
  vm.runInContext(source.slice(source.indexOf('function launchSettingsFields('), source.indexOf('function openProjectSettings(')), context);
  context.openLaunchSettings('p');
  assert.equal(nodes['#launch-settings-title'].textContent, 'Projet <test>');
  assert.equal(elements.frontDirectory.value, 'web');
  assert.equal(elements.frontCommand.value, 'npm start');
  assert.match(nodes['#launch-settings-fields'].innerHTML, /name="backDirectory"/);
  elements.backDirectory.value = 'api'; elements.backCommand.value = 'dotnet run';
  form.onsubmit({ preventDefault() {}, target: form }); await pending;
  assert.equal(JSON.stringify(calls), JSON.stringify([['/api/projects/p', 'PATCH', { frontCommand: 'npm start', backCommand: 'dotnet run', frontDirectory: 'web', backDirectory: 'api', frontPort: '', backPort: '', frontRepo: '', backRepo: '' }]]));
  assert.equal(nodes['#launch-settings-dialog'].open, false);
  assert.equal(button.disabled, false);
  context.openLaunchSettings('p');
  context.api = async () => { throw new Error('échec'); };
  form.onsubmit({ preventDefault() {}, target: form });
  await assert.rejects(pending, /échec/);
  assert.equal(nodes['#launch-settings-dialog'].open, true);
  assert.equal(button.disabled, false);
});

test('launch selections use each worktree independently, skip running projects and report partial failures', async () => {
  const calls = [];
  const context = view({ projects: [{ id: 'front', name: 'Front' }, { id: 'back', name: 'Back' }, { id: 'running' }], cards: [], devServers: [{ cardId: 'r', projectIds: ['running'], services: [{ status: 'running' }] }] });
  Object.assign(context, {
    launchingProjects: false, launchWorktrees: { front: 'f', back: 'b', running: 'r' }, cardPath: id => '/api/cards/' + id,
    api: async (...args) => { calls.push(args); if (args[0].includes('/f/')) throw new Error('échec'); },
    async refresh() {}, async refreshProcesses() {},
  });
  await assert.rejects(context.launchProjects(['front', 'back', 'running']), /Front : échec/);
  assert.equal(JSON.stringify(calls), JSON.stringify([
    ['/api/cards/f/dev-servers', 'POST', { standalone: true }], ['/api/cards/b/dev-servers', 'POST', { standalone: true }],
  ]));
  assert.equal(context.launchingProjects, false);
});

test('managed-port projects can select another worktree while the first one is running', () => {
  const project = { id: 'p', repo: 'repo', frontCommand: 'vite --port {port}' };
  const cards = ['a', 'b'].map(id => ({ id, title: id, repo: 'repo', worktree: { path: id } }));
  const session = { cardId: 'a', projectIds: ['p'], isolatedPorts: true, services: [{ id: 'p:front', status: 'running', processIds: [], endpoints: [] }] };
  const context = view({ projects: [project], cards, devServers: [session] });
  context.launchWorktrees.p = 'b';
  assert.equal(context.projectLaunchBlocked('p', 'b'), false);
  assert.equal(context.projectLaunchBlocked('p', 'a'), true);
  assert.match(context.processesMarkup(), /value="b" selected/);
  assert.match(context.processesMarkup(), /data-launch-one="p" >Lancer/);
  session.isolatedPorts = false;
  assert.equal(context.projectLaunchBlocked('p', 'b'), true);
});

test('detection offers .NET profiles, preserves manual commands and applies only selected profiles', async () => {
  const form = { elements: Object.fromEntries(['frontCommand', 'frontDirectory', 'backCommand', 'backDirectory'].map(name => [name, { value: '' }])) };
  form.elements.backCommand.value = 'manual command';
  const selects = ['front', 'back'].map(role => ({ dataset: { detectedRole: role }, value: '' }));
  const profileSelect = { value: '' }, hosts = { front: { innerHTML: '' }, back: { innerHTML: '', querySelector: () => profileSelect } };
  const results = { innerHTML: '', querySelectorAll: () => selects, querySelector: selector => hosts[selector.includes('"front"') ? 'front' : 'back'] };
  const button = {}, dialog = { open: true };
  form.dataset = { launchProjectId: 'p' };
  form.querySelector = selector => selector === '[data-detect-launch]' ? button : results;
  form.closest = () => dialog;
  const candidate = { technology: 'ASP.NET Core', file: 'Api.csproj', directory: 'src/Api', role: 'back', automaticPort: true, command: 'detected http command', baseCommand: 'dotnet run --project "Api.csproj"', defaultProfile: 'http', profiles: [
    { name: 'http', commandName: 'Project', urls: ['http://localhost:5050'], supported: true },
    { name: 'https', commandName: 'Project', urls: ['https://localhost:7050'], supported: true },
    { name: 'IIS Express', commandName: 'IISExpress', urls: [], supported: false },
  ] };
  const context = contextFor({ esc, encodeURIComponent, api: async () => ({ candidates: [candidate], warnings: ['<unsafe>'] }) });
  vm.runInContext(source.slice(source.indexOf('async function detectLaunchSettings('), source.indexOf('let editingLaunchProject =')), context);
  await context.detectLaunchSettings(form, 'p');
  assert.equal(form.elements.backCommand.value, 'manual command');
  assert.match(results.innerHTML, /&lt;unsafe&gt;/);
  assert.doesNotMatch(results.innerHTML.split('data-detected-role="front"')[1].split('</select>')[0], /Api.csproj/);
  selects[1].value = '0'; selects[1].onchange();
  assert.equal(form.elements.backDirectory.value, 'src/Api');
  assert.equal(profileSelect.value, 'http');
  assert.match(hosts.back.innerHTML, /value="IIS Express" disabled/);
  profileSelect.value = 'https'; profileSelect.onchange();
  assert.match(form.elements.backCommand.value, /--launch-profile "https" -- --urls http:\/\/127\.0\.0\.1:\{port\}/);
  assert.throws(() => context.detectedProfileCommand(candidate, 'IIS Express'), /non pris en charge/);
  context.api = async () => { throw new Error('détection indisponible'); };
  await assert.rejects(context.detectLaunchSettings(form, 'p'), /indisponible/);
  assert.equal(button.disabled, false);
  const previousResults = results.innerHTML;
  let complete;
  context.api = () => new Promise(resolve => { complete = resolve; });
  const detection = context.detectLaunchSettings(form, 'p');
  form.dataset.launchProjectId = 'another-project';
  complete({ candidates: [], warnings: [] });
  await detection;
  assert.equal(results.innerHTML, previousResults, 'a late response must not fill another project');
  form.dataset.launchProjectId = 'p';
  form.elements.repo = { value: 'new-location' };
  context.api = async (path, method, body) => { assert.equal(method, 'POST'); assert.equal(body.repo, 'new-location'); return { candidates: [], warnings: [] }; };
  await context.detectLaunchSettings(form, 'p');
});

test('project location picker updates the editable path, preserves it on cancel and recovers after failure', async () => {
  const button = {}, form = { elements: { repo: { value: 'old', readOnly: false } } };
  const context = contextFor({ $: selector => selector === '#browse-project-settings' ? button : form, action: fn => fn(), api: async () => ({ path: 'new' }) });
  vm.runInContext(source.slice(source.indexOf("$('#browse-project-settings').onclick"), source.indexOf('function launchSettingsFields(')), context);
  await button.onclick();
  assert.equal(form.elements.repo.value, 'new');
  assert.equal(button.disabled, false);
  context.api = async () => ({});
  await button.onclick();
  assert.equal(form.elements.repo.value, 'new');
  context.api = async () => { throw new Error('picker failed'); };
  await assert.rejects(button.onclick(), /picker failed/);
  assert.equal(button.disabled, false);
});

test('service repository pickers and detection keep Front and Back independent', async () => {
  const frontBrowse = { dataset: { browseServiceRepo: 'front' } }, backBrowse = { dataset: { browseServiceRepo: 'back' } };
  const frontDetect = { dataset: { detectRole: 'front' } }, backDetect = { dataset: { detectRole: 'back' } }, allDetect = {};
  const form = { dataset: { launchProjectId: 'p' }, elements: { frontRepo: { value: '' }, backRepo: { value: 'back-original' } }, querySelector: () => allDetect, querySelectorAll: selector => selector === '[data-detect-role]' ? [frontDetect, backDetect] : [frontBrowse, backBrowse] };
  const detections = [];
  const context = contextFor({ action: fn => fn(), api: async () => ({ path: 'front-selected' }), detectLaunchSettings: async (...args) => detections.push(args) });
  vm.runInContext(source.slice(source.indexOf('function bindLaunchSettings('), source.indexOf('async function detectLaunchSettings(')), context);
  context.bindLaunchSettings(form, 'p');
  await frontBrowse.onclick();
  assert.equal(form.elements.frontRepo.value, 'front-selected');
  assert.equal(form.elements.backRepo.value, 'back-original');
  await frontDetect.onclick(); await backDetect.onclick();
  assert.equal(detections[0][2], 'front');
  assert.equal(detections[1][2], 'back');
});

test('project settings include and wire the shared detection button to the project being edited', async () => {
  const fields = ['repo', 'frontRepo', 'backRepo', 'agent', 'baseBranch', 'repositoryConnector', 'jiraMode', 'frontDirectory', 'backDirectory', 'backCommand', 'frontPort', 'backPort', 'prTarget', 'testCommand', 'automationEnabled', 'autoFix', 'linkedProjectId'];
  const button = {}, results = { innerHTML: 'previous detection' }, sections = [], calls = [];
  const form = { dataset: {}, querySelectorAll: () => [], elements: Object.fromEntries(fields.map(name => [name, {}])), querySelector: selector => {
    if (selector === '[data-detect-launch]') return button;
    if (selector === '[data-launch-detection-results]') return results;
    return { before(section) { sections.push(section); form.elements.frontCommand = {}; } };
  } };
  const nodes = { '#project-repository-connector': {}, '#project-settings-form': form, '#project-settings-title': {}, '#project-jira-map': {}, '#browse-project-settings': {}, '#project-location-help': {}, '#project-settings-dialog': { showModal() {} } };
  const context = contextFor({
    $: selector => nodes[selector], document: { createElement: () => ({}) }, state: { cards: [], projects: [{ id: 'p', name: 'Premier', repo: 'first' }, { id: 'q', name: 'Second', repo: 'second' }] },
    names: {}, esc, action: fn => fn(), detectLaunchSettings: async (...args) => calls.push(args),
  });
  vm.runInContext(source.slice(source.indexOf('function launchSettingsFields('), source.indexOf('async function detectLaunchSettings(')), context);
  vm.runInContext(source.slice(source.indexOf('function openProjectSettings('), source.indexOf("$('#project-settings-form').onsubmit")), context);
  context.openProjectSettings('p');
  assert.match(sections[0].innerHTML, /data-detect-launch>Détecter la configuration du projet/);
  assert.match(sections[0].innerHTML, /data-launch-detection-results/);
  await button.onclick();
  assert.equal(calls[0][0], form);
  assert.equal(calls[0][1], 'p');
  results.innerHTML = 'results for p';
  context.openProjectSettings('q');
  assert.equal(results.innerHTML, '');
  assert.equal(form.dataset.launchProjectId, 'q');
  await button.onclick();
  assert.equal(calls[1][1], 'q');
  assert.equal(form.elements.repo.value, 'second');
  assert.equal(form.elements.repo.readOnly, false);
  context.state.cards = [{ repo: 'second', worktree: { path: 'work' } }];
  context.openProjectSettings('q');
  assert.equal(form.elements.repo.readOnly, true);
  assert.equal(nodes['#browse-project-settings'].disabled, true);
  assert.equal(nodes['#project-repository-connector'].hidden, true);
  context.state.connectors = { providers: { repositories: 'azure' }, modules: [{ id: 'azure', name: 'Azure DevOps', capabilities: ['repositories'] }, { id: 'github', name: 'GitHub', capabilities: ['repositories'] }, { id: 'jira', name: 'Jira', capabilities: ['tickets'] }] };
  context.state.projects[1].repositoryConnector = 'github';
  context.openProjectSettings('q');
  assert.equal(nodes['#project-repository-connector'].hidden, false);
  assert.equal(form.elements.repositoryConnector.value, 'github');
  assert.match(form.elements.repositoryConnector.innerHTML, /Hériter du fournisseur général \(Azure DevOps\)/);
  assert.doesNotMatch(form.elements.repositoryConnector.innerHTML, /Jira/);
  context.state.projects[1].repositoryConnector = 'removed';
  context.openProjectSettings('q');
  assert.match(form.elements.repositoryConnector.innerHTML, /disabled>Module indisponible : removed/);
});
test('process buttons send scoped start, stop and restart requests and recover after failure', async () => {
  for (const operation of ['start', 'stop', 'restart']) {
    const calls = [], button = { dataset: { processAction: operation, processCard: 'a', serviceId: 'p:front' }, isConnected: true };
    const context = contextFor({
      document: { querySelectorAll: () => [button] }, action: fn => fn(), cardPath: id => '/api/cards/' + id,
      api: async (...args) => calls.push(args), async refresh() {}, async refreshProcesses() {},
    });
    const start = source.indexOf("  document.querySelectorAll('[data-process-action]')");
    vm.runInContext(source.slice(start, source.indexOf("  document.querySelectorAll('[data-go-view]')", start)), context);
    await button.onclick();
    assert.equal(JSON.stringify(calls), JSON.stringify([['/api/cards/a/dev-servers', operation === 'stop' ? 'DELETE' : 'POST', { serviceId: 'p:front', ...(operation === 'restart' ? { restart: true } : {}) }]]));
    assert.equal(button.disabled, false);
    context.api = async () => { throw new Error('launch failed'); };
    await assert.rejects(button.onclick(), /launch failed/);
    assert.equal(button.disabled, false);
  }
});

test('starting an entire project from the process view also uses the worktree launcher', async () => {
  const calls = [], button = { dataset: { processAction: 'start', processCard: 'a' }, isConnected: true };
  const context = contextFor({
    document: { querySelectorAll: () => [button] }, action: fn => fn(), cardPath: id => '/api/cards/' + id,
    api: async (...args) => calls.push(args), async refresh() {}, async refreshProcesses() {},
  });
  const start = source.indexOf("  document.querySelectorAll('[data-process-action]')");
  vm.runInContext(source.slice(start, source.indexOf("  document.querySelectorAll('[data-go-view]')", start)), context);
  await button.onclick();
  assert.equal(JSON.stringify(calls), JSON.stringify([['/api/cards/a/dev-servers', 'POST', { standalone: true }]]));
});

test('stop-all attempts every running stack in the current view even if one stop fails', async () => {
  const calls = [], button = {};
  const context = contextFor({
    $: () => button, project: { id: 'visible-project' }, action: fn => fn(),
    processCards: project => { assert.equal(project.id, 'visible-project'); return [{ id: 'a' }, { id: 'b' }, { id: 'stopped' }]; },
    devRunning: id => id !== 'stopped', cardPath: id => '/api/cards/' + id, async refresh() {},
    api: async path => { calls.push(path); if (path.includes('/a/')) throw new Error('stop failed'); },
  });
  const start = source.indexOf("  if ($('#stop-visible-processes'))");
  vm.runInContext(source.slice(start, source.indexOf("  document.querySelectorAll('[data-process-action]')", start)), context);
  await assert.rejects(button.onclick(), /stop failed/);
  assert.deepEqual(calls.sort(), ['/api/cards/a/dev-servers', '/api/cards/b/dev-servers']);
});

function contextFor(values) { const context = vm.createContext({ sameRepository, ...values }); context.getState = () => context.state; return context; }
