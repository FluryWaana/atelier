export function transaction(db, change) {
  db.exec('BEGIN');
  try { const result = change(); db.exec('COMMIT'); return result; }
  catch (error) { db.exec('ROLLBACK'); throw error; }
}
