import { cpSync, mkdirSync, copyFileSync, chmodSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash, randomUUID } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { githubRepository, packageName, releaseVersion } from '../lib/domain/releases.mjs';
import { managedEntries, dataVersion } from '../lib/domain/release-package.mjs';

packageName('0.0.0', process.platform, process.arch);
const binary = process.platform === 'win32' ? 'node.exe' : 'node';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repository = githubRepository(process.argv[2] || process.env.GITHUB_REPOSITORY);
const { version } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')); releaseVersion(version);
const nodeVersion = process.versions.node;
if (Number(nodeVersion.split('.')[0]) < 24) throw new Error('Node.js 24 ou supérieur requis.');
const runtimeLicense = process.env.NODE_LICENSE || join(dirname(process.execPath), 'LICENSE');
if (!existsSync(runtimeLicense)) throw new Error('Le fichier LICENSE de Node.js doit être à côté du runtime (ou fourni via NODE_LICENSE).');
const dist = join(root, 'dist'); mkdirSync(dist, { recursive: true });
const stage = join(dist, 'build-' + randomUUID()); mkdirSync(stage);
for (const name of managedEntries.filter(name => !['runtime', 'release.json'].includes(name))) cpSync(join(root, name), join(stage, name), { recursive: true, dereference: false });
copyFileSync(join(root, 'LICENSE'), join(stage, 'docs', 'LICENSE'));
mkdirSync(join(stage, 'runtime')); copyFileSync(process.execPath, join(stage, 'runtime', binary)); copyFileSync(runtimeLicense, join(stage, 'runtime', 'LICENSE'));
writeFileSync(join(stage, 'release.json'), JSON.stringify({ repository, version, dataVersion, nodeVersion, platform: process.platform, arch: process.arch }, null, 2) + '\n');
const name = packageName(version, process.platform, process.arch), archive = join(dist, name);
if (existsSync(archive)) throw new Error('Ce package existe déjà dans dist. Utilise une nouvelle version ou déplace le précédent.');
if (process.platform === 'win32') {
  const compiler = join(process.env.SystemRoot, 'Microsoft.NET', 'Framework64', 'v4.0.30319', 'csc.exe');
  execFileSync(compiler, ['/nologo', '/target:winexe', '/reference:System.Windows.Forms.dll', '/reference:System.Drawing.dll', '/win32icon:' + join(stage, 'public', 'atelier-kanban.ico'), '/out:' + join(stage, 'scripts', 'WindowsTray.exe'), join(stage, 'scripts', 'WindowsTray.cs')], { windowsHide: true, stdio: 'inherit' });
  execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'RemoteSigned', '-File', join(root, 'scripts', 'Compress-Release.ps1'), '-Source', stage, '-Destination', archive], { windowsHide: true, stdio: 'inherit' });
}
else {
  if (process.platform === 'darwin') execFileSync('xcrun', ['swiftc', '-swift-version', '5', '-O', '-framework', 'AppKit', '-o', join(stage, 'scripts', 'MacTray'), join(stage, 'scripts', 'MacTray.swift')], { stdio: 'inherit' });
  for (const name of ['runtime/node', 'Start.command', 'Stop.command']) chmodSync(join(stage, name), 0o755);
  execFileSync('tar', ['--format=ustar', '-czf', archive, '-C', stage, ...managedEntries], { stdio: 'inherit', env: { ...process.env, COPYFILE_DISABLE: '1' } });
}
const hash = createHash('sha256').update(readFileSync(archive)).digest('hex');
writeFileSync(archive + '.sha256', `${hash}  ${name}\n`);
console.log(archive);
