import test from 'node:test';
import { verifyProjectDetection } from './fixtures/rust-project-detection-reference.mjs';

test('project detection preserves draft selection, role filters, scan caching and error priority', verifyProjectDetection);
