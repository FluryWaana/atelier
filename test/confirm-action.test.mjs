import test from 'node:test';
import assert from 'node:assert/strict';
import { confirmAction } from '../public/ui-actions.mjs';

test('one confirmation can approve only its own action, including while the close event is pending', async () => {
  const dialog = new EventTarget();
  dialog.showModal = () => { dialog.open = true; };
  const labels = new Map();
  const document = { querySelector(selector) {
    if (selector === '#confirm-dialog') return dialog;
    if (!labels.has(selector)) labels.set(selector, {});
    return labels.get(selector);
  } };
  const first = confirmAction(document, 'Supprimer A', 'Carte A', 'Supprimer');
  const second = confirmAction(document, 'Supprimer B', 'Carte B', 'Supprimer');
  assert.equal(labels.get('#confirm-message').textContent, 'Supprimer A');
  dialog.open = false; dialog.returnValue = 'confirm';
  const closing = confirmAction(document, 'Supprimer C', 'Carte C', 'Supprimer');
  dialog.dispatchEvent(new Event('close'));
  assert.deepEqual(await Promise.all([first, second, closing]), [true, false, false]);
  const retry = confirmAction(document, 'Supprimer B', 'Carte B', 'Supprimer');
  dialog.open = false; dialog.returnValue = '';
  dialog.dispatchEvent(new Event('close'));
  assert.equal(await retry, false);
});
