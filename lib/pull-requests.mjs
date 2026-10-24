import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, lstatSync, readlinkSync, openSync, closeSync, fstatSync, constants } from 'node:fs';
import { join } from 'node:path';

export function runCommand(command, args, cwd, input) {
  return new Promise((resolve, reject) => {
    // codeql[js/command-line-injection] Remote review refs start with refs/heads/, pass Git validation, and remain separate arguments with no shell.
    const child = execFile(command, args, { cwd, windowsHide: true, shell: false, timeout: 120000, maxBuffer: 8 * 1024 * 1024, env: { ...process.env, GIT_TERMINAL_PROMPT: '0' } }, (error, stdout, stderr) => {
      if (error) reject(new Error(`${command} : ${stderr?.trim() || error.message}`));
      else resolve(stdout.replace(/\r?\n$/, ''));
    });
    child.stdin.on('error', () => {});
    child.stdin.end(input || '');
  });
}

export function azureRepository(remote) {
  let parts;
  const ssh = /^(?:git@ssh\.dev\.azure\.com:v3\/|ssh:\/\/git@ssh\.dev\.azure\.com\/v3\/)([^/]+)\/([^/]+)\/([^/]+)$/.exec(remote);
  if (ssh) parts = ssh.slice(1);
  else {
    let url;
    try { url = new URL(remote); } catch { throw new Error('Remote Azure Repos HTTPS ou SSH attendu.'); }
    if (url.protocol !== 'https:' || url.password || url.search || url.hash || url.port) throw new Error('Remote Azure Repos invalide.');
    const segments = url.pathname.replace(/\/$/, '').split('/').slice(1);
    if (url.hostname === 'dev.azure.com' && segments.length === 4 && segments[2] === '_git') parts = [segments[0], segments[1], segments[3]];
    const legacy = /^([\w-]+)\.visualstudio\.com$/.exec(url.hostname);
    if (legacy && segments.length === 3 && segments[1] === '_git') parts = [legacy[1], segments[0], segments[2]];
  }
  if (!parts) throw new Error('Cette version prend en charge Azure DevOps Services (dev.azure.com).');
  parts = parts.map(part => decodeURIComponent(part));
  if (parts.some(part => !part || part === '.' || part === '..' || /[\/\\\x00-\x1f]/.test(part))) throw new Error('Remote Azure Repos invalide.');
  const [org, project, repo] = parts.map(encodeURIComponent);
  return { name: parts.join('/'), api: `https://dev.azure.com/${org}/${project}/_apis/git/repositories/${repo}`, web: `https://dev.azure.com/${org}/${project}/_git/${repo}` };
}

async function fetchAzure(url, options = {}) {
  try {
    return await fetch(url, { ...options, redirect: 'manual', signal: AbortSignal.timeout(20000), headers: { Accept: 'application/json', 'X-TFS-FedAuthRedirect': 'Suppress', ...options.headers } });
  } catch (error) {
    throw new Error(`Azure DevOps : connexion réseau impossible (${error.cause?.code || error.name}). Vérifie le réseau, le proxy et les certificats du serveur.`);
  }
}

export async function azureRequest(repository, token, path = '', body, run = runCommand, method = body ? 'POST' : 'GET') {
  let authorization;
  if (token) authorization = `Basic ${Buffer.from(':' + token).toString('base64')}`;
  else {
    const discovery = await fetchAzure(new URL(repository.api + '?api-version=7.1'));
    const tenant = discovery.headers.get('x-vss-resourcetenant');
    await discovery.body?.cancel();
    if (tenant && !/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i.test(tenant)) throw new Error('Azure DevOps : identifiant de tenant invalide.');
    const args = ['account', 'get-access-token', '--resource', '499b84ac-1321-427f-aa17-267ca6975798', '--query', 'accessToken', '--output', 'tsv'];
    if (tenant) args.push('--tenant', tenant);
    try {
      const accessToken = (await (process.platform === 'win32'
        ? run('cmd.exe', ['/d', '/s', '/c', 'az ' + args.join(' ')])
        : run('az', args))).trim();
      if (!accessToken) throw new Error('Jeton vide.');
      authorization = `Bearer ${accessToken}`;
    } catch {
      throw new Error(`Connexion Azure CLI indisponible. Vérifie que Azure CLI est installé et exécute az login${tenant ? ' --tenant ' + tenant : ''}, ou renseigne un PAT Azure DevOps dans les réglages.`);
    }
  }
  const url = new URL(repository.api + path);
  url.searchParams.set('api-version', '7.1');
  const response = await fetchAzure(url, { method, headers: { Authorization: authorization, 'Content-Type': 'application/json' }, body: body ? JSON.stringify(body) : undefined });
  if (!response.ok) throw new Error(`Azure DevOps : HTTP ${response.status}. Vérifie l’accès du compte à cette organisation et les droits Code (lecture et écriture).`);
  return response.json();
}

function context(card) {
  if (!card.worktree) throw new Error('Prépare le worktree avant de créer une PR.');
  if (card.branch !== card.worktree.branch) throw new Error('La branche de la carte ne correspond pas au worktree.');
  return card.worktree.path;
}

export function savedPullRequestRepository(saved, parse, suffix) {
  if (!saved || !Number.isSafeInteger(saved.number) || saved.number < 1) throw new Error('PR enregistrée invalide.');
  const ending = suffix + saved.number;
  if (typeof saved.url !== 'string' || !saved.url.endsWith(ending)) throw new Error('Adresse de PR invalide.');
  const repo = parse(saved.url.slice(0, -ending.length));
  if (repo.name !== saved.repository) throw new Error('Le dépôt de la PR ne correspond pas à la carte.');
  return repo;
}

export async function refreshPullRequest(card, token, request = azureRequest) {
  const saved = card.pullRequest;
  const repo = savedPullRequestRepository(saved, azureRepository, '/pullrequest/');
  const pr = await request(repo, token, `/pullrequests/${saved.number}`);
  if (pr.pullRequestId !== saved.number || pr.sourceRefName !== `refs/heads/${saved.branch || card.branch}` || pr.forkSource || !['active', 'completed', 'abandoned'].includes(pr.status)) throw new Error('La PR distante ne correspond pas à la carte.');
  const state = { status: pr.status, draft: Boolean(pr.isDraft), mergeStatus: pr.mergeStatus || '', title: pr.title || saved.title, checkedAt: new Date().toISOString(), sourceCommit: pr.lastMergeSourceCommit?.commitId || '', targetCommit: pr.lastMergeTargetCommit?.commitId || '', reviewers: (pr.reviewers || []).map(r => ({ name: r.displayName || 'Reviewer', vote: r.vote || 0 })), checks: [], checksError: '' };
  try {
    const [statuses, iterations] = await Promise.all([
      request(repo, token, `/pullrequests/${saved.number}/statuses`),
      request(repo, token, `/pullrequests/${saved.number}/iterations`),
    ]);
    if (!Array.isArray(statuses.value) || !Array.isArray(iterations.value)) throw new Error('Réponse des contrôles invalide.');
    const iteration = iterations.value.reduce((max, item) => Math.max(max, item.id || 0), 0);
    const latest = new Map();
    for (const item of statuses.value) {
      if (item.iterationId && item.iterationId !== iteration) continue;
      const key = JSON.stringify([item.context?.genre, item.context?.name]);
      if (!latest.has(key) || item.id > latest.get(key).id) latest.set(key, item);
    }
    state.checks = [...latest.values()].map(item => ({ name: item.context?.name || 'Contrôle', state: item.state || 'notSet', description: item.description || '' }));
  } catch (error) { state.checksError = error.message; }
  return state;
}

export async function previewPullRequest(card, token, run = runCommand, request = azureRequest, parseRepository = azureRepository) {
  const cwd = context(card);
  const git = args => run('git', args, cwd);
  const branch = await git(['branch', '--show-current']);
  if (branch !== card.branch) throw new Error('Le worktree a changé de branche. Publication refusée.');
  const remotes = (await git(['remote', 'get-url', '--push', '--all', 'origin'])).split('\n');
  if (remotes.length !== 1) throw new Error('origin doit avoir une seule destination de publication.');
  const repository = parseRepository(remotes[0]);
  const head = await git(['rev-parse', 'HEAD']);
  const status = await git(['status', '--short']);
  const diff = await git(['diff', '--no-ext-diff', '--no-textconv', '--binary', 'HEAD', '--']);
  const untracked = (await git(['ls-files', '--others', '--exclude-standard', '-z'])).split('\0').filter(Boolean);
  const fingerprint = createHash('sha256').update(JSON.stringify({ branch, head, status, diff, repository }));
  const previews = [];
  for (const file of untracked) {
    const path = join(cwd, file), stat = lstatSync(path, { bigint: true });
    if (stat.size > 20 * 1024 * 1024) throw new Error(`Fichier non suivi trop volumineux : ${file}. Ajoute-le à .gitignore ou traite-le séparément.`);
    let content;
    if (stat.isSymbolicLink()) content = Buffer.from(readlinkSync(path));
    else {
      // codeql[js/file-system-race] The opened descriptor's identity and size are rechecked before any content is read.
      const fd = openSync(path, constants.O_RDONLY | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0));
      try {
        const opened = fstatSync(fd, { bigint: true });
        if (opened.dev !== stat.dev || opened.ino !== stat.ino || !opened.isFile() || opened.size > 20 * 1024 * 1024) throw new Error(`Fichier modifié pendant sa lecture : ${file}. Recharge l’aperçu.`);
        content = readFileSync(fd);
      } finally { closeSync(fd); }
    }
    fingerprint.update(file).update(content);
    previews.push(`\nNouveau fichier : ${file}\n${content.includes(0) ? '[Fichier binaire]' : content.toString('utf8').slice(0, 8000)}`);
  }
  const info = await request(repository, token);
  const configuredBase = card.worktree.base === 'HEAD' ? '' : card.worktree.base?.replace(/^origin\//, '');
  const base = configuredBase || info.defaultBranch?.replace(/^refs\/heads\//, '') || '';
  const title = `${card.ticket ? `${card.ticket.key} — ` : ''}${card.title}`;
  const body = `${card.ticket ? `Ticket : [${card.ticket.key}](${card.ticket.url})\n\n` : ''}## Changements\n${card.description || card.title}\n\n## Vérification\nÀ compléter avec les tests effectués.`;
  const commits = await git(['log', '--oneline', `${card.worktree.baseCommit}..HEAD`, '--']);
  return { repository: repository.name, branch, base, title, body, status, commits, dirty: Boolean(status), fingerprint: fingerprint.digest('hex'), diff: (diff + previews.join('\n')).slice(0, 60000) };
}

export async function publishPullRequest(card, options, token, run = runCommand, request = azureRequest) {
  const cwd = context(card);
  const preview = await previewPullRequest(card, token, run, request);
  if (!options.fingerprint || options.fingerprint !== preview.fingerprint) throw new Error('Les changements ont évolué. Recharge l’aperçu avant de publier.');
  const { base, title, body } = options;
  if (typeof base !== 'string' || !base || base.startsWith('-') || base.length > 180 || base === card.branch) throw new Error('Choisis une branche cible différente de la branche de travail.');
  if (typeof title !== 'string' || !title.trim() || title.length > 300 || typeof body !== 'string' || body.length > 20000) throw new Error('Titre ou description de PR invalide.');
  if (card.ticket && (!title.includes(card.ticket.key) || !body.includes(card.ticket.url))) throw new Error('Conserve l’ID Jira dans le titre et le lien du ticket dans la description.');
  await run('git', ['check-ref-format', '--branch', base], cwd);
  const repo = azureRepository(await run('git', ['remote', 'get-url', '--push', 'origin'], cwd));
  if (card.pullRequest && savedPullRequestRepository(card.pullRequest, azureRepository, '/pullrequest/').web !== repo.web) throw new Error('Le dépôt origin ne correspond plus à la PR.');
  const branches = await request(repo, token, `/refs?filter=${encodeURIComponent('heads/' + base)}`);
  if (!branches.value?.some(ref => ref.name === `refs/heads/${base}`)) throw new Error('Branche cible Azure Repos introuvable.');
  const find = async () => {
    const query = new URLSearchParams({ 'searchCriteria.status': 'active', 'searchCriteria.sourceRefName': `refs/heads/${card.branch}`, 'searchCriteria.targetRefName': `refs/heads/${base}` });
    const result = await request(repo, token, '/pullrequests?' + query);
    return (result.value || []).filter(pr => pr.sourceRefName === `refs/heads/${card.branch}` && pr.targetRefName === `refs/heads/${base}`);
  };
  const existing = await find();
  if (existing.length > 1) throw new Error('Plusieurs PR correspondent à cette branche. Vérifie Azure DevOps.');
  if (card.pullRequest && (!existing.length || existing[0].pullRequestId !== card.pullRequest.number)) throw new Error('La PR enregistrée n’est plus active sur cette branche cible.');
  if (options.draftOnly && existing.some(pr => !pr.isDraft || pr.autoCompleteSetBy)) throw new Error('La PR est sortie du brouillon ou sa fusion automatique est activée. Publication automatique refusée.');
  if (!existing.length && !preview.dirty && !preview.commits) throw new Error('Aucun changement à publier depuis la création du worktree.');
  if ((await previewPullRequest(card, token, run, request)).fingerprint !== preview.fingerprint) throw new Error('Les changements ont évolué pendant la préparation. Recharge l’aperçu.');
  if (preview.dirty) {
    if (options.commitChanges !== true) throw new Error('Valide le commit des changements affichés avant de publier.');
    await run('git', ['add', '-A', '--', '.'], cwd);
    await run('git', ['commit', '-F', '-'], cwd, title + '\n');
  }
  if (await run('git', ['status', '--porcelain'], cwd)) throw new Error('Des modifications restent après le commit. Vérifie le worktree avant de publier.');
  if (await run('git', ['branch', '--show-current'], cwd) !== card.branch) throw new Error('La branche a changé. Publication refusée.');
  if (azureRepository(await run('git', ['remote', 'get-url', '--push', 'origin'], cwd)).web !== repo.web) throw new Error('La destination Git a changé. Publication refusée.');
  await run('git', ['push', '--set-upstream', 'origin', `HEAD:refs/heads/${card.branch}`], cwd);
  let pr = existing[0];
  if (pr && options.draftOnly) pr = await request(repo, token, `/pullrequests/${pr.pullRequestId}`, { title, description: body }, run, 'PATCH');
  if (!pr) {
    try {
      pr = await request(repo, token, '/pullrequests', { sourceRefName: `refs/heads/${card.branch}`, targetRefName: `refs/heads/${base}`, title, description: body, isDraft: true });
    } catch (error) {
      // A response can be lost after Azure creates the PR. Recover before retrying.
      pr = (await find())[0];
      if (!pr) throw error;
    }
  }
  if (!Number.isSafeInteger(pr.pullRequestId) || pr.pullRequestId < 1) throw new Error('Réponse de PR inattendue. Vérifie Azure DevOps avant de réessayer.');
  return { url: `${repo.web}/pullrequest/${pr.pullRequestId}`, number: pr.pullRequestId, repository: repo.name, branch: card.branch, base, title: pr.title || title, draft: pr.isDraft ?? true, ticketLinked: false };
}
