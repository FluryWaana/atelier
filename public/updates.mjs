import { messageText, msg } from './i18n.mjs';
import { localFetch } from './local-session.mjs';
export function initUpdates({ api, confirm }) {
  const banner = document.querySelector('#update-banner'), message = document.querySelector('#update-message');
  const checkButton = document.querySelector('#check-updates'), installButton = document.querySelector('#install-update'), notes = document.querySelector('#update-notes');
  const dismissButton = document.querySelector('#dismiss-update');
  const progress = document.querySelector('#update-progress');
  let available = null, installing = false, notice = '', dismissed = '';
  function controls(busy) {
    installing = busy; installButton.disabled = busy; checkButton.disabled = busy; dismissButton.disabled = busy;
    progress.hidden = !busy;
  }
  function showProgress(state) {
    progress.removeAttribute('value');
    if (state?.stage === 'download') {
      message.textContent = msg("Téléchargement et vérification de la mise à jour…");
      if (state.total > 0) {
        progress.value = Math.min(100, Math.floor(state.received / state.total * 100));
        message.textContent = msg("Téléchargement de la mise à jour : {0} %", progress.value);
      }
    } else {
      const messages = { verify: msg("Vérification de la mise à jour…"), extract: msg("Extraction de la mise à jour…"), backup: msg("Sauvegarde des données…") };
      message.textContent = messages[state?.stage] || msg("Installation et redémarrage en cours…");
    }
  }
  function render(state, manual) {
    available = state.available;
    notice = JSON.stringify([available?.version, state.error, state.result?.date]);
    banner.hidden = !manual && ((!available && !state.result && !state.error) || notice === dismissed);
    message.textContent = !state.configured ? msg("Les mises à jour seront disponibles dans la version distribuée depuis GitHub.") : messageText(state.error) || (available ? msg("La version {0} est disponible.", available.version) : msg("Atelier {0} est à jour.", state.currentVersion));
    if (!state.error && !available && state.result) message.textContent = state.result.success ? messageText(state.result.message) : msg("La dernière mise à jour a échoué : ") + messageText(state.result.message);
    installButton.hidden = !available || !state.canInstall;
    notes.hidden = !available;
    if (available) { notes.href = available.notesUrl; if (!state.canInstall) message.textContent += msg(" Télécharge le package de ton système depuis les nouveautés."); }
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
    catch (error) { if (manual && !installing) { banner.hidden = false; message.textContent = messageText(error.message); } }
    finally { checkButton.disabled = installing; }
  }
  dismissButton.onclick = () => { if (!installing) { dismissed = notice; banner.hidden = true; checkButton.focus(); } };
  checkButton.onclick = () => check(true);
  async function followInstallation(version, started = 0, begin) {
    controls(true); banner.hidden = false;
    showProgress({ stage: begin ? 'download' : 'restart' });
    let failure;
    // Poll while the installation request is still downloading the package.
    if (begin) Promise.resolve().then(begin).catch(error => {
      if (!(error instanceof TypeError) && !(error instanceof SyntaxError)) failure = error;
    });
    try {
      for (let i = 0; i < 600; i++) {
        await new Promise(resolve => setTimeout(resolve, 1000));
        if (failure) throw failure;
        let state;
        try {
          const response = await localFetch('/api/updates', { cache: 'no-store', signal: AbortSignal.timeout(2000) });
          if (response.status === 401) { message.textContent = msg('Session expirée ou absente. Rouvre Atelier avec son lanceur.'); return; }
          if (!response.ok) continue;
          state = await response.json();
        } catch { showProgress({ stage: 'restart' }); continue; }
        if (state.applying || state.phase === 'installing') { showProgress(state.progress); continue; }
        if (state.error) throw new Error(state.error);
        if (state.result && (!version || state.result.version === version) && Date.parse(state.result.date) >= started) {
          if (state.result.success) { progress.value = 100; location.reload(); return; }
          throw new Error(msg("La mise à jour a échoué. ") + messageText(state.result.message));
        }
      }
      throw new Error(msg("Le redémarrage prend plus de temps que prévu. Relance Atelier avec son lanceur si Atelier reste indisponible."));
    } catch (error) { message.textContent = messageText(error.message); }
    finally { controls(false); }
  }
  installButton.onclick = async () => {
    if (!available || installing) return;
    const version = available.version;
    if (!(await confirm(msg("Atelier va sauvegarder vos données, installer cette version puis redémarrer. Arrêtez les agents, les serveurs et les chronomètres avant de continuer."), msg("Installer la version ") + version + ' ?', msg("Mettre à jour"))) || installing) return;
    await followInstallation(version, Date.now(), () => api('/api/updates/install', 'POST', { version }));
  };
  check();
  setInterval(() => { if (!document.hidden) check(); }, 6 * 60 * 60 * 1000);
}
