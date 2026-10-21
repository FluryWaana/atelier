import { isNewer } from '../domain/releases.mjs';

export function createUpdates({ version, repository, releases, installer, assertIdle, backup, changed = () => {}, now = Date.now }) {
  let status = { configured: Boolean(repository), currentVersion: version, canInstall: Boolean(installer), phase: 'idle', available: null, checkedAt: null, error: '' };
  let checking = null;
  const state = () => ({ ...status });
  const save = fields => { status = { ...status, ...fields }; changed(); return state(); };
  async function check() {
    if (!repository || status.phase === 'installing') return state();
    if (checking) return checking;
    // GitHub's anonymous API quota is shared by all installations behind the same public IP.
    if (status.checkedAt && now() - Date.parse(status.checkedAt) < 60000) return state();
    checking = (async () => {
      try {
        const release = await releases.latest();
        return save({ available: release && isNewer(release.version, version) ? release : null, checkedAt: new Date(now()).toISOString(), error: '' });
      } catch (error) { return save({ error: error.message, checkedAt: new Date(now()).toISOString() }); }
      finally { checking = null; }
    })();
    return checking;
  }
  async function install(requestedVersion) {
    if (!installer || !repository) throw new Error('Installation automatique disponible dans le package publié pour ce système.');
    if (status.phase === 'installing') throw Object.assign(new Error('Mise à jour déjà en cours.'), { status: 409 });
    assertIdle();
    if (!status.available || requestedVersion !== status.available.version) throw new Error('Vérifie les mises à jour avant de choisir la version à installer.');
    const release = status.available;
    save({ phase: 'installing', error: '' });
    try {
      const staged = await installer.prepare(release);
      assertIdle();
      const rescue = backup();
      await installer.start(staged, rescue);
      return state();
    } catch (error) { save({ phase: 'idle', error: error.message }); throw error; }
  }
  return { state, check, install, busy: () => status.phase === 'installing' };
}
