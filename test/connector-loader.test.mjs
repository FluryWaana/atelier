import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { loadConnectors, loadBuiltinConnectors } from '../lib/infrastructure/connector-loader.mjs';

for (const builtin of [false, true]) {
  test(`${builtin ? 'builtin' : 'local'} connector import failures do not prevent other modules loading`, async () => {
    const directory = mkdtempSync(join(tmpdir(), 'atelier-loader-')), errors = [];
    try {
      for (const [name, source] of Object.entries({ broken: 'export default {', missing: "import './absent.mjs';", invalid: 'export default {};', healthy: "export default { id: 'healthy', name: 'Healthy', apiVersion: 1 };" })) {
        const folder = builtin ? directory : join(directory, name);
        mkdirSync(folder, { recursive: true });
        writeFileSync(join(folder, builtin ? name + '.mjs' : 'index.mjs'), source);
      }
      const load = builtin ? loadBuiltinConnectors : loadConnectors;
      const modules = await load(directory, error => errors.push(error));
      assert.deepEqual(modules.map(module => module.id), ['healthy']);
      assert.equal(errors.length, 3);
      assert.ok(errors.every(error => error.id && error.message));
    } finally { rmSync(directory, { recursive: true, force: true }); }
  });
}
