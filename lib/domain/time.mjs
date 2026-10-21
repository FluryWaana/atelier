export function timeFields(body) {
  const seconds = Number(body.seconds);
  if (!Number.isSafeInteger(seconds) || seconds < 1 || seconds > 86400) throw new Error('La durée doit être comprise entre 1 seconde et 24 heures.');
  if (typeof body.date !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(body.date) || new Date(body.date + 'T12:00:00Z').toISOString().slice(0, 10) !== body.date) throw new Error('Date invalide.');
  if (typeof body.description !== 'string' || !body.description.trim() || body.description.length > 4000) throw new Error('Une description de 1 à 4 000 caractères est nécessaire.');
  if (typeof body.startTime !== 'string' || !/^([01]\d|2[0-3]):[0-5]\d:[0-5]\d$/.test(body.startTime)) throw new Error('Heure invalide.');
  return { seconds, date: body.date, startTime: body.startTime, description: body.description.trim() };
}

