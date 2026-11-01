# Ticket and time integrations

Atelier works without an external service. Local tasks and local time tracking remain available with every provider selection.

## Provider preferences

`ticketProvider` and `timeProvider` are independent preferences for new operations. Changing a preference does not convert tickets, move time entries or send a remote write.

Settings group ticket sources, time tracking and connector management. The catalog includes Local, built in Jira and Tempo adapters, and installed process modules. Ticket capabilities determine supported search, read and creation actions. Time providers require the complete `timePreview`, `timeSend` and `timeReconcile` group. A repository integration alone is not a ticket source.

The selected provider's form appears before saving the preference. The choice, connection settings and secrets have separate saves. Connection drafts are retained between tabs and protected when changing provider. Loading errors and removed modules are shown explicitly.

## Saving defaults

`PUT /api/integrations/defaults` accepts `kind`, `provider` and `expected`. It checks the installed capability and previous preference in the same storage operation. A conflict returns HTTP 409 and preserves the draft. Saving a default does not read credentials or validate unrelated connection settings.

## Linked records

Tickets retain their connector, instance and remote identifier. Time previews and journals retain their provider. Existing Jira and Tempo recovery controls remain available when the preference returns to Local.

New Jira creation and Tempo export use the corresponding selected default. Local time is explicitly recorded locally. Local creation and manual entries remain available independently.

Removing a module resets only its defaults to Local while removing its registry entry and secrets in the same transaction. Linked cards remain. Unresolved remote effects block removal. Persistence failure rolls back both removal and preference changes.

## Remote writes

Preview and confirmation are separate steps. Each confirmed write has a persistent operation identity. After an interrupted response, reconciliation observes the original operation instead of submitting it again. Local descriptions, instructions and criteria remain protected.

See [board mapping](rust-kanban-columns.md), [connector contracts](rust-connectors.md) and [synthetic TLS testing](rust-synthetic-tls.md) for detailed boundaries.
