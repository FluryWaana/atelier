import { effortLevels as sourceLevels } from '../lib/domain/agent-options.mjs';
import { msg } from './i18n.mjs';
export { agentOptions, resolveAgentOptions } from '../lib/domain/agent-options.mjs';
export const effortLevels = Object.fromEntries(Object.entries(sourceLevels).map(([agent, levels]) => [agent,
  Object.fromEntries(Object.entries(levels).map(([value, label]) => [value, msg(label)])),
]));
