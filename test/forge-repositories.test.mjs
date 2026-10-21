import test from 'node:test';
import assert from 'node:assert/strict';
import { createForgeRepositories, forgeClient, forgeRepository, forgeUrl } from '../lib/forge-repositories.mjs';
import github from '../lib/forges/github.mjs';
import gitlab from '../lib/forges/gitlab.mjs';
import githubModule from '../lib/connectors/github.mjs';
import gitlabModule from '../lib/connectors/gitlab.mjs';
import { validateConnector } from '../lib/domain/connectors.mjs';

const source = 'a'.repeat(40), target = 'b'.repeat(40), ancestor = 'c'.repeat(40);
function fixture(adapter, options = {}) {
  const kind = adapter.id, host = kind + '.com', name = 'team/repo';
  const card = { id: 'card', title: 'Corriger le bouton', branch: 'task/fix', worktree: { path: '/worktree', branch: 'task/fix', base: 'origin/main', baseCommit: ancestor } };
  const calls = [], requests = [];
  let created = false, dirty = Boolean(options.dirty), previewCount = 0;
  const remotePR = () => kind === 'github'
    ? { number: 7, title: 'Corriger le bouton', state: 'open', draft: !options.ready, auto_merge: options.autoMerge ? {} : null, head: { ref: 'task/fix', sha: source, repo: { full_name: options.fork ? 'other/repo' : name } }, base: { ref: 'main', sha: target, repo: { full_name: name } }, ...options.pr }
    : { iid: 7, title: 'Draft: Corriger le bouton', state: 'opened', draft: !options.ready, source_project_id: options.fork ? 2 : 1, target_project_id: 1, source_branch: 'task/fix', target_branch: 'main', sha: source, diff_refs: { head_sha: source, start_sha: target }, merge_when_pipeline_succeeds: options.autoMerge || false, ...options.pr };
  async function run(command, args, cwd, input) {
    calls.push({ command, args, cwd, input });
    assert.equal(command, 'git');
    if (args[0] === 'branch') return options.wrongBranch ? 'another' : card.branch;
    if (args[0] === 'remote') return `git@${host}:${name}.git`;
    if (args[0] === 'rev-parse') return args[1] === 'FETCH_HEAD' ? (options.movedTarget ? ancestor : target) : options.wrongHead ? ancestor : source;
    if (args[0] === 'status') { if (args[1] === '--short') previewCount++; return dirty || (options.changed && previewCount > 1) ? ' M tracked.txt' : ''; }
    if (args[0] === 'diff') return dirty ? 'diff contents' : '';
    if (args[0] === 'ls-files') return '';
    if (args[0] === 'log') return 'a123 Change';
    if (args[0] === 'commit') dirty = false;
    if (args[0] === 'merge-base') return ancestor;
    return '';
  }
  async function fetcher(url, init) {
    requests.push({ url: String(url), ...init });
    assert.equal(init.headers.Authorization, 'Bearer fake-secret');
    assert.equal(init.redirect, 'manual');
    const path = decodeURIComponent(url.pathname), pullPath = kind === 'github' ? '/pulls' : '/merge_requests';
    let result;
    if (path.endsWith(pullPath) && init.method === 'POST') {
      created = true;
      const body = JSON.parse(init.body);
      if (kind === 'github') assert.equal(body.draft, true);
      else assert.match(body.title, /^Draft:/);
      if (options.lostResponse) throw new Error('lost');
      result = remotePR();
    } else if (path.endsWith(pullPath)) result = options.existing || created ? [remotePR()] : [];
    else if (path.endsWith(pullPath + '/7')) result = remotePR();
    else if (path.includes('/branches/')) result = { name: 'main', commit: { id: target } };
    else if (path.endsWith('/statuses')) result = [{ id: 3, context: 'CI', state: 'success' }, { id: 1, context: 'CI', state: 'failure' }];
    else if (path.endsWith('/check-runs')) result = { check_runs: [{ name: 'Tests', status: 'completed', conclusion: 'success' }] };
    else if (path.endsWith('/reviews')) result = [{ id: 1, user: { login: 'reviewer' }, state: 'CHANGES_REQUESTED' }, { id: 2, user: { login: 'reviewer' }, state: 'APPROVED' }];
    else if (path.endsWith('/pipelines')) result = [{ id: 4, sha: source, status: 'success' }, { id: 9, sha: ancestor, status: 'failed' }];
    else if (path.endsWith('/approvals')) result = { approved_by: [{ user: { name: 'reviewer' } }] };
    else result = { default_branch: 'main' };
    if (options.checksError && (path.endsWith('/statuses') || path.endsWith('/pipelines'))) return new Response('{}', { status: 403 });
    return Response.json(result);
  }
  const context = { settings: { url: `https://${host}` }, secret: () => 'fake-secret', config: () => ({ repo: '/repo' }) };
  const service = createForgeRepositories(adapter, context, { run, fetcher });
  const attach = () => { card.pullRequest = { number: 7, repository: name, url: `https://${host}/${name}${adapter.suffix}7`, branch: card.branch, base: 'main' }; };
  return { service, card, calls, requests, attach };
}

test('repository modules expose the shared contract and validate instance settings', () => {
  for (const module of [githubModule, gitlabModule]) {
    validateConnector(module);
    const settings = {}; module.validateSettings(settings);
    assert.equal(settings.url, `https://${module.id}.com`);
    assert.throws(() => module.validateSettings({ url: 'http://example.com' }), /HTTPS/);
  }
});

test('repository parsing binds HTTPS and SSH to the configured instance', () => {
  assert.equal(forgeRepository('git@github.com:team/repo.git', 'github').api, 'https://api.github.com/repos/team/repo');
  assert.equal(forgeRepository('ssh://git@git.example.com/group/sub/repo.git', 'gitlab', 'https://git.example.com').api, 'https://git.example.com/api/v4/projects/group%2Fsub%2Frepo');
  assert.equal(forgeRepository('https://enterprise.example/team/repo', 'github', 'https://enterprise.example').api, 'https://enterprise.example/api/v3/repos/team/repo');
  assert.equal(forgeRepository('https://git.example.com/git/group/repo', 'gitlab', 'https://git.example.com/git/').name, 'group/repo');
  assert.equal(forgeRepository('git@git.example.com:group/repo.git', 'gitlab', 'https://git.example.com/git/').web, 'https://git.example.com/git/group/repo');
  for (const remote of ['https://evil.example/team/repo', 'https://secret:token@github.com/team/repo', 'https://github.com/team/repo?token=x', 'file:///repo', 'git@github.com:team/a%2fb.git']) assert.throws(() => forgeRepository(remote, 'github'));
  assert.throws(() => forgeUrl('https://github.com/api/v3', 'github'));
});

for (const adapter of [github, gitlab]) {
  test(`${adapter.id}: creates a draft after the confirmed commit, pushes without a token argument`, async () => {
    const { service, card, calls, requests } = fixture(adapter, { dirty: true });
    const preview = await service.preview(card);
    const pr = await service.publish(card, { ...preview, commitChanges: true });
    assert.equal(pr.number, 7); assert.equal(pr.draft, true);
    assert.match(pr.url, adapter.id === 'github' ? /\/pull\/7$/ : /\/-\/merge_requests\/7$/);
    assert.ok(calls.find(call => call.args[0] === 'commit'));
    assert.deepEqual(calls.find(call => call.args[0] === 'push').args, ['push', '--set-upstream', 'origin', 'HEAD:refs/heads/task/fix']);
    assert.equal(JSON.stringify(calls).includes('fake-secret'), false);
    assert.equal(requests.filter(request => request.method === 'POST').length, 1);
  });
  test(`${adapter.id}: recovers a lost create response and reuses an existing PR`, async () => {
    for (const options of [{ lostResponse: true }, { existing: true }]) {
      const { service, card, requests } = fixture(adapter, options);
      assert.equal((await service.publish(card, await service.preview(card))).number, 7);
      assert.equal(requests.filter(request => request.method === 'POST').length, options.existing ? 0 : 1);
    }
  });
  test(`${adapter.id}: refuses changes without consent, changed previews and unsafe automatic publications`, async () => {
    for (const options of [{ dirty: true }, { changed: true }, { existing: true, ready: true, pr: adapter.id === 'gitlab' ? { title: 'Ready', draft: false } : {} }, { existing: true, autoMerge: true }, { fork: true, existing: true }]) {
      const { service, card, calls } = fixture(adapter, options);
      await assert.rejects(service.publish(card, { ...await service.preview(card), draftOnly: true }));
      assert.equal(calls.some(call => ['commit', 'push'].includes(call.args[0])), false);
    }
  });
  test(`${adapter.id}: refreshes status, current commit checks and reviews; reports incomplete checks`, async () => {
    const { service, card, attach } = fixture(adapter); attach();
    const state = await service.refresh(card);
    assert.equal(state.status, 'active'); assert.equal(state.sourceCommit, source);
    assert.ok(state.checks.length); assert.ok(state.checks.every(check => check.state === 'succeeded'));
    assert.deepEqual(state.reviewers, [{ name: 'reviewer', vote: 10 }]);
    const failed = fixture(adapter, { checksError: true }); failed.attach();
    assert.match((await failed.service.refresh(failed.card)).checksError, /403/);
    card.pullRequest.url = card.pullRequest.url.replace(adapter.id + '.com', 'evil.example');
    await assert.rejects(service.refresh(card), /forge/);
  });
  test(`${adapter.id}: maps merged, closed and conflicting PR states`, async () => {
    for (const [pr, expected] of adapter.id === 'github'
      ? [[{ state: 'closed', merged: true }, 'completed'], [{ state: 'closed' }, 'abandoned'], [{ mergeable: false }, 'active']]
      : [[{ state: 'merged' }, 'completed'], [{ state: 'closed' }, 'abandoned'], [{ detailed_merge_status: 'conflict' }, 'active']]) {
      const f = fixture(adapter, { pr }); f.attach();
      const state = await f.service.refresh(f.card);
      assert.equal(state.status, expected);
      if (expected === 'active') assert.equal(state.mergeStatus, 'conflicts');
    }
  });
  test(`${adapter.id}: reviews only a clean worktree at the exact remote commits`, async () => {
    const { service, card, attach } = fixture(adapter); attach();
    assert.deepEqual(await service.review(card), { source, target, base: ancestor, number: 7, url: card.pullRequest.url });
    for (const options of [{ dirty: true }, { wrongHead: true }, { wrongBranch: true }, { movedTarget: true }, { fork: true }]) {
      const f = fixture(adapter, options); f.attach();
      await assert.rejects(f.service.review(f.card));
    }
  });
  test(`${adapter.id}: explicit token wins, otherwise uses a host-specific CLI session without leaking errors`, async () => {
    const host = 'forge.example', repo = forgeRepository('https://' + host + '/team/repo', adapter.id, 'https://' + host);
    const calls = [], headers = [];
    const dependencies = { run: async (...args) => { calls.push(args); return 'cli-secret\n'; }, fetcher: async (_url, init) => { headers.push(init.headers); return Response.json({}); } };
    const context = { settings: { url: 'https://' + host }, secret: () => 'stored-secret' };
    await forgeClient(adapter.id, context, dependencies).request(repo);
    assert.equal(calls.length, 0); assert.equal(headers[0].Authorization, 'Bearer stored-secret');
    context.secret = () => '';
    await forgeClient(adapter.id, context, dependencies).request(repo);
    assert.deepEqual(calls, [[adapter.id === 'github' ? 'gh' : 'glab', ['auth', 'token', '--hostname', host]]]);
    assert.equal(headers[1].Authorization, 'Bearer cli-secret');
    await assert.rejects(forgeClient(adapter.id, context, { ...dependencies, run: async () => { throw Error('secret in CLI error'); } }).request(repo), error => /auth login/.test(error.message) && !error.message.includes('secret in CLI error'));
    await assert.rejects(forgeClient(adapter.id, context, { ...dependencies, fetcher: async () => new Response(null, { status: 302 }) }).request(repo), /HTTP 302/);
  });
}
