import { lstatSync, mkdtempSync, rmdirSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { sameLocation } from '../paths.mjs';
import { readCheckedFile } from './checked-file.mjs';

// Local-scope MCPs are keyed by the source checkout, not by each task's worktree.
export function claudeProjectMcp(source, worktree, env) {
  if (!source || sameLocation(source, worktree)) return null;
  const home = env.USERPROFILE || env.HOME;
  if (!env.CLAUDE_CONFIG_DIR && !home) return null;
  const path = join(env.CLAUDE_CONFIG_DIR || home, '.claude.json');
  let config;
  try {
    const stat = lstatSync(path, { bigint: true });
    if (!stat.isFile()) throw new Error('Configuration Claude non régulière.');
    const bytes = readCheckedFile(path, stat, 4 * 1024 * 1024);
    try { config = JSON.parse(bytes.toString('utf8')); }
    catch { throw new Error('Configuration Claude illisible : vérifiez .claude.json dans votre profil.'); }
  } catch (error) { if (error.code === 'ENOENT') return null; throw error; }
  const project = Object.entries(config?.projects || {}).find(([path]) => sameLocation(path, source))?.[1];
  if (!project?.mcpServers || typeof project.mcpServers !== 'object' || Array.isArray(project.mcpServers)) return null;
  const servers = Object.fromEntries(Object.entries(project.mcpServers).filter(([name]) => !project.disabledMcpServers?.includes(name)));
  if (!Object.keys(servers).length) return null;
  if (Object.values(servers).some(server => !server || typeof server !== 'object' || Array.isArray(server))) throw new Error('Configuration MCP Claude invalide.');
  const secrets = Object.values(servers).flatMap(server => [
    ...Object.values(server.env || {}), ...Object.values(server.headers || {}).flatMap(value => typeof value === 'string' ? [value, value.replace(/^(?:Bearer|Basic)\s+/i, '')] : []), server.url,
    ...(Array.isArray(server.args) ? server.args.filter(arg => typeof arg === 'string' && !arg.startsWith('-')) : []),
  ]).filter(value => typeof value === 'string' && value);
  const directory = mkdtempSync(join(tmpdir(), 'atelier-claude-mcp-'));
  const file = join(directory, 'servers.json');
  const cleanup = () => { try { unlinkSync(file); } catch (error) { if (error.code !== 'ENOENT') throw error; } rmdirSync(directory); };
  try { writeFileSync(file, JSON.stringify({ mcpServers: servers }), { mode: 0o600 }); }
  catch (error) { cleanup(); throw error; }
  return { file, secrets, cleanup };
}
