import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { DatabaseSync } from 'node:sqlite';
import { dataDirectory, installationMode } from '../lib/infrastructure/installation.mjs';
import { importPortableData } from '../lib/infrastructure/portable-import.mjs';
import { createPackageUpdater } from '../lib/infrastructure/package-updater.mjs';
import { openStore } from '../lib/store.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'atelier-install-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

test('portable copies keep their data, installed copies use user storage and Linux respects absolute XDG paths', t => {
  const root = fixture(t), home = join(root, 'home');
  assert.equal(installationMode(root), 'portable');
  assert.equal(dataDirectory(root), join(root, 'data'));
  writeFileSync(join(root, 'installation.json'), '{"mode":"user"}');
  assert.equal(dataDirectory(root, { platform: 'win32', home, env: {} }), join(home, 'AppData', 'Local', 'AtelierKanban', 'data'));
  assert.equal(dataDirectory(root, { platform: 'linux', home, env: { XDG_DATA_HOME: 'relative' } }), join(home, '.local', 'share', 'atelier-kanban'));
  assert.equal(dataDirectory(root, { platform: 'linux', home, env: { XDG_DATA_HOME: home } }), join(home, 'atelier-kanban'));
  writeFileSync(join(root, 'installation.json'), '{"mode":"system"}');
  assert.equal(createPackageUpdater({ root }), null);
  writeFileSync(join(root, 'installation.json'), '{"mode":"invalid"}');
  assert.throws(() => dataDirectory(root), /invalide/);
});

test('portable import snapshots committed SQLite WAL, preserves source and refuses to overwrite existing data', t => {
  const root = fixture(t), source = join(root, 'source'), destination = join(root, 'user', 'data');
  mkdirSync(source);
  const store = openStore(join(source, 'kanban.sqlite'));
  try {
    store.putCard({ id: 'preserved', title: 'À conserver', status: 'todo' });
    writeFileSync(join(source, 'attachment.txt'), 'document');
    for (const name of ['local-session', 'local-browser.html']) writeFileSync(join(source, name), 'expired session');
    importPortableData(source, destination);
    assert.equal(store.card('preserved').title, 'À conserver');
    assert.equal(readFileSync(join(destination, 'attachment.txt'), 'utf8'), 'document');
    for (const name of ['local-session', 'local-browser.html']) assert.equal(existsSync(join(destination, name)), false);
    const db = new DatabaseSync(join(destination, 'kanban.sqlite'), { readOnly: true });
    try { assert.match(db.prepare('SELECT data FROM cards WHERE id = ?').get('preserved').data, /À conserver/); }
    finally { db.close(); }
    assert.equal(readdirSync(join(destination, 'backups')).length, 1);
    assert.throws(() => importPortableData(source, destination), /écrasera pas/);
    assert.throws(() => importPortableData(source, source), /distinct/);
    assert.throws(() => importPortableData(source, join(source, 'nested')), /distinct/);
    assert.throws(() => importPortableData(source, join(source, '..nested')), /distinct/);
  } finally { store.db.close(); }
});

test('invalid imports never leave a partial destination', t => {
  const root = fixture(t), source = join(root, 'source'), destination = join(root, 'data');
  mkdirSync(source); writeFileSync(join(source, 'kanban.sqlite'), 'corrupted');
  assert.throws(() => importPortableData(source, destination));
  assert.equal(existsSync(destination), false);
  assert.deepEqual(readdirSync(root), ['source']);
  rmSync(join(source, 'kanban.sqlite'));
  symlinkSync(root, join(source, 'kanban.sqlite'), process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => importPortableData(source, destination), /invalide/);
});
