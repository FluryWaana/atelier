function normalize(mr, repo) {
  if (!Number.isSafeInteger(mr.source_project_id) || mr.source_project_id !== mr.target_project_id) throw new Error('Les MR depuis un fork ne sont pas prises en charge.');
  return { number: mr.iid, repository: repo.name, branch: mr.source_branch, base: mr.target_branch, source: mr.sha || mr.diff_refs?.head_sha, target: mr.diff_refs?.start_sha, title: mr.title, draft: Boolean(mr.draft || mr.work_in_progress || /^(Draft|WIP):/i.test(mr.title || '')), autoMerge: Boolean(mr.merge_when_pipeline_succeeds || mr.auto_merge_enabled), status: ({ opened: 'active', merged: 'completed', closed: 'abandoned' })[mr.state] || '', mergeStatus: mr.detailed_merge_status === 'conflict' ? 'conflicts' : mr.detailed_merge_status || mr.merge_status || '' };
}
const draftTitle = title => /^(Draft|WIP):/i.test(title) ? title : 'Draft: ' + title;

export default {
  id: 'gitlab', suffix: '/-/merge_requests/',
  async branch({ request }, repo, base) { await request(repo, '/repository/branches/' + encodeURIComponent(base)); },
  async get({ request }, repo, number) {
    const mr = normalize(await request(repo, '/merge_requests/' + number), repo);
    if (mr.status === 'active') mr.target = (await request(repo, '/repository/branches/' + encodeURIComponent(mr.base))).commit?.id;
    return mr;
  },
  async find({ pages }, repo, branch, base) {
    const query = new URLSearchParams({ state: 'opened', source_branch: branch, target_branch: base, scope: 'all' });
    return (await pages(repo, '/merge_requests?' + query)).map(mr => normalize(mr, repo));
  },
  async create({ request }, repo, { branch, base, title, body }) {
    return normalize(await request(repo, '/merge_requests', { source_branch: branch, target_branch: base, title: draftTitle(title), description: body, remove_source_branch: false }), repo);
  },
  async update({ request }, repo, number, { title, body }) {
    return normalize(await request(repo, '/merge_requests/' + number, { title: draftTitle(title), description: body }, 'PUT'), repo);
  },
  async checks({ pages, request }, repo, pr) {
    const pipelines = await pages(repo, `/merge_requests/${pr.number}/pipelines`);
    const latest = pipelines.filter(pipeline => pipeline.sha === pr.source).sort((a, b) => b.id - a.id)[0];
    const checks = latest ? [{ name: `Pipeline #${latest.id}`, state: ({ success: 'succeeded', failed: 'failed', canceled: 'failed', skipped: 'notApplicable', manual: 'pending', scheduled: 'pending', created: 'pending', waiting_for_resource: 'pending', preparing: 'pending', pending: 'pending', running: 'pending' })[latest.status] || 'notSet', description: latest.ref || '' }] : [];
    const approvals = await request(repo, `/merge_requests/${pr.number}/approvals`);
    return { checks, reviewers: (approvals.approved_by || []).map(item => ({ name: item.user?.name || item.user?.username || 'Reviewer', vote: 10 })) };
  },
};
