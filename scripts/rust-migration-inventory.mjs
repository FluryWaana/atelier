import assert from 'node:assert/strict';
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, posix } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const folders = ['lib', 'public', 'scripts', 'test', 'examples', 'styles', '.github', 'crates', 'xtask', '.cargo', 'docs/archive/node-delivery'];
const entrypoints = ['server.mjs', 'package.json', 'package-lock.json', '.node-version', 'runtime/LICENSE', 'LICENSE', '.gitignore', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'];
function filesIn(directory) {
  return readdirSync(join(root, directory), { withFileTypes: true }).flatMap(entry => {
    const path = directory + '/' + entry.name;
    return entry.isDirectory() ? filesIn(path) : [path];
  });
}
const cell = value => String(value).replaceAll('|', '\\|').replaceAll('\n', ' ');
const link = (path, line) => `[${path}${line ? ':' + line : ''}](../${path}${line ? '#L' + line : ''})`;

export function migrationInventory() {
  const files = [...entrypoints, ...folders.flatMap(filesIn)].sort();
  const sources = new Map(files.filter(path => /\.(mjs|js|html|scss|ps1|cs|swift|py|sh|iss|spec|desktop|yml|cmd|command|rs|toml)$|\/atelier-kanban$/.test(path) && !path.includes('/vendor/')).map(path => [path, readFileSync(join(root, path), 'utf8')]));
  const imports = new Map();
  for (const [path, source] of sources) {
    imports.set(path, [...new Set([...source.matchAll(/(?:\bfrom\s*|\bimport\s*\(?\s*)['"](\.[^'"]+)['"]/g)].map(match => posix.normalize(posix.join(posix.dirname(path), match[1]))))].sort());
  }
  const lines = ['# Inventaire de migration Node et Rust', '', 'Généré par `runtime/node.exe scripts/rust-migration-inventory.mjs`. Vérification sans écriture : ajouter `--check`.', '',
    'Périmètre : points d’entrée et tous les fichiers de lib, public, scripts, test, examples, styles, .github, crates, xtask, .cargo et docs/archive/node-delivery ; aucune donnée utilisateur ni artefact target. L’archive node-delivery est exclusivement historique : lanceurs/outils et tests Node de livraison se consultent au snapshot 7f48996, pas comme commandes actives. Les imports JavaScript sont des références statiques littérales (y compris réexports et import dynamique littéral), pas une preuve de couverture. Les liens Rust sont à examiner via Cargo et les déclarations mod/use ; les chargements calculés, tables de routes, appels par injection et fichiers natifs demandent la revue indiquée dans la matrice.', '',
    'Les numéros de ligne servent à retrouver les contrats. Les extraits sont des repères lexicaux : les lignes voisines restent nécessaires pour interpréter validation, erreurs et limites.', '',
    '## Fichiers et appelants directs', '', '| Fichier | Nature | Importé directement par |', '| --- | --- | --- |'];
  for (const path of files) {
    const callers = [...imports].filter(([, refs]) => refs.includes(path)).map(([caller]) => link(caller));
    const kind = path.startsWith('docs/archive/node-delivery/') ? 'Archive historique Node — non active' : path.startsWith('test/') ? 'Test / support' : path.includes('/vendor/') ? 'Ressource tierce' : sources.has(path) ? 'Source / configuration' : 'Ressource / entrée';
    lines.push(`| ${link(path)} | ${kind} | ${callers.join(', ') || 'Entrée, ressource ou appel calculé/injecté : voir matrice'} |`);
  }
  const sections = [
    ['Exports et réexports', (path, line) => /\.(mjs|js)$/.test(path) && /^export\b/.test(line)],
    ['Modules, dépendances et contrats Rust', (path, line) => /\.(rs|toml)$/.test(path) && /^\s*(?:pub |use |mod |#\[(?:test|tokio::test)|.*(?:workspace|version)\s*=|\[.*dependencies)/.test(line)],
    ['Tests et scénarios nommés', (path, line) => path.startsWith('test/') && /\btest\s*\(/.test(line)],
    ['Environnement et arguments CLI', (path, line) => !path.startsWith('test/') && /process\.(env|argv)|\b(?:env|environment)\s*(?:\.|\[)|\$env:|getenv|EnvironmentVariable|os\.environ|sys\.argv|CommandLine|GetCommandLineArgs|\bargs\b|^param\(|\$[@#0-9]|%(?:\*|~dp0)|\$\{?[A-Z][A-Z_]+/.test(line)],
    ['Routes, méthodes et événements', (path, line) => (path === 'server.mjs' || path.startsWith('lib/http/')) && /\/api\/|\\\/api\\\/|method|GET:|POST:|DELETE:|res\.write|heartbeat/.test(line)],
    ['Stockage navigateur et navigation', (path, line) => path.startsWith('public/') && /Storage|languageKey|location\.|history\.|popstate|hashchange/.test(line)],
    ['Sélecteurs et attributs UI', (path, line) => path.startsWith('public/') && /querySelector|\$\(['"`]|getElementById|data-[a-z-]+|\bid=/.test(line)],
    ['Fichiers, sessions et journaux persistés', (path, line) => !path.startsWith('test/') && /local-session|local-browser|kanban\.sqlite|token_vault|update-result|transaction\.json|config\.json|\.update-lock|\.log['"]|store\.log|CREATE TABLE|stream|\.updates/.test(line)],
  ];
  for (const [title, matches] of sections) {
    lines.push('', '## ' + title, '', '| Source | Repère |', '| --- | --- |');
    for (const [path, source] of sources) {
      if (path === 'scripts/rust-migration-inventory.mjs') continue;
      source.split(/\r?\n/).forEach((line, i) => {
        if (!matches(path, line)) return;
        const selectors = title === 'Sélecteurs et attributs UI' ? [...line.matchAll(/(?:querySelector(?:All)?|\$|getElementById)\s*\(\s*(['"`])([^'"\r\n`]+)\1|(?:id|data-[a-z-]+)=["']([^"']*)/g)].map(match => match[0]) : [];
        const excerpt = selectors.length ? [...new Set(selectors)].join(' ; ') : line.trim();
        lines.push(`| ${link(path, i + 1)} | <code>${cell(excerpt).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('`', '&#96;')}</code> |`);
      });
    }
  }
  lines.push('', `Total : ${files.length} fichiers ; ${files.filter(path => path.endsWith('.test.mjs')).length} fichiers de tests Node.`, '');
  return lines.join('\n');
}

if (import.meta.main) {
  assert.ok(process.argv.slice(2).every(arg => arg === '--check'), 'Seule option admise : --check');
  const destination = join(root, 'docs', 'rust-migration-inventory.md');
  const content = migrationInventory();
  if (process.argv.includes('--check')) assert.equal(readFileSync(destination, 'utf8').replaceAll('\r\n', '\n'), content, 'Inventaire périmé : régénérer puis revoir les nouveaux contrats.');
  else writeFileSync(destination, content);
  console.log('Inventaire de migration ' + (process.argv.includes('--check') ? 'vérifié.' : 'généré.'));
}
