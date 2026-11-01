# Security

## Report a vulnerability

Use [GitHub private vulnerability reporting](https://github.com/FluryWaana/atelier/security/advisories/new) to report suspected vulnerabilities. Keep exploit details, credentials and personal workspace data out of public issues and pull requests.

Include the affected version or commit, operating system, reproduction steps using synthetic data, expected behavior and security impact. Share a minimal example and sanitized logs when useful. Do not attach a personal database or session URL.

For ordinary bugs and usage questions, use [GitHub Issues](https://github.com/FluryWaana/atelier/issues).

## Security boundaries

Atelier Kanban serves its interface over loopback and requires a local session for application operations. Keep session URLs and workspace files private. Do not expose the server through a public proxy or port forwarding.

Agents, service commands and process connectors execute with your local user rights. Install programs you trust and authorize only the repositories they need. Process supervision is not an operating system sandbox. Detached POSIX descendants can escape process group cleanup.

External integrations and coding agents can transmit task content or repository files to their configured services. Review those tools and services before providing private information. Local storage alone does not isolate external programs from your files or network.

Credentials use native protected storage. Importing a workspace does not guarantee credential access from another account or machine. Backups and exports can contain sensitive task information and should be protected accordingly.

See [architecture](docs/architecture.md), [process supervision](docs/process-supervision.md) and [connector contracts](docs/connectors.md) for technical details.
