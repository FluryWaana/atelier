import { transaction } from './transaction.mjs';

export function createSettingsRepository(store) {
  return { settings: () => store.settings(), saveSettings: settings => store.saveSettings(settings), transaction: change => transaction(store.db, change) };
}
