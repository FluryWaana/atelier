import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { isNewer, releaseVersion, githubRepository } from '../lib/domain/releases.mjs';
import { createUpdates } from '../lib/application/updates.mjs';
import { createGithubReleases } from '../lib/infrastructure/github-releases.mjs';
import { verifyChecksum } from '../lib/infrastructure/windows-updater.mjs';
import { download } from '../lib/infrastructure/package-updater.mjs';

test('stable versions compare numerically and never downgrade', () => {
  assert.equal(isNewer('v0.10.0', '0.9.9'), true);
  for (const value of ['0.9.9', '0.1.0']) assert.equal(isNewer(value, '0.9.9'), false);
  for (const value of ['1.0', '1.0.0-beta.1', '01.0.0', '99999999999999999999.0.0']) assert.throws(() => releaseVersion(value));
  assert.equal(githubRepository('owner/atelier-kanban'), 'owner/atelier-kanban');
  for (const value of ['', 'https://github.com/a/b', 'a/../b', 'a/..', 'a/b?token=x']) assert.throws(() => githubRepository(value));
});

test('GitHub releases require both assets from the configured repository', async () => {
  const repository = 'owner/atelier', name = 'atelier-kanban-1.2.3-windows-x64.zip';
  const release = { tag_name: 'v1.2.3', html_url: `https://github.com/${repository}/releases/tag/v1.2.3`, assets: [name, name + '.sha256', name + '.sha256.sig'].map(name => ({ name, browser_download_url: `https://github.com/${repository}/releases/download/v1.2.3/${name}` })) };
  let status = 200;
  const service = createGithubReleases(repository, async (url, options) => {
    assert.equal(url, `https://api.github.com/repos/${repository}/releases/latest`);
    assert.equal(options.redirect, 'error');
    return { status, ok: status === 200, json: async () => release };
  }, 'win32', 'x64');
  assert.equal((await service.latest()).version, '1.2.3');
  release.assets[0].browser_download_url = `https://github.com/${repository}/releases/download/v1.2.2/${name}`;
  await assert.rejects(service.latest(), /Adresse/);
  release.assets[0].browser_download_url = `https://github.com/${repository}/releases/download/v1.2.3/${name}`;
  release.html_url += '/../../other';
  await assert.rejects(service.latest(), /nouveautés/);
  release.html_url = `https://github.com/${repository}/releases/tag/v1.2.3`;
  release.assets[0].browser_download_url = 'https://attacker.example/app.zip';
  await assert.rejects(service.latest(), /Adresse/);
  release.assets = []; await assert.rejects(service.latest(), /empreinte/);
  status = 404; assert.equal(await service.latest(), null);
  status = 403; await assert.rejects(service.latest(), /GitHub 403/);
});

test('downloads follow bounded HTTPS GitHub redirects and share one timeout', async () => {
  const calls = [];
  const bytes = await download('https://github.com/owner/repo/releases/download/v1.0.0/app.zip', 10, async (url, options) => {
    calls.push({ url, options });
    return calls.length === 1 ? new Response(null, { status: 302, headers: { location: 'https://release-assets.githubusercontent.com/app?signature=example' } }) : new Response('archive');
  });
  assert.equal(bytes.toString(), 'archive');
  assert.equal(calls.length, 2);
  assert.equal(calls[1].options.redirect, 'manual');
  assert.equal(calls[0].options.signal, calls[1].options.signal);
});

test('unsafe redirect targets are rejected before they receive a request', async () => {
  for (const location of ['http://github.com/app', 'https://127.0.0.1/app', 'https://github.com.evil.test/app', 'https://user:secret@github.com/app', 'https://github.com:8443/app']) {
    let calls = 0;
    await assert.rejects(download('https://github.com/app', 10, async () => {
      calls++;
      return new Response(null, { status: 302, headers: { location } });
    }), /non autorisée/);
    assert.equal(calls, 1);
  }
});

test('download progress reports received bytes with or without a content length', async () => {
  for (const total of [6, null]) {
    const progress = [];
    const request = async () => new Response(new ReadableStream({ start(controller) {
      controller.enqueue(Buffer.from('ab')); controller.enqueue(Buffer.from('cdef')); controller.close();
    } }), { headers: total ? { 'content-length': String(total) } : {} });
    assert.equal((await download('https://github.com/app', 10, request, value => progress.push(value))).toString(), 'abcdef');
    assert.deepEqual(progress, [0, 2, 6].map(received => ({ stage: 'download', received, total })));
  }
});

test('downloads reject redirect loops, HTTP errors and oversized bodies', async () => {
  let calls = 0;
  await assert.rejects(download('https://github.com/app', 10, async () => {
    calls++; return new Response(null, { status: 302, headers: { location: '/app' } });
  }), /Redirection/);
  assert.equal(calls, 6);
  for (const response of [new Response('unavailable', { status: 503 }), new Response('too long'), new Response('x', { headers: { 'content-length': '100' } })]) {
    await assert.rejects(download('https://github.com/app', 3, async () => response), /impossible|volumineux/);
  }
});

test('downloads reject corrupt archives and checksums for another file', () => {
  const bytes = Buffer.from('a release'), name = 'app.zip';
  const checksum = createHash('sha256').update(bytes).digest('hex') + '  ' + name;
  verifyChecksum(bytes, checksum, name);
  assert.throws(() => verifyChecksum(Buffer.from('changed'), checksum, name));
  assert.throws(() => verifyChecksum(bytes, checksum, 'other.zip'));
});

function fixture(overrides = {}) {
  const calls = [];
  const service = createUpdates({ version: '1.0.0', repository: 'owner/repo', now: () => 100000,
    releases: { latest: async () => { calls.push('check'); return { version: '1.1.0' }; } },
    installer: { prepare: async () => { calls.push('prepare'); return 'stage'; }, start: async (stage, backup) => { assert.equal(stage, 'stage'); assert.equal(backup, 'backup'); calls.push('start'); } },
    assertIdle: () => calls.push('idle'), backup: () => { calls.push('backup'); return 'backup'; }, ...overrides });
  return { service, calls };
}
test('update orchestration stages, rechecks safety, backs up, then hands over', async () => {
  const { service, calls } = fixture();
  await service.check(); await service.check();
  assert.equal(calls.filter(x => x === 'check').length, 1);
  await service.install('1.1.0');
  assert.deepEqual(calls, ['check', 'idle', 'prepare', 'idle', 'backup', 'start']);
  assert.equal(service.busy(), true);
  await assert.rejects(service.install('1.1.0'), /déjà/);
});
test('failed preparation never backs up or starts the installer and releases the UI lock', async () => {
  const { service, calls } = fixture({ installer: { prepare: async () => { throw new Error('corrupt'); } } });
  await service.check(); await assert.rejects(service.install('1.1.0'), /corrupt/);
  assert.equal(service.busy(), false); assert.equal(calls.includes('backup'), false);
  assert.equal(service.state().progress, null);
});

test('update state exposes preparation progress before the install request completes', async () => {
  const stages = [];
  const { service } = fixture({
    changed: () => { if (service.state().progress) stages.push(service.state().progress.stage); },
    installer: {
      prepare: async (release, progress) => {
        progress({ stage: 'download', received: 50, total: 100 });
        assert.equal(service.state().progress.received, 50);
        progress({ stage: 'verify' });
        assert.equal(service.state().progress.stage, 'verify');
        progress({ stage: 'extract' });
        assert.equal(service.state().progress.stage, 'extract');
        assert.equal(service.busy(), true);
      },
      start: async () => assert.equal(service.state().progress.stage, 'restart'),
    },
  });
  await service.check(); await service.install('1.1.0');
  assert.deepEqual(stages, ['download', 'backup', 'restart']);
});
test('busy operations and stale selections prevent installation', async () => {
  const { service, calls } = fixture({ assertIdle: () => { throw new Error('busy'); } });
  await service.check(); await assert.rejects(service.install('1.1.0'), /busy/);
  assert.equal(calls.includes('prepare'), false);
  const other = fixture(); await other.service.check();
  await assert.rejects(other.service.install('1.2.0'), /Vérifie/);
});
test('GitHub outages leave the app usable and unconfigured copies do not contact GitHub', async () => {
  const { service } = fixture({ releases: { latest: async () => { throw new Error('offline'); } } });
  assert.equal((await service.check()).error, 'offline'); assert.equal(service.busy(), false);
  const disabled = fixture({ repository: '' });
  assert.equal((await disabled.service.check()).configured, false); assert.deepEqual(disabled.calls, []);
});
