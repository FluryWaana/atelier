import { open, realpath } from 'node:fs/promises';
import { constants } from 'node:fs';
import { dirname, isAbsolute, join } from 'node:path';

// Both account readers refuse special files, network paths and links outside the selected profile.
export async function readAgentCredentials(directory, name) {
  if (!isAbsolute(directory) || /^[\\/]{2}/.test(directory)) return null;
  const root = await realpath(directory);
  if (/^[\\/]{2}/.test(root)) return null;
  const path = await realpath(join(root, name));
  if (dirname(path) !== root || /^[\\/]{2}/.test(path)) return null;
  const file = await open(path, constants.O_RDONLY | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0));
  try {
    const stat = await file.stat();
    if (!stat.isFile() || stat.size > 65536) return null;
    const buffer = Buffer.alloc(65537);
    const { bytesRead } = await file.read(buffer, 0, buffer.length, 0);
    if (bytesRead > 65536) return null;
    return JSON.parse(buffer.toString('utf8', 0, bytesRead));
  } finally { await file.close(); }
}
