export const migrationShape = Object.freeze({ projects: 10, cards: 500, runs: 100, times: 200, logs: 10000 });
export const migrationDate = '2026-10-23T08:00:00.123Z';

export function seedMigrationData(store) {
  store.db.exec('BEGIN');
  try {
    store.saveSettings({ credentialsReset: true, agent: 'codex', connectorProviders: { tickets: '', repositories: '', time: '' }, migrationExtra: { retained: true } });
    for (let i = 0; i < migrationShape.projects; i++) store.putProject({ id: 'project-' + i, name: 'Projet synthétique ' + i, repo: '', createdAt: migrationDate, migrationExtra: null });
    for (let i = 0; i < migrationShape.cards; i++) store.putCard({ id: 'card-' + i, title: 'Tâche synthétique ' + i + ' — é 🔧', description: 'Contexte de référence. '.repeat(10), status: ['todo', 'doing', 'review', 'done', 'standby'][i % 5], agent: 'codex', repo: '', createdAt: migrationDate,
      checklist: [{ text: 'Critère synthétique', done: false }], ...(i % 2 ? { nullable: null } : {}), migrationExtra: { array: [null, false, 0, 'é'], precision: 1.23456789012345 } });
    for (let i = 0; i < migrationShape.runs; i++) store.putRun({ id: 'run-' + i, cardId: 'card-' + i, agent: 'codex', mode: 'analyze', status: 'completed', startedAt: migrationDate, endedAt: '2026-10-23T08:00:01.456Z', migrationExtra: { retained: true } });
    for (let i = 0; i < migrationShape.times; i++) store.putTime({ id: 'time-' + i, cardId: 'card-' + i, date: '2026-10-23', seconds: 61, description: 'Temps synthétique', exportState: 'draft', migrationExtra: null });
    for (let i = 0; i < migrationShape.logs; i++) store.log('run-0', i % 2 ? 'stderr' : 'stdout', `Ligne synthétique ${i} — é 🔧`);
    store.db.exec('COMMIT');
  } catch (error) { store.db.exec('ROLLBACK'); throw error; }
}
