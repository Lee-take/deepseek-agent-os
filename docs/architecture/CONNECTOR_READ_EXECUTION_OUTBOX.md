# Connector Read Execution Outbox

Date: 2026-07-14
Status: accepted implementation slice

## Decision

Mail search and calendar listing will enter the connector runtime through a
durable, provider-neutral read execution outbox. Automation free-form goals and
DeepSeek responses cannot directly choose an account, provider, credential,
capability, query range, cursor, or result budget.

An execution is created only from a locally validated typed plan and a stable
source invocation. The outbox binds the source, account generation, capability,
canonical plan hash, and current authority before any provider lookup.

## Durable phases

`Pending -> Claimed -> RemoteCallStarted -> ResultPersisted -> Applied`

Additional terminal or repair phases are `AuthorityLost`,
`ReconciliationRequired`, `Cancelled`, and `RepairRequired`.

- Only `Pending`, or an expired `Claimed` execution that never reached
  `RemoteCallStarted`, may be claimed for provider I/O.
- `RemoteCallStarted` without a persisted result is externally uncertain after
  restart and is never silently replayed.
- `ResultPersisted` may replay only the local apply transaction.
- `Applied` is idempotent and returns its existing safe receipt.

## Authority and privacy

The kernel rechecks connected health, account generation, provider, tenant,
credential binding, granted capability, plan hash, and source validity before
claim, immediately before provider I/O, and after provider I/O. Provider I/O
never occurs while the EventStore mutex is held.

Kernel events and public views contain only a local execution id, read kind,
phase, bounded result count, safe error code, and opaque local evidence
reference. They never contain provider or tenant identifiers, credential
handles, cursors, remote references, claim ids, attempt ids, raw provider
errors, mail bodies, or calendar participant data.

## Automation boundary

Automation will later bind one typed read plan to a definition revision and
create its read execution in the same transaction as the automation occurrence.
Editing a free-form goal does not mutate an already queued plan. DeepSeek may
summarize explicitly authorized, bounded, untrusted evidence only after the
kernel has completed and redacted the local result.

## Initial implementation order

1. Provider-neutral plan, phase and safe public-view types.
2. Durable schema, unique source binding and phase compare-and-swap.
3. Explicit read submission and fake-provider worker.
4. Automation occurrence adapter and safe evidence/checkpoint projection.
5. UI status and evidence entry point.

Production registries remain empty until a separately reviewed activation
slice. This decision does not authorize a live Microsoft or Google provider.
