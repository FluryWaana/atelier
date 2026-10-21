# Repository Guidelines

## Project Structure & Module Organization

This directory is the Windows x64 portable distribution of Atelier Kanban, a local task board with Jira, Azure DevOps, Tempo, Codex, and Claude integrations. `server.mjs` runs the HTTP server; `launcher.mjs` manages startup and shutdown. Backend modules live in `lib/`, including storage, agents, worktrees, and service integrations. `public/` contains the native HTML, CSS, and browser JavaScript. Tests live in `test/`. `runtime/` supplies Node.js and its license; `data/` stores local SQLite data, backups, and logs.

## Build, Test, and Development Commands

- `.\Start.cmd`: start the portable application and open `http://127.0.0.1:4317`.
- `.\Stop.cmd`: stop the application through its launcher.
- `npm start`: run the server in the foreground with an installed Node.js 24 or newer.
- `npm test`: run `node --test test/*.test.mjs`.
- `.\runtime\node.exe --test test/*.test.mjs`: run tests using the bundled runtime.

No npm dependencies are needed at runtime. For style development, run `npm ci`, then `npm run build:css` (or `npm run watch:css`). Edit `styles/` Sass sources and regenerate the committed `public/style.css`; Bootstrap and Sass are development dependencies only. `npm run package:windows -- owner/repository` builds the Windows release; `npm run test:release` checks the extracted package and rollback. See `docs/releases.md` and `.github/workflows/release.yml`.

Pure rules live in `lib/domain/`, use cases in `lib/application/`, HTTP adapters in `lib/http/` and storage/system adapters in `lib/infrastructure/` or the existing integration modules in `lib/`. `server.mjs` composes these dependencies. Tasks, projects, settings, runs, card workflows, Jira sync, automation, time tracking and updates use these boundaries, enforced by `test/architecture.test.mjs`. Board, project-list, process and project-settings views live in `public/views/`, with state and actions passed explicitly. Keep these boundaries when extending them; see `docs/architecture.md`. The release matrix builds Windows x64, Linux x64/ARM64 and macOS x64/ARM64; native release tests must pass before publishing. This checkout still contains only the Windows runtime.

## Coding Style & Naming Conventions

Use ES modules: `.mjs` for backend modules and tests, with browser code under `public/`. Follow nearby indentation and existing semicolon and single-quote conventions. Use camelCase for functions and variables and descriptive lowercase, hyphenated module names such as `time-tracking.mjs`. Keep integration logic in its corresponding `lib/` module. No formatter or linter is configured. Preserve the French user-facing text.

## Testing Guidelines

Use `node:test` and `node:assert/strict`; name tests `test/<feature>.test.mjs`. Existing tests cover Jira authentication and token storage. Add behavioral regression tests for changes, mock external services, and avoid live credentials or paid agent calls. Run the full suite before review. No coverage threshold is configured.

## Commit & Pull Request Guidelines

Conventional Commits are mandatory for every commit. Use `<type>[optional scope][!]: <description>`, with a concise, imperative description in English. Use `feat` for new features, `fix` for bug fixes, and `docs`, `refactor`, `test`, `build`, `ci`, `perf`, `style`, or `chore` as appropriate. Mark breaking changes with `!` before the colon or a `BREAKING CHANGE:` footer, and explain the impact.

Examples: `docs: require conventional commits`, `fix(updater): preserve user data during rollback`, `feat!: rename portable launchers to Start and Stop`.

Describe the problem, changed behavior, and verification in each PR; link relevant issues and include screenshots for UI changes.

## Security & Configuration

Keep the server bound to loopback. Never commit tokens, local databases, backups, or logs. Back up `data/` before storage changes and stop the application before replacing distribution files.
