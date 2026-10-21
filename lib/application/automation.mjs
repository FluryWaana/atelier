import { correctionInstructions } from '../domain/reviews.mjs';
import { reviewRequirements } from '../domain/tasks.mjs';

const timestamp = () => new Date().toISOString();
export function createAutomation({ store, settingsFor, executeAgent, preview, publish, reviewContext, verify, changed, syncStatus, delay }) {
  const active = new Map();
  for (const card of store.cards()) if (card.automation?.state === 'running') {
    card.automation.state = 'interrupted'; card.automation.message = 'Parcours interrompu. Vérifie les processus et le worktree avant de reprendre.';
    card.status = 'standby'; store.putCard(card);
  }
  const save = (id, fields) => {
    const card = store.card(id);
    card.automation = { ...card.automation, ...fields, updatedAt: timestamp() };
    store.putCard(card); changed(); return card;
  };
  const step = (id, value) => save(id, { step: value, message: '' });
  async function currentReview(card, signal) {
    for (let attempt = 0; ; attempt++) {
      signal.throwIfAborted();
      try { return await reviewContext(card); }
      catch (error) {
        if (!error.retryable || attempt === 9) throw error;
        await delay(1000, undefined, { signal });
      }
    }
  }
  async function drive(id, signal) {
    const check = () => signal.throwIfAborted();
    try {
      while (true) {
        check();
        let card = store.card(id), flow = card.automation;
        if (['development', 'correction'].includes(flow.step)) {
          const run = await executeAgent(card, { ...flow.options, mode: 'edit', instructions: flow.step === 'correction' ? flow.correction : flow.options.instructions || '' }, null, signal, run => save(id, { runId: run.id }));
          check();
          if (run.status !== 'completed') throw new Error('L’agent n’a pas terminé. Consulte le journal avant de reprendre.');
          save(id, { summary: run.summary || '' }); step(id, 'tests');
        } else if (flow.step === 'tests') {
          const before = await preview(card); check();
          save(id, { tests: { state: 'running', command: flow.testCommand, startedAt: timestamp(), output: '' } });
          try {
            await verify(flow.testCommand, card.worktree.path, { signal, onOutput: chunk => {
              const tests = store.card(id).automation.tests;
              save(id, { tests: { ...tests, output: (tests.output + chunk).slice(-32000) } });
            } });
            check();
            const after = await preview(store.card(id)); check();
            if (before.fingerprint !== after.fingerprint) throw new Error('Les fichiers ont changé pendant les tests. Relance les vérifications sur leur nouvel état.');
            save(id, { tests: { ...store.card(id).automation.tests, state: 'passed', endedAt: timestamp(), fingerprint: after.fingerprint } });
          } catch (error) {
            save(id, { tests: { ...store.card(id).automation.tests, state: 'failed', endedAt: timestamp(), error: error.message } }); throw error;
          }
          step(id, 'publish');
        } else if (flow.step === 'publish') {
          const current = await preview(card); check();
          if (flow.tests?.state !== 'passed' || flow.tests.command !== flow.testCommand || current.fingerprint !== flow.tests.fingerprint) {
            step(id, 'tests'); throw new Error('Le contenu a changé depuis les tests ou une publication a été interrompue. Reprends pour revérifier avant publication.');
          }
          const verification = `${card.ticket ? `Ticket Jira : [${card.ticket.key}](${card.ticket.url})\n\n` : ''}## Vérification\nCommande : ${flow.testCommand}\nRésultat : réussi (${flow.tests.endedAt}).\n\n`;
          if (verification.length > 3800) throw new Error('Commande et lien Jira trop longs pour la description Azure (4 000 caractères). Raccourcis la commande du projet.');
          const details = `## Changements\n${card.description || card.title}\n\n## Critères\n${(card.checklist || []).map(c => `- [${c.done ? 'x' : ' '}] ${c.text}`).join('\n')}\n\n## Compte rendu de l’agent\n${flow.summary || ''}`;
          const body = verification + (details.length > 4000 - verification.length ? details.slice(0, 3900 - verification.length) + '\n[Compte rendu complet dans Atelier.]' : details);
          await publish(card, { ...current, base: flow.prTarget, body, commitChanges: true, draftOnly: true }); check();
          step(id, 'review');
        } else if (flow.step === 'review') {
          const context = await currentReview(card, signal); check();
          const report = await executeAgent(card, { ...flow.options, mode: 'analyze' }, context, signal, run => save(id, { runId: run.id })); check();
          if (report.status !== 'completed' || !report.review || report.reviewError) throw new Error(report.reviewError || 'Review non aboutie. Consulte le journal.');
          try {
            const current = await currentReview(store.card(id), signal); check();
            if (current.source !== context.source || current.target !== context.target) throw new Error('La PR a changé pendant la review. Relance cette étape.');
            if (report.reviewStaleReason) throw new Error(report.reviewStaleReason);
            const requirements = reviewRequirements(store.card(id));
            if (report.reviewContext?.requirements && JSON.stringify(report.reviewContext.requirements) !== JSON.stringify(requirements)) throw new Error('Le besoin ou les critères ont changé pendant la review. Relance cette étape.');
            if (!Array.isArray(report.review.criteria) || JSON.stringify(report.review.criteria.map(item => item.text)) !== JSON.stringify(requirements.criteria)) throw new Error('Le bilan ne couvre pas exactement les critères actuels. Relance la review.');
          } catch (error) { report.reviewStaleReason = error.message; store.putRun(report); throw error; }
          const acceptance = { total: report.review.criteria.length, covered: report.review.criteria.filter(item => item.status === 'covered').length, notCovered: report.review.criteria.filter(item => item.status === 'not-covered').length, unverifiable: report.review.criteria.filter(item => item.status === 'unverifiable').length };
          save(id, { reportId: report.id, acceptance });
          const indexes = report.review.findings.flatMap((f, i) => ['P0', 'P1'].includes(f.priority) ? [i] : []);
          if (indexes.length && flow.autoFix && flow.cycles < 2) {
            save(id, { cycles: flow.cycles + 1, correction: correctionInstructions(report, indexes) }); step(id, 'correction');
          } else {
            const counts = Object.fromEntries(['P0', 'P1', 'P2', 'P3', 'suggestion'].map(p => [p, report.review.findings.filter(f => f.priority === p).length]));
            const blocked = indexes.length > 0 || acceptance.notCovered > 0 || acceptance.unverifiable > 0;
            const message = indexes.length ? 'PR à corriger : problèmes bloquants restants.' : blocked ? 'Contrôle des critères incomplet : critères non couverts ou impossibles à vérifier. Intervention humaine nécessaire.' : acceptance.total ? 'PR prête pour ton contrôle.' : 'PR à contrôler : aucun critère de réussite défini.';
            card = save(id, { state: blocked ? 'blocked' : 'ready', counts, message, endedAt: timestamp() });
            card.status = blocked ? 'standby' : 'review'; store.putCard(card); await syncStatus(id); changed(); return;
          }
        } else throw new Error('Étape automatique inconnue.');
      }
    } catch (error) {
      const card = save(id, { state: signal.aborted ? 'cancelled' : 'error', message: signal.aborted ? 'Parcours arrêté. Vérifie le worktree avant de reprendre.' : error.message, endedAt: timestamp() });
      card.status = 'standby'; store.putCard(card); changed();
    } finally { active.delete(id); changed(); }
  }
  function start(card, options, resume = false) {
    if (active.has(card.id)) throw new Error('Parcours déjà en cours.');
    const settings = settingsFor(card);
    if (!settings.automationEnabled || !settings.testCommand || !settings.prTarget) throw new Error('Active le mode automatique et configure les vérifications et la branche cible dans le projet.');
    if (resume && !['error', 'interrupted', 'cancelled'].includes(card.automation?.state)) throw new Error('Aucune étape interrompue à reprendre.');
    if (resume && card.automation.prTarget !== settings.prTarget) throw new Error('La cible a changé : rétablis la cible du parcours ou lance un nouveau développement.');
    const commandChanged = resume && card.automation.testCommand !== settings.testCommand;
    card.automation = { ...(resume ? card.automation : { step: 'development', cycles: 0, startedAt: timestamp(), options }), state: 'running', message: '', testCommand: settings.testCommand, prTarget: settings.prTarget, autoFix: settings.autoFix };
    if (commandChanged && !['development', 'correction'].includes(card.automation.step)) card.automation.step = 'tests';
    card.status = 'doing'; store.putCard(card);
    const controller = new AbortController(), entry = { controller };
    active.set(card.id, entry);
    entry.done = drive(card.id, controller.signal);
    changed(); return store.card(card.id);
  }
  return {
    start, busy: id => active.has(id), size: () => active.size,
    stop(id) { const entry = active.get(id); if (!entry) throw new Error('Aucun parcours en cours.'); entry.controller.abort(); },
    async close() { const entries = [...active.values()]; for (const entry of entries) entry.controller.abort(); await Promise.all(entries.map(entry => entry.done)); },
  };
}
