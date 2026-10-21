import { previewPullRequest, runCommand, savedPullRequestRepository } from './pull-requests.mjs';

export function forgeUrl(value, kind) {
  const url = new URL(value || (kind === 'github' ? 'https://github.com' : 'https://gitlab.com'));
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash || (kind === 'github' && url.pathname !== '/')) throw new Error('Adresse HTTPS de la forge attendue, sans identifiants ni paramètres.');
  return url.href.replace(/\/+$/, '');
}

export function forgeRepository(remote, kind, instance) {
  const base = new URL(forgeUrl(instance, kind));
  let url;
  const ssh = /^git@([^/:]+):(.+)$/.exec(remote);
  try { url = new URL(ssh ? `ssh://git@${ssh[1]}/${ssh[2]}` : remote); }
  catch { throw new Error('Remote Git HTTPS ou SSH attendu.'); }
  if (!['https:', 'ssh:'].includes(url.protocol) || url.password || url.search || url.hash || url.hostname !== base.hostname || (url.protocol === 'https:' && url.port !== base.port) || (url.protocol === 'ssh:' && url.username !== 'git')) throw new Error('Le remote ne correspond pas à la forge configurée.');
  const prefix = url.protocol === 'ssh:' ? '/' : base.pathname.replace(/\/$/, '') + '/';
  if (!url.pathname.startsWith(prefix)) throw new Error('Le remote ne correspond pas au chemin de la forge.');
  const parts = url.pathname.slice(prefix.length).replace(/\/$/, '').replace(/\.git$/, '').split('/').map(decodeURIComponent);
  if (parts.length < 2 || (kind === 'github' && parts.length !== 2) || parts.some(part => !part || part === '.' || part === '..' || /[\\/\s?#\x00-\x1f]/.test(part))) throw new Error('Chemin du dépôt invalide.');
  const name = parts.join('/'), web = base.href.replace(/\/$/, '') + '/' + parts.map(encodeURIComponent).join('/');
  const apiRoot = kind === 'github' ? (base.origin === 'https://github.com' ? 'https://api.github.com' : base.origin + '/api/v3') : base.href.replace(/\/$/, '') + '/api/v4';
  return { name, web, apiRoot, api: apiRoot + (kind === 'github' ? '/repos/' + parts.map(encodeURIComponent).join('/') : '/projects/' + encodeURIComponent(name)) };
}

export function forgeClient(kind, context, { run = runCommand, fetcher = fetch } = {}) {
  const instance = forgeUrl(context.settings.url, kind), host = new URL(instance).host;
  let credential;
  async function token() {
    if (credential) return credential;
    credential = context.secret('token');
    if (!credential) {
      const cli = kind === 'github' ? 'gh' : 'glab';
      try { credential = (await run(cli, ['auth', 'token', '--hostname', host])).trim(); }
      catch { throw new Error(`Session ${cli} indisponible. Exécutez ${cli} auth login --hostname ${host}, ou renseignez un jeton dans les réglages.`); }
    }
    if (!credential || /[\r\n]/.test(credential)) throw new Error('Jeton de connexion invalide.');
    return credential;
  }
  const parse = remote => forgeRepository(remote, kind, instance);
  async function request(repo, path = '', body, method = body === undefined ? 'GET' : 'POST') {
    // Only repositories parsed against the configured instance may receive credentials.
    const expected = parse(repo.web);
    if (expected.api !== repo.api) throw new Error('Destination API invalide.');
    const url = new URL(repo.api + path);
    if (url.origin !== new URL(repo.api).origin || !url.pathname.startsWith(new URL(repo.api).pathname)) throw new Error('Destination API invalide.');
    const secret = await token();
    let response;
    try { response = await fetcher(url, { method, redirect: 'manual', signal: AbortSignal.timeout(20000), headers: { Accept: 'application/json', 'Content-Type': 'application/json', Authorization: `Bearer ${secret}`, ...(kind === 'github' ? { 'X-GitHub-Api-Version': '2022-11-28' } : {}) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) }); }
    catch { throw new Error(`${kind} : connexion réseau impossible. Vérifiez le réseau et les certificats.`); }
    if (!response.ok) throw new Error(`${kind} : HTTP ${response.status}. Vérifiez la session et les droits du compte sur le dépôt.`);
    return response.json();
  }
  async function pages(repo, path, key) {
    const result = [];
    for (let page = 1; page <= 100; page++) {
      const data = await request(repo, path + (path.includes('?') ? '&' : '?') + `per_page=100&page=${page}`);
      const items = key ? data[key] : data;
      if (!Array.isArray(items)) throw new Error('Réponse paginée invalide.');
      result.push(...items);
      if (items.length < 100) return result;
    }
    throw new Error('Trop de résultats distants pour garantir un relevé complet.');
  }
  return { parse, request, pages, run };
}

export function createForgeRepositories(adapter, context, dependencies) {
  const { parse, request, pages, run } = forgeClient(adapter.id, context, dependencies);
  const api = { request, pages };
  const preview = card => previewPullRequest(card, '', run, async repo => ({ defaultBranch: (await request(repo)).default_branch }), parse);
  const git = (card, args, input) => run('git', args, card.worktree.path, input);
  const savedRepository = card => savedPullRequestRepository(card.pullRequest, parse, adapter.suffix);
  function validate(pr, repo, branch, base, number) {
    if (!Number.isSafeInteger(pr.number) || pr.number < 1 || (number && pr.number !== number) || pr.branch !== branch || (base && pr.base !== base) || pr.repository !== repo.name || !['active', 'completed', 'abandoned'].includes(pr.status)) throw new Error('La PR distante ne correspond pas à la carte.');
    return pr;
  }
  async function get(card, repo) {
    const saved = card.pullRequest;
    return validate(await adapter.get(api, repo, saved.number), repo, saved.branch || card.branch, saved.base, saved.number);
  }
  async function publish(card, options) {
    const shown = await preview(card);
    if (!options.fingerprint || options.fingerprint !== shown.fingerprint) throw new Error('Les changements ont évolué. Rechargez l’aperçu avant de publier.');
    const { base, title, body } = options;
    if (typeof base !== 'string' || !base || base.startsWith('-') || base.length > 180 || base === card.branch) throw new Error('Choisissez une branche cible différente de la branche de travail.');
    if (typeof title !== 'string' || !title.trim() || title.length > 300 || typeof body !== 'string' || body.length > 20000) throw new Error('Titre ou description de PR invalide.');
    if (card.ticket && (!title.includes(card.ticket.key) || !body.includes(card.ticket.url))) throw new Error('Conservez l’ID du ticket dans le titre et son lien dans la description.');
    await git(card, ['check-ref-format', '--branch', base]);
    const repo = parse(await git(card, ['remote', 'get-url', '--push', 'origin']));
    if (card.pullRequest && savedRepository(card).web !== repo.web) throw new Error('Le dépôt origin ne correspond plus à la PR.');
    await adapter.branch(api, repo, base);
    const find = async () => (await adapter.find(api, repo, card.branch, base)).map(pr => validate(pr, repo, card.branch, base));
    const existing = await find();
    if (existing.length > 1) throw new Error('Plusieurs PR correspondent à cette branche. Vérifiez la forge.');
    if (card.pullRequest && (!existing.length || existing[0].number !== card.pullRequest.number)) throw new Error('La PR enregistrée n’est plus active sur cette branche cible.');
    if (options.draftOnly && existing.some(pr => !pr.draft || pr.autoMerge)) throw new Error('La PR est sortie du brouillon ou sa fusion automatique est activée. Publication automatique refusée.');
    if (!existing.length && !shown.dirty && !shown.commits) throw new Error('Aucun changement à publier depuis la création du worktree.');
    if ((await preview(card)).fingerprint !== shown.fingerprint) throw new Error('Les changements ont évolué pendant la préparation. Rechargez l’aperçu.');
    if (shown.dirty) {
      if (options.commitChanges !== true) throw new Error('Validez le commit des changements affichés avant de publier.');
      await git(card, ['add', '-A', '--', '.']);
      await git(card, ['commit', '-F', '-'], title + '\n');
    }
    if (await git(card, ['status', '--porcelain'])) throw new Error('Des modifications restent après le commit. Vérifiez le worktree avant de publier.');
    if (await git(card, ['branch', '--show-current']) !== card.branch) throw new Error('La branche a changé. Publication refusée.');
    if (parse(await git(card, ['remote', 'get-url', '--push', 'origin'])).web !== repo.web) throw new Error('La destination Git a changé. Publication refusée.');
    await git(card, ['push', '--set-upstream', 'origin', `HEAD:refs/heads/${card.branch}`]);
    let pr = existing[0];
    if (pr && options.draftOnly) pr = await adapter.update(api, repo, pr.number, { title, body });
    if (!pr) {
      try { pr = await adapter.create(api, repo, { branch: card.branch, base, title, body }); }
      catch (error) {
        // Recover a creation whose response was lost before allowing another attempt.
        const recovered = await find();
        if (recovered.length !== 1) throw error;
        pr = recovered[0];
      }
    }
    validate(pr, repo, card.branch, base);
    return { url: repo.web + adapter.suffix + pr.number, number: pr.number, repository: repo.name, branch: card.branch, base, title: pr.title || title, draft: pr.draft, ticketLinked: false };
  }
  async function refresh(card) {
    const repo = savedRepository(card), pr = await get(card, repo);
    const state = { status: pr.status, draft: pr.draft, mergeStatus: pr.mergeStatus || '', title: pr.title, checkedAt: new Date().toISOString(), sourceCommit: pr.source || '', targetCommit: pr.target || '', reviewers: [], checks: [], checksError: '' };
    try { Object.assign(state, await adapter.checks(api, repo, pr)); }
    catch (error) { state.checksError = error.message; }
    return state;
  }
  async function review(card) {
    if (!card.worktree) throw new Error('Une PR et son worktree sont nécessaires.');
    const repo = savedRepository(card);
    if (parse(await git(card, ['remote', 'get-url', 'origin'])).web !== repo.web) throw new Error('Le dépôt origin ne correspond plus à la PR.');
    const pr = await get(card, repo), { source, target } = pr;
    if (pr.status !== 'active' || pr.branch !== card.branch) throw new Error('Review disponible pour une PR active de cette branche.');
    const pending = message => Object.assign(new Error(message), { retryable: true });
    if (![source, target].every(sha => typeof sha === 'string' && /^[a-f0-9]{40}$/i.test(sha))) throw pending('Commits distants indisponibles. Actualisez la PR puis réessayez.');
    await git(card, ['check-ref-format', 'refs/heads/' + pr.base]);
    const check = async () => {
      if (await git(card, ['branch', '--show-current']) !== card.branch || await git(card, ['status', '--porcelain'])) throw new Error('Publiez les changements et gardez le worktree propre avant la review.');
      if (await git(card, ['rev-parse', 'HEAD']) !== source) throw pending('La forge ne référence pas le commit du worktree. Vérifiez la publication et réessayez.');
    };
    await check();
    await git(card, ['fetch', '--no-tags', 'origin', 'refs/heads/' + pr.base]);
    if (await git(card, ['rev-parse', 'FETCH_HEAD']) !== target) throw pending('La cible a évolué. Actualisez la PR puis réessayez.');
    const base = await git(card, ['merge-base', source, target]);
    await check();
    return { source, target, base, number: pr.number, url: card.pullRequest.url };
  }
  async function check(input = {}) {
    const directory = input.repo || context.config?.().repo;
    if (!directory) throw new Error('Choisissez un dépôt local pour vérifier la connexion.');
    const repo = parse(await run('git', ['remote', 'get-url', 'origin'], directory));
    await request(repo);
    return `Accès confirmé à ${repo.name}. Le push utilise les identifiants Git du système.`;
  }
  return { preview, publish, refresh, review, check };
}
