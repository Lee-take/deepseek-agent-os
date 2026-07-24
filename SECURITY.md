# Security Policy

DS Agent is a local-first Windows desktop agent. DS Agent v1.5.0 is the current
published stable release and is not an official DeepSeek product. Security
reports are welcome, especially around local credentials, permission gates,
audit records, Computer Use boundaries, update integrity, code signing, and
package import/export behavior.

## Supported Version

| Version | Supported |
| --- | --- |
| 1.5.0 | Supported |
| 1.4.0 | Supported |
| 1.3.0 | Supported |
| 1.2.0 | Supported |
| 1.1.0 | Supported |
| 1.0.2 | Supported |

## Reporting A Vulnerability

Use GitHub Private Vulnerability Reporting on this repository for sensitive
security reports. Do not open a public issue with exploit details, secrets,
screenshots, tokens, or private local paths.

Include:

- affected commit or release;
- operating system;
- steps to reproduce;
- expected and actual behavior;
- whether the issue can expose secrets, local files, screen contents, input
  control, mailbox data, or exported work packages.

## Security Boundaries

- Current stable v1.5.0 stores one user-supplied DeepSeek API key in a dedicated
  Windows DPAPI vault. A process-environment key is an explicit compatibility
  fallback and is not copied into the vault. Presence alone is never treated as
  verified readiness; balance and required V4 model checks produce only a
  secret-free local receipt. Raw keys, provider bodies, account details, and
  absolute vault paths must not be stored in events, retained UI state, logs,
  screenshots, or exported work packages.
- A model-returned `GoalEnvelope` is untrusted input. The Kernel owns validation,
  freeze, revision/fingerprint binding, verifier evidence, and completion. Model
  text, frontend state, approval state, or artifact existence cannot directly
  mark a goal complete; stale, unknown, failed, duplicate, mismatched, or
  incomplete evidence fails closed.
- A model-returned capability proposal is descriptive only. The Kernel binds it
  to the exact frozen task/Goal, derives the manifest/risk/preview, and persists
  one grouped authorization. Approval/rejection/revocation bind the exact task,
  group, projection, manifest, preview, revision, fingerprint, hash, scopes,
  targets, and expiry. Approval grants only exact authority; it does not execute
  a Tool, resume a task, or mark a Goal complete.
- The v1.5.0 package preserves the T1 reconciliation and PowerPoint executors,
  which require exact workspace paths, non-overwriting output, grouped authorization, persisted artifact
  identity, verifier evidence, and actual local render evidence. DeepSeek cannot
  approve those actions or mint their evidence or completion receipts. The
  ordinary chat UI does not yet automatically select or sequence these T1 tools.
- The v1.5.0 durable Computer Use lifecycle binds the application, process,
  window, frame, target, action, approval, screenshots, and semantic receipt.
  `ActionStarted` is persisted once before the effect; ambiguous outcomes fail
  closed and are not replayed. Installed reliability evidence covers only
  generated File Explorer and Excel targets plus installed Edge with a fresh
  profile and an ephemeral loopback test portal. It is not evidence of general
  browser control, stored-login reuse, or production-site completion.
- `pnpm test:secrets` scans tracked and unignored repository files for live
  `sk-` style keys and non-empty `DEEPSEEK_API_KEY` assignments without printing
  candidate values.
- Manual DeepSeek pricing settings are local configuration, not live price
  claims.
- Computer Use remains experimental and high-risk. Computer control requires
  explicit approval plus a short local unlock window.
- Optional local desktop bridge use requires a user-started local loopback bridge
  in this preview.
- Web search evidence must preserve source URLs.
- Import writes memories as reviewable candidates, not automatic long-term
  memory.
- Release identity must follow the [code signing policy](CODE_SIGNING_POLICY.md).
  An unsigned or invalidly signed artifact must not be represented as a signed
  release. DS Agent v1.5.0 is explicitly disclosed as Authenticode `NotSigned`;
  Windows may show `Unknown publisher` or a Microsoft Defender SmartScreen
  warning. See also the [privacy policy](PRIVACY.md).

## Out Of Scope For Alpha

- Hosted cloud sync.
- Real email sending or cloud-drive modification.
- DS Agent does not install, launch, or supervise local bridge services.
- Arbitrary third-party executable plugins.
