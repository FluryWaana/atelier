import test from 'node:test';
import assert from 'node:assert/strict';
import { lstatSync, mkdtempSync, renameSync, rmSync, writeFileSync, linkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { readCheckedFile } from '../lib/infrastructure/checked-file.mjs';

test('checked file reads reject replacement, new hard links and growth past the limit', t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-checked-file-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const path = join(root, 'config.json');
  writeFileSync(path, 'original');
  const original = lstatSync(path);
  assert.equal(readCheckedFile(path, original, 32).toString(), 'original');
  renameSync(path, join(root, 'before.json'));
  // codeql[js/file-system-race] Deliberately replace this temporary fixture after validation to test rejection.
  writeFileSync(path, 'replacement');
  assert.throws(() => readCheckedFile(path, original, 32), /changé/);
  const replaced = lstatSync(path);
  linkSync(path, join(root, 'alias.json'));
  assert.throws(() => readCheckedFile(path, replaced, 32), /changé/);
  const linked = lstatSync(path);
  // codeql[js/file-system-race] Deliberately grow this temporary fixture after validation to test the read limit.
  writeFileSync(path, 'x'.repeat(33));
  assert.throws(() => readCheckedFile(path, linked, 32), /volumineux/);
});
