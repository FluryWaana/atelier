import { closeSync, constants, fstatSync, openSync, readSync } from 'node:fs';

export function readCheckedFile(path, expected, limit) {
  // codeql[js/file-system-race] The opened descriptor must match the checked inode before any bounded read.
  const fd = openSync(path, constants.O_RDONLY | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0));
  try {
    const stat = fstatSync(fd, { bigint: true });
    if (!stat.isFile() || stat.dev !== expected.dev || stat.ino !== expected.ino || stat.nlink !== expected.nlink) throw new Error('Le fichier a changé pendant sa lecture.');
    if (stat.size > limit) throw new Error('Fichier trop volumineux.');
    const buffer = Buffer.alloc(limit + 1);
    let size = 0, count;
    while (size < buffer.length && (count = readSync(fd, buffer, size, buffer.length - size, null))) size += count;
    if (size > limit) throw new Error('Fichier trop volumineux.');
    return buffer.subarray(0, size);
  } finally { closeSync(fd); }
}
