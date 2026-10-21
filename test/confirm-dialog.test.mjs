import './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { confirmAction } from '../public/ui-actions.mjs';

test('confirmation waits for a choice and dismissing never approves the action', async () => {
  const fields = new Map();
  let close, opens = 0;
  const dialog = { returnValue: 'confirm', addEventListener(type, listener, options) { assert.equal(type, 'close'); assert.equal(options.once, true); close = listener; }, showModal() { opens++; } };
  const context = {  $: selector => { if (selector === '#confirm-dialog') return dialog; if (!fields.has(selector)) fields.set(selector, {}); return fields.get(selector); } };
  context.confirmAction = (...args) => confirmAction({ querySelector: context.$ }, ...args);
  for (const choice of ['confirm', 'cancel', '']) {
    let settled = false;
    const result = context.confirmAction('<Texte sûr>', 'Supprimer ?', 'Supprimer localement');
    result.then(() => { settled = true; });
    await Promise.resolve();
    assert.equal(settled, false);
    assert.equal(dialog.returnValue, '');
    assert.equal(fields.get('#confirm-message').textContent, '<Texte sûr>');
    assert.equal(fields.get('#confirm-title').textContent, 'Supprimer ?');
    assert.equal(fields.get('#confirm-action').textContent, 'Supprimer localement');
    dialog.returnValue = choice; close();
    assert.equal(await result, choice === 'confirm');
  }
  assert.equal(opens, 3);
});
