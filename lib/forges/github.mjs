function normalize(pr, repo) {
  if (pr.head?.repo?.full_name?.toLowerCase() !== repo.name.toLowerCase() || pr.base?.repo?.full_name?.toLowerCase() !== repo.name.toLowerCase()) throw new Error('Les PR depuis un fork ne sont pas prises en charge.');
  return { number: pr.number, repository: repo.name, branch: pr.head.ref, base: pr.base.ref, source: pr.head.sha, target: pr.base.sha, title: pr.title, draft: Boolean(pr.draft), autoMerge: Boolean(pr.auto_merge), status: pr.merged || pr.merged_at ? 'completed' : pr.state === 'open' ? 'active' : pr.state === 'closed' ? 'abandoned' : '', mergeStatus: pr.mergeable === false || pr.mergeable_state === 'dirty' ? 'conflicts' : pr.mergeable_state || '' };
}

export default {
  id: 'github', suffix: '/pull/',
  async branch({ request }, repo, base) { await request(repo, '/branches/' + encodeURIComponent(base)); },
  async get({ request }, repo, number) { return normalize(await request(repo, '/pulls/' + number), repo); },
  async find({ pages }, repo, branch, base) {
    const query = new URLSearchParams({ state: 'open', head: repo.name.split('/')[0] + ':' + branch, base });
    return (await pages(repo, '/pulls?' + query)).map(pr => normalize(pr, repo));
  },
  async create({ request }, repo, { branch, base, title, body }) {
    return normalize(await request(repo, '/pulls', { head: branch, base, title, body, draft: true }), repo);
  },
  async update({ request }, repo, number, { title, body }) {
    return normalize(await request(repo, '/pulls/' + number, { title, body }, 'PATCH'), repo);
  },
  async checks({ pages }, repo, pr) {
    if (!/^[a-f0-9]{40}$/i.test(pr.source)) throw new Error('Commit source invalide.');
    const [statuses, runs, reviews] = await Promise.all([
      pages(repo, `/commits/${pr.source}/statuses`),
      pages(repo, `/commits/${pr.source}/check-runs?filter=latest`, 'check_runs'),
      pages(repo, `/pulls/${pr.number}/reviews`),
    ]);
    const latest = new Map();
    for (const item of statuses) if (!latest.has(item.context) || item.id > latest.get(item.context).id) latest.set(item.context, item);
    const checks = [...latest.values()].map(item => ({ name: item.context || 'Statut', state: ({ success: 'succeeded', failure: 'failed', error: 'error', pending: 'pending' })[item.state] || 'notSet', description: item.description || '' }));
    checks.push(...runs.map(item => ({ name: item.name || 'Contrôle', state: item.status !== 'completed' ? 'pending' : ({ success: 'succeeded', neutral: 'notApplicable', skipped: 'notApplicable', failure: 'failed', cancelled: 'failed', timed_out: 'failed', action_required: 'failed', startup_failure: 'error', stale: 'error' })[item.conclusion] || 'notSet', description: item.output?.title || '' })));
    const reviewers = new Map();
    for (const item of reviews) {
      if (!['APPROVED', 'CHANGES_REQUESTED', 'DISMISSED'].includes(item.state) || !item.user?.login) continue;
      if (!reviewers.has(item.user.login) || item.id > reviewers.get(item.user.login).id) reviewers.set(item.user.login, item);
    }
    return { checks, reviewers: [...reviewers.values()].map(item => ({ name: item.user.login, vote: item.state === 'APPROVED' ? 10 : item.state === 'CHANGES_REQUESTED' ? -10 : 0 })) };
  },
};
