import test from 'node:test';
import assert from 'node:assert/strict';
import { createSettingsView } from '../public/views/settings.mjs';

test('settings expose providers used by projects even when a different global provider is selected', async () => {
  const host = { innerHTML: '', querySelector: () => ({}), querySelectorAll: () => [] }, navigation = {};
  const document = { querySelector: selector => selector === '#connector-settings' ? host : navigation, querySelectorAll: () => [] };
  const modules = ['azure', 'github', 'gitlab'].map(id => ({ id, name: id, description: '', capabilities: ['repositories'], fields: [], tabs: [], values: {}, secrets: {} }));
  const catalog = { modules, providers: { repositories: 'azure' }, projectRepositoryProviders: ['github'] };
  const view = createSettingsView({ document, api: async () => catalog });
  await view.open();
  assert.match(navigation.innerHTML, /data-settings-tab="github"/);
  assert.match(navigation.innerHTML, /Dépôts Git · github/);
  assert.doesNotMatch(navigation.innerHTML, /data-settings-tab="gitlab"/);
  assert.match(host.innerHTML, /data-connector-form="github"/);
  catalog.projectRepositoryProviders = [];
  await view.open();
  assert.doesNotMatch(navigation.innerHTML, /data-settings-tab="github"/);
  catalog.pullRequestProviders = ['github'];
  catalog.errors = [{ id: '<broken>', message: '<script>alert(1)</script>' }];
  await view.open();
  assert.match(navigation.innerHTML, /data-settings-tab="github"/);
  assert.match(host.innerHTML, /data-connector-form="github"/);
  assert.match(host.innerHTML, /Modules non chargés/);
  assert.match(host.innerHTML, /&lt;broken&gt;/);
  assert.match(host.innerHTML, /&lt;script&gt;/);
  assert.doesNotMatch(host.innerHTML, /<script>/);
  assert.equal(catalog.providers.repositories, 'azure');
});
