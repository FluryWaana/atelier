import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import { join, resolve, sep } from 'node:path';
import { tmpdir } from 'node:os';
import { readSession } from '../lib/infrastructure/local-session.mjs';
import { detectLaunch } from '../lib/launch-detection.mjs';
import { previewPullRequest } from '../lib/pull-requests.mjs';

for (const kind of ['session', 'manifest', 'preview']) test(`${kind} refuses a file replaced between validation and opening`, async t => {
  const root = fs.mkdtempSync(join(tmpdir(), 'atelier-read-race-'));
  const name = { session: 'local-session', manifest: 'package.json', preview: 'untracked.txt' }[kind];
  const path = join(root, name);
  const content = kind === 'manifest' ? JSON.stringify({ scripts: { start: 'node app.js' } }) : 'a'.repeat(64);
  fs.writeFileSync(path, content, { mode: 0o600 });
  fs.writeFileSync(path + '.replacement', content, { mode: 0o600 });
  const open = fs.openSync;
  let replaced = false, fd;
  t.after(() => {
    t.mock.restoreAll(); syncBuiltinESMExports();
    assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep));
    fs.rmSync(root, { recursive: true, force: true });
  });
  t.mock.method(fs, 'openSync', (file, ...args) => {
    if (file !== path) return open(file, ...args);
    assert.equal(replaced, false);
    fs.renameSync(path, path + '.old');
    fs.renameSync(path + '.replacement', path);
    replaced = true;
    fd = open(file, ...args);
    return fd;
  });
  syncBuiltinESMExports();
  if (kind === 'session') assert.throws(() => readSession(root), /session locale a changé/);
  if (kind === 'manifest') {
    const result = detectLaunch(root);
    assert.equal(result.candidates.length, 0);
    assert.equal(result.warnings.length, 1);
  }
  if (kind === 'preview') {
    const card = { branch: 'feature/test', worktree: { path: root, branch: 'feature/test' } };
    const run = async (command, args) => {
      if (args[0] === 'branch') return card.branch;
      if (args[0] === 'remote') return 'https://dev.azure.com/org/project/_git/repo';
      if (args[0] === 'ls-files') return name + '\0';
      return '';
    };
    await assert.rejects(previewPullRequest(card, '', run, async () => assert.fail('Do not contact the provider after a file race')), /Fichier modifié pendant sa lecture/);
  }
  assert.equal(replaced, true);
  assert.throws(() => fs.fstatSync(fd), { code: 'EBADF' });
});
