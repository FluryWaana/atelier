import { blockers } from './workflow.mjs';

export const taskTemplates = {
  feature: { title: 'Fonctionnalité', mode: 'edit', description: 'Objectif\n\nComportement attendu\n\nCritères de réussite\n- \n\nContraintes\n' },
  bug: { title: 'Correction', mode: 'edit', description: 'Problème observé\n\nÉtapes pour reproduire\n1. \n\nRésultat attendu\n\nVérification de la correction\n' },
  analysis: { title: 'Analyse', mode: 'analyze', description: 'Question à examiner\n\nContexte\n\nLivrable attendu\nUne analyse avec les constats, les pistes et leurs limites. Ne pas modifier les fichiers.\n' },
};

export function matchesFocus(card, runs, focus, cards = []) {
  if (card.archived) return false;
  if (focus === 'review') return card.status === 'review';
  if (focus !== 'attention') return true;
  const run = runs.find(r => r.cardId === card.id);
  return card.status !== 'done' && (blockers(card, cards).length > 0 || card.ticketSync?.state === 'error' || ['error', 'interrupted', 'cancelled', 'blocked'].includes(card.automation?.state) || Boolean(run && (run.attention || ['failed', 'interrupted', 'cancelled'].includes(run.status))));
}

export function shortcutAction(event, editing, dialogOpen) {
  if (event.repeat || event.isComposing || editing || dialogOpen || event.altKey) return null;
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') return 'palette';
  if (event.ctrlKey || event.metaKey || event.shiftKey) return null;
  return event.key.toLowerCase() === 'n' ? 'new' : event.key === '/' ? 'search' : null;
}


export function paletteItems(state, query) {
  const normalize = text => text.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase();
  const words = normalize(query.trim()).split(/\s+/).filter(Boolean);
  const items = [
    { kind: 'action', id: 'new', title: 'Créer une tâche', detail: 'Nouvelle tâche locale' },
    { kind: 'action', id: 'projects', title: 'Gérer les projets', detail: 'Répertoires locaux' },
    { kind: 'action', id: 'diagnostics', title: 'Tester les connexions', detail: 'Git, agents et services' },
    ...state.projects.map(p => ({ kind: 'project', id: p.id, title: p.name, detail: p.repo })),
    ...state.cards.map(c => ({ kind: 'card', id: c.id, title: c.title, detail: `${c.ticket?.key || 'Tâche locale'}${c.archived ? ' · Archivée' : ''}` })),
  ];
  return items.filter(item => words.every(word => normalize(item.title + ' ' + item.detail).includes(word))).slice(0, 30);
}


export function filterTimes(entries, { from = '', to = '', status = 'all' } = {}) {
  return entries.filter(e => (!from || e.date >= from) && (!to || e.date <= to) && (status === 'all' || (status === 'exported' ? e.exportState === 'exported' : e.exportState !== 'exported')));
}

export function timeCsv(entries, cards) {
  const cell = value => {
    let text = String(value ?? '');
    if (/^[\s]*[=+@-]/.test(text) || /^[\t\r\n]/.test(text)) text = "'" + text;
    return '"' + text.replaceAll('"', '""') + '"';
  };
  const rows = [['Date', 'Heure', 'Ticket', 'Tâche', 'Secondes', 'Description', 'État Tempo', 'ID Tempo']];
  for (const entry of entries.filter(e => !e.runningSince)) {
    const card = cards.find(c => c.id === entry.cardId);
    rows.push([entry.date, entry.startTime, card?.ticket?.key, card?.title, entry.seconds, entry.description, entry.exportState, entry.tempoId]);
  }
  return '\ufeff' + rows.map(row => row.map(cell).join(';')).join('\r\n');
}

export function localDay(date = new Date()) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}
export function agentTime(runs, now = Date.now(), { from = '', to = '' } = {}) {
  const lower = from ? new Date(from + 'T00:00:00').getTime() : -Infinity;
  const nextDay = to ? new Date(to + 'T00:00:00') : null;
  if (nextDay) nextDay.setDate(nextDay.getDate() + 1);
  const upper = nextDay ? nextDay.getTime() : Infinity;
  const groups = new Map();
  for (const run of runs) {
    const start = new Date(run.startedAt).getTime();
    const stoppedAt = run.status === 'interrupted' ? run.lastEventAt || run.startedAt : run.endedAt;
    const end = Math.max(start, stoppedAt ? new Date(stoppedAt).getTime() : run.status === 'running' ? now : new Date(run.lastEventAt || run.startedAt).getTime());
    if (!Number.isFinite(start) || !Number.isFinite(end) || lower >= upper || start >= upper || end < lower || (end === lower && start < lower)) continue;
    const group = groups.get(run.cardId) || { cardId: run.cardId, seconds: 0, runs: 0, running: 0 };
    group.seconds += Math.floor((Math.min(end, upper) - start) / 1000) - Math.floor((Math.max(start, lower) - start) / 1000);
    group.runs++;
    if (run.status === 'running') group.running++;
    if (run.status === 'interrupted') group.estimated = true;
    groups.set(run.cardId, group);
  }
  return [...groups.values()];
}
export function dailyTime(entries, day, now = Date.now()) {
  const groups = new Map();
  for (const entry of entries.filter(e => e.date === day)) {
    const group = groups.get(entry.cardId) || { cardId: entry.cardId, seconds: 0, pending: 0, running: 0 };
    group.seconds += entry.seconds + (entry.runningSince ? Math.max(0, Math.floor((now - new Date(entry.runningSince).getTime()) / 1000)) : 0);
    if (entry.runningSince) group.running++;
    else if (entry.exportState !== 'exported') group.pending++;
    groups.set(entry.cardId, group);
  }
  return [...groups.values()].sort((a,b) => b.seconds - a.seconds);
}
