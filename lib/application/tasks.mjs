import { fail, text } from '../domain/validation.mjs';
import { statuses, dependencies, reviewRequirements } from '../domain/tasks.mjs';
import { agentOptions } from '../domain/agent-options.mjs';
import { checklist } from '../domain/checklist.mjs';

export function createTasks({ store, repository, config, configFor, busy, queuedIds, launchTask, syncCardStatus, invalidateReviews, newId, timestamp }) {
  function create(body, brainstorming = false) {
    const title = text(body.title, 300); if (!title) fail('Un titre est nécessaire.');
    const template = body.templateId ? (store.settings()?.taskTemplates || []).find(t => t.id === body.templateId) : null;
    if (body.templateId && !template) fail('Modèle introuvable.');
    const agent = body.agent || configFor({ repo: body.repo }).agent, mode = brainstorming ? 'analyze' : body.mode || template?.mode || 'edit';
    if (!['codex', 'claude'].includes(agent) || !['analyze', 'edit'].includes(mode)) fail('Agent ou mode invalide.');
    const status = brainstorming ? 'todo' : body.status ?? 'todo';
    if (!['todo', 'doing', 'review'].includes(status)) fail('Statut initial invalide.');
    const card = { id: newId(), title, description: text(body.description ?? template?.description ?? ''), instructions: text(body.instructions ?? template?.instructions ?? ''), repo: body.repo ? repository(body.repo) : brainstorming ? '' : config().repo, agent, ...agentOptions(agent, body), mode, checklist: checklist(body.checklist ?? template?.checklist ?? []), status, ...(brainstorming ? { brainstormSession: true } : {}), createdAt: timestamp(), updatedAt: timestamp() };
    store.putCard(card); return card;
  }
  function archive(id, body) {
    const card = store.card(id); if (!card) fail('Carte introuvable.', 'not-found');
    if (typeof body.archived !== 'boolean') fail('État d’archive invalide.');
    if (busy(card.id) || store.times().some(e => e.cardId === card.id && (e.runningSince || e.exportState === 'sending'))) fail('Attends la fin des opérations et arrête le chronomètre avant d’archiver.', 'conflict');
    card.archived = body.archived; card.updatedAt = timestamp(); store.putCard(card); return card;
  }
  function duplicate(id) {
    const card = store.card(id); if (!card) fail('Carte introuvable.', 'not-found');
    const copy = { id: newId(), title: ('Copie — ' + card.title).slice(0, 300), description: card.description || '', repo: card.repo || '', agent: card.agent, model: card.model || '', effort: card.effort || '', mode: card.mode || 'edit', instructions: card.instructions || '', baseBranch: card.baseBranch || '', checklist: (card.checklist || []).map(item => ({ text: item.text, done: false })), status: 'todo', createdAt: timestamp(), updatedAt: timestamp() };
    store.putCard(copy); return copy;
  }
  function update(id, body) {
    const card = store.card(id); if (!card) fail('Carte introuvable.', 'not-found');
    if (card.archived) fail('Restaure cette tâche avant de reprendre le travail.', 'conflict');
    if (busy(card.id)) fail('Arrête l’agent avant de modifier cette carte.', 'conflict');
    const autoStart = body.status === 'doing' && card.status !== 'doing';
    const previousStatus = card.status;
    const previousAgent = card.agent;
    const previousRequirements = JSON.stringify(reviewRequirements(card));
    if (card.worktree && (body.repo !== undefined && body.repo !== card.repo || body.branch !== undefined && body.branch !== card.branch || body.baseBranch !== undefined && body.baseBranch !== (card.baseBranch || ''))) fail('Retire d’abord le worktree avant de changer le dépôt ou la branche.');
    if (body.status !== undefined) { if (!statuses.includes(body.status)) fail('Statut invalide.'); card.status = body.status; }
    if (body.title !== undefined && !card.ticket) { card.title = text(body.title, 300); if (!card.title) fail('Titre requis.'); }
    if (body.description !== undefined && !card.ticket) card.description = text(body.description);
    if (body.repo !== undefined) card.repo = body.repo ? repository(body.repo) : '';
    if (body.agent !== undefined) { if (!['codex', 'claude'].includes(body.agent)) fail('Agent invalide.'); if (body.agent !== card.agent) card.agentExplicit = true; card.agent = body.agent; }
    Object.assign(card, agentOptions(card.agent, { model: body.model ?? (card.agent === previousAgent ? card.model : ''), effort: body.effort ?? (card.agent === previousAgent ? card.effort : '') }));
    for (const key of ['branch', 'baseBranch']) if (body[key] !== undefined) card[key] = text(body[key], 180);
    if (body.checklist !== undefined) card.checklist = checklist(body.checklist);
    if (body.blockedBy !== undefined) card.blockedBy = dependencies(card.id, body.blockedBy, store.cards());
    if (body.instructions !== undefined) card.instructions = text(body.instructions);
    if (body.mode !== undefined) { if (!['edit', 'analyze'].includes(body.mode)) fail('Mode invalide.'); card.mode = body.mode; }
    if (autoStart) {
      card.status = previousStatus;
      launchTask(card, { agent: card.agent, mode: card.mode || 'edit', instructions: card.instructions || '' });
      return syncCardStatus(card.id);
    }
    card.updatedAt = timestamp(); store.putCard(card);
    if (JSON.stringify(reviewRequirements(card)) !== previousRequirements) invalidateReviews(card.id, 'Le besoin ou les critères ont changé. Relancez la review.');
    return body.status !== undefined && body.status !== previousStatus ? syncCardStatus(card.id) : store.card(card.id);
  }
  function remove(id) {
    const card = store.card(id); if (!card) fail('Carte introuvable.', 'not-found');
    if (card.ticketCreation) fail('Vérifie et rattache le ticket Jira de l’envoi incertain avant de supprimer cette carte.', 'conflict');
    if (queuedIds().includes(card.id)) fail('Retire cette tâche de la file avant de la supprimer.', 'conflict');
    if (store.cards().some(c => c.blockedBy?.includes(card.id))) fail('Retire les liens « Bloquée par » avant de supprimer cette tâche.', 'conflict');
    if (busy(card.id)) fail('Une exécution est active.', 'conflict');
    if (store.times().some(e => e.cardId === card.id)) fail('Cette carte possède des entrées de temps. Conserve-la pour garder leur historique.', 'conflict');
    if (card.worktree || Object.keys(card.serviceWorktrees || {}).length) fail('Retire d’abord les worktrees de cette carte. Les branches seront conservées.', 'conflict');
    store.deleteCard(card.id); return { ok: true };
  }
  function createTemplate(body) {
    const name = text(body.name, 100); if (!name) fail('Nom de modèle requis.');
    const card = store.card(body.cardId); if (!card) fail('Carte introuvable.', 'not-found');
    const templates = store.settings()?.taskTemplates || [];
    if (templates.length >= 50) fail('Retire un modèle avant d’en ajouter (50 maximum).');
    const template = { id: newId(), name, description: card.description || '', instructions: card.instructions || '', mode: card.mode || 'edit', checklist: (card.checklist || []).map(i => ({ text: i.text, done: false })) };
    store.saveSettings({ ...store.settings(), taskTemplates: [...templates, template] }); return template;
  }
  function removeTemplate(id) {
    store.saveSettings({ ...store.settings(), taskTemplates: (store.settings()?.taskTemplates || []).filter(t => t.id !== id) }); return { ok: true };
  }
  return { create, archive, duplicate, update, remove, createTemplate, removeTemplate };
}
