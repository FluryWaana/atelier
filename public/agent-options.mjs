import { effortLevels as sourceLevels, agentProviders } from '../lib/domain/agent-options.mjs';
import { msg } from './i18n.mjs';
import { esc } from './view-helpers.mjs';
export { agentOptions, resolveAgentOptions, agentProviders, agentProvider } from '../lib/domain/agent-options.mjs';
export const effortLevels = Object.fromEntries(Object.entries(sourceLevels).map(([agent, levels]) => [agent,
  Object.fromEntries(Object.entries(levels).map(([value, label]) => [value, msg(label)])),
]));

export function agentName(settings, agent, run) {
  return run?.agentName || agentProviders(settings).find(provider => provider.id === agent)?.name || agent;
}

export function agentSelect(settings, selected) {
  return agentProviders(settings).map(provider => `<option value="${esc(provider.id)}" ${selected === provider.id ? 'selected' : ''}>${esc(provider.name)}</option>`).join('');
}

export function fillAgentSelect(select, settings) {
  const selected = select.value, inherit = [...select.options].find(option => option.value === '');
  select.innerHTML = (inherit ? `<option value="">${esc(inherit.textContent)}</option>` : '') + agentSelect(settings, selected);
}

export function agentStatusText(status) {
  if (!status) return msg('Détection en cours…');
  if (status.manualCheckRequired) return msg('Exécutable personnalisé : vérification manuelle requise.');
  return [status.installed ? status.version : msg('Non installé'), status.connected ? msg('Connecté') + (status.authMethod ? ' (' + status.authMethod + ')' : '') : status.configured ? msg('Clé API configurée, validité non vérifiée') : msg('Connexion non vérifiée'), status.account || msg('Compte non communiqué par le CLI'),
    status.updateState === 'outdated' ? msg('Mise à jour disponible : {0}', status.latestVersion) : status.updateState === 'current' ? msg('À jour') : status.updateState === 'ahead' ? msg('Version en avance sur la version stable') : msg('Vérification de mise à jour indisponible')].join(' · ');
}

export function agentConnection(status = {}) {
  return status.manualCheckRequired ? msg('À vérifier') : status.connected ? msg('Connecté') : status.configured ? msg('Clé API configurée') : status.installed === false ? msg('Non installé') : msg('Connexion à vérifier');
}

export function agentBadge(type) {
  return '<span class="agent-icon agent-icon-' + (['codex', 'claude'].includes(type) ? type : 'command') + '" aria-hidden="true">' + (type === 'claude' ? '✳' : type === 'codex' ? 'C' : '&gt;_') + '</span>';
}

export function agentSidebarMarkup(agents) {
  return Object.entries(agents).map(([id, status]) => '<button type="button" class="agent-row" data-agent-account="' + esc(id) + '" aria-label="' + esc(msg('Voir le compte de {0}', status.name || id)) + '">' + agentBadge(status.type) + '<span class="agent-row-copy"><strong>' + esc(status.name || id) + '</strong><small><span class="online-dot ' + (status.connected ? '' : 'offline') + '"></span>' + esc(agentConnection(status)) + '</small></span><span class="agent-chevron" aria-hidden="true">›</span></button>').join('');
}
