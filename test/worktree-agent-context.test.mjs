import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, linkSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { prepareWorktree, removeWorktree } from '../lib/worktrees.mjs';

const git = (repo, ...args) => execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8', windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] }).trim();
const put = (root, name, content) => { const path = join(root, name); mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, content); };

test('task worktrees inherit local agent context, refresh untouched copies and preserve branch files', async t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-agent-context-')), source = join(root, 'source');
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(source);
  git(source, 'init'); git(source, 'config', 'user.email', 'test@example.invalid'); git(source, 'config', 'user.name', 'Test');
  put(source, 'CLAUDE.md', 'Tracked instructions');
  put(source, '.gitignore', '.claude/\n.mcp.json\n');
  git(source, 'add', '.'); git(source, 'commit', '-m', 'initial'); git(source, 'remote', 'add', 'origin', source);
  put(source, 'CLAUDE.md', 'Uncommitted source edit');
  const local = {
    'AGENTS.md': 'Local agent instructions',
    'CLAUDE.local.md': 'Personal project instructions',
    '.claude/settings.json': '{"enabledPlugins":{"test@example":true}}',
    '.claude/settings.local.json': '{"permissions":{"deny":["Bash(git push *)"]}}',
    '.claude/skills/ponytail/SKILL.md': 'Local skill',
    '.claude/commands/check.md': 'Local command',
    '.claude/agents/reviewer.md': 'Local agent',
    '.claude/rules/testing.md': 'Local rule',
    '.claude/hooks/check.mjs': 'Local hook',
    '.mcp.json': '{"mcpServers":{"test":{"command":"test-mcp","env":{"TOKEN":"private"}}}}',
  };
  for (const [name, content] of Object.entries(local)) put(source, name, content);
  put(source, '.claude/worktrees/other/private.txt', 'Do not copy other worktrees');
  put(source, '.env', 'Do not copy arbitrary secrets');
  const card = { id: 'test', title: 'Context', repo: source };
  card.worktree = await prepareWorktree(card, {});
  const path = card.worktree.path;
  assert.equal(readFileSync(join(path, 'CLAUDE.md'), 'utf8'), 'Tracked instructions');
  for (const [name, content] of Object.entries(local)) assert.equal(readFileSync(join(path, name), 'utf8'), content, name);
  assert.equal(existsSync(join(path, '.env')), false);
  assert.equal(existsSync(join(path, '.claude/worktrees')), false);
  assert.equal(git(path, 'status', '--porcelain'), '');
  put(source, '.claude/settings.local.json', '{"updated":true}');
  put(path, '.claude/rules/testing.md', 'Task-specific rule');
  put(source, '.claude/rules/testing.md', 'Changed source rule');
  unlinkSync(join(source, '.claude/commands/check.md'));
  await prepareWorktree(card, {});
  assert.equal(readFileSync(join(path, '.claude/settings.local.json'), 'utf8'), '{"updated":true}');
  assert.equal(readFileSync(join(path, '.claude/rules/testing.md'), 'utf8'), 'Task-specific rule');
  assert.equal(existsSync(join(path, '.claude/commands/check.md')), false);
  assert.equal(git(path, 'status', '--porcelain'), '');
  await removeWorktree(card);
  assert.equal(readFileSync(join(source, 'CLAUDE.md'), 'utf8'), 'Uncommitted source edit');
  assert.ok(existsSync(join(source, '.mcp.json')));
});

test('agent context refuses redirected worktree folders before copying private configuration', async t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-context-link-')), source = join(root, 'source'), outside = join(root, 'outside');
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(source); mkdirSync(outside);
  git(source, 'init'); git(source, '-c', 'user.email=test@example.invalid', '-c', 'user.name=Test', 'commit', '--allow-empty', '-m', 'initial');
  git(source, 'remote', 'add', 'origin', source);
  const card = { id: 'link', title: 'Context', repo: source };
  card.worktree = await prepareWorktree(card, {});
  symlinkSync(outside, join(card.worktree.path, '.claude'), process.platform === 'win32' ? 'junction' : 'dir');
  put(source, '.claude/settings.local.json', '{"private":true}');
  await assert.rejects(prepareWorktree(card, {}), /hors du worktree/);
  assert.equal(existsSync(join(outside, 'settings.local.json')), false);
  unlinkSync(join(card.worktree.path, '.claude'));
  await prepareWorktree(card, {});
  const destination = join(card.worktree.path, '.claude/settings.local.json');
  unlinkSync(destination);
  put(outside, 'settings.local.json', '{"private":true}');
  linkSync(join(outside, 'settings.local.json'), destination);
  put(source, '.claude/settings.local.json', '{"updated":true}');
  await assert.rejects(prepareWorktree(card, {}), /lien ou un dossier/);
  assert.equal(readFileSync(join(outside, 'settings.local.json'), 'utf8'), '{"private":true}');
  unlinkSync(destination);
  await removeWorktree(card);
});
