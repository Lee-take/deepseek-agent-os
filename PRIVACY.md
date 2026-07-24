# Privacy Policy

Last updated: 2026-07-25

This policy describes the current published DS Agent desktop application and
public project. DS Agent is local-first and does not operate a project cloud
backend, advertising service, or project analytics or telemetry service.

## Information kept locally

DS Agent keeps settings, audit and recovery events, task state, memory, and
other runtime state under the operating system's application-data location.
User-selected workspaces hold approved evidence, exports, reports, work
packages, screenshots, and other artifacts. This information is not silently
synced to a DS Agent-operated server.

The current stable `v1.5.0` accepts one user-supplied DeepSeek API key through
the onboarding screen and stores it in a dedicated Windows DPAPI-protected
local vault. A process-environment key remains an explicit compatibility
fallback and is never silently copied into that vault. The project does not
provide a shared key. The raw key exists in frontend memory only while it is
being entered and submitted; it must not be retained in UI state, events, logs,
exports, screenshots, work packages, or verification receipts.

Goal contracts, exact-task grouped authorizations, lifecycle events, and UI
projections are bounded and designed
to exclude raw secrets, provider bodies, absolute app-data or vault paths,
local authority material, and internal claim tokens. The read-only UI receives
stable status/reason codes, bounded goal text, revision/fingerprint values, and
coverage counts plus a redacted authorization intent; it does not receive the
private capability proposal, a preparation/compiler command, or a local
completion writer.

The v1.5.0 package preserves the T1 Office verification engine, which keeps source identities, bounded fact
provenance, artifact hashes, render receipts, revision receipts, and
continuation checkpoints locally in the selected workspace or application data.
Ordinary chat does not yet automatically select or sequence the two T1 tools;
their presence does not cause workspace files to be scanned or uploaded merely
because the application is open.

The v1.5.0 durable Computer Use lifecycle stores bounded application, process,
window, frame, target, action, approval, screenshot-evidence, and semantic
receipt identities locally. Its installed reliability tests use generated
files, workbooks, data, fresh Edge profiles, an ephemeral loopback portal, and
isolated Event Stores. Those tests do not read stored browser logins, production
accounts, or the user's normal DS Agent AppData.

Uninstalling the application may not delete a user-selected workspace or every
application-data file. Review and remove those local locations separately when
you no longer want to retain them.

## Network activity initiated by a user action

DS Agent transfers information to networked systems only for work requested by
the user or person operating the application:

- **DeepSeek.** Explicit credential verification contacts DeepSeek's balance
  and model-list endpoints. Model-backed work sends the user's prompt and the
  bounded context selected for that request to the configured DeepSeek API.
  The user-supplied API key is used for authentication. DeepSeek processes
  those requests under its
  [privacy policy](https://cdn.deepseek.com/policies/en-US/deepseek-privacy-policy.html).
- **Web search and visited sites.** Source-linked web search can send the search
  query to DuckDuckGo, a user-configured route, or an optional user-started
  local bridge. Opening or submitting a requested web page sends normal
  browser requests to that destination. The destination's privacy policy
  applies; see [DuckDuckGo's privacy policy](https://duckduckgo.com/privacy).
- **GitHub.** User-requested update checks, installer downloads, and supported
  GitHub skill-source operations contact GitHub release, API, download, or
  source endpoints. GitHub's
  [privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement)
  applies.
- **Hugging Face.** A user-requested skill-source operation can contact a
  selected Hugging Face repository. Hugging Face's
  [privacy policy](https://huggingface.co/privacy) applies.
- **Microsoft WebView2.** The Windows desktop shell uses Microsoft Edge
  WebView2. WebView2 diagnostic data is controlled by Microsoft and Windows
  settings as described in Microsoft's
  [WebView2 data and privacy documentation](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy).

The optional local desktop bridge accepts only loopback addresses and is
started and controlled by the user. DS Agent does not install or supervise that
service. Production Microsoft and Google account registration and live
mail/calendar writes are disabled in `v1.5.0`; offline connector contracts do
not authorize a production account or external write.

## What can be included in a model request

Depending on the action the user approves, a request can include chat text,
selected memory, excerpts from selected local files, source-linked web content,
or a description of the desired artifact. DS Agent is designed to package only
the context needed for the requested work, but users remain responsible for
not selecting information they are not permitted to send to the configured
service.

Local paths, credentials, provider response bodies, screenshots, and file
contents are not uploaded merely because the application is open. Computer
control and local file mutations require the product's permission boundaries;
approval of a local action does not grant unrelated network authority.

## User choices

Users control whether to configure or remove a DeepSeek key, run its explicit
verification, request model-backed work, run web search, open a web destination,
import a remote skill source, or start an optional local bridge. Remove the
relevant credential or do not invoke the network-backed capability to prevent
that route from being used.

For a sensitive security or privacy issue, use
[GitHub Private Vulnerability Reporting](https://github.com/Lee-take/dsagent/security/advisories/new).
Do not include API keys, private files, tokens, screenshots, or private local
paths in a public issue.

Material changes to reachable network behavior or data handling require an
update to this policy before the changed release is published.
