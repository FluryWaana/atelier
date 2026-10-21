import { mkdirSync, readdirSync, lstatSync } from 'node:fs';
import { join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';

const tables = ['projects', 'cards', 'runs', 'logs', 'settings', 'time_entries'];
const filename = /^atelier-\d{13}-[a-f0-9-]{36}\.sqlite$/;
export function createBackups(store, dataDir) {
  const directory = join(dataDir, 'backups'); mkdirSync(directory, { recursive: true });
  const list = () => readdirSync(directory).filter(name => filename.test(name)).flatMap(name => {
    const stat = lstatSync(join(directory, name));
    return stat.isFile() && !stat.isSymbolicLink() ? [{ name, date: stat.mtime.toISOString(), bytes: stat.size }] : [];
  }).sort((a,b) => b.name.localeCompare(a.name));
  const create = () => {
    const name = `atelier-${Date.now()}-${randomUUID()}.sqlite`;
    store.db.prepare('VACUUM INTO ?').run(join(directory, name));
    return { name, directory };
  };
  function restore(name) {
    if (typeof name !== 'string' || !filename.test(name) || !list().some(b => b.name === name)) throw new Error('Sauvegarde locale introuvable.');
    const source = new DatabaseSync(join(directory, name), { readOnly: true });
    let rows;
    try {
      if (source.prepare('PRAGMA integrity_check').get().integrity_check !== 'ok') throw new Error('Sauvegarde endommagée.');
      rows = Object.fromEntries(tables.map(table => [table, source.prepare(`SELECT * FROM ${table}`).all()]));
      const times = new Map(rows.time_entries.map(row => [row.id, JSON.parse(row.data)]));
      for (const current of store.times().filter(e => e.exportState !== 'draft')) {
        const saved = times.get(current.id);
        if (!saved || saved.exportState !== current.exportState || saved.tempoId !== current.tempoId || saved.exportState !== 'exported') throw new Error('Restauration refusée : des envois Tempo plus récents ou incertains doivent être conservés. Choisissez une sauvegarde récente.');
      }
      if (rows.runs.some(r => JSON.parse(r.data).status === 'running') || rows.cards.some(c => JSON.parse(c.data).automation?.state === 'running') || [...times.values()].some(e => e.runningSince || ['sending', 'uncertain'].includes(e.exportState))) throw new Error('Cette sauvegarde contient des opérations inachevées.');
    } finally { source.close(); }
    const rescue = create();
    store.db.exec('BEGIN');
    try {
      for (const table of tables) {
        store.db.exec(`DELETE FROM ${table}`);
        const columns = table === 'logs' ? ['seq','runId','stream','text'] : ['id','data'];
        const insert = store.db.prepare(`INSERT INTO ${table} (${columns.join(',')}) VALUES (${columns.map(()=>'?').join(',')})`);
        for (const row of rows[table]) insert.run(...columns.map(key => row[key]));
      }
      // Credentials must never be paired with restored addresses, including after restart.
      if (store.db.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='token_vault'").get()) store.db.exec('DELETE FROM token_vault');
      store.saveSettings({ ...store.settings(), credentialsReset: true });
      for (const run of store.runs().filter(r => r.kind === 'pr-review')) { run.reviewStaleReason = 'Données restaurées : relancez la review pour vérifier les commits actuels.'; store.putRun(run); }
      for (const card of store.cards().filter(c => c.automation)) {
        card.automation = { ...card.automation, state: 'interrupted', step: 'tests', message: 'Données restaurées : reprends les vérifications et la review sur le worktree actuel.' };
        if (!card.archived && card.status !== 'done') card.status = 'standby';
        store.putCard(card);
      }
      store.db.exec('COMMIT');
    } catch (error) { store.db.exec('ROLLBACK'); throw error; }
    return { restored: name, rescue: rescue.name };
  }
  return { list, create, restore, directory };
}
