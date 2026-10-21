export async function tempoRequest(token, path, body, fetcher = fetch) {
  if (!token) throw new Error('Renseigne le jeton Tempo Cloud dans les réglages.');
  const response = await fetcher('https://api.tempo.io/4' + path, {
    method: body ? 'POST' : 'GET', redirect: 'error', signal: AbortSignal.timeout(20000),
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
    ...(body ? { body: JSON.stringify(body) } : {}),
  });
  if (!response.ok) throw Object.assign(new Error(`Tempo : HTTP ${response.status}. Vérifie le jeton, les droits et les attributs requis par ton équipe.`), { status: response.status });
  return response.json();
}

export async function findTempoEntry(token, entry, request = tempoRequest) {
  const payload = entry.exportPayload;
  for (let offset = 0; offset < 50000; offset += 1000) {
    const query = new URLSearchParams({ from: payload.startDate, to: payload.startDate, limit: '1000', offset: String(offset) });
    const page = await request(token, `/worklogs/user/${encodeURIComponent(payload.authorAccountId)}?${query}`);
    if (!Array.isArray(page.results)) throw new Error('Réponse Tempo inattendue. Vérifie le journal dans Tempo.');
    const found = page.results.filter(log => log.description?.includes(`[atelier:${entry.id}]`));
    if (found.length > 1) throw new Error('Plusieurs écritures Tempo correspondent. Vérifie-les dans Tempo.');
    if (found[0]) return found[0];
    if (!page.metadata?.next && page.results.length < 1000) return null;
  }
  throw new Error('Trop de résultats Tempo pour vérifier cet envoi.');
}
