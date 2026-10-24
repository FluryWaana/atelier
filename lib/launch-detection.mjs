import { lstatSync, fstatSync, openSync, closeSync, constants, readdirSync, readFileSync, realpathSync } from 'node:fs';
import { isAbsolute, join, relative, sep } from 'node:path';

export function detectLaunch(repo) {
  const root = realpathSync(repo), candidates = [], warnings = [];
  const skipped = new Set(['node_modules', 'vendor', 'bin', 'obj', 'dist', 'build', 'coverage', 'data', 'runtime']);
  let visited = 0;
  function read(path) {
    const stat = lstatSync(path, { bigint: true });
    const rel = relative(root, realpathSync(path));
    if (rel === '..' || rel.startsWith('..' + sep) || isAbsolute(rel)) throw new Error('Fichier hors du projet.');
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 1024 * 1024) throw new Error('Fichier ignoré (lien ou taille excessive).');
    // codeql[js/file-system-race] The opened descriptor's identity and size are rechecked before any content is read.
    const fd = openSync(path, constants.O_RDONLY | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0));
    try {
      const opened = fstatSync(fd, { bigint: true });
      if (opened.dev !== stat.dev || opened.ino !== stat.ino || !opened.isFile() || opened.size > 1024 * 1024) throw new Error('Fichier modifié pendant sa lecture.');
      return readFileSync(fd, 'utf8');
    } finally { closeSync(fd); }
  }
  function scan(path, depth, inheritedManager = 'npm') {
    if (++visited > 300) return;
    let entries;
    try { entries = readdirSync(path, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)); }
    catch { warnings.push(`Dossier illisible : ${relative(root, path) || '.'}`); return; }
    const files = new Set(entries.filter(entry => entry.isFile()).map(entry => entry.name));
    let manager = files.has('pnpm-lock.yaml') ? 'pnpm' : files.has('yarn.lock') ? 'yarn' : files.has('package-lock.json') ? 'npm' : inheritedManager;
    const directory = relative(root, path).replaceAll('\\', '/');
    if (files.has('package.json')) {
      try {
        const manifest = JSON.parse(read(join(path, 'package.json')));
        const declared = manifest.packageManager?.split('@')[0];
        if (['npm', 'pnpm', 'yarn'].includes(declared)) manager = declared;
        const scripts = manifest.scripts || {}, dependencies = { ...manifest.dependencies, ...manifest.devDependencies };
        const script = ['dev', 'start:dev', 'serve', 'start'].find(name => typeof scripts[name] === 'string');
        if (script) {
          const source = scripts[script];
          const direct = !/[;&|<>]/.test(source);
          const framework = direct && /^\s*vite(?:\s|$)/.test(source) ? 'Vite' : direct && /^\s*next\s+(dev|start)(?:\s|$)/.test(source) ? 'Next.js' : direct && /^\s*ng\s+serve(?:\s|$)/.test(source) ? 'Angular' : '';
          const technology = framework || (dependencies['@nestjs/core'] ? 'NestJS' : dependencies.express ? 'Express' : dependencies.fastify ? 'Fastify' : dependencies['react-scripts'] ? 'Create React App' : 'Node.js');
          let command = `${manager} run ${script}`;
          if (framework) command += `${manager === 'npm' ? ' --' : ''} --port {port} ${framework === 'Next.js' ? '--hostname' : '--host'} 127.0.0.1${framework === 'Vite' ? ' --strictPort' : ''}`;
          candidates.push({ id: `${directory}/package.json`, directory, command, technology, role: framework || technology === 'Create React App' ? 'front' : 'back', file: `${directory ? directory + '/' : ''}package.json`, automaticPort: Boolean(framework) });
        }
      } catch { warnings.push(`package.json illisible ou invalide : ${directory || '.'}`); }
    }
    for (const file of files) if (/^[\p{L}\p{N}_. -]+\.csproj$/iu.test(file)) {
      try {
        const source = read(join(path, file)), profiles = [];
        const launchPath = join(path, 'Properties', 'launchSettings.json');
        try {
          // Read only profile names and URLs; profile environment values stay in the repository.
          const settings = JSON.parse(read(launchPath));
          for (const [name, profile] of Object.entries(settings.profiles || {})) {
            if (!profile || typeof profile !== 'object') continue;
            const supported = profile.commandName === 'Project' && /^[\p{L}\p{N}_. :/()-]+$/u.test(name);
            const urls = typeof profile.applicationUrl === 'string' ? profile.applicationUrl.split(';').flatMap(value => {
              try { const url = new URL(value.trim()); return ['http:', 'https:'].includes(url.protocol) ? [url.origin] : []; } catch { return []; }
            }) : [];
            profiles.push({ name, commandName: typeof profile.commandName === 'string' ? profile.commandName : '', urls, supported });
          }
        } catch (error) { if (error.code !== 'ENOENT') warnings.push(`Profils .NET illisibles : ${directory || '.'}/Properties/launchSettings.json`); }
        const web = /\bSdk\s*=\s*["']Microsoft\.NET\.Sdk\.Web["']/i.test(source);
        const runnable = web || /<OutputType>\s*(Exe|WinExe)\s*<\/OutputType>/i.test(source) || profiles.some(profile => profile.supported);
        const baseCommand = `dotnet run --project "${file}"`;
        const defaultProfile = profiles.find(profile => profile.supported)?.name || '';
        const command = runnable ? `${baseCommand} ${defaultProfile ? `--launch-profile "${defaultProfile}"` : '--no-launch-profile'}${web ? ' -- --urls http://127.0.0.1:{port}' : ''}` : '';
        candidates.push({ id: `${directory}/${file}`, directory, command, baseCommand, defaultProfile, profiles, technology: web ? 'ASP.NET Core' : runnable ? '.NET' : '.NET (bibliothèque)', role: 'back', file: `${directory ? directory + '/' : ''}${file}`, automaticPort: web, runnable });
      } catch { warnings.push(`Projet .NET illisible : ${directory}/${file}`); }
    }
    // shortcut: bounded manifest scan; extend discovery when deeper repositories need it.
    if (depth < 3) for (const entry of entries) if (entry.isDirectory() && !entry.name.startsWith('.') && !skipped.has(entry.name)) scan(join(path, entry.name), depth + 1, manager);
  }
  scan(root, 0);
  if (visited > 300) warnings.push('Analyse limitée aux 300 premiers dossiers.');
  return { candidates, warnings };
}
