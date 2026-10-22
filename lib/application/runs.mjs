import { fail, text } from '../domain/validation.mjs';
import { reviewRequirements } from '../domain/tasks.mjs';
import { resolveAgentOptions, agentProvider } from '../domain/agent-options.mjs';

export function createRuns({ store, active, publishing, automationBusy, checkDependencies, configFor, repository, samePath, prepareWorktree, brainstormDirectory, newId, timestamp, launch, changed, invalidateReviews, brainstormPrompt, preparationPrompt, reviewPrompt, checklistText, parseBrainstorm, parsePreparation, parseReview, recordAgentUsage, eventProgress, eventText, assertCanStart = () => {} }) {
  let closing = false;
  const pending = new Set();
  async function startRun(card, body, reviewContext = null, correction = null, onFinished = null, preparing = false) {
    assertCanStart();
    const brainstorming = preparing === 'brainstorm';
    if (!preparing) checkDependencies(card);
    if (card.archived) fail('Restaure cette tâche avant de lancer un agent.', 'conflict');
    if (publishing.has(card.id) || [...active.values()].some(run => run.cardId === card.id) || !onFinished && automationBusy(card.id)) fail('Cette carte a déjà une exécution active.', 'conflict');
    const provider = agentProvider(configFor(card), body.agent);
    if (!['analyze', 'edit'].includes(body.mode)) fail('Agent ou mode invalide.');
    const instructions = text(body.instructions || '');
    const options = resolveAgentOptions(configFor(card), card, body);
    if (reviewContext) reviewContext = { ...reviewContext, requirements: reviewRequirements(card), verification: onFinished && card.automation?.step === 'review' ? card.automation.tests : null };
    const requirements = preparing ? reviewRequirements(card) : null;
    const brainstormHistory = brainstorming ? store.runs().filter(run => run.cardId === card.id && run.kind === 'brainstorm' && run.status !== 'running' && (run.instructions || run.status === 'completed' && run.summary)).reverse().map(run => ({ requirements: run.requirements, reply: run.instructions || '', message: run.brainstorm?.message || '', questions: run.brainstorm?.questions || [], proposals: run.proposals || [] })) : [];
    if (brainstorming && JSON.stringify(brainstormHistory).length > 200000) fail('Historique trop long. Crée un nouveau brainstorming en résumant les décisions à conserver.');
    let analysisRepo = card.worktree?.path || card.repo || configFor(card).repo;
    if (brainstorming && card.brainstormSession && !card.repo) { analysisRepo = brainstormDirectory(); }
    let worktree;
    publishing.add(card.id); changed();
    try { worktree = preparing ? { path: repository(analysisRepo), branch: card.branch || '' } : await prepareWorktree(card, configFor(card)); }
    finally { publishing.delete(card.id); changed(); }
    const repo = worktree.path;
    if (!preparing) { card.worktree = worktree; card.branch = worktree.branch; }
    store.putCard(card);
    if (closing) fail('Atelier est en cours d’arrêt.', 'conflict');
    body.signal?.throwIfAborted();
    if ([...active.values()].some(r => samePath(r.repo, repo))) fail('Un agent travaille déjà dans ce dossier.', 'conflict');

    const run = { id: newId(), cardId: card.id, agent: body.agent, ...options, mode: body.mode, repo, branch: worktree.branch, ticketKey: card.ticket?.key || null, status: 'running', startedAt: timestamp(), instructions, ...(preparing ? { kind: brainstorming ? 'brainstorm' : 'ticket-preparation', requirements } : {}), ...(reviewContext ? { kind: 'pr-review', reviewContext } : {}), ...(correction ? { correctionOf: correction.runId, correctedIndexes: correction.indexes } : {}), activity: 'Démarrage de l’agent', lastEventAt: timestamp(), liveText: '' };
    run.agentType = provider.type; run.agentName = provider.name;
    if (brainstorming) run.brainstormRepo = card.repo || '';
    const prompt = brainstorming ? brainstormPrompt(requirements, brainstormHistory, instructions) : preparing ? preparationPrompt(requirements) : reviewContext ? reviewPrompt(reviewContext) : `Travaille sur cette tâche dans le worktree courant, branche ${worktree.branch}${card.ticket ? `, ticket Jira ${card.ticket.key}` : ''}. Ne change pas de branche. ${body.mode === 'analyze' ? 'Analyse seulement, sans modifier les fichiers.' : 'Effectue les modifications nécessaires et les vérifications adaptées.'} Ne crée aucun commit, ne pousse rien et ne publie rien. Termine par un résumé des changements, tests et limites.\n\nTitre : ${card.title}\nDescription :\n${card.description}${checklistText(card.checklist)}\n\nInstructions complémentaires :\n${instructions}`;
    if (body.mode === 'edit') invalidateReviews(card.id, 'Un agent de modification a été lancé depuis cette review. Publiez puis relancez la review.');
    if (card.automation) card.automation = store.card(card.id).automation;
    store.putRun(run); if (!reviewContext && !preparing) card.status = 'doing';
    if (card.agent !== body.agent) { card.model = ''; card.effort = ''; }
    card.agent = body.agent; store.putCard(card);
    const entry = { cardId: card.id, repo, stop: () => {}, cancelled: false };
    active.set(run.id, entry);
    let bytes = 0;
    let agentError = false;
    const finish = (code, error) => {
      if (!active.has(run.id)) return;
      run.status = entry.cancelled ? 'cancelled' : code === 0 && !agentError ? 'completed' : 'failed';
      run.endedAt = timestamp(); run.exitCode = code;
      if (preparing && run.status === 'completed') {
        try {
          if (brainstorming) {
            const { proposals, ...conversation } = parseBrainstorm(run.summary || '');
            run.brainstorm = conversation; run.proposals = proposals;
          } else run.preparation = parsePreparation(run.summary || '');
        }
        catch { run.attention = brainstorming ? 'La réponse de l’agent ne respecte pas le format attendu. Tu peux la consulter et réessayer.' : 'Proposition invalide ou incomplète. Consulte le journal et relance la préparation.'; }
      }
      if (reviewContext && run.status === 'completed') {
        try { run.review = parseReview(run.summary || '', reviewContext.requirements.criteria); }
        catch { run.reviewError = 'Le résultat ou le bilan des critères ne respecte pas le format attendu. Consulte le compte rendu brut et relance la review.'; run.attention = run.reviewError; }
        if (JSON.stringify(reviewContext.requirements) !== JSON.stringify(reviewRequirements(store.card(card.id)))) run.reviewStaleReason = 'Le besoin ou les critères ont changé pendant la review. Relancez l’analyse.';
      }
      store.putRun(run);
      if (error) store.log(run.id, 'system', error);
      const current = store.card(card.id);
      if (current && !reviewContext && !onFinished && !preparing) { current.status = run.status === 'completed' ? 'review' : 'todo'; store.putCard(current); }
      active.delete(run.id);
      changed();
      onFinished?.(run);
    };
    try {
      const handle = launch({ agent: body.agent, ...options, mode: body.mode, repo, prompt }, (stream, line) => {
        if (stream === 'stdout') recordAgentUsage(run, line);
        try { const event = JSON.parse(line); if (event.is_error || ['error', 'turn.failed'].includes(event.type)) agentError = true; } catch {}
        try {
          const event = JSON.parse(line);
          const summary = event.item?.type === 'agent_message' ? event.item.text : event.type === 'result' ? event.result : null;
          if (typeof summary === 'string') run.summary = summary.slice(-(reviewContext || preparing ? 100000 : 12000));
          if (event.permission_denials?.length) { run.attention = 'Permission refusée : adapte les instructions puis relance.'; agentError = true; }
          if (event.type === 'error' || event.type === 'turn.failed' || event.is_error) run.attention = 'Erreur signalée par l’agent. Consulte le journal.';
        } catch {}
        const progress = eventProgress(line);
        run.lastEventAt = timestamp();
        if (progress && !entry.cancelled) {
          run.activity = progress.activity.slice(0, 300);
          if (progress.delta) run.liveText = (run.liveText + progress.delta).slice(-4000);
          if (progress.clearPreview) run.liveText = '';
        }
        if (entry.cancelled) run.activity = 'Arrêt demandé…';
        store.putRun(run);
        bytes += new TextEncoder().encode(line).length;
        const message = eventText(line);
        if (bytes <= 5 * 1024 * 1024 && message) store.log(run.id, stream, message);
        else if (bytes > 5 * 1024 * 1024 && !entry.truncated) { entry.truncated = true; store.log(run.id, 'system', 'Journal limité à 5 Mo.'); }
        changed();
      }, finish);
      entry.stop = handle.stop;
    } catch (error) { finish(1, error.message); }
    return run;
  }


  return {
    async start(...args) {
      if (closing) fail('Atelier est en cours d’arrêt.', 'conflict');
      const operation = startRun(...args); pending.add(operation);
      try { return await operation; } finally { pending.delete(operation); }
    },
    async close() { closing = true; await Promise.allSettled(pending); },
  };
}
