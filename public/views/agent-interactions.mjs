import { msg, markupText } from '../i18n.mjs';
import { esc } from '../view-helpers.mjs';

export function createAgentInteractions({ document, window, getState, api, action, refresh, toast }) {
  const banner = document.querySelector('#agent-interactions');
  const dialog = document.querySelector('#agent-reply-dialog');
  const form = document.querySelector('#agent-reply-form');
  const submitting = new Set();
  let selected, seen = new Set(), signature = '';
  const pending = () => getState().runs.filter(run => run.status === 'running').flatMap(run => (run.interactions || []).map(request => ({ run, request })));
  function open(id) {
    const item = pending().find(item => item.request.id === id);
    if (!item) return;
    selected = item;
    const { request, run } = item;
    form.innerHTML = `<div class="dialog-heading"><h2 id="agent-reply-title">${esc(getState().cards.find(card => card.id === run.cardId)?.title || msg("Agent"))}</h2></div><p>${esc(request.title)}</p>${request.detail ? `<pre class="log">${esc(request.detail)}</pre>` : ''}${(request.questions || []).map((question, index) => `<fieldset class="form-section"><legend>${esc(question.question)}</legend>${question.options.map((option, optionIndex) => `<label class="check"><input type="${question.multiSelect ? 'checkbox' : 'radio'}" name="choice-${index}" value="${optionIndex}"> ${esc(option.label)}${option.description ? ` — ${esc(option.description)}` : ''}</label>`).join('')}<label>${markupText("Réponse libre")}<input type="${question.isSecret ? 'password' : 'text'}" name="answer-${index}" maxlength="10000" autocomplete="off"></label></fieldset>`).join('')}<div class="form-actions">${request.kind === 'question' ? `<button class="primary" type="submit" value="answer">${markupText("Envoyer la réponse")}</button>` : request.canAllow ? `<button class="primary" type="submit" value="allow">${markupText("Autoriser cette demande")}</button>` : ''}<button type="submit" value="deny" formnovalidate>${markupText("Refuser")}</button><button type="button" data-later>${markupText("Plus tard")}</button></div>`;
    form.querySelector('[data-later]').onclick = () => dialog.close();
    form.querySelectorAll('button, input').forEach(field => { field.disabled = submitting.has(request.id); });
    if (!dialog.open) dialog.showModal();
  }
  form.onsubmit = event => {
    event.preventDefault();
    if (!selected || submitting.has(selected.request.id)) return;
    const { run, request } = selected, decision = event.submitter?.value || 'answer';
    const answers = Object.fromEntries((request.questions || []).map((question, index) => {
      const value = form.elements.namedItem('answer-' + index).value;
      const free = question.isSecret ? value : value.trim();
      const choices = [...form.querySelectorAll(`[name="choice-${index}"]:checked`)].map(input => question.options[Number(input.value)].label);
      return [question.id, free ? [free] : choices];
    }));
    if (decision === 'answer' && Object.values(answers).some(values => !values.length)) return toast(msg("Répondez à chaque question."), true);
    submitting.add(request.id);
    form.querySelectorAll('button, input').forEach(field => { field.disabled = true; });
    action(async () => {
      try {
        await api('/api/runs/' + encodeURIComponent(run.id) + '/reply', 'POST', { requestId: request.id, decision, answers });
        if (selected?.request.id === request.id) dialog.close();
        await refresh();
      } finally {
        submitting.delete(request.id);
        if (selected?.request.id === request.id) form.querySelectorAll('button, input').forEach(field => { field.disabled = false; });
      }
    });
  };
  function render() {
    const entries = pending();
    const ids = new Set(entries.map(item => item.request.id));
    if (dialog.open && selected && !ids.has(selected.request.id)) dialog.close();
    banner.hidden = !entries.length;
    document.title = entries.length ? msg("({0}) Réponse attendue · Atelier Kanban", entries.length) : 'Atelier Kanban';
    const next = JSON.stringify(entries.map(({ run, request }) => [run.id, request.id, getState().cards.find(card => card.id === run.cardId)?.title]));
    if (next !== signature) {
      signature = next;
      banner.innerHTML = `<p role="status">${markupText("{0} demande(s) en attente de votre réponse", entries.length)}</p><div class="row-actions">${entries.map(({ run, request }) => `<button type="button" data-reply="${esc(request.id)}">${markupText("Répondre · {0}", esc(getState().cards.find(card => card.id === run.cardId)?.title || run.agentName))}</button>`).join('')}${window.Notification && window.Notification.permission === 'default' ? `<button type="button" data-enable-notifications>${markupText("Activer les notifications système")}</button>` : ''}</div>`;
      banner.querySelectorAll('[data-reply]').forEach(button => { button.onclick = () => open(button.dataset.reply); });
      const enable = banner.querySelector('[data-enable-notifications]');
      if (enable) enable.onclick = () => action(async () => { await window.Notification.requestPermission(); signature = ''; render(); });
    }
    const fresh = entries.filter(item => !seen.has(item.request.id));
    seen = ids;
    if (fresh.length) {
      toast(msg("Un agent attend votre réponse."));
      if (window.Notification?.permission === 'granted' && document.hidden) {
        try {
          const notification = new window.Notification(msg("Un agent attend votre réponse."), { body: msg("Ouvrez Atelier Kanban pour continuer."), tag: 'atelier-agent-input' });
          notification.onclick = () => { window.focus(); notification.close(); open(fresh[0].request.id); };
        } catch { /* The persistent banner remains available if desktop notifications are unsupported. */ }
      }
    }
  }
  return { render };
}
