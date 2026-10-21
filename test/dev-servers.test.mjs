import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { createDevServers } from '../lib/dev-servers.mjs';
import { projectSettings } from '../lib/project-settings.mjs';
import { execFileSync } from 'node:child_process';
import { createApp } from '../server.mjs';

async function waitFor(check, timeout = 10000) {
  const deadline = Date.now() + timeout;
  while (!await check()) { assert.ok(Date.now() < deadline, 'process readiness timeout'); await new Promise(resolve => setTimeout(resolve, 30)); }
}
test('front and back run in the worktree, log failures, prevent port conflicts and stop', async () => {
  const path = mkdtempSync(join(tmpdir(), 'atelier-dev-'));
  const servers = createDevServers(() => {});
  try {
    writeFileSync(join(path, 'serve.cjs'), "console.log(process.cwd()); setInterval(() => {}, 1000);");
    const command = `"${process.execPath}" serve.cjs`;
    const project = { id: 'project', ...projectSettings({ frontCommand: command, backCommand: command }) };
    const card = { id: 'a', worktree: { path } };
    assert.throws(() => servers.start({ id: 'missing' }, project), /worktree/);
    assert.throws(() => servers.start(card, { id: 'empty' }), /commandes/);
    servers.start(card, project);
    await waitFor(() => servers.state()[0].log.includes('[Front]') && servers.state()[0].log.includes('[Back]'));
    assert.ok(servers.state()[0].log.includes(path), servers.state()[0].log);
    assert.equal(servers.running('a'), true);
    assert.throws(() => servers.start({ ...card, id: 'b' }, project), /ports/);
    await servers.stop('a');
    assert.equal(servers.running('a'), false);
    servers.start(card, { ...project, frontCommand: `"${process.execPath}" -e "process.exit(7)"`, backCommand: '' });
    await waitFor(() => !servers.running('a'));
    assert.equal(servers.state()[0].services[0].exitCode, 7);
    assert.throws(() => projectSettings({ frontCommand: 42 }), /invalide/);
  } finally { await servers.close(); rmSync(path, { recursive: true, force: true }); }
});

test('linked projects run from the task worktree and companion source, in either direction', async () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'atelier-linked-dev-')));
  const servers = createDevServers(() => {});
  try {
    const [frontSource, frontWorktree, backSource, backWorktree] = ['front-source', 'front-worktree', 'back-source', 'back-worktree'].map(name => join(root, name));
    for (const path of [frontSource, frontWorktree, backSource, backWorktree]) {
      mkdirSync(path);
      writeFileSync(join(path, 'serve.cjs'), 'console.log(process.cwd()); setInterval(() => {}, 1000);');
    }
    const command = `"${process.execPath}" serve.cjs`;
    const front = { id: 'front', name: 'TOTO', repo: frontSource, frontCommand: command, linkedProjectId: 'back' };
    const back = { id: 'back', name: 'TOTO2', repo: backSource, backCommand: command, linkedProjectId: 'front' };
    const frontCard = { id: 'f', worktree: { path: frontWorktree } }, backCard = { id: 'b', worktree: { path: backWorktree } };
    assert.throws(() => servers.start(frontCard, front), /introuvable/);
    assert.throws(() => servers.start(frontCard, front, front), /introuvable|lui-même/);
    assert.throws(() => servers.start(frontCard, front, { ...back, repo: join(root, 'missing') }), /ENOENT/);
    assert.throws(() => servers.start(frontCard, front, { ...back, backCommand: '' }), /commandes/);
    assert.deepEqual(servers.state(), [], 'invalid companion never starts the task service');
    servers.start(frontCard, front, back);
    await waitFor(() => servers.state()[0].log.includes(frontWorktree) && servers.state()[0].log.includes(backSource));
    assert.deepEqual(servers.state()[0].projectIds, ['front', 'back']);
    assert.deepEqual(servers.state()[0].services.map(service => [service.name, service.repo]), [['TOTO · Front', frontWorktree], ['TOTO2 · Back', backSource]]);
    assert.throws(() => servers.start(backCard, back, front), /ports/);
    assert.throws(() => servers.start({ ...backCard, id: 'b2' }, { ...back, linkedProjectId: '' }), /ports/, 'companion cannot also start alone');
    assert.throws(() => servers.start({ ...frontCard, id: 'f2' }, { id: 'other', frontCommand: command, linkedProjectId: 'back' }, back), /ports/, 'another stack cannot reuse the companion');
    await servers.stop(frontCard.id);
    assert.ok(servers.state()[0].services.every(service => service.status === 'stopped'));
    servers.start(backCard, back, front);
    assert.throws(() => servers.startService(frontCard.id, 'front:front'), /ports/);
    await waitFor(() => servers.state().find(session => session.cardId === 'b').log.includes(backWorktree) && servers.state().find(session => session.cardId === 'b').log.includes(frontSource));
    assert.deepEqual(servers.state().find(session => session.cardId === 'b').services.map(service => service.repo), [backWorktree, frontSource]);
    await servers.close();
    assert.equal(servers.running('b'), false);
    assert.throws(() => projectSettings({ linkedProjectId: [] }), /invalide/);
  } finally { await servers.close(); rmSync(root, { recursive: true, force: true }); }
});

test('individual services keep their sibling alive and report ports owned by descendants', async () => {
  const path = mkdtempSync(join(tmpdir(), 'atelier-services-'));
  let failInspection = false;
  const servers = createDevServers(() => {}, async () => {
    if (failInspection) throw new Error('inspection unavailable');
    const [front, back] = servers.state()[0].services;
    return {
      processes: [{ ProcessId: front.pid + 100000, ParentProcessId: front.pid }],
      listeners: [{ pid: front.pid + 100000, address: '127.0.0.1', port: 5173 }, { pid: back.pid, address: '::', port: 5080 }, { pid: 1, address: '0.0.0.0', port: 80 }],
    };
  });
  try {
    const command = `"${process.execPath}" -e "setInterval(() => {}, 1000)"`;
    const card = { id: 'a', worktree: { path } }, project = { id: 'p', frontCommand: command, backCommand: command };
    servers.start(card, project);
    let [front, back] = (await servers.refresh())[0].services;
    assert.deepEqual(front.endpoints.map(endpoint => endpoint.port), [5173]);
    assert.deepEqual(back.endpoints.map(endpoint => endpoint.port), [5080]);
    assert.ok(front.processIds.includes(front.pid + 100000));
    assert.equal(back.endpoints[0].url, 'http://[::1]:5080/');
    const oldFrontPid = front.pid, backPid = back.pid;
    await servers.stop('a', front.id);
    assert.equal(front.status, 'stopped');
    assert.deepEqual(front.endpoints, []);
    assert.equal(back.status, 'running');
    assert.equal(back.pid, backPid);
    servers.startService('a', front.id);
    assert.notEqual(front.pid, oldFrontPid);
    assert.equal(back.pid, backPid);
    assert.throws(() => servers.startService('a', front.id), /déjà/);
    await assert.rejects(servers.stop('a', 'unknown'), /introuvable/);
    assert.throws(() => servers.startService('a', 'unknown'), /introuvable/);
    failInspection = true;
    await servers.refresh();
    assert.match(front.inspectionError, /indisponible/);
    assert.deepEqual(front.endpoints, []);
    assert.equal(front.status, 'running', 'inspection failure never stops a service');
  } finally { await servers.close(); rmSync(path, { recursive: true, force: true }); }
});

test('Windows detects the actual listening port of a launched child process', { skip: process.platform !== 'win32' }, async () => {
  const path = mkdtempSync(join(tmpdir(), 'atelier-live-port-'));
  const servers = createDevServers(() => {});
  try {
    writeFileSync(join(path, 'listen.cjs'), "const s = require('node:net').createServer(); s.listen(0, '127.0.0.1', () => console.log(JSON.stringify({port:s.address().port,pid:process.pid})));");
    servers.start({ id: 'a', worktree: { path } }, { id: 'p', frontCommand: `"${process.execPath}" listen.cjs` });
    await waitFor(() => servers.state()[0].log.includes('"port":'));
    const expected = JSON.parse(servers.state()[0].log.match(/\{[^\n]+\}/)[0]);
    let service;
    // A cold Windows CIM provider can time out once; the UI also refreshes until inspection recovers.
    await waitFor(async () => {
      service = (await servers.refresh())[0].services[0];
      return service.endpoints.some(endpoint => endpoint.pid === expected.pid && endpoint.port === expected.port);
    }, 60000);
    assert.equal(service.inspectionError, '');
    assert.ok(service.processIds.includes(expected.pid));
    assert.ok(service.endpoints.some(endpoint => endpoint.pid === expected.pid && endpoint.port === expected.port && endpoint.address === '127.0.0.1'));
    await servers.stop('a');
    assert.deepEqual((await servers.refresh())[0].services[0].endpoints, []);
  } finally { await servers.close(); rmSync(path, { recursive: true, force: true }); }
});

test('API validates the worktree, saves commands and blocks removal until servers stop', async () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'atelier-dev-api-')));
  const git = (...args) => execFileSync('git', ['-C', root, ...args], { windowsHide: true, stdio: 'ignore' });
  git('init', '-b', 'master');
  git('-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '--allow-empty', '-m', 'initial');
  git('remote', 'add', 'origin', root);
  const app = createApp({ dataDir: join(root, 'data') });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${app.server.address().port}`;
    const request = (path, method, body = {}) => fetch(url + path, { method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, body: JSON.stringify(body) });
    const project = await (await request('/api/projects', 'POST', { name: 'Test', repo: root })).json();
    const companionRepo = join(root, 'companion'); mkdirSync(companionRepo);
    const companion = await (await request('/api/projects', 'POST', { name: 'Companion', repo: companionRepo })).json();
    const command = `"${process.execPath}" -e "console.log(process.cwd()); setInterval(() => {}, 1000)"`;
    assert.equal((await request('/api/projects/' + companion.id, 'PATCH', { backCommand: command })).status, 200);
    for (const linkedProjectId of [project.id, 'missing', 42]) assert.equal((await request('/api/projects/' + project.id, 'PATCH', { linkedProjectId })).status, 400);
    const saved = await request('/api/projects/' + project.id, 'PATCH', { frontCommand: command, linkedProjectId: companion.id });
    assert.equal(saved.status, 200);
    assert.equal((await saved.json()).linkedProjectId, companion.id);
    const card = await (await request('/api/cards', 'POST', { title: 'Test', repo: root })).json();
    const path = '/api/cards/' + card.id;
    assert.equal((await request(path + '/dev-servers', 'POST')).status, 400);
    assert.equal(app.store.card(card.id).worktree, undefined);
    assert.equal((await request(path + '/worktree', 'POST')).status, 200);
    assert.equal((await request(path + '/dev-servers', 'POST')).status, 200);
    assert.equal((await request(path + '/worktree', 'DELETE')).status, 409);
    assert.equal((await request('/api/projects/' + project.id, 'DELETE')).status, 409);
    assert.equal((await request('/api/projects/' + companion.id, 'DELETE')).status, 409);
    const session = (await (await fetch(url + '/api/state', { headers: { Authorization: 'Bearer ' + app.authToken } })).json()).devServers[0];
    assert.equal(session.services[0].status, 'running');
    assert.equal(session.services.length, 2);
    assert.equal(session.services[0].repo, app.store.card(card.id).worktree.path);
    assert.equal(session.services[1].repo, companionRepo);
    const [frontService, backService] = session.services;
    for (const body of [{ serviceId: 42 }, { serviceId: '' }, { restart: true }, { serviceId: frontService.id, restart: 'yes' }, { serviceId: 'unknown', restart: true }]) assert.equal((await request(path + '/dev-servers', 'POST', body)).status, 400);
    assert.equal((await request(path + '/dev-servers', 'DELETE', { serviceId: frontService.id })).status, 200);
    let current = (await (await fetch(url + '/api/state', { headers: { Authorization: 'Bearer ' + app.authToken } })).json()).devServers[0];
    assert.equal(current.services[0].status, 'stopped');
    assert.equal(current.services[1].pid, backService.pid);
    assert.equal(current.services[1].status, 'running');
    assert.equal((await request(path + '/dev-servers', 'POST', { serviceId: frontService.id })).status, 200);
    current = (await (await fetch(url + '/api/state', { headers: { Authorization: 'Bearer ' + app.authToken } })).json()).devServers[0];
    const restartedPid = current.services[0].pid;
    assert.notEqual(restartedPid, frontService.pid);
    assert.equal((await request(path + '/dev-servers', 'POST', { serviceId: frontService.id, restart: true })).status, 200);
    current = (await (await fetch(url + '/api/state', { headers: { Authorization: 'Bearer ' + app.authToken } })).json()).devServers[0];
    assert.notEqual(current.services[0].pid, restartedPid);
    assert.equal(current.services[1].pid, backService.pid);
    const inspected = await fetch(url + '/api/dev-servers', { headers: { Authorization: 'Bearer ' + app.authToken } });
    assert.equal(inspected.status, 200);
    assert.equal((await inspected.json())[0].services.length, 2);
    assert.equal((await request(path + '/dev-servers', 'DELETE')).status, 200);
    assert.equal((await request(path + '/dev-servers', 'POST', { standalone: 'yes' })).status, 400);
    assert.equal((await request(path + '/dev-servers', 'POST', { standalone: true })).status, 200);
    const standalone = (await (await fetch(url + '/api/state', { headers: { Authorization: 'Bearer ' + app.authToken } })).json()).devServers.find(session => session.cardId === card.id);
    assert.deepEqual(standalone.projectIds, [project.id]);
    assert.equal(standalone.services.length, 1);
    assert.equal(standalone.services[0].repo, app.store.card(card.id).worktree.path);
    assert.equal((await request(path + '/dev-servers', 'DELETE')).status, 200);
    assert.equal((await request(path + '/worktree', 'DELETE')).status, 200);
    assert.equal((await request('/api/projects/' + companion.id, 'DELETE')).status, 200);
    assert.equal((await request(path + '/worktree', 'POST')).status, 200);
    assert.equal((await request(path + '/dev-servers', 'POST')).status, 400, 'a removed companion must not be silently skipped');
    assert.equal((await request(path + '/worktree', 'DELETE')).status, 200);
  } finally { await app.close(); rmSync(root, { recursive: true, force: true }); }
});
