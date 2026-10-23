import { acquireDataLock } from '../../lib/infrastructure/data-lock.mjs';
import { createServer } from 'node:http';

const [mode, directory] = process.argv.slice(2);
let lock;
try { lock = acquireDataLock(directory); }
catch { process.stdout.write('blocked\n'); process.exit(2); }
if (mode === 'try') { lock.close(); process.stdout.write('acquired\n'); }
else if (mode === 'hold') {
  const server = createServer((_request, response) => response.end('synthetic'));
  server.listen(0, '127.0.0.1', () => process.stdout.write('ready\n'));
  process.stdin.resume();
  process.stdin.once('end', () => server.close(() => lock.close()));
} else { lock.close(); throw new Error('Unknown lock fixture mode.'); }
