# Process connector protocol

Process connectors extend ticket, repository and time integrations through explicitly installed native executables. A connector runs with the user's operating system rights. Approval and supervision do not make it a sandbox.

## Installation and settings

Installation has a probe and a separate confirmation. The host reads the executable identity and manifest again before approval. Duplicate modules and reserved built in identifiers are refused. The registry supports at most 16 modules and 2 MiB of metadata.

Public settings and secrets are stored separately. The frontend receives only the presence of saved secrets. Address or `credentialScope` changes remove secrets that are not explicitly replaced and advance the revision. Binary changes, removal, restoration and reinstallation invalidate stale approvals and reports. Restoration advances the registry generation beyond both snapshots.

Removal requires confirmation and preserves linked cards. Unresolved remote effects block removal. Settings drafts remain available between tabs and after conflicts.

## Transport

Each invocation starts one native process in a private working directory with a selected environment. The host writes one UTF8 JSON request to stdin and closes it. The process writes exactly one JSON response to stdout and exits. Trailing whitespace is allowed. Additional documents, progress text, truncated JSON and mismatched identities are refused.

Requests are limited to 256 KiB and responses to 512 KiB. Stderr is separate from the response and is never displayed as raw user output. Reflected secrets are rejected, including inside nested objects and JSON keys. Process errors use safe host messages.

The host uses an explicitly installed executable without implicit shell execution or repository discovery. Executable identity is verified for every call. See [process supervision](process-supervision.md) for platform behavior.

## Request envelope

```json
{
  "protocol": "atelier-connector/1",
  "requestId": "request-synthetic-1",
  "module": "demo",
  "method": "describe",
  "operationId": null,
  "action": null,
  "settings": {},
  "secrets": {},
  "input": null
}
```

Identifiers start with a lowercase letter and continue with lowercase letters, digits or ASCII hyphens, up to 64 bytes. Unknown envelope fields are refused.

The response repeats `protocol`, `requestId`, `module` and `method`. It supplies `status` as `ok`, `error` or `uncertain`, plus `message` and `output`. Envelope acceptance does not replace validation of the method result.

`describe` receives empty settings and secrets, null input and no operation or action. It returns the manifest.

For the request above, a minimal successful discovery response is:

```json
{
  "protocol": "atelier-connector/1",
  "requestId": "request-synthetic-1",
  "module": "demo",
  "method": "describe",
  "status": "ok",
  "message": "Demo connector available",
  "output": {
    "id": "demo",
    "name": "Demo connector",
    "description": "A minimal connector with a connection check",
    "sourceLocale": "en-US",
    "methods": ["check"],
    "fields": [],
    "tabs": [],
    "translations": {}
  }
}
```

After installation, implement `check` with the same response envelope and `method` set to `check`. Each response must copy the actual request's identity values. A successful check can return an empty output object.

## Manifest

The manifest uses `id`, `name`, `description`, `sourceLocale`, `methods`, `fields`, `tabs` and optional `translations`. Source languages are `en-US` and `fr-FR`. Labels and messages are plain text. The public Rust definitions and validation rules are in [extensions.rs](../crates/atelier-core/src/application/extensions.rs).

| Item | Limit |
| --- | --- |
| Manifest | 128 KiB |
| Fields | 32 |
| Methods | 32 |
| Tabs | 8 |
| Actions per tab | 16 |
| Choices per select | 100 |
| Messages per language | 200 |

Field types are `text`, `url`, `password`, `checkbox`, `select` and `textarea`. Select choices must be distinct. Other types cannot declare choices. Checkboxes require booleans without coercion. Other public values are strings limited to 4,000 units.

Password fields cannot appear in `settings`. Only secrets declared by the module may appear in `secrets`. HTML, scripts and executable styles are not accepted as labels or translations.

## Methods

| Group | Methods |
| --- | --- |
| Description and checks | `describe`, `check` |
| Tickets | `ticketsSync`, `ticketsSearch`, `ticketsRead`, `ticketsReset`, `ticketsCreate`, `ticketsTransitions`, `ticketsTransition` |
| Repositories | `repositoriesPreview`, `repositoriesPublish`, `repositoriesRefresh`, `repositoriesReview` |
| Time | `timePreview`, `timeSend`, `timeReconcile` |
| Actions and recovery | `action`, `reconcile` |

`check` is required. `describe` is implicit and is not repeated in the manifest. Unsupported method combinations are refused.

`ticketsCreate`, `ticketsTransition`, `repositoriesPublish` and `timeSend` declare remote effects. `ticketsReset` declares a local effect. Actions declare `read`, `local` or `remote`. Other methods are reads.

Remote methods and actions require `reconcile`. Time export instead requires the full `timePreview`, `timeSend` and `timeReconcile` group.

## Effects and reconciliation

Every write uses a persisted operation identifier. Preview and confirmation bind the module revision, credential scope, relevant local snapshots and destination. The host revalidates them before dispatch and local application.

After interruption or a lost response, the host observes the original operation through reconciliation and never resends the write automatically. A reconciliation request retains the original `operationId` and input and uses a new `requestId`. Positive confirmation requires the matching receipt and validated method output. A plain `ok` or an empty search is insufficient evidence of a remote effect.

The `output.receipt` object contains `operationId`, `outcome`, `reference` and `observedAt`. Outcomes are `applied`, `notAttempted` and `unknown`. `reference` is a nonempty observation reference. `observedAt` uses RFC 3339 and must be no more than ten minutes old or one minute in the future. The operation identity must match the original intention. Method specific results, such as the observed ticket or exact time proposal, remain required alongside the receipt.

Remote state can change after preflight. The connector's result is its own observation and does not independently prove that Atelier caused the effect. This limit applies to remote transitions and publication.

Uncertain effects block incompatible writes, restoration, updates and new worker launches. Reads and explicit recovery remain available. Queues pause and require explicit resume after resolution.

## Tickets

Search is paginated. Confirmed import handles 1 to 20 tickets per page, bound to the selected module, instance, project and exact report. Import is atomic and never overwrites existing cards with the same scoped identity.

`ticketsRead` prepares a dated refresh. Title and description are separate opt in replacements. Applying it requires the same ticket, profile revision, scope and card snapshot, with a report less than ten minutes old. Metadata can refresh while the local column, instructions and other human fields remain protected. Requirement changes invalidate dependent reviews in the same transaction.

`ticketsTransitions` returns the exact ticket identity, current status and up to 100 choices. Each choice supplies `id`, `name`, `status` and `requiresFields`. Duplicates and missing statuses are refused. Choices requiring extra fields remain unavailable.

A transition preview expires after ten minutes. Before sending, the host rereads choices through the pinned executable and checks the starting status, choice, ticket and card. Confirmed results update ticket metadata without overwriting title, description, instructions, criteria, local column or worktree.

`ticketsReset` prepares a complete scoped ticket inventory before confirmed local reconciliation. It marks linked tickets as present or outside the scope without deleting cards or their links. Partial pages or a changed scope refuse the preview. Local human content remains protected. Reset does not imply a remote deletion.

## Time

`timePreview` receives a stopped, unsent human time entry projection containing `id`, `cardId`, `cardTitle`, optional `ticket`, `seconds`, `date`, `startTime` and `description`. Agent time, costs, repository files and unrelated secrets are excluded. Dates and times retain local meaning without implicit timezone conversion.

The result contains the unchanged `source`, canonical HTTPS `instance`, `account` and bounded `reference`. A changed source, destination, account or unknown field refuses the report. The host binds an exact report less than ten minutes old before confirmation.

`timeSend` receives the frozen proposal and operation identity. `timeReconcile` observes that proposal after uncertainty. Confirmation requires the original proposal and matching receipt. An uncertain export stays reserved and cannot be reset into a second send.

## Repositories

Publication requires `repositoriesPreview`, `repositoriesPublish`, `repositoriesRefresh`, `repositoriesReview` and reconciliation. Module references retain their own instance, repository, origin, request identity, branches, commits, state and revision. They are stored separately from built in forge references.

The exact HTTPS worktree origin and selected module define scope. Before and after a call, the host checks the managed worktree, HEAD, branch references, source content, index and FETCH_HEAD in both worktree and main repository. Target commits must exist locally. Reviews require the current clean source HEAD and a locally computed merge base. Source and target branches must differ.

The connector owns its transport and object import. It must preserve human branches, index and FETCH_HEAD. Before and after checks cannot detect malicious temporary mutations restored by arbitrary connector code.

Publication input contains only the approved repository reference, worktree, branches, commits, expected remote source, title, description, draft choice and existing request. Private notes, instructions, criteria, raw cards and host audit context remain excluded. A lost or divergent response leaves the intention uncertain.

`repositoriesReview` supplies an observation rather than an accepted agent report. Atelier creates the review context and runs the selected profile through its normal engine. Revalidation checks sources and module state. Profile revision changes and removal invalidate affected reviews without deleting reports or human content. Human criteria remain unchanged.

See [workflows](workflows.md) for approved publication plans and [verification](testing.md) for compiled synthetic connectors.
