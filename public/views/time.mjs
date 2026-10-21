import { messageText, msg, markupText, locale } from '../i18n.mjs';
import { esc, duration, agentState } from '../view-helpers.mjs';
import { sameRepository } from '../../lib/domain/paths.mjs';
import { connectorSupports } from '../connector-availability.mjs';
import { agentTime, filterTimes, dailyTime, localDay, timeCsv } from '../ui-helpers.mjs';
import { workReport } from '../workflow.mjs';

export function createTimeView({ document, getState, api, action, confirmAction, refresh, toast, board, scopedCards, connectorName, setView, getSelectedRun, navigator = globalThis.navigator }) {
  const $ = selector => document.querySelector(selector);
  let timeEdit = null, tempoEntry = null, summaryDay = localDay(), timeFilters = { from: '', to: '', status: 'all' };
function elapsed(start, end = Date.now()) {
  const seconds = Math.max(0, Math.floor((new Date(end) - new Date(start)) / 1000));
  return seconds < 60 ? msg("{0} s", seconds) : msg("{0} min {1} s", Math.floor(seconds / 60), String(seconds % 60).padStart(2, '0'));
}
function updateClocks() {
  if ($('#human-total')) { const ids = new Set(scopedCards(true).map(c => c.id)); $('#human-total').textContent = duration(filterTimes((getState().times || []).filter(e => ids.has(e.cardId)), timeFilters).reduce((sum, e) => sum + e.seconds + (e.runningSince ? Math.max(0, Math.floor((Date.now() - new Date(e.runningSince)) / 1000)) : 0), 0)); }
  const now = Date.now();
  const agentTimes = new Map(agentTime(getState().runs, now).map(g => [g.cardId, g]));
  const periodTimes = $('#agent-total') ? new Map(agentTime(getState().runs, now, timeFilters).map(g => [g.cardId, g])) : agentTimes;
  if ($('#agent-total')) { const ids = new Set(scopedCards(true).map(c => c.id)); $('#agent-total').textContent = duration([...periodTimes.values()].filter(g => ids.has(g.cardId)).reduce((sum, g) => sum + g.seconds, 0)); }
  document.querySelectorAll('[data-agent-time]').forEach(el => { el.textContent = duration((el.hasAttribute('data-agent-period') ? periodTimes : agentTimes).get(el.dataset.agentTime)?.seconds || 0); });
  const entries = new Map((getState().times || []).map(e => [e.id, e]));
  const runs = new Map(getState().runs.map(r => [r.id, r]));
  document.querySelectorAll('[data-time-entry]').forEach(el => { const entry = entries.get(el.dataset.timeEntry); if (entry) el.textContent = duration(entry.seconds + (entry.runningSince ? Math.max(0, Math.floor((now - new Date(entry.runningSince)) / 1000)) : 0)); });
  document.querySelectorAll('[data-agent-state]').forEach(el => { const run = runs.get(el.dataset.agentState); if (run) el.textContent = agentState(run); });
  document.querySelectorAll('[data-clock]').forEach(el => {
    const run = runs.get(el.dataset.clock);
    if (run) el.textContent = msg("Durée : {0}", duration(agentTime([run], now)[0]?.seconds || 0));
  });
  const run = runs.get(getSelectedRun());
  if ($('#last-event') && run) $('#last-event').textContent = run.status === 'running' ? msg("Dernier signal il y a {0}{1}", elapsed(run.lastEventAt || run.startedAt), Date.now() - new Date(run.lastEventAt || run.startedAt) > 30000 ? msg(" · En attente de nouvelles de l’agent") : '') : run.status === 'interrupted' ? msg("Dernier signal : {0} · durée estimée", new Date(run.lastEventAt || run.startedAt).toLocaleTimeString(locale)) : msg("Fin : {0}", new Date(run.endedAt).toLocaleTimeString(locale));
}
function timeAction(id, actionName, body = {}) { return action(async () => { await api(`/api/time/${id}/${actionName}`, 'POST', body); await refresh(); }); }
function openTime(cardId, entryId) {
  timeEdit = entryId || null;
  const entry = getState().times?.find(e => e.id === entryId), form = $('#time-form');
  form.reset();
  const cards = cardId || entry ? getState().cards : scopedCards();
  form.elements.cardId.innerHTML = cards.map(c => `<option value="${esc(c.id)}">${esc(c.ticket?.key || '')} ${esc(c.title)}</option>`).join('');
  form.elements.cardId.value = entry?.cardId || cardId || cards[0]?.id || '';
  form.elements.cardId.disabled = Boolean(entry);
  const now = new Date(), local = new Date(now.getTime() - now.getTimezoneOffset() * 60000).toISOString();
  form.elements.date.value = entry?.date || local.slice(0, 10);
  form.elements.startTime.value = entry?.startTime || local.slice(11, 19);
  form.elements.minutes.value = entry ? Math.max(0.02, Math.round(entry.seconds / 60 * 100) / 100) : 30;
  form.elements.description.value = entry?.description || getState().cards.find(c => c.id === form.elements.cardId.value)?.title || '';
  $('#start-timer').hidden = Boolean(entry);
  $('#time-title').textContent = entry ? msg("Corriger le temps") : msg("Saisir du temps");
  $('#time-dialog').showModal();
}
async function saveTime(start) {
  const form = $('#time-form'); if (!form.reportValidity()) return;
  const body = Object.fromEntries(new FormData(form));
  body.cardId = form.elements.cardId.value;
  body.startTime = body.startTime.length === 5 ? body.startTime + ':00' : body.startTime;
  body.seconds = Math.round(Number(body.minutes) * 60); body.start = start;
  const buttons = [...form.querySelectorAll('button')]; buttons.forEach(b => b.disabled = true);
  try {
    await api(timeEdit ? '/api/time/' + timeEdit : '/api/time', timeEdit ? 'PATCH' : 'POST', body);
    $('#time-dialog').close(); $('#card-dialog').close(); setView('time'); await refresh(); toast(start ? msg("Chronomètre démarré. Retrouvez-le dans Temps.") : msg("Temps enregistré localement."));
  } finally { buttons.forEach(b => b.disabled = false); }
}
function renderTimerBanner() {
  const entry = (getState().times || []).find(e => e.runningSince);
  $('#timer-banner').hidden = !entry;
  if (!entry) return;
  const card = getState().cards.find(c => c.id === entry.cardId);
  $('#timer-task').textContent = card?.ticket?.key || card?.title || msg("Temps humain");
  $('#timer-value').dataset.timeEntry = entry.id;
}
function bind() {
$('#time-form').onsubmit = e => { e.preventDefault(); action(() => saveTime(false)); };
$('#start-timer').onclick = () => action(() => saveTime(true));
$('#confirm-tempo').onclick = () => action(async () => {
  const button = $('#confirm-tempo'); button.disabled = true;
  try {
    const entry = await api(`/api/time/${tempoEntry}/export`, 'POST', { confirmed: true });
    $('#tempo-dialog').close(); await refresh(); toast(entry.exportState === 'exported' ? msg("Temps exporté.") : entry.error, entry.exportState !== 'exported');
  } finally { button.disabled = false; }
});
$('#timer-stop').onclick = () => action(async () => {
  const entry = (getState().times || []).find(e => e.runningSince); if (!entry) return;
  const button = $('#timer-stop'); button.disabled = true;
  try { await api(`/api/time/${entry.id}/stop`, 'POST', {}); await refresh(); toast(msg("Chronomètre arrêté. La saisie reste à valider dans Temps.")); }
  finally { button.disabled = false; }
});
$('#timer-open').onclick = () => {
  const entry = (getState().times || []).find(e => e.runningSince), card = getState().cards.find(c => c.id === entry?.cardId);
  const project = getState().projects.find(p => sameRepository(p.repo, card?.repo, getState().settings.platform));
  $('#project-filter').value = project?.id || ''; setView('time'); board();
};
}
function timeMarkup(cards, runs, times) {
  const ids = new Set(cards.map(card => card.id));
  let html;
    const human = times.reduce((sum, e) => sum + e.seconds, 0);
    const agentTimes = agentTime(runs, Date.now(), timeFilters);
    const agent = agentTimes.reduce((sum, g) => sum + g.seconds, 0);
    html = `<div class="panel-heading"><div><h2>${markupText("Saisies & chronomètres")}</h2><p class="hint">${markupText("Des saisies locales, à relire avant tout envoi au service connecté.")}</p></div><button id="add-time" class="primary" ${cards.some(c => !c.archived) ? '' : 'disabled'}>${markupText("Saisir du temps")}</button></div>`;
    html += `<div class="view-stats time-stats"><div><span>${markupText("Temps humain")}</span><strong id="human-total">${duration(human)}</strong><small>${markupText("Période sélectionnée")}</small></div><div><span>${markupText("Temps des agents")}</span><strong id="agent-total">${duration(agent)}</strong><small>${markupText("Période sélectionnée · non facturé")}</small></div><div><span>${markupText("À valider")}</span><strong>${times.filter(entry => !entry.runningSince && entry.exportState !== 'exported').length}</strong><small>${markupText("Saisies à envoyer ou vérifier")}</small></div></div>`;
    html += `<section class="daily-summary"><h3>${markupText("Temps des agents par tâche")}</h3><p class="hint">${markupText("Ajouté automatiquement à partir des exécutions, y compris les reviews, échecs et arrêts. Période sélectionnée · jours locaux · hors export de temps. Une exécution interrompue est estimée au dernier signal reçu.")}</p>${agentTimes.map(g => { const card = cards.find(c => c.id === g.cardId); return `<div class="daily-row"><button data-open-card="${esc(g.cardId)}">${esc(card.ticket?.key || card.title)}</button><strong data-agent-period data-agent-time="${esc(g.cardId)}">${duration(g.seconds)}</strong><span>${markupText("{0} exécution(s){1}{2}", g.runs, g.running ? msg(" · en cours") : '', g.estimated ? msg(" · durée estimée") : '')}</span></div>`; }).join('') || `<p class="hint">${markupText("Aucune durée d’agent pour cette période.")}</p>`}</section>`;
    const daily = dailyTime((getState().times || []).filter(e => ids.has(e.cardId)), summaryDay);
    html += `<section class="daily-summary"><div class="panel-heading"><div><h3>${markupText("Bilan de la journée")}</h3><p class="hint">${markupText("{0} · {1} saisie(s) à envoyer ou vérifier · {2} chronomètre(s) actif(s)", duration(daily.reduce((sum,g) => sum + g.seconds, 0)), daily.reduce((sum,g) => sum + g.pending, 0), daily.reduce((sum,g) => sum + g.running, 0))}</p></div><label>${markupText("Journée")}<input id="summary-day" type="date" value="${esc(summaryDay)}"></label></div>${daily.map(g => { const card = cards.find(c => c.id === g.cardId); return `<div class="daily-row"><button data-open-card="${esc(g.cardId)}">${esc(card?.ticket?.key || card?.title || msg("Tâche"))}</button><strong>${duration(g.seconds)}</strong><span>${markupText("{0} à valider", g.pending)}</span></div>`; }).join('') || `<p class="hint">${markupText("Aucun temps saisi pour cette journée.")}</p>`}<p class="hint">${markupText("Regroupement par date de saisie. Les durées des agents sont exclues.")}</p><button id="show-summary-day">${markupText("Afficher les saisies de cette journée")}</button></section>`;
    html += `<section class="time-filters" aria-label="${markupText("Filtrer les saisies")}"><label>${markupText("Du")}<input id="time-from" type="date" value="${esc(timeFilters.from)}"></label><label>${markupText("Au")}<input id="time-to" type="date" value="${esc(timeFilters.to)}"></label><label>${markupText("Export de temps")}<select id="time-status">${[['all', msg("Toutes les saisies")], ['pending', msg("À valider / vérifier")], ['exported', msg("Déjà envoyées")]].map(([value, label]) => `<option value="${value}" ${timeFilters.status === value ? 'selected' : ''}>${label}</option>`).join('')}</select></label><button id="reset-time-filters">${markupText("Tout afficher")}</button><button id="export-time-csv" ${times.some(e => !e.runningSince) ? '' : 'disabled'}>${markupText("Exporter en CSV")}</button></section><p class="hint">${markupText("Les dates filtrent les saisies et les agents ; le statut d’export filtre uniquement les saisies humaines. {0} saisie(s) dans cette vue. Le CSV reprend ces filtres et exclut les chronomètres actifs. Il n’envoie rien au service connecté.", times.length)}</p>`;
    html += times.map(entry => {
      const card = cards.find(c => c.id === entry.cardId);
      const labels = { draft: msg("Local · à valider"), sending: msg("Envoi en cours"), uncertain: msg("Envoi à vérifier"), exported: msg("Envoyé au service") };
      return `<article class="workspace-row time-entry"><div><span class="section-kicker">${esc(entry.date)} · ${esc(entry.startTime)}</span><h3>${esc(card.ticket?.key || card.title)} · <span data-time-entry="${entry.id}">${duration(entry.seconds)}</span></h3><p class="hint">${markupText("{0} à {1} · {2}{3}", esc(entry.date), esc(entry.startTime), entry.runningSince ? msg("Chronomètre actif") : esc(labels[entry.exportState]), entry.interrupted ? msg(" · interrompu au redémarrage, durée à vérifier") : '')}</p><p class="entry-description">${esc(entry.description)}</p>${entry.error ? `<p class="inline-error">${esc(messageText(entry.error))}</p>` : ''}</div><div class="row-actions">${entry.runningSince ? `<button data-stop-time="${entry.id}">${markupText("Arrêter")}</button>` : entry.exportState === 'draft' ? `<button data-edit-time="${entry.id}">${markupText("Corriger")}</button><button ${connectorSupports(getState().connectors, 'time', 'send') ? '' : 'hidden'} data-export-time="${entry.id}" ${card.ticket || getState().connectors?.providers?.time !== 'tempo' ? '' : 'disabled'}>${markupText("Exporter…")}</button><button data-delete-time="${entry.id}" class="danger">${markupText("Retirer")}</button>` : entry.exportState === 'uncertain' ? `<button ${connectorSupports(getState().connectors, 'time', 'reconcile', entry.exportConnector || getState().connectors?.providers?.time) ? '' : 'hidden'} data-reconcile-time="${entry.id}">${markupText("Vérifier auprès du service")}</button><button data-reset-time="${entry.id}">${markupText("Débloquer après vérification…")}</button>` : ''}</div></article>`;
    }).join('') || `<p class="empty">${markupText("Aucune saisie dans cette vue. Ajustez les filtres ou ajoutez du temps.")}</p>`;
  return html;
}
function bindEntries(cards, times) {
  if ($('#summary-day')) { $('#summary-day').onchange = event => { if (event.target.value) { summaryDay = event.target.value; board(); } }; $('#show-summary-day').onclick = () => { timeFilters = { from: summaryDay, to: summaryDay, status: 'all' }; board(); }; }
  if ($('#time-from')) {
    for (const key of ['from', 'to', 'status']) $('#time-' + key).onchange = event => { timeFilters[key] = event.target.value; board(); };
    $('#reset-time-filters').onclick = () => { timeFilters = { from: '', to: '', status: 'all' }; board(); };
    $('#export-time-csv').onclick = () => {
      const url = URL.createObjectURL(new Blob([timeCsv(times, cards)], { type: 'text/csv;charset=utf-8' }));
      const link = document.createElement('a'); link.href = url; link.download = 'atelier-temps.csv'; link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
      toast(msg("CSV exporté : saisies arrêtées uniquement."));
    };
  }
  if ($('#add-time')) $('#add-time').onclick = () => openTime();
  document.querySelectorAll('[data-edit-time]').forEach(b => b.onclick = () => openTime(null, b.dataset.editTime));
  document.querySelectorAll('[data-stop-time]').forEach(b => b.onclick = () => timeAction(b.dataset.stopTime, 'stop'));
  document.querySelectorAll('[data-reconcile-time]').forEach(b => b.onclick = () => timeAction(b.dataset.reconcileTime, 'reconcile'));
  document.querySelectorAll('[data-reset-time]').forEach(b => b.onclick = async () => { if (await confirmAction(msg("Confirmez avoir vérifié directement dans le service de temps que cette saisie n’existe pas. Débloquer un envoi déjà reçu peut créer un doublon."), msg("Débloquer cet envoi ?"), msg("Débloquer l’envoi"))) timeAction(b.dataset.resetTime, 'reset-export', { confirmed: true }); });
  document.querySelectorAll('[data-delete-time]').forEach(b => b.onclick = () => action(async () => { if (!(await confirmAction(msg("Retirer cette saisie locale ?"), msg("Supprimer la saisie ?"), msg("Supprimer la saisie")))) return; await api('/api/time/' + b.dataset.deleteTime, 'DELETE', {}); await refresh(); }));
  document.querySelectorAll('[data-export-time]').forEach(b => b.onclick = () => action(async () => {
    tempoEntry = b.dataset.exportTime;
    const entry = getState().times.find(e => e.id === tempoEntry), card = getState().cards.find(c => c.id === entry.cardId);
    const preview = await api('/api/time/' + entry.id + '/export-preview');
    $('#tempo-preview').innerHTML = `<h3>${esc(card.ticket?.key || '')} · ${esc(card.title)}</h3><p>${markupText("{0} à {1} ·", esc(entry.date), esc(entry.startTime))} <strong>${duration(entry.seconds)}</strong></p><p class="entry-description">${esc(entry.description)}</p><p class="hint">${markupText("Destination : {0}", esc(connectorName('time')))}</p>`;
    if (preview?.message) { const message = document.createElement('p'); message.textContent = preview.message; $('#tempo-preview').append(message); }
    $('#tempo-dialog').showModal();
  }));
}
function reportMarkup() {
  return `<details class="disclosure"><summary>${markupText("Bilan de travail à copier")}</summary><label>${markupText("Journée")}<input id="report-day" type="date" value="${esc(summaryDay)}"></label><p class="hint">${markupText("Projet sélectionné, archives incluses. Les blocages reflètent l’état actuel. Aucun appel IA.")}</p><label>${markupText("Bilan")}<textarea id="work-report" rows="12" readonly>${esc(workReport(scopedCards(true), getState().runs, getState().times || [], summaryDay, getState().cards))}</textarea></label><button id="copy-work-report">${markupText("Copier le bilan")}</button></details>`;
}
function bindReport() {
  if ($('#report-day')) $('#report-day').onchange = e => { if (e.target.value) { summaryDay = e.target.value; board(); } };
  if ($('#copy-work-report')) $('#copy-work-report').onclick = () => action(async () => {
    const report = $('#work-report');
    try { await navigator.clipboard.writeText(report.value); toast(msg("Bilan copié.")); }
    catch { report.focus(); report.select(); toast(msg("Copie indisponible : le texte est sélectionné, utilise Ctrl+C.")); }
  });
}

  return { bind, bindEntries, bindReport, openTime, saveTime, timeMarkup, reportMarkup, renderTimerBanner, updateClocks, get filters() { return timeFilters; }, get day() { return summaryDay; } };
}
