import { request } from 'node:http';

// The bundled fetch client can crash on macOS when a local server closes its socket (undici#5544).
export async function localRequest(url, { method = 'GET', headers = {}, body, signal = AbortSignal.timeout(5000) } = {}) {
  const address = new URL(url);
  if (address.protocol !== 'http:' || address.hostname !== '127.0.0.1' || address.username || address.password || address.hash) throw new Error('Adresse du serveur local invalide.');
  return new Promise((resolve, reject) => {
    const req = request(address, { method, headers, signal, agent: false }, res => {
      res.once('error', reject);
      if (res.statusCode >= 300 && res.statusCode < 400) {
        res.destroy(new Error('Redirection du serveur local refusée.'));
        return;
      }
      const chunks = [];
      let bytes = 0;
      res.on('data', chunk => {
        bytes += chunk.length;
        if (bytes > 65536) res.destroy(new Error('Réponse du serveur local trop volumineuse.'));
        else chunks.push(chunk);
      });
      res.once('end', () => resolve({
        ok: res.statusCode >= 200 && res.statusCode < 300,
        status: res.statusCode,
        async json() { return JSON.parse(Buffer.concat(chunks).toString('utf8')); },
      }));
    });
    req.once('error', reject);
    req.end(body);
  });
}
