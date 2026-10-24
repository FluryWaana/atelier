import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { detectLaunch } from '../../lib/launch-detection.mjs';

const [mode, root, result] = process.argv.slice(2);
if (mode === 'seed') {
  const cases = [];
  const write = (name, file, value) => {
    const path = join(root, name, file);
    mkdirSync(join(path, '..'), { recursive: true });
    writeFileSync(path, typeof value === 'string' || Buffer.isBuffer(value) ? value : JSON.stringify(value));
  };
  const add = (name, files) => { mkdirSync(join(root, name), { recursive: true }); cases.push(name); for (const [file, value] of Object.entries(files)) write(name, file, value); };
  add('frameworks', {});
  for (const [index, source] of ['vite', ' vite --port 9999 ', '\ufeffnext\tdev', 'next start', 'ng\u00a0serve', 'vite && echo forbidden', 'VITE', 'vite-preview', 'vite\u0085', 'node server.mjs'].entries()) {
    write('frameworks', `${String(index).padStart(2, '0')}/package.json`, { scripts: { dev: source, start: 'ignored' } });
  }
  add('managers', { 'pnpm-lock.yaml': '', 'package.json': { packageManager: 'yarn@4' }, 'nested/package.json': { scripts: { serve: 'next dev' } }, 'nested/npm/package-lock.json': '', 'nested/npm/package.json': { scripts: { start: 'vite' } }, 'bad/package.json': { packageManager: false }, 'bad/child/package.json': { scripts: { dev: 'vite' } } });
  add('technologies', {});
  for (const [index, dependencies] of [{ '@nestjs/core': '*' }, { express: '*' }, { fastify: '*' }, { 'react-scripts': '*' }, { express: false }].entries()) write('technologies', `${index}/package.json`, { scripts: { start: 'node app.js' }, dependencies });
  write('technologies', 'override/package.json', { scripts: { dev: 1, 'start:dev': '' }, dependencies: { express: '*' }, devDependencies: { express: null, fastify: '*' } });
  add('dotnet', {
    'Api.csproj': '<Project Sdk="Microsoft.NET.Sdk.Web"/>',
    'Worker.csproj': '<Project><OutputType> WinExe </OutputType></Project>',
    'Library.csproj': '<Project Sdk="Microsoft.NET.Sdk"/>',
    'Équipe.CSPROJ': '<Project SDK=\'microsoft.net.sdk.web\'/>',
    'ignored%.csproj': '<Project Sdk="Microsoft.NET.Sdk.Web"/>',
    'Properties/launchSettings.json': { profiles: { '10': { commandName: 'Project' }, '2': { commandName: 'Project' }, http: { commandName: 'Project', applicationUrl: 'https://localhost:7000/a;http://localhost:5000;https://user:secret@example.com:443/path;invalid;file:///private', environmentVariables: { PRIVATE: 'never-return-this' } }, 'IIS Express': { commandName: 'IISExpress' }, 'unsafe%NAME%': { commandName: 'Project' }, '\ud800': { commandName: '\udc00' }, ignored: false, array: [] } },
  });
  add('libraries', { 'Library.csproj': '<Project/>', 'worker/Worker.csproj': '<Project><OutputType>exe</OutputType></Project>', 'bad/Api.csproj': '<Project Sdk="Microsoft.NET.Sdk.Web"/>', 'bad/Other.csproj': '<Project/>', 'bad/Properties/launchSettings.json': '{bad' });
  add('malformed', {});
  for (const [index, value] of [null, [], false, 123, '"a string"', '{bad', { packageManager: [] }, { packageManager: {} }, { packageManager: null, scripts: { dev: 'vite' } }, { scripts: 'literal' }, { scripts: { dev: 'vite' }, dependencies: 'literal' }].entries()) write('malformed', `${index}/package.json`, value);
  add('ordering', {});
  for (const name of ['z', 'A', 'a', 'é', 'e\u0301', 'ä', 'Ö', 'ø', 'œ', 'æ', 'a-1', 'a_1']) write('ordering', `${name}/package.json`, { scripts: { dev: 'vite' } });
  add('bounds', { 'package.json': ' '.repeat(1024 * 1024 + 1), 'one/two/three/four/package.json': { scripts: { dev: 'vite' } }, 'one/two/three/package.json': { scripts: { dev: 'vite' } } });
  for (const name of ['node_modules', 'vendor', 'bin', 'obj', 'dist', 'build', 'coverage', 'data', 'runtime', '.hidden']) write('bounds', `${name}/package.json`, { scripts: { dev: 'vite' } });
  add('directory-limit', {});
  for (let index = 0; index < 305; index++) write('directory-limit', `${String(index).padStart(3, '0')}/package.json`, { scripts: { dev: 'vite' } });
  add('utf8', { 'package.json': Buffer.from('{"scripts":{"dev":"vite \xff"}}', 'latin1') });
  add('links', { 'Api.csproj': '<Project/>' });
  const outside = join(root, 'outside-synthetic'); mkdirSync(outside);
  writeFileSync(join(outside, 'launchSettings.json'), '{"profiles":{"secret":{"commandName":"Project"}}}');
  symlinkSync(outside, join(root, 'links/Properties'), process.platform === 'win32' ? 'junction' : 'dir');
  symlinkSync(outside, join(root, 'links/escape'), process.platform === 'win32' ? 'junction' : 'dir');
  const expected = cases.map(name => ({ name, value: detectLaunch(join(root, name)) }));
  assert.ok(!JSON.stringify(expected).includes('never-return-this'));
  writeFileSync(result, JSON.stringify(expected));
} else if (mode === 'verify') {
  const expected = JSON.parse(readFileSync(join(root, 'expected.json'), 'utf8'));
  const actual = JSON.parse(readFileSync(result, 'utf8'));
  assert.deepEqual(actual, expected);
  console.log(`${expected.length} arbres synthétiques : manifestes, Unicode, profils, limites et liens Node/Rust identiques.`);
} else throw new Error('Unknown detection fixture mode.');
