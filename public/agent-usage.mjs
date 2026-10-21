export function usageText(run) {
  const u = run.usage;
  const model = u?.models?.length ? u.models.join(', ') : run.actualModel || run.model || 'Modèle non communiqué par le CLI';
  if (!u) return `${model} · Tokens et coût ${run.status === 'running' ? 'en attente du bilan CLI' : 'indisponibles (bilan CLI absent)'}.`;
  const n = value => value.toLocaleString('fr-FR');
  const cost = u.referenceModel ? 'Coût indisponible (ancienne estimation sur un autre modèle)' : u.costUsd == null ? 'Coût indisponible (modèle ou tarif inconnu)' : `Coût estimé : ${u.costUsd.toLocaleString('fr-FR', { style: 'currency', currency: 'USD', minimumFractionDigits: 4, maximumFractionDigits: 6 })}`;
  return `${model} · ${n(u.totalTokens)} tokens · Input : ${n(u.inputTokens)} (dont cache lu : ${n(u.cacheReadTokens)}, écrit : ${n(u.cacheWriteTokens)}) · Output : ${n(u.outputTokens)} · ${cost}${u.source === 'standard-api' ? ` · Tarif API standard du ${u.pricingDate}, contexte court, hors outils et majorations` : ' · Estimation CLI'}${u.scope === 'main-agent' ? ' · Tokens de l’agent principal uniquement' : ''}${run.status !== 'completed' ? ' · Bilan partiel possible' : ''}.`;
}

export function usageTotals(runs) {
  const measured = runs.filter(run => !run.usage?.referenceModel && typeof run.usage?.costUsd === 'number' && Number.isFinite(run.usage.costUsd) && run.usage.costUsd >= 0);
  return { costUsd: measured.reduce((sum, run) => sum + run.usage.costUsd, 0), measured: measured.length, missing: runs.length - measured.length };
}
