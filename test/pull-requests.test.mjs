import test from 'node:test';
import assert from 'node:assert/strict';
import { previewPullRequest, publishPullRequest, azureRequest } from '../lib/pull-requests.mjs';

function setup(existing = []) {
  const calls = [], commands = [];
  let content = 'diff';
  const card = { title: 'Titre', description: '', branch: 'feature/test', worktree: { path: 'repo', branch: 'feature/test', base: 'main', baseCommit: 'base' } };
  const run = async (command, args) => {
    commands.push(args);
    if (args[0] === 'remote') return 'https://dev.azure.com/org/project/_git/repo';
    if (args[0] === 'branch') return card.branch;
    if (args[0] === 'rev-parse') return 'head';
    if (args[0] === 'diff') return content;
    if (args[0] === 'log') return 'commit initial';
    return '';
  };
  const request = async (repo, token, path = '', body, commandRunner, method) => {
    calls.push({ path, body, method });
    if (!path) return { defaultBranch: 'refs/heads/main' };
    if (path.startsWith('/refs?')) return { value: [{ name: 'refs/heads/main' }] };
    if (path.startsWith('/pullrequests?')) return { value: existing.map(pr => ({ sourceRefName: 'refs/heads/feature/test', targetRefName: 'refs/heads/main', ...pr })) };
    return { pullRequestId: 1, title: body.title, isDraft: true };
  };
  const options = async () => ({ ...await previewPullRequest(card, '', run, request), base: 'main', title: 'Titre', body: 'Tests réussis', draftOnly: true });
  return { card, run, request, calls, commands, options, change: () => { content = 'changed'; } };
}
test('automatic publication creates a draft and updates an existing draft without duplication', async () => {
  for (const existing of [[], [{ pullRequestId: 1, isDraft: true }]]) {
    const f = setup(existing);
    const result = await publishPullRequest(f.card, await f.options(), '', f.run, f.request);
    assert.equal(result.number, 1); assert.equal(result.draft, true);
    const mutations = f.calls.filter(c => c.body);
    assert.equal(mutations.length, 1);
    assert.equal(mutations[0].method, existing.length ? 'PATCH' : undefined);
    assert.equal(mutations[0].body.description, 'Tests réussis');
    if (!existing.length) assert.equal(mutations[0].body.isDraft, true);
    assert.equal(f.commands.filter(args => args[0] === 'push').length, 1);
  }
});
test('automatic publication refuses a ready PR or enabled auto completion before push', async () => {
  for (const pr of [{ isDraft: false }, { isDraft: true, autoCompleteSetBy: { id: 'user' } }]) {
    const f = setup([{ pullRequestId: 1, ...pr }]);
    await assert.rejects(publishPullRequest(f.card, await f.options(), '', f.run, f.request), /Publication automatique refusée/);
    assert.equal(f.commands.some(args => args[0] === 'push'), false);
  }
});
test('publication refuses files changed since the verified preview', async () => {
  const f = setup(), options = await f.options(); f.change();
  await assert.rejects(publishPullRequest(f.card, options, '', f.run, f.request), /évolué/);
  assert.equal(f.commands.some(args => args[0] === 'push'), false);
});
test('Azure request uses PATCH when updating an existing PR', async t => {
  t.mock.method(globalThis, 'fetch', async (url, options) => {
    assert.equal(options.method, 'PATCH'); assert.deepEqual(JSON.parse(options.body), { description: 'Tests réussis' });
    return new Response(JSON.stringify({ pullRequestId: 1 }), { status: 200 });
  });
  const pr = await azureRequest({ api: 'https://dev.azure.com/org/project/_apis/git/repositories/repo' }, 'fake-pat', '/pullrequests/1', { description: 'Tests réussis' }, undefined, 'PATCH');
  assert.equal(pr.pullRequestId, 1);
});
