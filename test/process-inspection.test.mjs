import test from 'node:test';
import assert from 'node:assert/strict';
import { descendantPids, inspectProcesses, listenerUrl, tcpListeners } from '../lib/process-inspection.mjs';

test('Windows inspection joins CIM ancestry and netstat sockets and propagates failures', async () => {
  const snapshot = await inspectProcesses('win32', async command => ({ stdout: command === 'powershell.exe'
    ? '{"ProcessId":123,"ParentProcessId":42}'
    : '  TCP  127.0.0.1:4317  0.0.0.0:0  LISTENING  123\n' }));
  assert.deepEqual(descendantPids(42, snapshot.processes), [42, 123]);
  assert.deepEqual(snapshot.listeners, [{ pid: 123, address: '127.0.0.1', port: 4317 }]);
  await assert.rejects(inspectProcesses('win32', async () => { throw new Error('inspection timeout'); }), /inspection timeout/);
});

test('TCP inspection accepts listening IPv4/IPv6 sockets and rejects unrelated connections', () => {
  assert.deepEqual(tcpListeners(`
  TCP    0.0.0.0:5173       0.0.0.0:0      LISTENING    123
  TCP    [::1]:5080        [::]:0         LISTENING    456
  TCP    0.0.0.0:5173       0.0.0.0:0      LISTENING    123
  TCP    127.0.0.1:8000    127.0.0.1:9000 ESTABLISHED  123
  UDP    127.0.0.1:9000    *:*                         123
  TCP    invalid:8000      0.0.0.0:0      LISTENING    123
  TCP    127.0.0.1:99999    0.0.0.0:0      LISTENING    123
  TCP    127.0.0.1:8000    0.0.0.0:0      LISTENING    0
`), [{ pid: 123, address: '0.0.0.0', port: 5173 }, { pid: 456, address: '::1', port: 5080 }]);
});

test('process ancestry includes npm/dotnet descendants but excludes siblings and other applications', () => {
  assert.deepEqual(descendantPids(10, [
    { ProcessId: 30, ParentProcessId: 20 }, { ProcessId: 40, ParentProcessId: 1 },
    { ProcessId: 20, ParentProcessId: 10 }, { ProcessId: 10, ParentProcessId: 1 },
  ]).sort((a, b) => a - b), [10, 20, 30]);
});

test('web links replace wildcard addresses and use HTTPS only when announced for that socket', () => {
  const listener = { address: '0.0.0.0', port: 5173 };
  assert.equal(listenerUrl(listener), 'http://127.0.0.1:5173/');
  assert.equal(listenerUrl({ address: '::', port: 5080 }), 'http://[::1]:5080/');
  assert.equal(listenerUrl(listener, '\u001b[32mhttps://localhost:5173/\u001b[0m'), 'https://127.0.0.1:5173/');
  assert.equal(listenerUrl(listener, 'http://localhost:5173/app/'), 'http://127.0.0.1:5173/app/');
  assert.equal(listenerUrl(listener, 'https://example.com:5173/ https://localhost:9000/'), 'http://127.0.0.1:5173/');
  assert.equal(listenerUrl(listener, 'https://secret:token@localhost:5173/'), 'http://127.0.0.1:5173/');
});
