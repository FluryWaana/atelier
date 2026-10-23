import hljs from './vendor/highlightjs/highlight.min.js';
import { esc } from './view-helpers.mjs';

// Vendored BSD-3-Clause build: https://cdn.jsdelivr.net/npm/@highlightjs/cdn-assets@11.11.1/es/highlight.min.js
const languages = new Map(Object.entries({
  javascript: 'js mjs cjs jsx', typescript: 'ts mts cts tsx', json: 'json',
  xml: 'html htm xml svg csproj props targets xaml vue', css: 'css', scss: 'scss', less: 'less',
  python: 'py pyw', csharp: 'cs', java: 'java', kotlin: 'kt kts',
  c: 'c h', cpp: 'cc cpp cxx hpp hxx', go: 'go', rust: 'rs', swift: 'swift',
  php: 'php', ruby: 'rb', bash: 'sh bash zsh', sql: 'sql',
  yaml: 'yml yaml', ini: 'ini toml cfg', markdown: 'md markdown',
  lua: 'lua', perl: 'pl pm', r: 'r', graphql: 'graphql gql', diff: 'diff patch',
}).flatMap(([language, extensions]) => extensions.split(' ').map(extension => [extension, language])));

export function highlightedLines(content, path = '') {
  const name = path.split('/').at(-1).toLowerCase();
  const language = name === 'makefile' ? 'makefile' : languages.get(name.match(/\.([^.]+)$/)?.[1]);
  // shortcut: large previews stay plain text; use a worker if coloring them becomes necessary.
  if (!language || content.length > 128 * 1024) return content.split('\n').map(esc);
  const html = hljs.highlight(content, { language, ignoreIllegals: true }).value;
  const spans = [];
  // Reopen multiline tokens so each numbered row has balanced markup.
  return html.replace(/<span class="[^"]+">|<\/span>|\n/g, token => {
    if (token === '\n') return '</span>'.repeat(spans.length) + '\n' + spans.join('');
    if (token === '</span>') spans.pop();
    else spans.push(token);
    return token;
  }).split('\n');
}
