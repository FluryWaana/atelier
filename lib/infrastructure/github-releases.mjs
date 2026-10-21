import { githubRepository, releaseVersion, packageName } from '../domain/releases.mjs';

export function createGithubReleases(repository, request = fetch, platform = process.platform, arch = process.arch) {
  githubRepository(repository);
  const releasePrefix = `https://github.com/${repository}/releases/`;
  function assetUrl(value, filename, tag) {
    const url = new URL(value);
    if (url.origin !== 'https://github.com' || url.pathname !== `/${repository}/releases/download/${tag}/${filename}` || url.search || url.hash || url.username || url.password) throw new Error('Adresse du package GitHub invalide.');
    return url.href;
  }
  return {
    async latest() {
      const response = await request(`https://api.github.com/repos/${repository}/releases/latest`, { headers: { Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' }, redirect: 'error', signal: AbortSignal.timeout(15000) });
      if (response.status === 404) return null;
      if (!response.ok) throw new Error(`Recherche de mise à jour indisponible (GitHub ${response.status}). Réessaie plus tard.`);
      const release = await response.json();
      if (release.draft || release.prerelease) return null;
      const version = releaseVersion(release.tag_name), name = packageName(version, platform, arch);
      const archive = release.assets?.find(asset => asset.name === name), checksum = release.assets?.find(asset => asset.name === name + '.sha256'), signature = release.assets?.find(asset => asset.name === name + '.sha256.sig');
      if (!archive || !checksum || !signature) throw new Error('Cette publication ne contient pas le package de ce système, son empreinte et sa signature.');
      if (release.html_url !== releasePrefix + 'tag/' + release.tag_name) throw new Error('Lien des nouveautés invalide.');
      return { version, name, url: assetUrl(archive.browser_download_url, name, release.tag_name), checksumUrl: assetUrl(checksum.browser_download_url, name + '.sha256', release.tag_name), signatureUrl: assetUrl(signature.browser_download_url, name + '.sha256.sig', release.tag_name), notesUrl: release.html_url };
    },
  };
}
