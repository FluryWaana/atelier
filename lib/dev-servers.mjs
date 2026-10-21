import { samePath } from './paths.mjs';
import { spawn, execFile } from 'node:child_process';
import { realpathSync, statSync } from 'node:fs';
import { isAbsolute, relative, resolve, sep, win32 } from 'node:path';
import { descendantPids, inspectProcesses, listenerUrl } from './process-inspection.mjs';
import { reservePort } from './launch-ports.mjs';

export function serviceDirectory(repo, directory = '') {
  if (typeof directory !== 'string' || isAbsolute(directory) || win32.isAbsolute(directory)) throw new Error('Le dossier du service doit être relatif au worktree.');
  const root = realpathSync(repo), path = realpathSync(resolve(root, directory));
  const rel = relative(root, path);
  if (rel === '..' || rel.startsWith('..' + sep) || isAbsolute(rel)) throw new Error('Le dossier du service doit rester dans le worktree.');
  if (!statSync(path).isDirectory()) throw new Error('Le dossier du service doit être un répertoire.');
  return path;
}

export function createDevServers(changed, inspect = inspectProcesses) {
  const sessions = new Map();
  const pending = new Map();
  const reservedPorts = new Set();
  let closing = false;
  let inspecting, lastInspection = 0;
  const active = session => session.services.some(service => ['running', 'stopping'].includes(service.status));
  const state = () => [...sessions.values()].map(({ children, ...session }) => session);
  function start(card, project, linkedProject = null) {
    if (!card.worktree) throw new Error('Prépare le worktree avant de lancer le projet.');
    if (!project || ![project.frontCommand, project.backCommand].some(Boolean)) throw new Error('Renseigne les commandes Front / Back dans les réglages du projet.');
    if (project.linkedProjectId && (!linkedProject || linkedProject.id !== project.linkedProjectId)) throw new Error('Le projet lié est introuvable. Vérifie les réglages du projet.');
    if (linkedProject && linkedProject.id === project.id) throw new Error('Un projet ne peut pas être lié à lui-même.');
    const projects = [project, ...(linkedProject ? [linkedProject] : [])];
    const launches = projects.map(item => {
      if (![item.frontCommand, item.backCommand].some(Boolean)) throw new Error(`Renseigne les commandes Front / Back du projet ${item.name || item.id}.`);
      const repo = realpathSync(item.id === project.id ? card.worktree.path : item.repo);
      if (!statSync(repo).isDirectory()) throw new Error(`Le répertoire du projet ${item.name || item.id} doit être un dossier.`);
      return [['Front', item.frontCommand, item.frontDirectory], ['Back', item.backCommand, item.backDirectory]]
        .filter(([, command]) => command).map(([role, command, directory]) => {
          const key = role.toLowerCase(), source = item[key + 'Repo'];
          let serviceRoot = repo;
          if (source && item.id === project.id && !samePath(realpathSync(source), realpathSync(card.worktree.source || card.repo))) {
            const worktree = card.serviceWorktrees?.[key];
            if (!worktree || !samePath(realpathSync(worktree.source), realpathSync(source)) || worktree.branch !== card.worktree.branch) throw new Error(`Prépare le worktree ${role} associé avant de lancer les applications.`);
            serviceRoot = worktree.path;
          }
          if (source && item.id !== project.id) throw new Error('Les dépôts distincts du projet lié doivent être lancés depuis leur propre carte.');
          return { project: item, role, command, port: item['_' + key + 'Port'] || null, repo: serviceDirectory(serviceRoot, directory) };
        });
    }).flat();
    const projectIds = projects.map(item => item.id);
    const isolatedPorts = launches.every(item => item.port);
    if ([...sessions.values()].some(session => active(session) && (session.cardId === card.id || session.projectIds.some(id => projectIds.includes(id)) && (!isolatedPorts || !session.isolatedPorts)))) throw new Error('Arrête la pile déjà lancée pour un de ces projets afin de libérer ses ports, ou utilise des commandes avec {port} pour chaque service.');
    const session = { cardId: card.id, projectId: project.id, projectIds, isolatedPorts, services: [], children: [], log: '' };
    sessions.set(card.id, session);
    for (const { project: item, repo, role, command, port } of launches) {
      const name = linkedProject ? `${item.name || item.id} · ${role}` : role;
      const service = { id: `${item.id}:${role.toLowerCase()}`, name, projectId: item.id, repo, port, status: 'stopped', pid: null, processIds: [], endpoints: [] };
      session.services.push(service);
      const entry = { service, command, output: '', child: null, closed: Promise.resolve() };
      session.children.push(entry);
      launchService(session, entry);
    }
    changed();
    return state().find(item => item.cardId === card.id);
  }
  async function startManaged(card, project, linkedProject = null) {
    if (closing || pending.has(card.id)) throw new Error('Un lancement ou un arrêt est déjà en cours.');
    const task = (async () => {
      const leases = [], allocated = [], projects = [project, ...(linkedProject ? [linkedProject] : [])].map(item => item && ({ ...item }));
      try {
        const usedPorts = new Set([...sessions.values()].filter(active).flatMap(session => session.services.map(service => service.port)));
        for (const item of projects) if (item) for (const role of ['front', 'back']) {
          if (!item[role + 'Command']?.includes('{port}')) continue;
          const requested = item[role + 'Port'] || 0;
          if (requested && (usedPorts.has(requested) || reservedPorts.has(requested))) throw new Error(`Le port ${requested} est déjà attribué à un service Atelier.`);
          let lease;
          do {
            lease = await reservePort(requested);
            if (!usedPorts.has(lease.port) && !reservedPorts.has(lease.port) && ![...sessions.values()].some(session => active(session) && session.services.some(service => service.port === lease.port))) break;
            await lease.release();
            if (requested) throw new Error(`Le port ${requested} est déjà attribué à un service Atelier.`);
          } while (true);
          leases.push(lease); usedPorts.add(lease.port);
          allocated.push(lease.port); reservedPorts.add(lease.port);
          item['_' + role + 'Port'] = lease.port;
          item[role + 'Command'] = item[role + 'Command'].replaceAll('{port}', String(lease.port));
        }
        // shortcut: release immediately before spawn; the child must fail if another application wins the port race.
        await Promise.all(leases.splice(0).map(lease => lease.release()));
        return start(card, projects[0], projects[1]);
      } finally { await Promise.all(leases.map(lease => lease.release())); for (const port of allocated) reservedPorts.delete(port); }
    })();
    pending.set(card.id, task);
    try { return await task; } finally { pending.delete(card.id); }
  }
  function launchService(session, entry) {
    const { service, command } = entry, { name, repo } = service;
    const env = { ...process.env };
    if (service.port) { env.PORT = String(service.port); env.HOST = '127.0.0.1'; }
    for (const name of ['JIRA_TOKEN', 'AZURE_DEVOPS_PAT', 'TEMPO_TOKEN']) delete env[name];
    const log = chunk => { entry.output = (entry.output + chunk).slice(-8000); session.log = (session.log + `[${name}] ${chunk}`).slice(-32000); changed(); };
    entry.output = ''; service.status = 'running'; service.exitCode = null; service.pid = null; service.processIds = []; service.endpoints = []; service.inspectionError = ''; lastInspection = 0;
    try {
        const child = process.platform === 'win32'
          ? spawn(process.env.ComSpec || 'cmd.exe', ['/d', '/s', '/c', `"${command}"`], { cwd: repo, windowsHide: true, windowsVerbatimArguments: true, env, stdio: ['ignore', 'pipe', 'pipe'] })
          : spawn('/bin/sh', ['-c', command], { cwd: repo, detached: true, env, stdio: ['ignore', 'pipe', 'pipe'] });
        service.pid = child.pid || null;
        service.processIds = child.pid ? [child.pid] : [];
        entry.child = child;
        entry.closed = new Promise(resolve => child.once('close', code => { service.status = 'stopped'; service.exitCode = code; service.processIds = []; service.endpoints = []; service.inspectionError = ''; changed(); resolve(); }));
        child.once('error', error => log(error.message + '\n'));
        for (const stream of [child.stdout, child.stderr]) { stream.setEncoding('utf8'); stream.on('data', log); }
    } catch (error) { service.status = 'stopped'; service.exitCode = 1; log(error.message + '\n'); }
    changed();
  }
  function startService(cardId, serviceId) {
    const session = sessions.get(cardId), entry = session?.children.find(entry => entry.service.id === serviceId);
    if (!entry) throw new Error('Service introuvable : lance d’abord le projet depuis cette carte.');
    if (entry.service.status !== 'stopped') throw new Error('Ce service est déjà lancé ou en cours d’arrêt.');
    if ([...sessions.values()].some(other => other !== session && active(other) && other.projectIds.some(id => session.projectIds.includes(id)) && (!session.isolatedPorts || !other.isolatedPorts))) throw new Error('Arrête la pile déjà lancée pour un de ces projets afin de libérer ses ports.');
    if (!statSync(entry.service.repo).isDirectory()) throw new Error('Le dossier du service est introuvable.');
    launchService(session, entry);
    return state().find(item => item.cardId === cardId);
  }
  async function startManagedService(cardId, serviceId) {
    const session = sessions.get(cardId), service = session?.services.find(service => service.id === serviceId);
    if (!service?.port) return startService(cardId, serviceId);
    if (closing || pending.has(cardId) || reservedPorts.has(service.port) || [...sessions.values()].some(other => other.services.some(item => item !== service && item.status !== 'stopped' && item.port === service.port))) throw new Error(`Le port ${service.port} est déjà attribué ou un lancement est en cours. Relance l’ensemble pour choisir de nouveaux ports.`);
    reservedPorts.add(service.port);
    const task = (async () => {
      const lease = await reservePort(service.port);
      await lease.release();
      return startService(cardId, serviceId);
    })();
    pending.set(cardId, task);
    try { return await task; } finally { pending.delete(cardId); reservedPorts.delete(service.port); }
  }
  async function stop(cardId, serviceId) {
    if (pending.has(cardId)) await pending.get(cardId).catch(() => {});
    const session = sessions.get(cardId);
    if (!session) { if (serviceId) throw new Error('Service introuvable.'); return; }
    if (serviceId && !session.services.some(service => service.id === serviceId)) throw new Error('Service introuvable.');
    await Promise.all(session.children.filter(entry => !serviceId || entry.service.id === serviceId).map(async ({ child, service, closed }) => {
      if (service.status === 'stopped') return;
      service.status = 'stopping'; changed();
      if (child.pid) {
        if (process.platform === 'win32') await new Promise((resolve, reject) => execFile('taskkill', ['/pid', String(child.pid), '/T', '/F'], { windowsHide: true }, error => service.status === 'stopped' || !error ? resolve() : reject(error)));
        else { try { process.kill(-child.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; } }
      }
      await closed;
    }));
  }
  async function refresh() {
    if (inspecting) { await inspecting; return state(); }
    if (Date.now() - lastInspection < 2000 || ![...sessions.values()].some(active)) return state();
    inspecting = (async () => {
      try {
        const snapshot = await inspect();
        for (const session of sessions.values()) for (const entry of session.children) {
          const { service } = entry;
          if (service.status !== 'running' || !service.pid) continue;
          service.processIds = descendantPids(service.pid, snapshot.processes);
          service.endpoints = snapshot.listeners.filter(listener => service.processIds.includes(listener.pid)).map(listener => ({ ...listener, url: listenerUrl(listener, entry.output) }));
          service.inspectionError = '';
        }
      } catch {
        for (const session of sessions.values()) for (const service of session.services) if (service.status === 'running') { service.processIds = service.pid ? [service.pid] : []; service.endpoints = []; service.inspectionError = 'Détection des ports indisponible. Les commandes de démarrage et d’arrêt restent disponibles.'; }
      }
      lastInspection = Date.now();
    })();
    try { await inspecting; } finally { inspecting = null; }
    return state();
  }
  return { state, start, startManaged, startService, startManagedService, stop, refresh, running: cardId => pending.has(cardId) || active(sessions.get(cardId) || { services: [] }), close: async () => { closing = true; await Promise.allSettled(pending.values()); await Promise.all([...sessions.keys()].map(cardId => stop(cardId))); } };
}
