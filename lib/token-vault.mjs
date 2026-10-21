import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { createCipheriv, createDecipheriv, randomBytes } from 'node:crypto';

export function windowsProtect(value, decrypt = false) {
  if (process.platform !== 'win32') throw new Error('Le coffre de jetons nécessite Windows.');
  const script = [
    "$ErrorActionPreference = 'Stop'",
    // Load only the cmdlets: the module manifest can conflict with existing type data.
    "Import-Module ([System.Reflection.Assembly]::LoadWithPartialName('Microsoft.PowerShell.Security').Location)",
    '[Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)',
    '[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)',
    '$value = [Console]::In.ReadToEnd()',
    decrypt
      ? '$secure = ConvertTo-SecureString -String $value; [Console]::Write(([System.Net.NetworkCredential]::new("", $secure)).Password)'
      : '$secure = ConvertTo-SecureString -String $value -AsPlainText -Force; [Console]::Write((ConvertFrom-SecureString -SecureString $secure))',
  ].join('; ');
  const result = spawnSync(join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe'),
    ['-NoProfile', '-NonInteractive', '-Command', script],
    { input: value, encoding: 'utf8', windowsHide: true, timeout: 15000, maxBuffer: 1024 * 1024 });
  if (result.error || result.status !== 0 || !result.stdout) {
    throw new Error('Impossible d’accéder au coffre Windows des jetons.', { cause: result.error });
  }
  return result.stdout;
}

export function systemProtect({ platform = process.platform, execute = spawnSync } = {}) {
  if (platform === 'win32') return windowsProtect;
  if (!['darwin', 'linux'].includes(platform)) return () => { throw new Error('Coffre système indisponible.'); };
  function run(command, args, input) {
    const result = execute(command, args, { input, encoding: 'utf8', timeout: 15000, maxBuffer: 65536 });
    if (result.error) throw new Error('Coffre système inaccessible. Sur Linux, installez secret-tool (libsecret) et déverrouillez le trousseau.', { cause: result.error });
    return result;
  }
  function key(create) {
    const result = platform === 'darwin'
      ? run('security', ['find-generic-password', '-s', 'atelier-kanban', '-a', 'token-key', '-w'])
      : run('secret-tool', ['lookup', 'application', 'atelier-kanban', 'purpose', 'token-key']);
    let value = result.stdout?.trim();
    if (result.status !== 0) {
      if (!create || result.status !== (platform === 'darwin' ? 44 : 1)) throw new Error('Déverrouillez le trousseau système pour accéder aux jetons.');
      value = randomBytes(32).toString('hex');
      // Pass the key on stdin so it never appears in the process command line.
      const saved = platform === 'darwin'
        ? run('security', ['-i'], `add-generic-password -s atelier-kanban -a token-key -w ${value}\n`)
        : run('secret-tool', ['store', '--label=Atelier Kanban', 'application', 'atelier-kanban', 'purpose', 'token-key'], value);
      if (saved.status !== 0) throw new Error('Impossible d’enregistrer la clé dans le trousseau système.');
      return key(false);
    }
    if (!/^[a-f0-9]{64}$/.test(value || '')) throw new Error('Clé du trousseau système invalide.');
    return Buffer.from(value, 'hex');
  }
  return (value, decrypt = false) => {
    if (decrypt) {
      const envelope = JSON.parse(value);
      if (envelope.format !== 'keyring-aes-256-gcm-v1' || envelope.platform !== platform) throw new Error('Ce coffre appartient à un autre système. Renseignez de nouveau les jetons.');
      const cipher = createDecipheriv('aes-256-gcm', key(false), Buffer.from(envelope.iv, 'hex'));
      cipher.setAuthTag(Buffer.from(envelope.tag, 'hex'));
      return Buffer.concat([cipher.update(Buffer.from(envelope.data, 'base64')), cipher.final()]).toString('utf8');
    }
    const iv = randomBytes(12), cipher = createCipheriv('aes-256-gcm', key(true), iv);
    const data = Buffer.concat([cipher.update(value, 'utf8'), cipher.final()]);
    return JSON.stringify({ format: 'keyring-aes-256-gcm-v1', platform, iv: iv.toString('hex'), tag: cipher.getAuthTag().toString('hex'), data: data.toString('base64') });
  };
}

export function openTokenVault(db, protect = systemProtect()) {
  db.exec('CREATE TABLE IF NOT EXISTS token_vault (id INTEGER PRIMARY KEY CHECK(id=1), encrypted TEXT NOT NULL)');
  return {
    load() {
      const row = db.prepare('SELECT encrypted FROM token_vault WHERE id=1').get();
      return row ? JSON.parse(protect(row.encrypted, true)) : {};
    },
    save(tokens) {
      if (Object.values(tokens).every(value => !value)) { db.exec('DELETE FROM token_vault'); return; }
      const encrypted = protect(JSON.stringify(tokens));
      db.prepare('INSERT INTO token_vault VALUES (1, ?) ON CONFLICT(id) DO UPDATE SET encrypted=excluded.encrypted').run(encrypted);
    },
  };
}
