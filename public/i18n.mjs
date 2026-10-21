import { defaultLocale, sourceLocale, languages } from './locales/index.mjs';

export { languages };
export const languageKey = 'atelier-language';
export function resolveLocale(value) {
  return typeof value === 'string' && Object.hasOwn(languages, value) ? value : defaultLocale;
}
function savedLocale() {
  try {
    const requested = new URL(globalThis.location.href).searchParams.get('lang');
    if (requested) return resolveLocale(requested);
  } catch {}
  try { return resolveLocale(globalThis.localStorage.getItem(languageKey)); } catch { return defaultLocale; }
}
export let locale = savedLocale();
export function setLocale(value) {
  locale = resolveLocale(value);
  return locale;
}
function template(source) {
  if (Object.hasOwn(languages[locale].messages, source)) return languages[locale].messages[source];
  if (locale === sourceLocale) return source;
  return Object.hasOwn(languages[defaultLocale].messages, source) ? languages[defaultLocale].messages[source] : source;
}
export function msg(source, ...values) {
  return template(source).replace(/\{(\d+)\}/g, (token, index) => index < values.length ? String(values[index]) : token);
}
const escape = value => value.replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[char]);
// Values already escaped by the view stay escaped; only catalog text is escaped here.
export function markupText(source, ...values) {
  return escape(template(source)).replace(/\{(\d+)\}/g, (token, index) => index < values.length ? String(values[index]) : token);
}
const messagePatterns = Object.keys(languages[defaultLocale].messages).filter(key => /\{\d+\}/.test(key)).map(key => [key, key.split(/\{\d+\}/)]);
export function messageText(value) {
  if (typeof value !== 'string' || locale === sourceLocale) return value;
  const exact = msg(value);
  if (exact !== value) return exact;
  for (const [key, parts] of messagePatterns) {
    if (!value.startsWith(parts[0])) continue;
    const values = [];
    let position = parts[0].length;
    for (const [index, part] of parts.slice(1).entries()) {
      const end = index === parts.length - 2 ? value.length - part.length : value.indexOf(part, position);
      if (end < position || value.slice(end, end + part.length) !== part) break;
      values.push(value.slice(position, end)); position = end + part.length;
    }
    if (values.length === parts.length - 1 && position === value.length) return msg(key, ...values);
  }
  return value;
}
export function moduleMessage(module, source) {
  const messages = module.translations?.[locale];
  if (messages && Object.hasOwn(messages, source)) return messages[source];
  if (locale === module.sourceLocale) return source;
  const fallback = module.translations?.[defaultLocale];
  return fallback && Object.hasOwn(fallback, source) ? fallback[source] : msg(source);
}
export function localizeModule(module) {
  const text = source => moduleMessage(module, source || '');
  return { ...module, name: text(module.name), description: text(module.description),
    fields: (module.fields || []).map(field => ({ ...field, label: text(field.label), ...(field.options ? { options: field.options.map(option => ({ ...option, label: text(option.label) })) } : {}) })),
    tabs: (module.tabs || []).map(tab => ({ ...tab, label: text(tab.label), description: text(tab.description), actions: (tab.actions || []).map(action => ({ ...action, label: text(action.label) })) })),
  };
}
export function initializeLanguage(document) {
  document.documentElement.lang = locale;
  // Translate only the initial application shell, before any user content is rendered.
  const walker = document.createTreeWalker(document.body, 4);
  while (walker.nextNode()) {
    const node = walker.currentNode;
    if (node.parentElement.closest('script, style')) continue;
    node.nodeValue = node.nodeValue.replace(/^(\s*)([\s\S]*?)(\s*)$/, (_, before, text, after) => before + msg(text) + after);
  }
  for (const element of document.querySelectorAll('[title], [placeholder], [aria-label]')) {
    for (const name of ['title', 'placeholder', 'aria-label']) if (element.hasAttribute(name)) element.setAttribute(name, msg(element.getAttribute(name)));
  }
  const select = document.querySelector('#interface-language');
  for (const [code, language] of Object.entries(languages)) {
    const option = document.createElement('option');
    option.value = code; option.textContent = language.label; select.append(option);
  }
  select.value = locale;
  select.onchange = () => {
    setLocale(select.value);
    try { globalThis.localStorage.setItem(languageKey, locale); } catch {}
    const url = new URL(globalThis.location.href);
    url.searchParams.set('lang', locale);
    globalThis.location.assign(url.href);
  };
}
