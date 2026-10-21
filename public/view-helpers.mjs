export const names = { todo: 'À faire', doing: 'En cours', review: 'À vérifier', done: 'Terminé', standby: 'Stand by' };
export const runNames = { running: 'En cours', completed: 'Terminé · à vérifier', failed: 'Échec', cancelled: 'Arrêté', interrupted: 'Interrompu · durée estimée' };
export const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
export const cardPath = id => `/api/cards/${encodeURIComponent(id)}`;
