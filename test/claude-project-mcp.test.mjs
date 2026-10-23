import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { claudeProjectMcp } from '../lib/infrastructure/claude-project-mcp.mjs';
import { startAgent } from '../lib/agents.mjs';

test('source-scoped MCPs follow task worktrees without copying user or unrelated project configuration', t => {
  const profile = mkdtempSync(join(tmpdir(), 'atelier-mcp-profile-'));
  t.after(() => rmSync(profile, { recursive: true, force: true }));
  const source = join(profile, 'source'), worktree = join(profile, 'task'), path = join(profile, '.claude.json');
  const server = { command: 'mcp-tool', env: { TOKEN: 'private-mcp-value' } };
  const config = JSON.stringify({ mcpServers: { global: { command: 'global-tool' } }, projects: {
    [source]: { mcpServers: { local: server, disabled: server }, disabledMcpServers: ['disabled'] },
    [join(profile, 'other')]: { mcpServers: { other: server } },
  } });
  writeFileSync(path, config);
  const mcp = claudeProjectMcp(source, worktree, { CLAUDE_CONFIG_DIR: profile });
  try {
    assert.deepEqual(JSON.parse(readFileSync(mcp.file, 'utf8')), { mcpServers: { local: server } });
    assert.ok(mcp.secrets.includes('private-mcp-value'));
    if (process.platform !== 'win32') assert.equal(statSync(mcp.file).mode & 0o777, 0o600);
    assert.equal(readFileSync(path, 'utf8'), config);
    assert.equal(claudeProjectMcp(source, source, { CLAUDE_CONFIG_DIR: profile }), null);
    assert.equal(claudeProjectMcp(join(profile, 'unknown'), worktree, { CLAUDE_CONFIG_DIR: profile }), null);
  } finally { mcp.cleanup(); }
  assert.equal(existsSync(mcp.file), false);
});

test('Claude receives local MCP definitions through a private file, redacts them and cleans up on exit or spawn error', t => {
  const profile = mkdtempSync(join(tmpdir(), 'atelier-mcp-launch-'));
  t.after(() => rmSync(profile, { recursive: true, force: true }));
  const sourceRepo = join(profile, 'source'), repo = join(profile, 'worktree');
  writeFileSync(join(profile, '.claude.json'), JSON.stringify({ projects: { [sourceRepo]: { mcpServers: {
    personal: { type: 'http', url: 'https://mcp.invalid', headers: { Authorization: 'Bearer private-mcp-key' } },
  } } } }));
  const options = { agent: 'claude', mode: 'edit', repo, sourceRepo, prompt: 'Test', provider: { type: 'claude', command: process.execPath, configDir: profile } };
  const child = new EventEmitter(); Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough() });
  const lines = []; let file, exit;
  startAgent(options, (_, line) => lines.push(line), code => { exit = code; }, (_, args, spawnOptions) => {
    file = args[args.indexOf('--mcp-config') + 1];
    assert.ok(existsSync(file)); assert.equal(spawnOptions.cwd, repo);
    assert.ok(!args.join(' ').includes('private-mcp-key'));
    return child;
  });
  child.stderr.write('MCP failed: Bearer private-mcp-key\nToken: private-mcp-key\n'); child.emit('close', 1);
  assert.equal(exit, 1); assert.equal(existsSync(file), false);
  assert.equal(lines[0], 'MCP failed: [secret]');
  assert.equal(lines[1], 'Token: [secret]');
  assert.throws(() => startAgent(options, () => {}, () => {}, (_, args) => {
    file = args[args.indexOf('--mcp-config') + 1]; throw new Error('spawn failed');
  }), /spawn failed/);
  assert.equal(existsSync(file), false);
});

test('missing and invalid Claude profiles fail without exposing their contents', t => {
  const profile = mkdtempSync(join(tmpdir(), 'atelier-mcp-invalid-'));
  t.after(() => rmSync(profile, { recursive: true, force: true }));
  const read = () => claudeProjectMcp(join(profile, 'source'), join(profile, 'task'), { HOME: profile });
  assert.equal(read(), null);
  writeFileSync(join(profile, '.claude.json'), 'private-invalid-json');
  assert.throws(read, error => /illisible/.test(error.message) && !error.message.includes('private-invalid-json'));
  writeFileSync(join(profile, '.claude.json'), ' '.repeat(4 * 1024 * 1024 + 1));
  assert.throws(read, /volumineux/);
});
