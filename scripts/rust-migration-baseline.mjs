import assert from 'node:assert/strict';
import { fork, spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtempSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { cpus, release, tmpdir, totalmem } from 'node:os';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { openStore } from '../lib/store.mjs';
import { seedMigrationData, migrationShape } from '../test/fixtures/migration-data.mjs';

const here = fileURLToPath(import.meta.url);
const rounded = value => Math.round(value * 100) / 100;

async function browserBaseline(executable, address, token) {
  assert.ok(existsSync(executable), 'Exécutable Chromium/Edge requis.');
  const profile = mkdtempSync(join(tmpdir(), 'atelier-migration-browser-'));
  const child = spawn(executable, ['--headless=new', '--no-first-run', '--disable-background-networking', '--window-size=1440,1000', '--remote-debugging-port=0', '--user-data-dir=' + profile, 'about:blank'], { windowsHide: true, stdio: 'ignore' });
  const exited = once(child, 'exit');
  let socket;
  try {
    const portFile = join(profile, 'DevToolsActivePort');
    for (let i = 0; !existsSync(portFile) && i < 200; i++) { assert.equal(child.exitCode, null, 'Navigateur arrêté avant disponibilité.'); await delay(50); }
    assert.ok(existsSync(portFile), 'Port DevTools absent après 10 s.');
    const port = Number(readFileSync(portFile, 'utf8').split('\n')[0]);
    assert.ok(Number.isInteger(port) && port > 0 && port < 65536);
    const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
    const target = targets.find(item => item.type === 'page');
    assert.ok(target?.webSocketDebuggerUrl?.startsWith(`ws://127.0.0.1:${port}/`));
    socket = new WebSocket(target.webSocketDebuggerUrl);
    await once(socket, 'open', { signal: AbortSignal.timeout(5000) });
    let id = 0;
    const pending = new Map();
    socket.onmessage = event => {
      const message = JSON.parse(event.data), callback = pending.get(message.id);
      if (callback) { pending.delete(message.id); callback(message); }
    };
    const command = (method, params = {}) => new Promise((resolve, reject) => {
      const current = ++id;
      const timeout = setTimeout(() => { pending.delete(current); reject(new Error('Délai DevTools : ' + method)); }, 30000);
      pending.set(current, message => { clearTimeout(timeout); message.error ? reject(new Error(message.error.message)) : resolve(message.result); });
      socket.send(JSON.stringify({ id: current, method, params }));
    });
    const evaluate = async expression => {
      const result = await command('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      assert.equal(result.exceptionDetails, undefined, 'Échec du scénario navigateur.');
      return result.result.value;
    };
    const version = await command('Browser.getVersion');
    await command('Page.enable');
    await command('Page.addScriptToEvaluateOnNewDocument', { source: `
      window.migrationReady = null;
      const timer = setInterval(() => {
        if (document.querySelectorAll('#board .card').length === ${migrationShape.cards} && document.querySelector('#live-connection.connected')) {
          clearInterval(timer);
          requestAnimationFrame(() => requestAnimationFrame(() => { window.migrationReady = performance.now(); }));
        }
      }, 10);
    ` });
    const samples = [];
    for (let i = 0; i < 5; i++) {
      await command('Page.navigate', { url: 'about:blank' });
      await command('Page.navigate', { url: address + '/#view=board&session=' + token });
      let ready;
      for (let attempt = 0; attempt < 400; attempt++) {
        ready = await evaluate('window.migrationReady');
        if (ready) break;
        await delay(50);
      }
      assert.ok(ready > 0, 'Tableau et connexion SSE non prêts après 20 s.');
      samples.push(rounded(ready));
    }
    // Exercise the real local mutation -> SSE -> board refresh path with synthetic data only.
    const liveMs = await evaluate(`(async () => {
      const start = performance.now();
      const response = await fetch('/api/cards/card-0', { method: 'PATCH', headers: { Authorization: 'Bearer ' + localStorage.getItem('atelier-session'), 'Content-Type': 'application/json', 'X-Local-Request': '1' }, body: JSON.stringify({ title: 'Tâche synthétique actualisée' }) });
      if (!response.ok) throw new Error('Mutation refusée');
      for (let i = 0; i < 400; i++) {
        if (document.querySelector('[data-id="card-0"] .card-title')?.textContent === 'Tâche synthétique actualisée') return performance.now() - start;
        await new Promise(resolve => setTimeout(resolve, 10));
      }
      throw new Error('Actualisation SSE absente');
    })()`);
    const logsMs = await evaluate(`(async () => {
      const start = performance.now();
      document.querySelector('[data-id="card-0"] .card-title').click();
      for (let i = 0; i < 2000; i++) {
        if (document.querySelector('#log')?.textContent.includes('Ligne synthétique 9999 — é 🔧')) {
          await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
          return performance.now() - start;
        }
        await new Promise(resolve => setTimeout(resolve, 10));
      }
      throw new Error('Journal incomplet');
    })()`);
    return { browser: version.product, viewport: await evaluate('({ width: innerWidth, height: innerHeight, devicePixelRatio })'), mode: 'headless, profil neuf puis quatre navigations, HTTP no-store, deux frames après tableau et SSE prêts', boardReadyMs: samples, mutationToBoardMs: rounded(liveMs), openCardWith10000LogsMs: rounded(logsMs) };
  } finally {
    socket?.close();
    try { if (child.exitCode === null) child.kill(); await exited; }
    finally { rmSync(profile, { recursive: true, force: true, maxRetries: 30, retryDelay: 100 }); }
  }
}

async function serve() {
  assert.ok(process.send, 'Mode interne : canal IPC requis.');
  const [{ directory }] = await once(process, 'message');
  const { createApp } = await import('../server.mjs');
  const refuse = () => { throw new Error('Effet externe interdit dans la référence synthétique.'); };
  const app = createApp({ dataDir: directory, builtinModules: [], launch: refuse, requestJira: refuse, requestTempo: refuse, verify: refuse, previewPR: refuse, publishPR: refuse, refreshPR: refuse, reviewPR: refuse, installAgentUpdate: refuse, readAgentAccount: refuse, pickFolder: refuse,
    probe: async () => ({ installed: false, connected: false }), protectTokens: refuse, updateOptions: { repository: '', releases: null, installer: null } });
  app.server.listen(0, '127.0.0.1');
  await once(app.server, 'listening');
  process.send({ port: app.server.address().port, token: app.authToken });
  let stopping = false;
  const stop = async () => { if (stopping) return; stopping = true; await app.close(); if (process.connected) process.disconnect(); };
  process.on('message', async message => {
    if (message === 'memory') process.send({ memory: process.memoryUsage() });
    if (message === 'stop') await stop();
  });
  process.on('disconnect', () => { stop().catch(() => {}); });
}

async function baseline() {
  const args = process.argv.slice(2);
  assert.ok(args.length === 0 || args.length === 2 && args[0] === '--browser', 'Usage : rust-migration-baseline.mjs [--browser chemin-exécutable]');
  const directory = mkdtempSync(join(tmpdir(), 'atelier-migration-baseline-'));
  const result = { platform: process.platform, arch: process.arch, osRelease: release(), cpu: cpus()[0]?.model, logicalCpus: cpus().length, ramBytes: totalmem(), node: process.versions.node, fixture: migrationShape,
    scope: 'createApp en processus neuf ; données déjà créées ; sans tray, lanceur, coffre réel ni services distants ; cache disque OS non purgé', samples: [] };
  let child;
  try {
    const store = openStore(join(directory, 'kanban.sqlite'));
    try { seedMigrationData(store); } finally { store.db.close(); }
    for (let i = 0; i < 5; i++) {
      const start = performance.now();
      child = fork(here, ['--serve'], { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe', 'ipc'], env: Object.fromEntries(Object.entries(process.env).filter(([key]) => /^(path|systemroot|windir|temp|tmp)$/i.test(key))) });
      const exited = once(child, 'exit');
      const ready = once(child, 'message', { signal: AbortSignal.timeout(15000) });
      child.send({ directory });
      const [{ port, token }] = await ready;
      const address = `http://127.0.0.1:${port}`;
      const request = async path => {
        const response = await fetch(address + path, { headers: { Authorization: 'Bearer ' + token }, signal: AbortSignal.timeout(10000) });
        assert.equal(response.status, 200, path);
        return response.json();
      };
      assert.equal((await request('/api/health')).app, 'atelier-kanban');
      const sample = { readyMs: rounded(performance.now() - start), stateMs: [] };
      for (let j = 0; j < 20; j++) {
        const before = performance.now(), state = await request('/api/state');
        sample.stateMs.push(rounded(performance.now() - before));
        assert.equal(state.cards.length, migrationShape.cards);
        assert.equal(state.projects.length, migrationShape.projects);
      }
      const logStart = performance.now();
      let after = 0, count = 0;
      for (;;) {
        const page = await request('/api/runs/run-0/logs?after=' + after);
        if (!page.length) break;
        assert.ok(page[0].seq > after && page.length <= 500);
        after = page.at(-1).seq; count += page.length;
      }
      assert.equal(count, migrationShape.logs);
      sample.logsMs = rounded(performance.now() - logStart);
      await delay(500);
      const memory = once(child, 'message', { signal: AbortSignal.timeout(5000) }); child.send('memory');
      sample.memory = (await memory)[0].memory;
      if (i === 4 && args.length) result.ui = await browserBaseline(args[1], address, token);
      result.samples.push(sample);
      child.send('stop');
      const timeout = setTimeout(() => child.kill(), 10000);
      try { assert.equal((await exited)[0], 0, 'Arrêt serveur incomplet.'); } finally { clearTimeout(timeout); }
      child = null;
    }
    console.log(JSON.stringify(result, null, 2));
  } finally {
    if (child && child.exitCode === null) { const stopped = once(child, 'exit'); child.kill(); await stopped; }
    rmSync(directory, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
  }
}

if (import.meta.main) {
  if (process.argv[2] === '--serve') await serve();
  else await baseline();
}
