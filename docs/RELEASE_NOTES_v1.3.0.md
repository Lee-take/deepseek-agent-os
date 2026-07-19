# DS Agent v1.3.0

`v1.3.0` adds Step 3 exact-task grouped authorization to the stable Windows
desktop app. Package, desktop, Tauri, Cargo, updater, and installer metadata are
`1.3.0` / `v1.3.0`.

## One exact task authorization

- A normal queued chat task can carry a bounded, descriptive-only capability
  proposal alongside its `GoalEnvelope`. The proposal cannot name internal Tool
  IDs or supply risk, authority, approval, grant, claim, revision, fingerprint,
  preview, or hash values.
- After the same run/task Goal is validated and frozen, the Kernel reloads that
  exact Goal, maps the descriptive needs through its own catalog, derives the
  manifest and risk, renders the authorization preview, and persists one durable
  grouped authorization through the existing Kernel state machine.
- Chat shows the authorization card only when the stored authorization task ID
  equals that message's frozen Goal ID. The private model proposal is not
  serialized to the frontend, and there is no frontend or Tauri preparation or
  manifest-compilation command.

## Exact binding and fail-closed lifecycle

- Approve, reject, and revoke bind the exact task ID, group ID, projection
  revision, manifest revision and fingerprint, preview schema and renderer
  revisions, preview hash, expiry, scopes, targets, and per-capability audit.
- Malformed, stale, cross-task, scope- or target-changed, expired, rejected,
  revoked, tampered, secret-like, private-path, provider-reference, claim, or
  token-bearing input fails closed.
- One user decision resolves the task group while the Kernel retains exact
  per-capability audit and authority checks. DeepSeek and frontend payloads
  cannot approve or revoke their own authority.
- Approval creates only the exact task authority. It does not execute a Tool,
  resume a task, create an Office/connector/Computer Use effect, or mark the
  Goal complete.

## Compatibility and migration

- The grouped authorization tables are additive and restart-safe. Duplicate
  preparation is deterministic; active scope replacement, expiry, rejection,
  revocation, terminal replay, and tamper detection preserve one authoritative
  state machine.
- Existing exact-tool approvals remain compatible and cannot be resolved through
  the grouped per-item audit path. Legacy databases gain no synthetic task
  authority or completion state.
- Existing v1.2.0 GoalEnvelope validation/freeze and evidence completion rules
  remain authoritative. Model text, approval status, Tool prose, frontend state,
  or artifact existence alone still cannot complete a Goal.

## Windows download and integrity

- The Windows GNU release profile strips the target-path-bearing COFF symbol
  table and disables linker-generated PE timestamps. NSIS excludes source-file
  modification times from its data block. The final release gate compares the
  application and installer from two fresh, distinct `CARGO_TARGET_DIR` builds
  byte-for-byte before one canonical installer may be published.

> **Unsigned release:** both `ds-agent.exe` and `DS.Agent_1.3.0_x64-setup.exe` are intentionally Authenticode `NotSigned`.
> Windows may display `Unknown publisher` or a Microsoft Defender SmartScreen
> warning. This is not a signed or SignPath-approved release.

Download only over HTTPS from the official
[`v1.3.0` GitHub Release](https://github.com/Lee-take/dsagent/releases/tag/v1.3.0).
Before running the installer, compare its filename, product version, exact byte
size, and SHA-256 with the values in that Release. The final Release body binds
the sole x64 installer asset to the exact source commit and immutable annotated
tag after exact-main CI and fresh-download readback.

The SignPath Foundation application remains submitted and approval is pending.
If the project is accepted later, signing starts only with a subsequent new
version; the immutable v1.1.0, v1.2.0, and v1.3.0 tags, Releases, and assets will
not be moved, overwritten, or replaced.

## Scope and limits

This release is limited to C3A/C3B/C3C/C3D exact-task grouped authorization:
Kernel manifest/risk/preview derivation, durable grouped state, ordinary chat
production reachability, one read-only authorization card, user resolution and
auditable revocation. It does not add task execution or automatic resume, an
Office golden path, a connector or Computer Use expansion, background external
writes, production accounts, or signed binaries. Production Microsoft/Google
account registration and live mail/calendar writes remain disabled. DS Agent
remains an independent open-source project, not an official DeepSeek product.

Release verification covers the production seam, manifest/risk/preview,
grouped migration/restart/adversarial behavior, Goal lifecycle/completion,
legacy exact-tool compatibility, policy/secret/path/provider-reference
regressions, production frontend build, Rust formatting, full tests,
release-source/secret/binary boundaries, isolated Windows NSIS construction,
PR CI, exact-main CI, and final version/filename/byte-size/SHA-256/source/tag/
Release/Latest/fresh-download readback. The cancelled formal 20-run Windows lab
is not claimed.

## Deterministic briefing templates

`docs/templates/operations-briefing-smoke-evidence` remains deterministic local
test material. The bundled smoke files are marked as
`SMOKE SAMPLE evidence for local verification only` and
`Replace before operational use`; replace them before operational use. The
desktop seed action continues to use the blank operator templates under
`docs/templates/operations-briefing-evidence`.
