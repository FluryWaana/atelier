# Board columns and Jira mappings

Configure columns from the selected project's board. The all projects view first asks which project to configure. Saving selects that project so the board displays its configuration.

The five stages remain `todo`, `doing`, `review`, `done` and `standby`. Display names and order are configurable per project. Cards retain their identifiers and stages. Task editors use project column names. The all projects board uses common names.

## Read a Jira board

Enter a Jira board identifier and request its columns. The adapter calls `GET /rest/agile/1.0/board/{boardId}/configuration`, retaining the Data Center context path or configured Cloud gateway.

The request requires the local session and current connection revision. The result is revalidated after reading. Returned URLs are not followed. Identity mismatches, duplicate status identifiers, control characters and oversized responses are refused. Public fields are checked for secret disclosure.

The result retains column order and every status identifier in each column.

## Map statuses

Each Jira column can map to one local stage or remain unmapped. Its statuses stay grouped. Several Jira columns can map to the same stage. Mapping is explicit and does not infer matches from names.

A later read preserves mappings for the same board and account. A partially changed group requires review before saving. Address, account and Cloud or Data Center mode define the mapping scope.

Saving compares the previous project configuration and connection revision. A concurrent change produces a conflict. Choosing Local as the ticket source does not disable local column settings.

## Apply mappings

Saving configuration moves no cards and sends no Jira transitions. New imports use the explicit mapping for the received status. Existing imported or attached cards retain local progress.

Synchronization proposes a mapped stage and requires separate confirmation. Mapping changes invalidate older synchronization previews. An explicit configuration takes precedence over name matching.

See [integrations](rust-integrations.md) for provider preferences and [verification](rust-final-qualification.md) for synthetic import and concurrency checks.
