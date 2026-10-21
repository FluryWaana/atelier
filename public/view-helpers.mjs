import { msg } from './i18n.mjs';
export const names = { todo: msg("À faire"), doing: msg("En cours"), review: msg("À vérifier"), done: msg("Terminé"), standby: msg("Stand by") };
export const runNames = { running: msg("En cours"), completed: msg("Terminé · à vérifier"), failed: msg("Échec"), cancelled: msg("Arrêté"), interrupted: msg("Interrompu · durée estimée") };
export const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
export const cardPath = id => `/api/cards/${encodeURIComponent(id)}`;

export function duration(seconds) {
  seconds = Math.max(0, Math.floor(seconds || 0));
  return msg("{0} h {1} min {2} s", Math.floor(seconds / 3600), String(Math.floor(seconds % 3600 / 60)).padStart(2, '0'), String(seconds % 60).padStart(2, '0'));
}
export function agentState(run) {
  if (run.attention) return run.status === 'running' ? msg("Attention requise") : msg("Bloqué · consulter le journal");
  if (run.status !== 'running') return runNames[run.status];
  return Date.now() - new Date(run.lastEventAt || run.startedAt) > 60000 ? msg("Sans signal · état à vérifier") : msg("Travaille");
}
