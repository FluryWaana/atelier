export function sameTicket(ticket, connector, instance, id) {
  if (!ticket || (ticket.connector || 'jira') !== connector || String(ticket.id) !== String(id)) return false;
  const scope = ticket.instance || ticket.base || new URL(ticket.url).origin;
  return scope.replace(/\/+$/, '') === instance.replace(/\/+$/, '');
}
