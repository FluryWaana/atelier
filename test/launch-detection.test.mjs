import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { detectLaunch } from '../lib/launch-detection.mjs';

test('detects front and ASP.NET apps, inherits package manager and proposes port overrides without running scripts', () => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-detect-'));
  const write = (path, content) => { mkdirSync(join(root, path, '..'), { recursive: true }); writeFileSync(join(root, path), typeof content === 'string' ? content : JSON.stringify(content)); };
  try {
    write('pnpm-lock.yaml', '');
    write('apps/web/package.json', { scripts: { dev: 'vite --port 5173' }, dependencies: { vite: '*' } });
    write('src/Api/Api.csproj', '<Project Sdk="Microsoft.NET.Sdk.Web"></Project>');
    write('src/Library/Library.csproj', '<Project Sdk="Microsoft.NET.Sdk"></Project>');
    write('node_modules/ignored/package.json', { scripts: { dev: 'next dev' } });
    const { candidates, warnings } = detectLaunch(root);
    assert.equal(candidates.length, 3);
    const front = candidates.find(candidate => candidate.role === 'front'), back = candidates.find(candidate => candidate.technology === 'ASP.NET Core');
    assert.equal(candidates.find(candidate => candidate.technology === '.NET (bibliothèque)').runnable, false);
    assert.equal(front.directory, 'apps/web');
    assert.equal(front.technology, 'Vite');
    assert.equal(front.command, 'pnpm run dev --port {port} --host 127.0.0.1 --strictPort');
    assert.equal(back.directory, 'src/Api');
    assert.match(back.command, /--urls http:\/\/127\.0\.0\.1:\{port\}/);
    assert.deepEqual(warnings, []);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('.NET detection lists projects and profiles, keeps profile URLs but never returns environment secrets', () => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-detect-dotnet-'));
  try {
    writeFileSync(join(root, 'Api.csproj'), '<Project Sdk="Microsoft.NET.Sdk.Web"/>');
    writeFileSync(join(root, 'Worker.csproj'), '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType></PropertyGroup></Project>');
    mkdirSync(join(root, 'Properties'));
    writeFileSync(join(root, 'Properties/launchSettings.json'), JSON.stringify({ profiles: {
      http: { commandName: 'Project', applicationUrl: 'http://localhost:5050', environmentVariables: { Secret: 'do-not-expose' } },
      https: { commandName: 'Project', applicationUrl: 'https://localhost:7050;http://localhost:5050' },
      'IIS Express': { commandName: 'IISExpress' },
      'unsafe%NAME%': { commandName: 'Project' },
    } }));
    const result = detectLaunch(root), api = result.candidates.find(candidate => candidate.file === 'Api.csproj');
    assert.equal(result.candidates.length, 2);
    assert.equal(api.profiles.length, 4);
    assert.equal(api.defaultProfile, 'http');
    assert.match(api.command, /--launch-profile "http" -- --urls http:\/\/127\.0\.0\.1:\{port\}/);
    assert.deepEqual(api.profiles[1].urls, ['https://localhost:7050', 'http://localhost:5050']);
    assert.equal(api.profiles[2].supported, false);
    assert.equal(api.profiles[3].supported, false);
    assert.doesNotMatch(JSON.stringify(result), /do-not-expose|environmentVariables/);
    assert.equal(result.candidates.find(candidate => candidate.file === 'Worker.csproj').automaticPort, false);
    writeFileSync(join(root, 'Properties/launchSettings.json'), '{invalid');
    assert.equal(detectLaunch(root).warnings.length, 2);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('does not assume port support for custom scripts or execute project code, and reports broken manifests', () => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-detect-custom-'));
  try {
    writeFileSync(join(root, 'package.json'), JSON.stringify({ packageManager: 'yarn@4', scripts: { dev: 'vite && echo unexpected > executed.txt' } }));
    mkdirSync(join(root, 'broken')); writeFileSync(join(root, 'broken/package.json'), '{bad');
    const detected = detectLaunch(root);
    assert.equal(detected.candidates[0].command, 'yarn run dev');
    assert.equal(detected.candidates[0].automaticPort, false);
    assert.equal(existsSync(join(root, 'executed.txt')), false);
    assert.equal(detected.warnings.length, 1);
    writeFileSync(join(root, 'package.json'), JSON.stringify({ scripts: { start: 'next dev' } }));
    assert.match(detectLaunch(root).candidates[0].command, /^npm run start -- --port \{port\} --hostname 127.0.0.1$/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
