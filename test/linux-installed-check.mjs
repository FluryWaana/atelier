import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { homedir } from 'node:os';

execFileSync('atelier-kanban', ['--no-browser']);
const health = await (await fetch('http://127.0.0.1:4317/api/health')).json();
assert.equal(health.workspace, '/opt/atelier-kanban');
assert.equal(health.version, JSON.parse(readFileSync('/opt/atelier-kanban/package.json')).version);
assert.equal(existsSync('/opt/atelier-kanban/data'), false);
assert.equal(existsSync(join(homedir(), '.local/share/atelier-kanban/kanban.sqlite')), true);
const updates = await (await fetch('http://127.0.0.1:4317/api/updates')).json();
assert.equal(updates.canInstall, false);
if (process.argv[2] === 'create') {
  const response = await fetch('http://127.0.0.1:4317/api/cards', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1' }, body: JSON.stringify({ title: 'Carte conservée par le gestionnaire de paquets', status: 'todo' }) });
  assert.equal(response.ok, true);
}
const state = await (await fetch('http://127.0.0.1:4317/api/state')).json();
assert.equal(state.cards[0].title, 'Carte conservée par le gestionnaire de paquets');
