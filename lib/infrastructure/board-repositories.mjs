import { transaction } from './transaction.mjs';

export function createProjectRepository(store) {
  return {
    projects: () => store.projects(), putProject: project => store.putProject(project), deleteProject: id => store.deleteProject(id),
    cards: () => store.cards(), putCard: card => store.putCard(card), times: () => store.times(),
    transaction: change => transaction(store.db, change),
  };
}

export function createTaskRepository(store) {
  return {
    cards: () => store.cards(), card: id => store.card(id), putCard: card => store.putCard(card), times: () => store.times(),
    settings: () => store.settings(), saveSettings: settings => store.saveSettings(settings),
    deleteCard: id => store.db.prepare('DELETE FROM cards WHERE id=?').run(id),
  };
}
