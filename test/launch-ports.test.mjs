import test from 'node:test';
import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import { EventEmitter } from 'node:events';
import { syncBuiltinESMExports } from 'node:module';
import { createDevServers } from '../lib/dev-servers.mjs';
import { reservePort } from '../lib/launch-ports.mjs';
import { projectSettings } from '../lib/project-settings.mjs';

test('native port reservations remain unique and occupied until released', async () => {
  const leases = [];
  try {
    for (let index = 0; index < 16; index++) leases.push(await reservePort());
    assert.equal(new Set(leases.map(lease => lease.port)).size, leases.length);
    const port = leases[0].port;
    assert.ok(port > 0 && port <= 65535);
    await assert.rejects(reservePort(port), { message: `Le port ${port} est indisponible. Choisissez un autre port ou laissez Atelier le choisir.` });
    await leases.shift().release();
    leases.push(await reservePort(port));
    assert.equal(leases.at(-1).port, port);
  } finally { await Promise.all(leases.map(lease => lease.release())); }
});

test('ports accept automatic or explicit values and require the command placeholder', () => {
  assert.equal(projectSettings({ frontCommand: 'vite --port {port}', frontPort: '5174' }).frontPort, 5174);
  assert.equal(projectSettings({}).frontPort, 0);
  for (const port of [-1, 65536, 'abc', true, null, 1.5]) assert.throws(() => projectSettings({ frontPort: port }), /Port invalide/);
  assert.throws(() => projectSettings({ frontCommand: 'vite', frontPort: 5173 }), /\{port\}/);
});

test('managed launches allocate unique ports per worktree, preserve commands, and reject occupied or reused fixed ports', async t => {
  const calls = [], children = [], servers = createDevServers(() => {});
  const spawn = t.mock.method(childProcess, 'spawn', (...args) => {
    calls.push(args);
    const child = new EventEmitter();
    child.stdout = new EventEmitter(); child.stderr = new EventEmitter();
    child.stdout.setEncoding = child.stderr.setEncoding = () => {};
    children.push(child);
    return child;
  });
  syncBuiltinESMExports();
  let occupied;
  try {
    const project = { id: 'p', frontCommand: 'vite --port {port}', backCommand: 'api --port {port}' };
    const card = { id: 'one', worktree: { path: process.cwd() } };
    const [first, second] = await Promise.all([servers.startManaged(card, project), servers.startManaged({ ...card, id: 'two' }, project)]);
    const ports = [...first.services, ...second.services].map(service => service.port);
    assert.equal(new Set(ports).size, 4);
    assert.equal(first.isolatedPorts, true);
    assert.equal(project.frontCommand, 'vite --port {port}', 'stored configuration is not rewritten');
    for (const [, args, options] of calls) {
      assert.ok(args.some(argument => argument.includes(options.env.PORT)));
      assert.ok(ports.includes(Number(options.env.PORT)));
      assert.equal(options.env.HOST, '127.0.0.1');
    }
    await assert.rejects(servers.startManaged(card, project), /ports/);
    await assert.rejects(servers.startManaged({ ...card, id: 'three' }, { ...project, frontPort: ports[0] }), /déjà attribué/);
    assert.throws(() => servers.start({ ...card, id: 'legacy' }, { id: 'p', frontCommand: 'vite' }), /ports/);
    occupied = await reservePort();
    const before = calls.length;
    await assert.rejects(servers.startManaged({ ...card, id: 'occupied' }, { id: 'other', frontCommand: 'vite --port {port}', frontPort: occupied.port }), /indisponible/);
    assert.equal(calls.length, before);
    children[calls.findIndex(([, , options]) => options.env.PORT === String(first.services[0].port))].emit('close', 0);
    await servers.startManagedService('one', first.services[0].id);
    assert.equal(calls.at(-1)[2].env.PORT, String(first.services[0].port), 'a service restart keeps its port');
  } finally {
    if (occupied) await occupied.release();
    for (const child of children) child.emit('close', 0);
    await servers.close(); spawn.mock.restore(); syncBuiltinESMExports();
  }
});
