import { gunzipSync } from 'node:zlib';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { managedEntries } from '../domain/release-package.mjs';

// Accept only the bounded ustar subset produced by our release pipeline, never links or metadata extensions.
export function extractTarPackage(archive, destination) {
  const bytes = gunzipSync(archive, { maxOutputLength: 600 * 1024 * 1024 });
  const files = [], seen = new Set();
  const text = (header, start, end) => header.subarray(start, end).toString('utf8').replace(/\0.*$/s, '');
  const octal = field => {
    if (!/^[0-7]+$/.test(field.trim())) throw new Error('Entier tar invalide.');
    return parseInt(field.trim(), 8);
  };
  let offset = 0;
  while (offset + 512 <= bytes.length) {
    const header = bytes.subarray(offset, offset + 512);
    if (header.every(value => value === 0)) break;
    const checksum = [...header].reduce((sum, value, i) => sum + (i >= 148 && i < 156 ? 32 : value), 0);
    if (checksum !== octal(text(header, 148, 156)) || text(header, 257, 263) !== 'ustar') throw new Error('En-tête tar invalide.');
    const prefix = text(header, 345, 500), name = (prefix ? prefix + '/' : '') + text(header, 0, 100);
    const type = header[156], directory = type === 53;
    const path = directory && name.endsWith('/') ? name.slice(0, -1) : name;
    if (!path || path.includes('\\') || /[\x00-\x1f\x7f:]/.test(path) || path.split('/').some(part => !part || ['.', '..'].includes(part)) || !managedEntries.includes(path.split('/')[0]) || seen.has(path)) throw new Error('Chemin interdit ou dupliqué dans le package.');
    if (![0, 48, 53].includes(type) || text(header, 157, 257)) throw new Error('Liens et extensions tar interdits.');
    const size = octal(text(header, 124, 136));
    if (!Number.isSafeInteger(size) || size > 300 * 1024 * 1024 || directory && size !== 0 || offset + 512 + size > bytes.length) throw new Error('Taille tar invalide.');
    seen.add(path);
    if (seen.size > 10000) throw new Error('Trop de fichiers dans le package.');
    files.push({ path, directory, start: offset + 512, size });
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  if (offset + 1024 > bytes.length || !bytes.subarray(offset).every(value => value === 0)) throw new Error('Archive tar tronquée ou données finales invalides.');
  // Validate the complete archive before writing; create a fresh directory to exclude pre-existing symlinks.
  mkdirSync(destination);
  for (const file of files) {
    const target = join(destination, ...file.path.split('/'));
    if (file.directory) mkdirSync(target, { recursive: true });
    else {
      mkdirSync(dirname(target), { recursive: true });
      const executable = ['runtime/node', 'Start.command', 'Stop.command'].includes(file.path);
      writeFileSync(target, bytes.subarray(file.start, file.start + file.size), { flag: 'wx', mode: executable ? 0o755 : 0o644 });
    }
  }
}
