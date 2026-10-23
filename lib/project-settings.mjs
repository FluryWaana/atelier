import { samePath } from './paths.mjs';
import { win32 } from 'node:path';
import { agentProvider } from './domain/agent-options.mjs';

const statuses = ['todo', 'doing', 'review', 'done', 'standby'];
export function projectSettings(body, settings = {}) {
  const text = (value, max) => { if (typeof value !== 'string' || value.length > max) throw new Error('Réglage de projet invalide.'); return value.trim(); };
  const directory = value => {
    const path = text(value ?? '', 2000), normalized = win32.normalize(path);
    if (win32.parse(path).root || normalized === '..' || normalized.startsWith('..\\')) throw new Error('Le dossier du service doit être relatif et rester dans le worktree.');
    return path;
  };
  const port = value => {
    if (value === undefined || value === '') return 0;
    const number = typeof value === 'string' && /^\d+$/.test(value) ? Number(value) : value;
    if (!Number.isInteger(number) || number < 0 || number > 65535) throw new Error('Port invalide : indique un nombre de 1 à 65535, ou laisse vide pour un port automatique.');
    return number;
  };
  const agent = body.agent || '', baseBranch = text(body.baseBranch ?? '', 180), jiraMode = body.jiraMode || 'inherit';
  const repositoryConnector = text(body.repositoryConnector ?? '', 64);
  if (repositoryConnector && !/^[a-z][a-z0-9-]{0,63}$/.test(repositoryConnector)) throw new Error('Fournisseur Git invalide.');
  if (agent) agentProvider(settings, agent);
  if (!['inherit', 'off', 'custom'].includes(jiraMode) || baseBranch.startsWith('-')) throw new Error('Réglage de projet invalide.');
  const mapping = body.jiraStatusMap ?? {};
  if (!mapping || typeof mapping !== 'object' || Array.isArray(mapping) || Object.keys(mapping).some(key => !statuses.includes(key))) throw new Error('Correspondance Jira invalide.');
  const jiraStatusMap = Object.fromEntries(statuses.map(key => [key, text(mapping[key] ?? '', 100)]));
  if (jiraMode === 'custom' && !Object.values(jiraStatusMap).some(Boolean)) throw new Error('Renseigne au moins un statut Jira.');
  for (const key of ['automationEnabled', 'autoFix']) if (body[key] !== undefined && typeof body[key] !== 'boolean') throw new Error('Option automatique invalide.');
  const prTarget = text(body.prTarget ?? '', 180), testCommand = text(body.testCommand ?? '', 2000);
  for (const role of ['front', 'back']) if (port(body[role + 'Port']) && !text(body[role + 'Command'] ?? '', 2000).includes('{port}')) throw new Error('Ajoute {port} dans la commande pour appliquer le port fourni par Atelier.');
  if (prTarget.startsWith('-') || body.automationEnabled && (!prTarget || !testCommand)) throw new Error('Renseigne la branche cible et la commande de vérification avant d’activer le mode automatique.');
  return { projectRules: text(body.projectRules ?? '', 12000), agent, baseBranch, repositoryConnector, jiraMode, jiraStatusMap, frontCommand: text(body.frontCommand ?? '', 2000), backCommand: text(body.backCommand ?? '', 2000), frontRepo: text(body.frontRepo ?? '', 2000), backRepo: text(body.backRepo ?? '', 2000), frontDirectory: directory(body.frontDirectory), backDirectory: directory(body.backDirectory), frontPort: port(body.frontPort), backPort: port(body.backPort), linkedProjectId: text(body.linkedProjectId ?? '', 100), automationEnabled: body.automationEnabled ?? false, autoFix: body.autoFix ?? true, prTarget, testCommand };
}
export function settingsForProject(settings, projects, repo) {
  const project = projects.find(p => samePath(p.repo, repo || settings.repo));
  return { ...settings, projectRules: repo ? project?.projectRules || '' : '', repositoryConnector: project?.repositoryConnector || '', automationEnabled: Boolean(project?.automationEnabled), autoFix: project?.autoFix ?? true, testCommand: project?.testCommand || '', prTarget: project?.prTarget || '', ...(project?.baseBranch ? { baseBranch: project.baseBranch } : {}), agent: project?.agent || settings.agent || 'codex', ...(project?.jiraMode === 'off' ? { jiraAutoTransition: false } : project?.jiraMode === 'custom' ? { jiraAutoTransition: true, jiraStatusMap: project.jiraStatusMap } : {}) };
}
