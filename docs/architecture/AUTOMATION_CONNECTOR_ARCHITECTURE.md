# Automation and Connector Architecture

Status: implementation baseline for DS Agent v0.5-v1.0
Date: 2026-07-12

## Decision

DS Agent will add durable automation and connectors as focused Kernel domains.
They will not be implemented inside `commands.rs` or `App.tsx`, and they will
not create a second execution engine beside Agent runs.

The target layers are:

1. `kernel/automation`: deterministic schedules, definitions, trigger windows,
   durable runs, retries, missed-run policy, checkpoints, and review links.
2. existing Agent runs: execution leases, heartbeat, recovery, cancellation,
   resource claims, tool execution, evidence, and completion.
3. `kernel/review_queue`: reviewable results and frozen external-mutation
   previews. Review is not approval.
4. `kernel/connectors`: provider-neutral accounts, capabilities, invocations,
   cursors, health, redaction, credentials, and provider contracts.
5. thin Tauri adapters and ordinary-language UI surfaces.

DeepSeek continues to own open-ended understanding and planning. Automation
stores the user's goal and deterministic execution policy; it does not locally
interpret natural language or invent a plan.

## Provider adapter boundary

Mail and calendar providers implement shared typed requests and normalized
domain results. Provider-private HTTP DTOs, paths, pagination links, and error
bodies do not cross this boundary. Inputs are bounded before an adapter builds
a request; returned mail and calendar content remains untrusted evidence.

Provider metadata is intentionally not an execution interface. `ConnectorProvider`
contains only stable identity and advertised capabilities; bounded mail/calendar,
delta sync, draft creation, mutation application, and read-only reconciliation
are separate narrow traits. There is no generic capability-only read method.
Contract metadata checks are pure. Explicit runners exercise each advertised
capability exactly once and reject duplicate, missing, or unadvertised coverage.
Typed search/list contracts permit legitimate empty results while still enforcing
result limits, thread and time-range identity, normalized untrusted evidence,
continuation shape, and bounded receipts. Fake and Microsoft offline adapters use
the same read/sync gates; only Fake advertises mutation capabilities.

Provider rollout is deliberately split:

1. scripted in-memory HTTP transport proves endpoint construction, scope
   mapping, response normalization, hostile-link rejection, and contract
   behavior without opening a socket or reading a real credential;
2. the production backend transport resolves an opaque credential handle,
   disables redirects, enforces fixed provider hosts and response budgets, and
   returns only normalized failures;
3. live read access requires an explicit isolated-account validation decision;
4. external mutations are advertised only after provider-specific idempotency
   and reconciliation tests satisfy the shared approval contract.

A future reconciliation worker receives only `ConnectorMutationReconciler`; the
trait has no `apply_mutation` method. This compile-time boundary supplements the
durable rule that an uncertain external effect can only be queried, never replayed.

The provider-neutral worker kernel now uses a nullable legacy-safe frozen account
generation and private persistent claim columns. New mutations require generation
in the exact Tool fingerprint. A dedicated uncertain-result transition schedules
reconciliation while leaving the Tool running. Claim, 300-second renewal, expired
takeover, persistent defer, and completion are fenced by an opaque token. Before a
provider query, claim and renewal revalidate connected generation, provider,
capability, immutable envelope, running Tool fingerprint, consumed one-shot
approval, and pending Review binding. Completion repeats those checks transactionally
with the reconciled receipt. Provider errors are discarded. No production provider
registry is activated yet, so this does not enable Microsoft writes.

The first Microsoft slice advertises account discovery, bounded mail
search/thread read, bounded calendar range read, and separately consented
mail-inbox/calendar delta sync. The delta capabilities are not implied by a
one-shot search/list grant. This slice cannot send mail or create, update, or
cancel meetings and is not connected to a live account by default.

The transport call carries a secret-free account id and opaque credential
handle separately from the request. Provider adapters cannot place bearer
tokens in recorded request headers. OAuth exchange returns the provider's
actual granted scopes with the non-serializable credential; completion fails
closed unless they exactly match the approved session scopes.

The production HTTP transport is backend-only. It validates the exact provider
origin, method, safe header allowlist, body absence, and response budget before
resolving a credential. It disables redirects, injects only the resolved access
token inside reqwest, copies only safe response headers, and enforces both
Content-Length and streaming byte limits. Microsoft credentials are a private
vault envelope containing access token, refresh token, expiry, and exact access
scopes. The opaque handle resolves to a 64 KiB-bounded, current-user DPAPI
protected file under the application-private vault. Same-directory staging plus
atomic replacement prevents torn refresh writes; plaintext and owned DPAPI
buffers are zeroized. One
application-owned connector runtime makes refresh, resolution, disconnect, and
revocation share a credential-handle lock so a deleted credential cannot be
resurrected by an in-flight refresh. The global credential-store lock is never
held during remote work. A separate durable account generation binds provider,
tenant, credential handle, and granted capabilities. Disconnect increments the
generation; every sync page and recovery commit verifies the account is still
connected on the same generation in its SQLite transaction, so an HTTP request
that began before disconnect cannot recreate cleared state or content.

Incremental sync is separate from bounded search/list reads. Shared sync pages
carry typed upserts or deletion tombstones plus a non-debuggable opaque `next`
or final `delta` continuation. Event Store commits normalized projections and
cursor revision in one transaction, rejects stale revisions with CAS, and emits
only token-free receipts. A 410 rebuild atomically clears the old stream
projection before rebuilding so missing tombstones cannot leave ghost items.
Rate limits, network failures, invalid responses, consecutive rebuilds,
exhausted state, and account repair all survive restart with bounded attempts.
Each stream is bound to a request fingerprint. Retention is capped at 500 items
per stream, 8 LRU streams and 1,000 items per account, 2 MiB per account,
32 KiB per normalized item, and 90 idle days. The same retention helper runs for
successful page commits and failure/rebuild state commits, so failing rolling
calendar windows cannot grow the cursor table without bound. Disconnect
atomically removes the account's sync cursors and projections.

Local disconnect uses a two-phase cross-store state machine. Event Store first
persists `disconnect_pending`, increments the account generation, clears sync
state, and writes a secret-free started receipt in one transaction. The runtime
then performs an idempotent vault deletion. A generation- and account-bound
ticket completes `disconnected` in a second transaction. A bounded startup
sweep resumes either crash window without attempting remote revocation.

Remote credential revocation now has a separate persistent, generation-bound
saga rather than the former in-memory `revoke_account` path. Beginning the saga
increments generation, sets `revocation_pending`, invalidates old attachment and
sync authority, and stores an immutable provider/account/credential binding. A
worker first acquires an opaque 300-second claim and then commits
`remote_call_started` with a fresh attempt id before the provider can see the
credential. Provider output is reduced to `revoked`, `already_revoked`,
`known_not_applied`, or `uncertain`; provider error text is discarded.
`known_not_applied` is the only remote outcome that may schedule bounded retry.
An uncertain result, or startup recovery of `remote_call_started`, enters
`reconciliation_required` and is never replayed. Only `remote_confirmed` may
delete the local credential. That delete is idempotent, lease fenced, and can be
resumed after a crash, including `already_absent`, before the account becomes
`disconnected`. The worker and registry are Fake-only Kernel surfaces: no Tauri
command, Microsoft revoker, live provider registration, or Recovery action is
activated.

Microsoft authorization-code exchange and refresh use a separate fixed token
endpoint client rather than weakening the Graph GET transport. The client posts
only the public-client PKCE fields to the fixed organizations authority, sends
no client secret or bearer header, disables redirects, caps streamed responses
at 64 KiB, validates Bearer/expiry/scope, and normalizes all provider errors.
`offline_access` is treated as a protocol scope that need not appear in the
access-token scope response; all actual access scopes must still match exactly.

The internal attachment preflight keeps metadata and bytes as separate
operations. It remains unadvertised and unreachable from Tauri/UI. Microsoft
metadata requests use an explicit `$select` that excludes `contentBytes`.
Downloading `$value` requires a consumed, non-cloneable permit derived from the
exact approved metadata, account generation, visible frozen preview, and durable
workspace identity. The provider-specific client accepts only Event Store plus
that permit; it reloads account, private metadata, workspace, and generation from
the reservation instead of accepting caller snapshots. It
uses the fixed Graph origin, no redirects, a resolved access token, declared
size/type checks, and streams directly into the provider-neutral Safe Landing.
Safe Landing writes only to a managed workspace directory, caps a file at
20 MiB, checks filename, declared MIME, extension, detected magic, UTF-8/JSON,
and bounded Office archive contents, rejects macro/executable/embedded active
content, persists a frozen `ready` receipt before rename, completes only through
connected-generation CAS, and returns a receipt with
a relative landing reference and hash rather than an absolute path or remote
provider reference. It never opens the file or adds it to model context. This
backend is a real durable authorization slice but remains intentionally
unadvertised. Indexed Tool/approval current projections now commit beside their
events, and the dedicated attachment workflow atomically prepares exact preview,
approves plus reserves, or rejects plus terminalizes. Raw provider refs live only
in short-lived private source rows and are removed once staging reaches `ready`.
On Windows, workspace binding includes workspace/landing FILE_ID values; held
no-reparse directory and file handles anchor create, validation, rename, crash
recovery, and identity-checked delete. Both ready-before-rename and
ready-after-rename crashes are recovered by receipt identity/hash/type rather
than destructive path guessing. Staging persists FILE_ID before the first byte;
completed files have 30-day expiry, 32-item/256-MiB workspace budgets, and a
runtime worker with persistent bounded cleanup backoff. Office packages are XML-
parsed for exact main content type, root officeDocument relationship, and every
relationship target; malformed/external structures fail closed. Production
Recovery Center exposes only redacted state. Its attachment retry is bound to a
Kernel action fingerprint and can only queue identity-checked local cleanup; it
cannot reauthorize, call a provider, or restart a Tool. Startup-only recovery may
claim abandoned `reserved/staging` rows before workers start, while the runtime
worker cannot claim active downloads. Ready, cleanup, and retention recovery now
share a persistent 300-second lease plus opaque per-batch token. A worker renews
the same row/token before a filesystem side effect, and every complete, defer,
repair, or fail transition fences on the same unexpired token. Runtime ready
recovery can take only an explicitly due defer or an expired claim; a normal live
`ready` checkpoint with no claim and no retry time is not recoverable at runtime.
The Windows single-instance guard is acquired before Event Store startup recovery,
so clearing old-process claims cannot race another DS Agent process. Tokens and
expiry values remain backend-only and do not enter events, IPC, Recovery Center,
or model context. Missing file identity is not automatically treated as manual
repair: no-reparse probes must prove both managed names absent; unknown presence is
preserved. A broken Tool projection quarantines only its own row and cannot starve
the rest of a cleanup batch. Schema upgrades fail closed on real column-migration
errors, and legacy completed rows without complete identity receipts are preserved
for Recovery Center. Production activation still requires a dedicated attachment
download UI/IPC path and a fresh explicit decision before live Microsoft access.

Recovery presentation is now a typed Kernel contract rather than backend-authored
prose. Every item carries a bounded reason, external-effect state, next-step code,
status, and optional tagged action. The UI exhaustively maps those codes to local
copy and never renders raw backend status, provider errors, or invocation bodies.
Stopped mail/calendar sync streams are projected as read-only `sync_exhausted`
items with a stable opaque item id; continuations, stream fingerprints, retry
state, account ids, tenant references, and credential handles remain private.
Reconciliation listing reads only invocation id/status/time projection columns.
Account repair, revocation pending, stopped sync, and reconciliation remain
actionless. Revocation now has a durable Kernel ticket and safe Fake-only worker,
but its card cannot start, replay, or reconcile a provider call until a
provider-specific read-only revocation contract and explicit live activation
decision exist. An uncertain mutation likewise must never become a generic retry
button.

## Durable execution model

An automation scheduler only determines that a trigger window is due and
creates or claims one `AutomationRun`. The run is linked to one queued Agent
run. The existing Agent worker performs the work.

The durable deduplication identity is `(definition_id, trigger_window_key)`.
Manual runs use an explicit invocation id and never impersonate a scheduled
window. Waiting review and waiting approval are stable states, not failures,
and are excluded from retry.

Long-lived automation state must not depend on the current 500-event replay
window. Automation persistence uses indexed entity projections plus append-only
Kernel events. State changes that claim a window and create its durable run are
transactional. A stable sequence, not timestamp alone, orders transitions.

## Approval and side-effect contract

`waiting_review` means a user can accept, edit, or reject a result or draft. It
does not authorize an external mutation.

`waiting_approval` means the exact mutation preview is frozen and bound to a
one-shot approval and tool invocation fingerprint. The fingerprint includes
provider, account, normalized capability, target or draft reference, preview
hash, automation run id, and idempotency key. Editing any field invalidates the
old approval.

Connector writes reuse the precise Tool Runtime authorization boundary. They
must not use the legacy broad email capability grant. A timeout after an
external write enters reconciliation; it is never blindly retried.

## Migration plan

- Stage 0: this decision and the threat model become testable constraints.
- Stage 1: durable automation definition/run/checkpoint persistence, atomic
  claiming, pause/resume, missed-run `skip` and `run_once`, bounded retry, and
  restart tests. No provider APIs.
- Stage 2: Automation Center and review queue UI over secret-free DTOs.
- Stage 3: connector contracts, backend credential handles, fake provider,
  exact approval, redaction, refresh single-flight, cursor and recovery tests.
- Stages 4-5: Microsoft and Google adapters share the same contract suite.
- Later stages extend artifacts, Computer Use, Subagents, recovery, and memory
  without changing the execution and approval invariants above.

## Compatibility constraints

- Existing Agent run events and recovery behavior remain readable.
- Existing capability, evidence, audit, Memory, Skill, and Subagent boundaries
  remain authoritative until migrated behind shared Kernel services.
- New Tauri commands are boundary adapters only.
- No OAuth token, message body, or attachment content is stored in Kernel
  events merely to make a projection convenient.
