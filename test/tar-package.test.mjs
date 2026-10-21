import test from 'node:test';
import assert from 'node:assert/strict';
import { gzipSync, gunzipSync } from 'node:zlib';
import { mkdtempSync, rmSync, readFileSync, existsSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { extractTarPackage } from '../lib/infrastructure/tar-package.mjs';

function archive(entries) {
  const blocks = [];
  for (const [name, value = 'content', type = '0'] of entries) {
    const data = Buffer.from(value), header = Buffer.alloc(512);
    header.write(name); header.write('0000644\0', 100); header.write(data.length.toString(8).padStart(11, '0') + '\0', 124);
    header.fill(32, 148, 156); header.write(type, 156); header.write('ustar\0', 257);
    const checksum = [...header].reduce((a, b) => a + b, 0); header.write(checksum.toString(8).padStart(6, '0') + '\0 ', 148);
    blocks.push(header, data, Buffer.alloc((512 - data.length % 512) % 512));
  }
  return gzipSync(Buffer.concat([...blocks, Buffer.alloc(1024)]));
}

test('tar extraction validates all entries before writing and rejects traversal, links and duplicates', t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-tar-')); t.after(() => rmSync(root, { recursive: true, force: true }));
  const target = join(root, 'valid');
  extractTarPackage(archive([['server.mjs'], ['runtime/node', 'runtime'], ['Start.command', 'start'], ['Stop.command', 'stop']]), target);
  assert.equal(readFileSync(join(target, 'runtime/node'), 'utf8'), 'runtime');
  for (const [name, content] of [['Start.command', 'start'], ['Stop.command', 'stop']]) {
    assert.equal(readFileSync(join(target, name), 'utf8'), content);
    if (process.platform !== 'win32') assert.equal(statSync(join(target, name)).mode & 0o777, 0o755);
  }
  for (const entries of [
    [['data/kanban.sqlite']], [['lib/../../outside']], [['/server.mjs']], [['lib\\escape']],
    [['lib/link', '', '2']], [['server.mjs'], ['server.mjs']], [['lib/link', '', '1']], [['lib/pax', '', 'x']],
  ]) {
    const invalid = join(root, 'invalid');
    assert.throws(() => extractTarPackage(archive(entries), invalid)); assert.equal(existsSync(invalid), false);
  }
  assert.throws(() => extractTarPackage(archive([['server.mjs']]), target));
});

test('corrupt headers, truncated content and invalid trailers fail before creating the destination', t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-tar-corrupt-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const valid = gunzipSync(archive([['server.mjs']]));
  const header = Buffer.from(valid); header[0] ^= 1;
  const trailer = Buffer.from(valid); trailer[trailer.length - 1] = 1;
  for (const bytes of [header, valid.subarray(0, 515), valid.subarray(0, valid.length - 512), trailer]) {
    const destination = join(root, 'rejected');
    assert.throws(() => extractTarPackage(gzipSync(bytes), destination));
    assert.equal(existsSync(destination), false);
  }
});
