import { msg, markupText } from '../i18n.mjs';
import { esc } from '../view-helpers.mjs';

export function createBrainstormQuestions({ host, replyForm, localStorage, onSubmit, action }) {
  let runId = '', sessionId = '', questions = [], answers = [], index = 0, free = false, busy = false;
  const $ = selector => host.querySelector(selector);
  const value = i => answers[i]?.choice === 'other' ? answers[i].text.trim() : questions[i].options[answers[i]?.choice] || '';
  const reply = () => questions.map((question, i) => value(i) ? `${question.question}\n${value(i)}` : '').filter(Boolean).join('\n\n');
  const save = () => { try { localStorage.setItem('atelier-brainstorm-questions:' + sessionId, JSON.stringify({ runId, answers, index, free })); } catch {} };
  const ready = () => answers.filter((_, i) => value(i)).length === questions.length;

  function update() {
    $('[data-next]').disabled = busy || !value(index) || index === questions.length - 1 && (!ready() || reply().length > 20000);
    $('[data-progress]').textContent = msg("{0} réponse(s) sur {1}", questions.filter((_, i) => value(i)).length, questions.length);
    $('[data-limit]').hidden = reply().length <= 20000;
  }

  function draw(focus = false) {
    replyForm.hidden = !free;
    const question = questions[index], answer = answers[index];
    host.innerHTML = `<div class="brainstorm-question-toolbar"><span>${markupText("Quelques choix pour préciser votre idée")}</span><button type="button" class="text-button" data-free ${busy ? 'disabled' : ''}>${free ? markupText("Reprendre les questions") : markupText("Écrire un message libre")}</button></div>${free ? '' : `<form class="brainstorm-question-slide">
      <div class="brainstorm-question-progress"><span>${markupText("Question {0} sur {1}", index + 1, questions.length)}</span><span data-progress aria-live="polite"></span></div>
      <div class="brainstorm-question-steps" aria-hidden="true">${questions.map((_, i) => `<span class="${i === index ? 'current' : value(i) ? 'answered' : ''}"></span>`).join('')}</div>
      <fieldset ${busy ? 'disabled' : ''}><legend tabindex="-1">${esc(question.question)}</legend>
      <div class="brainstorm-question-options">${question.options.map((option, i) => `<label class="brainstorm-question-option"><input type="radio" name="answer" value="${i}" ${answer.choice === String(i) ? 'checked' : ''}><span>${esc(option)}</span></label>`).join('')}
      ${question.options.length ? `<label class="brainstorm-question-option"><input type="radio" name="answer" value="other" ${answer.choice === 'other' ? 'checked' : ''}><span>${markupText("Autre réponse")}</span></label>` : ''}</div>
      <label class="brainstorm-question-custom" ${answer.choice === 'other' ? '' : 'hidden'}>${markupText("Votre réponse")}<textarea rows="2" maxlength="2000" placeholder="${markupText("Précisez votre idée…")}">${esc(answer.text)}</textarea></label></fieldset>
      <p class="inline-error" data-limit hidden>${markupText("Vos réponses sont trop longues. Raccourcissez-les avant l’envoi.")}</p>
      <div class="brainstorm-question-navigation"><button type="button" data-previous ${!index || busy ? 'disabled' : ''}>${markupText("Précédent")}</button><span class="hint">${markupText("Vous pourrez revenir sur vos choix.")}</span><button type="submit" class="primary" data-next>${index === questions.length - 1 ? markupText("Envoyer mes réponses") : markupText("Suivant")}</button></div>
    </form>`}`;
    $('[data-free]').onclick = () => { free = !free; save(); draw(true); };
    if (free) { if (focus) replyForm.querySelector('textarea').focus(); return; }
    host.querySelectorAll('input[name=answer]').forEach(input => input.onchange = () => {
      answer.choice = input.value;
      $('.brainstorm-question-custom').hidden = answer.choice !== 'other';
      save(); update();
      if (answer.choice === 'other') $('textarea').focus();
    });
    $('textarea').oninput = event => { answer.text = event.target.value; save(); update(); };
    $('[data-previous]').onclick = () => { if (busy || !index) return; index--; save(); draw(true); };
    $('form').onsubmit = event => {
      event.preventDefault();
      if ($('[data-next]').disabled) return;
      if (index < questions.length - 1) { index++; save(); draw(true); }
      else return action(() => onSubmit(reply()));
    };
    update();
    if (focus) { host.scrollTop = 0; $('legend').focus({ preventScroll: true }); }
  }

  function render(run, items, disabled) {
    host.hidden = !items.length;
    if (!items.length) { replyForm.hidden = false; return; }
    const changed = runId !== run.id;
    if (changed) {
      runId = run.id; sessionId = run.cardId; questions = items;
      let saved; try { saved = JSON.parse(localStorage.getItem('atelier-brainstorm-questions:' + sessionId)); } catch {}
      if (saved?.runId !== runId) saved = null;
      answers = questions.map(question => ({ choice: question.options.length ? '' : 'other', text: '' }));
      answers.forEach((answer, i) => {
        const stored = saved?.answers?.[i];
        if (stored?.choice === 'other' || questions[i].options.some((_, n) => stored?.choice === String(n))) answer.choice = stored.choice;
        if (typeof stored?.text === 'string') answer.text = stored.text.slice(0, 2000);
      });
      index = Number.isInteger(saved?.index) ? Math.max(0, Math.min(questions.length - 1, saved.index)) : 0;
      free = saved?.free === true;
    }
    if (changed || busy !== disabled) { busy = disabled; draw(); }
    else replyForm.hidden = !free;
  }

  return { render, reply, get active() { return !host.hidden && !free; } };
}
