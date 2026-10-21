export function createTaskQueue({ store, launch, busy, result, changed }) {
  let current = null, execution = null, closed = false;
  const read = () => ({ ids: [], paused: true, message: '', ...store.settings()?.taskQueue });
  const save = fields => { store.saveSettings({ ...store.settings(), taskQueue: { ...read(), ...fields } }); changed(); };
  if (read().ids.length || !read().paused) save({ paused: true, message: 'File en pause après redémarrage. Vérifie les tâches avant de reprendre.' });
  function tick() {
    if (closed) return;
    if (current) {
      if (busy()) return;
      const id = current; current = null;
      if (!result(id, execution)) { save({ paused: true, message: 'Exécution arrêtée ou en échec. Consulte la tâche avant de reprendre.', ids: read().ids }); return; }
      save({ ids: read().ids.filter(value => value !== id) });
    }
    const queue = read();
    if (queue.paused || !queue.ids.length || busy()) return;
    current = queue.ids[0];
    try { execution = launch(current); changed(); }
    catch (error) { current = null; save({ paused: true, message: error.message }); }
  }
  const timer = setInterval(tick, 200); timer.unref();
  return {
    state: () => ({ ...read(), current }),
    add(id) {
      if (!read().ids.includes(id)) save({ ids: [...read().ids, id] });
      return this.state();
    },
    remove(id) {
      if (current === id) throw new Error('Mets la file en pause et attends la fin de cette tâche avant de la retirer.');
      save({ ids: read().ids.filter(value => value !== id) }); return this.state();
    },
    pause(paused) { save({ paused, message: paused ? 'File en pause ; la tâche active peut se terminer.' : '' }); return this.state(); },
    tick,
    close() { closed = true; clearInterval(timer); save({ paused: true }); },
  };
}
