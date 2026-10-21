import { DatabaseSync } from 'node:sqlite';

export function openStore(path) {
  const db = new DatabaseSync(path);
  db.exec(`PRAGMA journal_mode=WAL;
    CREATE TABLE IF NOT EXISTS time_entries (id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS cards (id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS runs (id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS logs (seq INTEGER PRIMARY KEY AUTOINCREMENT, runId TEXT NOT NULL, stream TEXT NOT NULL, text TEXT NOT NULL);
    CREATE INDEX IF NOT EXISTS logs_run ON logs(runId, seq);
    CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL);`);
  const rows = table => db.prepare(`SELECT data FROM ${table} ORDER BY rowid DESC`).all().map(row => JSON.parse(row.data));
  const get = (table, id) => { const row = db.prepare(`SELECT data FROM ${table} WHERE id=?`).get(id); return row ? JSON.parse(row.data) : null; };
  const put = (table, item) => db.prepare(`INSERT INTO ${table}(id,data) VALUES (?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data`).run(item.id, JSON.stringify(item));
  return { db, times: () => rows('time_entries'), time: id => get('time_entries', id), putTime: item => put('time_entries', item), deleteTime: id => db.prepare('DELETE FROM time_entries WHERE id=?').run(id), projects: () => rows('projects'), putProject: item => put('projects', item), deleteProject: id => db.prepare('DELETE FROM projects WHERE id=?').run(id), cards: () => rows('cards'), runs: () => rows('runs'), card: id => get('cards', id), run: id => get('runs', id),
    putCard: item => put('cards', item), putRun: item => put('runs', item),
    settings: () => JSON.parse(db.prepare('SELECT data FROM settings WHERE id=1').get()?.data || '{}'),
    saveSettings: settings => db.prepare('INSERT INTO settings VALUES (1,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data').run(JSON.stringify(settings)),
    log: (id, stream, text) => db.prepare('INSERT INTO logs(runId,stream,text) VALUES (?,?,?)').run(id, stream, text.slice(0,65536)),
    logs: (id, after = 0) => db.prepare('SELECT seq,stream,text FROM logs WHERE runId=? AND seq>? ORDER BY seq LIMIT 500').all(id, after),
  };
}
