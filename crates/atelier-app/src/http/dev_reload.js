
;(() => {
  let edited = false;
  document.addEventListener('input', () => { edited = true; }, true);
  document.addEventListener('change', () => { edited = true; }, true);
  async function poll() {
    try {
      const response = await fetch('/', { cache: 'no-store' });
      if (response.ok) {
        const html = await response.text();
        if (html.includes('<!--atelier-dev:') &&
            !html.includes('<!--atelier-dev:__ATELIER_REVISION__-->')) {
          if (!edited || window.confirm('Nouvelle version prête. Actualiser la page ? Les saisies non enregistrées seront perdues.')) {
            window.location.reload();
          }
          return;
        }
      }
    } catch (_) {
      // The supervised server can be unavailable while Rust is compiling.
    }
    window.setTimeout(poll, 2000);
  }
  window.setTimeout(poll, 2000);
})();
