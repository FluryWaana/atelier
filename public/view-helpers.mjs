import { msg } from './i18n.mjs';
export const names = { todo: msg("À faire"), doing: msg("En cours"), review: msg("À vérifier"), done: msg("Terminé"), standby: msg("Stand by") };
export const runNames = { running: msg("En cours"), completed: msg("Terminé · à vérifier"), failed: msg("Échec"), cancelled: msg("Arrêté"), interrupted: msg("Interrompu · durée estimée") };
export const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
export const cardPath = id => `/api/cards/${encodeURIComponent(id)}`;
