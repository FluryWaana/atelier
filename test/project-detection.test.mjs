import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createApp } from '../server.mjs';
import { verifyProjectDetection } from './fixtures/rust-project-detection-reference.mjs';

test('project detection preserves draft selection, role filters, scan caching and error priority', verifyProjectDetection);

test('project HTTP detection reads manifests without saving drafts or exposing profile environments', async () => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-detect-http-'));
  const repo = join(root, 'repositories/main'), draft = join(root, 'repositories/draft');
  mkdirSync(join(repo, 'Properties'), { recursive: true }); mkdirSync(draft, { recursive: true });
  writeFileSync(join(repo, 'package.json'), '{"scripts":{"dev":"vite"}}');
  writeFileSync(join(repo, 'Api.csproj'), '<Project Sdk="Microsoft.NET.Sdk.Web"/>');
  writeFileSync(join(repo, 'Properties/launchSettings.json'), '{"profiles":{"http":{"commandName":"Project","environmentVariables":{"SECRET":"synthetic-private-value"}}}}');
  writeFileSync(join(draft, 'package.json'), '{"scripts":{"start":"node server.mjs"},"dependencies":{"express":"*"}}');
  const app = createApp({ dataDir: join(root, 'data'), protectTokens: value => value });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const url = 'http://127.0.0.1:' + app.server.address().port;
    const request = (path, method, body) => fetch(url + path, { method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    const created = await request('/api/projects', 'POST', { name: 'Synthetic detection', repo });
    assert.equal(created.status, 200); const project = await created.json();
    const path = '/api/projects/' + project.id + '/detect-launch';
    const detected = await (await request(path, 'GET')).json();
    assert.deepEqual(detected.candidates.map(candidate => candidate.technology), ['Vite', 'ASP.NET Core']);
    assert.ok(detected.candidates.every(candidate => candidate.sourceRepo === project.repo));
    assert.ok(!JSON.stringify(detected).includes('synthetic-private-value'));
    const edited = await request(path, 'POST', { role: 'front', frontRepo: draft, backRepo: join(root, 'missing-ignored') });
    assert.equal(edited.status, 200); const result = await edited.json();
    assert.equal(result.candidates.length, 1); assert.equal(result.candidates[0].technology, 'Express');
    assert.equal(result.candidates[0].role, 'front'); assert.notEqual(result.candidates[0].sourceRepo, project.repo);
    assert.equal((await request(path, 'POST', { role: 'invalid' })).status, 400);
    assert.equal((await request('/api/projects/missing/detect-launch', 'POST', { role: 'invalid' })).status, 404);
    assert.equal((await fetch(url + path)).status, 401);
    const state = await (await request('/api/state', 'GET')).json();
    assert.deepEqual(state.projects[0], project);
  } finally { await app.close(); rmSync(root, { recursive: true, force: true }); }
});
