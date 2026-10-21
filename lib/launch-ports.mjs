import { createServer } from 'node:net';

export async function reservePort(port = 0) {
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen({ port, host: '127.0.0.1', exclusive: true }, resolve);
  }).catch(() => { throw new Error(port ? `Le port ${port} est indisponible. Choisissez un autre port ou laissez Atelier le choisir.` : 'Impossible de réserver un port local.'); });
  return { port: server.address().port, release: () => new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve())) };
}
