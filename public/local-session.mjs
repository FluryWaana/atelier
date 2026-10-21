let token = '';
if (globalThis.location) {
  const fragment = new URLSearchParams(location.hash.slice(1));
  const supplied = fragment.get('session');
  if (supplied && /^[a-f0-9]{64}$/.test(supplied)) {
    token = supplied;
    fragment.delete('session');
    history.replaceState(null, '', location.pathname + location.search + (fragment.size ? '#' + fragment : ''));
  }
  if (!token) { try { token = localStorage.getItem('atelier-session') || ''; } catch {} }
  if (!token) { try { token = sessionStorage.getItem('atelier-session') || ''; } catch {} }
  if (token) {
    try { localStorage.setItem('atelier-session', token); } catch {}
    try { sessionStorage.setItem('atelier-session', token); } catch {}
  }
}

export function localFetch(path, options = {}) {
  return fetch(path, { ...options, headers: { ...options.headers, Authorization: 'Bearer ' + token } });
}

export const eventsUrl = () => '/api/events?session=' + encodeURIComponent(token);
