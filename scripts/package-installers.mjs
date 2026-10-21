import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { extractTarPackage } from '../lib/infrastructure/tar-package.mjs';
import { verifyChecksum } from '../lib/infrastructure/package-updater.mjs';
import { githubRepository, packageName, installerNames } from '../lib/domain/releases.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const dist = join(root, 'dist'), templates = join(root, 'scripts', 'installers');
const { version } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
const names = installerNames(version, process.platform, process.arch);
if (!names.length) throw new Error('Installateurs disponibles uniquement pour Windows x64 et Linux x64/ARM64.');
for (const name of names) if (existsSync(join(dist, name))) throw new Error('Installateur déjà présent : ' + name);
const archiveName = packageName(version, process.platform, process.arch), archive = join(dist, archiveName);
verifyChecksum(readFileSync(archive), readFileSync(archive + '.sha256', 'utf8'), archiveName);
const stage = mkdtempSync(join(tmpdir(), 'atelier-installer-build-')), app = join(stage, 'app');
const run = (command, args) => execFileSync(command, args, { windowsHide: true, stdio: 'inherit' });
if (process.platform === 'win32') {
  run('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'RemoteSigned', '-File', join(root, 'scripts', 'Expand-Update.ps1'), '-Archive', archive, '-Destination', app]);
  writeFileSync(join(app, 'installation.json'), '{"mode":"user"}\n');
  const compiler = join(process.env.SystemRoot, 'Microsoft.NET', 'Framework64', 'v4.0.30319', 'csc.exe');
  run(compiler, ['/nologo', '/target:winexe', '/reference:System.Windows.Forms.dll', '/out:' + join(app, 'AtelierKanban.exe'), join(templates, 'AtelierKanban.cs')]);
  const iscc = process.env.ISCC || join(process.env['ProgramFiles(x86)'], 'Inno Setup 6', 'ISCC.exe');
  run(iscc, ['/DSourceDir=' + app, '/DOutputDir=' + dist, '/DReleaseVersion=' + version, join(templates, 'windows.iss')]);
} else {
  extractTarPackage(readFileSync(archive), app);
  const manifest = JSON.parse(readFileSync(join(app, 'release.json'), 'utf8'));
  const repository = githubRepository(manifest.repository);
  const payload = join(stage, 'payload');
  mkdirSync(join(payload, 'opt'), { recursive: true });
  cpSync(app, join(payload, 'opt', 'atelier-kanban'), { recursive: true });
  writeFileSync(join(payload, 'opt', 'atelier-kanban', 'installation.json'), '{"mode":"system"}\n');
  for (const [source, target, mode] of [
    ['atelier-kanban', 'usr/bin/atelier-kanban', 0o755],
    ['atelier-kanban.desktop', 'usr/share/applications/atelier-kanban.desktop', 0o644],
    ['atelier-kanban.svg', 'usr/share/icons/hicolor/scalable/apps/atelier-kanban.svg', 0o644],
  ]) {
    mkdirSync(dirname(join(payload, target)), { recursive: true });
    copyFileSync(join(templates, source), join(payload, target)); chmodSync(join(payload, target), mode);
  }
  run('desktop-file-validate', [join(payload, 'usr/share/applications/atelier-kanban.desktop')]);
  const deb = join(stage, 'deb'); cpSync(payload, deb, { recursive: true }); mkdirSync(join(deb, 'DEBIAN'));
  writeFileSync(join(deb, 'DEBIAN', 'control'), `Package: atelier-kanban\nVersion: ${version}\nArchitecture: ${process.arch === 'x64' ? 'amd64' : 'arm64'}\nMaintainer: Atelier Kanban <noreply@github.com>\nSection: devel\nPriority: optional\nDepends: libc6 (>= 2.28), libstdc++6 (>= 8), libgcc-s1, libsecret-tools, xdg-utils, lsof, zenity\nHomepage: https://github.com/${repository}\nDescription: Tableau de tâches local Atelier Kanban\n Node.js est inclus. Les données restent dans le dossier personnel.\n`);
  for (const name of ['preinst', 'prerm']) { copyFileSync(join(templates, 'check-stopped.sh'), join(deb, 'DEBIAN', name)); chmodSync(join(deb, 'DEBIAN', name), 0o755); }
  run('dpkg-deb', ['--root-owner-group', '--build', deb, join(dist, names[0])]);
  const rpmRoot = join(stage, 'rpmbuild'); mkdirSync(rpmRoot);
  const spec = join(stage, 'linux.spec');
  writeFileSync(spec, readFileSync(join(templates, 'linux.spec'), 'utf8').replaceAll('%{check_stopped}', () => readFileSync(join(templates, 'check-stopped.sh'), 'utf8').trim()));
  run('rpm', ['--dbpath', join(rpmRoot, 'db'), '--initdb']);
  run('rpmbuild', ['-bb', '--target', process.arch === 'x64' ? 'x86_64' : 'aarch64',
    '--define', '_topdir ' + rpmRoot, '--define', '_rpmdir ' + dist, '--define', '_rpmfilename ' + names[1],
    '--define', '_dbpath ' + join(rpmRoot, 'db'),
    '--define', 'release_version ' + version, '--define', 'repository ' + repository,
    '--define', 'payload ' + payload, spec]);
}
for (const name of names) {
  const hash = createHash('sha256').update(readFileSync(join(dist, name))).digest('hex');
  writeFileSync(join(dist, name + '.sha256'), `${hash}  ${name}\n`);
  console.log(join(dist, name));
}
