import { readdirSync, existsSync, realpathSync } from 'node:fs';
import { join, relative, isAbsolute } from 'node:path';
import { pathToFileURL } from 'node:url';
import { validateConnector } from '../domain/connectors.mjs';

// Local connectors execute with the application's privileges; installation is explicit.
export async function loadConnectors(directory, onError = error => console.warn(error.id + ' : ' + error.message)) {
  if (!existsSync(directory)) return [];
  const root = realpathSync(directory), modules = [];
  for (const entry of readdirSync(root, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    if (!entry.isDirectory()) continue;
    const file = join(root, entry.name, 'index.mjs');
    if (!existsSync(file)) continue;
    try {
      const path = realpathSync(file), location = relative(root, path);
      if (location.startsWith('..') || isAbsolute(location)) throw new Error('Le connecteur doit rester dans son dossier d’installation.');
      modules.push(validateConnector((await import(pathToFileURL(path).href)).default));
    } catch (error) { onError({ id: entry.name, message: String(error?.message || error) }); }
  }
  return modules;
}

export async function loadBuiltinConnectors(directory, onError = error => console.warn(error.id + ' : ' + error.message)) {
  if (!existsSync(directory)) return [];
  const modules = [];
  for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    if (!entry.isFile() || !entry.name.endsWith('.mjs')) continue;
    try { modules.push(validateConnector((await import(pathToFileURL(join(directory, entry.name)).href)).default)); }
    catch (error) { onError({ id: entry.name, message: String(error?.message || error) }); }
  }
  return modules;
}
