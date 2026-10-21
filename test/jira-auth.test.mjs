import test from 'node:test';
import assert from 'node:assert/strict';
import { jiraRequest } from '../lib/jira.mjs';

const config = { jiraUrl: 'https://otami.atlassian.net/', jiraEmail: 'test@example.com', jiraType: 'cloud' };
const cloudId = '12345678-1234-1234-1234-123456789abc';
const json = (value, status = 200) => new Response(JSON.stringify(value), { status });

test('scoped token retries through the tenant gateway without exposing credentials to discovery', async () => {
  const calls = [];
  const request = async (url, options) => {
    calls.push({ url, options });
    return [json({}, 401), json({ cloudId }), json({ issues: [] })][calls.length - 1];
  };
  const body = { jql: 'project = TEST' };
  assert.deepEqual(await jiraRequest(config, 'test-token', '/rest/api/3/search/jql', body, request), { issues: [] });
  assert.equal(calls[1].url, 'https://otami.atlassian.net/_edge/tenant_info');
  assert.equal(calls[1].options.headers.Authorization, undefined);
  assert.equal(calls[2].url, `https://api.atlassian.com/ex/jira/${cloudId}/rest/api/3/search/jql`);
  assert.equal(calls[2].options.headers.Authorization, `Basic ${Buffer.from('test@example.com:test-token').toString('base64')}`);
  assert.equal(calls[2].options.body, JSON.stringify(body));
});

test('classic token succeeds directly', async () => {
  let count = 0;
  await jiraRequest(config, 'test-token', '/rest/api/3/myself', undefined, async () => {
    count++;
    return json({});
  });
  assert.equal(count, 1);
});

test('Data Center never uses the Cloud gateway', async () => {
  let count = 0;
  await assert.rejects(jiraRequest({ ...config, jiraType: 'server' }, 'test-token', '/rest/api/2/myself', undefined, async (_, options) => {
    count++;
    assert.equal(options.headers.Authorization, 'Bearer test-token');
    return json({}, 401);
  }), /401/);
  assert.equal(count, 1);
});

test('invalid tenant identifiers cannot redirect credentials', async () => {
  let count = 0;
  await assert.rejects(jiraRequest(config, 'test-token', '/rest/api/3/myself', undefined, async () => {
    count++;
    return count === 1 ? json({}, 401) : json({ cloudId: '../other-tenant' });
  }), /401/);
  assert.equal(count, 2);
});
