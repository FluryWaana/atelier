import test from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';

test('domain and application dependencies point inward and contain no HTTP or SQLite access', () => {
  for (const layer of ['domain', 'application']) {
    const directory = new URL('../lib/' + layer + '/', import.meta.url);
    for (const name of readdirSync(directory).filter(name => name.endsWith('.mjs'))) {
      const source = readFileSync(new URL(name, directory), 'utf8');
      for (const [, dependency] of source.matchAll(/(?:from\s+|import\s*)['"]([^'"]+)['"]/g)) {
        assert.ok(dependency.startsWith(layer === 'domain' ? './' : '../domain/') || layer === 'application' && dependency.startsWith('./'), `${layer}/${name}: ${dependency}`);
      }
      assert.doesNotMatch(source, /\b(?:store\.db|req\.method|process\.platform|process\.env|fetch\s*\()/, layer + '/' + name);
    }
  }
});
