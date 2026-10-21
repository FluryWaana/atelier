import { setTimeout as delay } from 'node:timers/promises';
import { createAutomation as createUseCases } from './application/automation.mjs';
export function createAutomation(options) { return createUseCases({ delay, ...options }); }
