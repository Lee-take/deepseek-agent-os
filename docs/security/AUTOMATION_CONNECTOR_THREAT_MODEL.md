# Automation and Connector Threat Model

Status: Stage 0 security baseline
Date: 2026-07-12

## Assets and data classes

| Class | Examples | Persistence rule |
| --- | --- | --- |
| Credential | access token, refresh token | current-user DPAPI protected app-private vault; Kernel stores an opaque handle |
| Account metadata | provider, tenant, display name, scopes, health | secret-free indexed projection and events |
| Content | mail body, calendar detail | task-bounded untrusted evidence; no default mirror or ordinary logs |
| Attachment | remote reference, downloaded file | validated workspace path, size/type/macro checks, retention policy |
| External mutation | send, create/update/cancel meeting | frozen preview, exact approval, idempotency and reconciliation |

## Trust boundaries and rules

- Mail, calendar text, attachments, and provider error bodies are untrusted
  evidence. They never become system instructions or tool policy.
- Unknown provider, capability, scope, remote action, or state transition fails
  closed.
- Credentials are non-serializable and non-displayable secret values held by a
  Rust backend credential service. They never cross Tauri IPC or enter events,
  logs, evidence, telemetry, exports, errors, or model context.
- Account disconnect immediately disables invocation and atomically removes
  local sync cursors/projections. Local credential deletion and remote
  revocation are separate recorded outcomes; revoke failure remains visible and
  fail-closed.
- Refresh is single-flight per credential handle and uses atomic credential
  replacement through the one application-owned connector runtime.
- The Windows vault accepts at most 64 KiB plaintext, derives a non-secret file
  name from the opaque handle, rejects symlink/non-file entries, uses DPAPI
  current-user protection, same-directory atomic replacement, and zeroizes
  plaintext, ciphertext staging, and DPAPI-owned buffers.
- OAuth callbacks are parsed URLs with exact `http`, `127.0.0.1`, dynamic port,
  and `/callback` checks; userinfo, query, fragment, and alternate hosts fail
  closed. Exchange claims are durable and one-shot. The result credential uses
  a preallocated opaque handle so interrupted exchanges can be cleaned and
  surfaced as `repair_required` without orphaning a token.
- Provider responses are normalized before audit. Raw authorization headers,
  response bodies, tokens, and sensitive identifiers are not logged.
- Provider adapters accept typed operations, never a model-supplied URL. Any
  provider pagination or delta link must use HTTPS, the exact allowlisted host
  and resource path, no userinfo or fragment, and a bounded response budget.
- Provider metadata validation cannot call draft, mutation, or reconciliation
  methods. Execution traits are split by operation class, the untyped generic
  read path is absent, and exact coverage fails if an advertised capability has
  no typed runner or is exercised more than once.
- Authorization URLs are rebuilt only from pending Kernel sessions. Provider,
  PKCE method and challenge, state, exact loopback redirect, requested
  capability, and minimal scope mapping are revalidated before navigation.
- OAuth token exchange must return actual granted scopes as trusted provider
  metadata. Requested scopes or capabilities are never persisted as granted
  merely because they appeared in the authorization request.
- Access and refresh tokens have distinct vault-only semantics. Only a
  short-lived resolved access token reaches the HTTP builder; refresh tokens
  never enter adapter requests. Secret and response buffers are zeroized when
  their owning Kernel values are dropped.
- Refresh, access-token resolution, disconnect, and revocation serialize on the
  same credential-handle lock while the global credential-store lock is never
  held across provider network work.
- Sync work carries a durable account generation bound to provider, tenant,
  credential handle, and granted capabilities. Page, retry, rebuild, and repair
  commits verify connected health plus the same generation transactionally;
  disconnect increments the generation before clearing sync data.
- Bounded one-shot mail search and calendar list grants do not authorize durable
  delta sync. Inbox and calendar sync require separate account capabilities and
  a future schedule must disclose scope, retention limits, and a disconnect
  path before activation.

## Scheduling and recovery threats

| Threat | Required control |
| --- | --- |
| Two wakeups claim one window | transaction plus unique `(definition_id, trigger_window_key)` |
| Restart repeats a side effect | durable invocation fingerprint, idempotency key, evidence and verification |
| Network timeout hides remote success | `reconciliation_required`; query remote state before retry |
| Old evidence completes a new mutation | provider receipt binds account, capability, target, request fingerprint, idempotency key and reconciliation state |
| Waiting approval is retried as failure | waiting states excluded from retry policy |
| Missed windows create a storm | explicit `skip` or bounded `run_once`; no implicit run-all |
| Long history disappears from replay | indexed projections and entity queries, not bounded global replay |
| Stale sync worker rolls cursor backward | per-stream revision CAS in the same transaction as projection changes |
| Delta token leaks through audit | opaque continuation stored only in the sync-state table; receipts expose booleans and revisions only |
| 410 rebuild leaves ghost content | projection clear and cursor rebuild commit in one CAS transaction |
| In-flight HTTP completes after disconnect | account-generation CAS rejects late page, retry, rebuild, and repair commits |
| Retry loop survives restart forever | persistent bounded attempts/backoff for rate, network, invalid response, and consecutive rebuild failures |
| Sync becomes a mailbox mirror | shared success/failure retention invariant: 500 items/stream, 8 LRU streams, 1,000 items/account, 2 MiB/account, 32 KiB/item, 90-day idle expiry, and disconnect cleanup |
| Provider returns calendar content outside consented range | adapter validates every normalized upsert overlaps the typed request window before persistence |
| Credential deletion succeeds but SQLite disconnect completion fails | durable `disconnect_pending`; idempotent startup repair completes only with the same generation-bound ticket |
| Crash between SQLite disconnect intent and vault deletion | durable `disconnect_pending`, generation-bound ticket, idempotent delete, and bounded startup sweep |
| Crash after vault deletion but before SQLite completion | `AlreadyAbsent` is a valid idempotent result; startup completes only with the same pending generation and account binding |
| Token endpoint redirects or returns an oversized/error body | fixed organizations token endpoint, redirects disabled, 64 KiB Content-Length and streaming caps, normalized secret-free errors |
| Microsoft omits `offline_access` from response scope | remove only that protocol scope, then require exact normalized access-scope equality; any missing or extra resource scope fails closed |

## Approval threats

- A broad capability grant cannot authorize a connector mutation.
- Approval binds the complete normalized mutation preview to one automation run
  and one tool invocation fingerprint.
- The Kernel derives an immutable mutation envelope from the validated Tool
  request and recomputes its fingerprint; callers cannot supply an independent
  fingerprint or substitute another account, target, capability, or key.
- Critical one-shot approval is uniquely reserved in the same transaction that
  advances Tool and connector state to running. It is consumed before any
  provider call, not after success.
- Changing recipient, subject, body hash, attendees, time, provider, account,
  target, or idempotency key invalidates approval.
- Rejected or expired approval cannot be consumed by another run.
- Scheduled execution never weakens a capability's interactive approval rule.
- Review items use monotonic revisions. Accepted/rejected items cannot be
  reopened, and stale copies cannot overwrite a newer review decision.

## Attachment controls

The unreachable attachment path now reserves a single-use Critical Tool permit
bound to provider, account, message, attachment, metadata, account generation,
exact visible preview, and hashed workspace identity before the provider `$value`
request can be sent. Microsoft metadata explicitly excludes
JSON `contentBytes`. The binary transport uses the fixed Graph origin, disables
redirects, checks Content-Length and Content-Type, and streams into Safe Landing.
Safe Landing permits only a managed workspace destination, normalized filename,
20 MiB maximum, exact declared/extension/detected type agreement, UTF-8/JSON
validation, bounded Office archive entry/expansion limits, and no macros,
embedded objects, executable bits, scripts, or binary active content. Event Store
reloads the durable account/metadata/workspace authority before network access and
checks generation plus workspace identity again after staging. A frozen receipt
then advances `reserved -> ready`, rename occurs, and connected-generation CAS
advances `ready -> completed`. Failed execution first claims `cleanup_required`,
terminalizes the Tool without provider/path secrets, and only then deletes exact
reservation-derived paths. Startup claims stale work before connector workers,
and unsafe identity changes become `repair_required`. The path never
auto-opens or executes the result, and emits only a relative landing reference,
hash, size, type, and untrusted-evidence receipt. Unsupported or ambiguous types
fail closed. Completed or uncertain-commit files are never selected by failure
cleanup. Browser automation is a disclosed fallback and cannot capture or
bypass login credentials for a connector. The permit now comes only from a
dedicated transactional exact-preview workflow; generic Tool and permission entry
points reject this capability. Indexed current projections use row revisions and
idempotent event application. Raw provider refs are isolated from Kernel events
and durable landing metadata, then deleted when the active download reaches
`ready`. Windows workspace/file identity uses held no-reparse handles plus
volume/FILE_ID values; validation and rename stay on the same file handle,
cleanup deletes only an exact receipt identity, and both sides of the ready/rename
crash window reconcile without guessing by path. Staging persists identity before
the first byte; completed files have bounded expiry/count/bytes and a runtime
cleanup worker with persistent backoff. OPC content types and relationships are
XML-parsed; malformed declarations and external/URI targets fail closed.

Recovery ownership is a persistent fencing boundary, not an in-memory worker
convention. Each claimed row stores an opaque token and a 300-second expiry. The
worker renews that exact row/token before rename, delete, or validation, and all
completion, defer, repair, and failure updates require the same token with an
unexpired lease. A second claimant cannot overwrite a live token; after expiry it
receives a different token, and the stale worker can no longer mutate durable
state. Runtime ready recovery is limited to an explicitly due deferred row or an
expired prior claim, so it cannot steal the normal `mark_ready -> complete` window.
Startup respects future backoff. On Windows, `runtime-instance.lock` is acquired
before Event Store open and recovery; startup token reset therefore applies only
to the previous process. This safety claim is Windows-first: non-Windows attachment
activation remains unsupported until it has equivalent single-instance and
handle/inode identity guarantees.

Missing `storage_identity` does not authorize a guessed delete. No-reparse probes
must confirm that both reservation-derived basenames are absent before the row can
fail cleanly. Any unknown file presence is preserved for repair; transient access
is deferred. A missing or corrupt Tool projection quarantines only that row as
`recovery_projection_unavailable`, without touching the file or aborting the rest
of the batch. That Recovery Center card is path/token-free and non-retryable until
the projection is repaired. Recovery Center never receives paths, provider refs,
credentials, storage identities, claim tokens, or lease expiry; its opaque action
fingerprint binds the exact repair state, and retry only queues local FILE_ID-
checked cleanup with a secret-free audit event. The runtime worker cannot claim
active `reserved/staging` downloads; only the startup sweep runs after the Windows
single-instance guard and before connector workers may treat those rows as
abandoned.

## Recovery presentation and action threats

| Threat | Required control |
| --- | --- |
| Provider body or internal error is shown as recovery copy | DTO uses bounded reason/effect/next-step enums; Tauri list/retry errors and UI errors are fixed local text |
| Sync continuation, stream fingerprint, tenant, credential handle, remote ref, or provider evidence leaks through a card | Decode and identity-check private state, then emit only opaque derived item id, safe account label, bounded sync kind, and timestamps; dynamic marker tests scan serialized DTOs |
| Frontend invents a state transition | Only a tagged Kernel-issued action is clickable; attachment command recomputes its fingerprint and CASes the exact current row |
| `revocation_pending` is treated as authority to call or retry the provider | The card remains read-only; only the private durable saga can claim work, and no live registry or Tauri command is activated |
| Process crashes around remote revocation | Commit `remote_call_started` before provider access; startup converts that uncertain window to reconciliation required and never replays it |
| Provider error or ambiguous response is guessed to mean no effect | Discard raw text and persist only `uncertain`; only typed `known_not_applied` may schedule a bounded retry |
| Local credential is deleted before remote revocation is known | Local deletion is reachable only from lease-fenced `remote_confirmed`; deletion is idempotent and completion accepts `already_absent` after restart |
| A stale revocation worker uses reconnected account authority | Freeze provider, tenant, credential handle, capabilities, and incremented generation; claim, remote-start, outcome, and completion all revalidate the same binding |
| Uncertain external mutation is blindly replayed | `reconciliation_required` has no action and explicitly tells the user to verify provider state |
| Recovery worker accidentally gains write authority | Compile it against `ConnectorMutationReconciler` only; that trait exposes no `apply_mutation`, and source guards must reject an apply call in the worker |
| Old account approval queries with newly reconnected credentials | Freeze account generation in the Tool fingerprint/envelope/projection; claim, renew, and completion require the same current connected generation |
| Two reconciliation workers complete one uncertain mutation | Persist opaque 300-second claim lease; live claims cannot be overwritten, expiry creates a new token, and stale renew/defer/complete all fail |
| Provider error leaks through recovery persistence | Discard provider error text, persist only fixed backoff state, and scan SQLite with a dynamic marker |
| Stopped sync directly calls a provider from Recovery Center | `sync_exhausted` is read-only; a future resume action may only update a consent/generation/revision-bound durable schedule, never call the provider in the IPC transaction |
| Arbitrary or unexpected capability is projected as harmless read sync | Only stopped `mail_sync_inbox` and `calendar_sync_events` states are accepted; other capabilities fail the list safely without exposing decoder details |

Residual attachment action-token debt remains explicit. The current fingerprint
binds landing id, failure kind, workspace identity, storage identity, and current
timestamp, while the transition also CASes the row timestamp and later cleanup is
FILE_ID/lease fenced. Before broader v1.0 activation it should become an opaque
Kernel action token covering independent row revision, account generation,
request/landing binding, and every other non-secret field that changes action
authority. This debt does not authorize weakening the existing exact check.

## Mandatory gates before live providers

1. Fake credential store and provider contract tests.
   The same explicit runner suite must finish exact capability coverage; metadata
   checks are pure, legitimate empty reads pass, and bounded typed read/sync output
   remains untrusted evidence.
2. SQLite, event, error, export, DTO, conversation-persistence, and model-prompt
   scans prove marker tokens and full content do not leak.
3. Concurrent refresh proves one refresh and atomic replacement.
4. Disconnect/revoke races fail closed.
5. Exact approval, timeout reconciliation, and duplicate-write tests pass.
6. Provider-specific live OAuth/account activation remains absent until these
   gates pass and the user makes a fresh explicit decision.
7. The provider adapter passes offline typed Mail/Calendar contract, fixed-host,
   hostile continuation-link, response-size, normalized-error, durable-consent,
   retry-budget, retention-budget, and disconnected-account tests before live
   account access is enabled.
8. Cross-store disconnect recovery reconciles DPAPI vault deletion with SQLite
   account health before a real account can be activated.
9. The real token endpoint passes offline POST/form, no-redirect, response-size,
   expiry, scope, rotation, invalid-grant, timeout, and secret-leak tests before
   a real account can be activated.
