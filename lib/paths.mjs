import { resolve } from 'node:path';
import { sameRepository } from './domain/paths.mjs';

export const samePath = (a, b) => sameRepository(a, b, process.platform);
export const sameLocation = (a, b) => samePath(resolve(a), resolve(b));
