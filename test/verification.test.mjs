import test from 'node:test';
import assert from 'node:assert/strict';
import { runVerification } from '../lib/verification.mjs';

const node = code => `"${process.execPath}" -e "${code}"`;
test('verification runs the configured command, captures logs and enforces the exit status', async () => {
  const controller = new AbortController(); let output = '';
  await runVerification(node("console.log('ok')"), process.cwd(), { signal: controller.signal, onOutput: chunk => { output += chunk; } });
  assert.match(output, /ok/);
  await assert.rejects(runVerification(node('process.exit(3)'), process.cwd(), { signal: controller.signal }), /code 3/);
});
test('verification never accepts a successful exit after cancellation', async () => {
  const controller = new AbortController();
  const task = runVerification(node("console.log('started'); setTimeout(() => process.exit(0), 1000)"), process.cwd(), { signal: controller.signal, onOutput: () => controller.abort() });
  await assert.rejects(task, /arrêté/);
});
