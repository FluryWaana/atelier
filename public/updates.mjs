export function initUpdates({ api, confirm }) {
  const banner = document.querySelector('#update-banner'), message = document.querySelector('#update-message');
  const checkButton = document.querySelector('#check-updates'), installButton = document.querySelector('#install-update'), notes = document.querySelector('#update-notes');
  const dismissButton = document.querySelector('#dismiss-update');
  let available = null, installing = false, notice = '', dismissed = '';
  function controls(busy) {
    installing = busy; installButton.disabled = busy; checkButton.disabled = busy; dismissButton.disabled = busy;
  }
  function render(state, manual) {
    available = state.available;
    notice = JSON.stringify([available?.version, state.error, state.result?.date]);
    banner.hidden = !manual && ((!available && !state.result && !state.error) || notice === dismissed);
    message.textContent = !state.configured ? 'Les mises à jour seront disponibles dans la version distribuée depuis GitHub.' : state.error || (available ? `La version ${available.version} est disponible.` : `Atelier ${state.currentVersion} est à jour.`);
    if (!state.error && !available && state.result) message.textContent = state.result.success ? state.result.message : 'La dernière mise à jour a échoué : ' + state.result.message;
    installButton.hidden = !available || !state.canInstall;
    notes.hidden = !available;
    if (available) { notes.href = available.notesUrl; if (!state.canInstall) message.textContent += ' Télécharge le package de ton système depuis les nouveautés.'; }
  }
  async function check(manual = false) {
    if (installing) return;
    checkButton.disabled = true;
    try {
      const state = await api('/api/updates/check', 'POST', {});
      if (installing) return;
      render(state, manual);
      if (state.applying || state.phase === 'installing') await followInstallation(state.available?.version);
    }
    catch (error) { if (manual && !installing) { banner.hidden = false; message.textContent = error.message; } }
    finally { checkButton.disabled = installing; }
  }
  dismissButton.onclick = () => { if (!installing) { dismissed = notice; banner.hidden = true; checkButton.focus(); } };
  checkButton.onclick = () => check(true);
  async function followInstallation(version, started = 0, begin) {
    controls(true); banner.hidden = false;
    try {
      if (begin) {
        message.textContent = 'Téléchargement et vérification de la mise à jour…';
        try { await begin(); }
        catch (error) { if (!(error instanceof TypeError) && !(error instanceof SyntaxError)) throw error; }
      }
      message.textContent = 'Installation et redémarrage en cours…';
      for (let i = 0; i < 180; i++) {
        await new Promise(resolve => setTimeout(resolve, 1000));
        let state;
        try {
          const response = await fetch('/api/updates', { cache: 'no-store', signal: AbortSignal.timeout(2000) });
          if (!response.ok) continue;
          state = await response.json();
        } catch { continue; }
        if (state.applying || state.phase === 'installing') continue;
        if (state.error) throw new Error(state.error);
        if (state.result && (!version || state.result.version === version) && Date.parse(state.result.date) >= started) {
          if (state.result.success) { location.reload(); return; }
          throw new Error('La mise à jour a échoué. ' + state.result.message);
        }
      }
      throw new Error('Le redémarrage prend plus de temps que prévu. Relance Atelier avec son lanceur si Atelier reste indisponible.');
    } catch (error) { message.textContent = error.message; }
    finally { controls(false); }
  }
  installButton.onclick = async () => {
    if (!available || installing) return;
    const version = available.version;
    if (!(await confirm('Atelier va sauvegarder vos données, installer cette version puis redémarrer. Arrêtez les agents, les serveurs et les chronomètres avant de continuer.', 'Installer la version ' + version + ' ?', 'Mettre à jour')) || installing) return;
    await followInstallation(version, Date.now(), () => api('/api/updates/install', 'POST', { version }));
  };
  check();
  setInterval(() => { if (!document.hidden) check(); }, 6 * 60 * 60 * 1000);
}
