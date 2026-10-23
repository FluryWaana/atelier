import assert from 'node:assert/strict';
import { isDeepStrictEqual } from 'node:util';
import { readFileSync, writeFileSync } from 'node:fs';
import { openStore } from '../../lib/store.mjs';
import { seedMigrationData } from './migration-data.mjs';

function snapshot(store) {
  const logs = [];
  for (;;) {
    const page = store.logs('run-0', logs.at(-1)?.seq || 0);
    if (!page.length) break;
    // SQLite rows have null prototypes; compare their complete JSON-visible fields.
    logs.push(...page.map(row => ({ ...row })));
  }
  return { projects: store.projects(), cards: store.cards(), runs: store.runs(), times: store.times(), settings: store.settings(), logs };
}

const [mode, path, reference] = process.argv.slice(2);
assert.ok(['seed', 'verify', 'change'].includes(mode) && path && reference, 'Usage: seed|verify|change DATABASE REFERENCE_JSON');
const store = openStore(path);
try {
  if (mode === 'seed') {
    seedMigrationData(store);
    store.log('run-0', 'stderr', 'a'.repeat(65535) + '🔧tail');
    writeFileSync(reference, JSON.stringify(snapshot(store)));
  } else {
    const actual = snapshot(store), expected = JSON.parse(readFileSync(reference, 'utf8'));
    for (const key of Object.keys(actual)) {
      if (Array.isArray(actual[key])) {
        assert.equal(actual[key].length, expected[key].length, key + ' length');
        actual[key].forEach((row, index) => assert.ok(isDeepStrictEqual(row, expected[key][index]), `${key}[${index}] differs after Rust round trip`));
      } else assert.ok(isDeepStrictEqual(actual[key], expected[key]), key + ' differs after Rust round trip');
    }
    if (mode === 'change') {
      store.putCard({ ...store.card('card-0'), nodeRoundTrip: { preserved: true } });
      writeFileSync(reference, JSON.stringify(snapshot(store)));
    }
  }
} finally { store.db.close(); }
