# Project workflows and the execution queue

A project workflow coordinates development, verification and review for a task worktree. Project settings define the verification command, target branch, workflow permission and optional P0/P1 corrections. Saving settings does not execute commands.

## Start and verify

Confirmation binds the task requirements, public agent profile, project and prepared worktree. The workflow persists its identity, steps and run references. The shared supervisor reserves each launch before preparing it. A stop accepted before startup invalidates that reservation.

Development is followed by verification. Runs retain their interactions, bounded logs and cancellation controls. A successful verification leads to explicit commit and publication controls. Human criteria are never checked automatically.

## Commit

A commit preview binds author, message, paths, parent and Git tree. Execution uses a private index and a conditional branch update. The human index remains unchanged. Only the expected observed commit advances the workflow. A divergent or uncertain result suspends it.

Verification runs again after a commit. A previous result is not reused as evidence for new sources. Form fields are cleared only when they still match the confirmed preview. Other drafts remain intact.

## Publish and review

Push and pull request or merge request previews bind the site, credential revision, repository, branches, commits, target and content. Creating a missing branch requires an additional approval for the same snapshot. Editing a draft invalidates approval even when its original text is restored.

Before sending, the workflow rechecks sources and remote state. It reuses persistent push and publication intentions. An existing request must retain its expected identity, state, branches and commits. Unexpected changes suspend the workflow.

Review uses observed source, target and base commits and the current task requirements. Stale sources prevent acceptance. Optional P0/P1 correction runs at most two cycles, each followed by verification and publication control. Other results remain ready for human review or blocked by uncovered criteria.

## Recovery

Restart interrupts active workflows without launching another process or replaying a commit or publication. Explicit resume rejects changed requirements, profile, project or worktree. Interrupted publication and review return through verification. Uncertain remote effects require reconciliation and fresh approval.

Restored workflows remain interrupted and their review and verification results become stale. Incompatible changes to tasks, projects, profiles, backups and updates remain blocked during activity.

## Queue

Each queue entry selects a single run or its project workflow. Workflow creation or resume and queue reservation share a SQLite transaction. The queue stores the exact workflow identity and waits for commit and publication approval.

A ready workflow advances the queue. Failure, blocking, cancellation or interruption pauses it. Restart leaves the queue paused. Explicit resume never replays publication. A completed workflow is removed without another agent run. Replacing it with a different workflow requires removing and adding the queue entry.

Starting work places the task in progress. Waiting, failure or stop places it on standby. Readiness places it in human review. A later transition changes the stage only if the workflow still owns the current stage, preserving human changes and completed tasks. An explicit new launch can reopen a completed task.

See [connectors](rust-connectors.md) for module publication and [verification](rust-final-qualification.md) for end to end checks.
