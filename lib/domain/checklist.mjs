export function checklist(value) {
  if (!Array.isArray(value) || value.length > 20) throw new Error('Maximum 20 critères de réussite.');
  return value.map(item => {
    if (!item || typeof item.text !== 'string' || !item.text.trim() || item.text.length > 500 || typeof item.done !== 'boolean') throw new Error('Chaque critère doit contenir un texte et un état valide.');
    return { text: item.text.trim(), done: item.done };
  });
}

export function checklistText(items = []) {
  return items.length ? '\n\nCritères de réussite (validation humaine) :\n' + items.map(item => `- [${item.done ? 'x' : ' '}] ${item.text}`).join('\n') : '';
}
