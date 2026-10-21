import { azureRepository, azureRequest, runCommand } from './pull-requests.mjs';
const pending = message => Object.assign(new Error(message), { retryable: true });

export { reviewRequirements } from './domain/tasks.mjs';

export async function prepareReview(card, token, run = runCommand, request = azureRequest) {
  if (!card.pullRequest || !card.worktree) throw new Error('Une PR et son worktree sont nécessaires.');
  const git = args => run('git', args, card.worktree.path);
  const repository = azureRepository(await git(['remote', 'get-url', 'origin']));
  if (repository.name !== card.pullRequest.repository) throw new Error('Le dépôt origin ne correspond plus à la PR.');
  const pr = await request(repository, token, `/pullrequests/${card.pullRequest.number}`);
  if (pr.status !== 'active' || pr.sourceRefName !== `refs/heads/${card.branch}` || pr.forkSource) throw new Error('Review disponible pour une PR active de cette branche, sans fork.');
  const source = pr.lastMergeSourceCommit?.commitId, target = pr.lastMergeTargetCommit?.commitId;
  if (![source, target].every(id => typeof id === 'string' && /^[a-f0-9]{40}$/i.test(id))) throw pending('Commits de PR indisponibles. Réessaie après son actualisation dans Azure.');
  if (!pr.targetRefName?.startsWith('refs/heads/')) throw new Error('Branche cible invalide.');
  await git(['check-ref-format', pr.targetRefName]);
  const check = async () => {
    if (await git(['branch', '--show-current']) !== card.branch || await git(['status', '--porcelain'])) throw new Error('Publie les changements et garde le worktree propre sur le commit de la PR avant la review.');
    if (await git(['rev-parse', 'HEAD']) !== source) throw pending('Azure ne référence pas encore le commit du worktree. Vérifie la publication et réessaie après actualisation de la PR.');
  };
  await check();
  await git(['fetch', '--no-tags', 'origin', pr.targetRefName]);
  if (await git(['rev-parse', 'FETCH_HEAD']) !== target) throw pending('La cible de la PR a évolué. Réessaie après son actualisation dans Azure.');
  const base = await git(['merge-base', source, target]);
  await check();
  return { source, target, base, number: card.pullRequest.number, url: card.pullRequest.url };
}

export { reviewPrompt, parseReview, correctionInstructions } from './domain/reviews.mjs';
