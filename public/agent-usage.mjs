import { msg, locale } from './i18n.mjs';
export function usageText(run) {
  const u = run.usage;
  const model = u?.models?.length ? u.models.join(', ') : run.actualModel || run.model || msg("Modèle non communiqué par le CLI");
  if (!u) return msg("{0} · Tokens et coût {1}.", model, run.status === 'running' ? msg("en attente du bilan CLI") : msg("indisponibles (bilan CLI absent)"));
  const n = value => value.toLocaleString(locale);
  const cost = u.referenceModel ? msg("Coût indisponible (ancienne estimation sur un autre modèle)") : u.costUsd == null ? msg("Coût indisponible (modèle ou tarif inconnu)") : msg("Coût estimé : {0}", u.costUsd.toLocaleString(locale, { style: 'currency', currency: 'USD', minimumFractionDigits: 4, maximumFractionDigits: 6 }));
  return msg("{0} · {1} tokens · Input : {2} (dont cache lu : {3}, écrit : {4}) · Output : {5} · {6}{7}{8}{9}.", model, n(u.totalTokens), n(u.inputTokens), n(u.cacheReadTokens), n(u.cacheWriteTokens), n(u.outputTokens), cost, u.source === 'standard-api' ? msg(" · Tarif API standard du {0}, contexte court, hors outils et majorations", u.pricingDate) : msg(" · Estimation CLI"), u.scope === 'main-agent' ? msg(" · Tokens de l’agent principal uniquement") : '', run.status !== 'completed' ? msg(" · Bilan partiel possible") : '');
}

export function usageTotals(runs) {
  const measured = runs.filter(run => !run.usage?.referenceModel && typeof run.usage?.costUsd === 'number' && Number.isFinite(run.usage.costUsd) && run.usage.costUsd >= 0);
  return { costUsd: measured.reduce((sum, run) => sum + run.usage.costUsd, 0), measured: measured.length, missing: runs.length - measured.length };
}
