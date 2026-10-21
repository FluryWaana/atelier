import { fail } from './validation.mjs';

export const statuses = ['todo', 'doing', 'review', 'done', 'standby'];

export function reviewRequirements(card) {
  return { title: card.title || '', description: card.description || '', instructions: card.instructions || '', criteria: (card.checklist || []).map(item => item.text) };
}

export function blockers(card, cards) {
  return (card.blockedBy || []).map(id => cards.find(c => c.id === id) || { id, title: 'Tâche introuvable', status: 'missing' }).filter(c => c.status !== 'done');
}

export function dependencies(id, value, cards) {
  const byId = new Map(cards.map(card => [card.id, card]));
  if (!Array.isArray(value) || value.length > 20 || value.some(item => typeof item !== 'string' || item === id || !byId.has(item)) || new Set(value).size !== value.length) fail('Dépendances invalides (20 tâches maximum, sans doublon ni référence à soi-même).');
  const visit = (key, seen = new Set()) => {
    if (key === id) fail('Cette dépendance créerait un cycle.');
    if (seen.has(key)) return;
    seen.add(key);
    for (const next of byId.get(key)?.blockedBy || []) visit(next, seen);
  };
  value.forEach(key => visit(key)); return value;
}
