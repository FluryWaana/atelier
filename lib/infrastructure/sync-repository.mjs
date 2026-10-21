import { transaction } from './transaction.mjs';

export function createSyncRepository(store) {
  return {
    card: id => store.card(id), cards: () => store.cards(), putCard: card => store.putCard(card), runs: () => store.runs(),
    transaction: change => transaction(store.db, change),
  };
}
