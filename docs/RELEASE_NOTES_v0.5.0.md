# DS Agent v0.5.0 Release Notes

Status: Windows-first feature release. The `v0.5.0` release supersedes
`v0.4.1` for ordinary downloads.

Repository: https://github.com/Lee-take/dsagent

DS Agent v0.5.0 adds a durable Automation and Artifact Engine foundation for
local office work. Automation definitions and runs now keep revision-bound
source receipts, restart-safe execution state, review queues, evidence and
recovery without depending on a live connected-account provider.

Word, Excel, PowerPoint and PDF outputs now move through a durable lifecycle:
generation, structure checks, actual Office/PDF rendering, bounded automatic
revision and delivery. Rendered pages can be reviewed in the app. Preview pages
are bound to validation evidence, and missing or changed completed files return
to a needs-attention state instead of continuing to appear complete.

The release also includes the provider-neutral connected-account foundation:
typed Mail and Calendar reads, bounded sync state, attachment landing,
revocation and recovery boundaries, and redacted public status views. Production
provider execution remains disabled until a provider is explicitly configured;
this release does not silently connect Microsoft or Google accounts.

Permissions remain local and explicit. DeepSeek handles understanding,
reasoning and content generation, while DS Agent owns tool authorization,
paths, execution, templates, validation, evidence, recovery and delivery state.
Office writes require exact approved paths. Validation revisions are limited to
three named sibling files and never overwrite the approved original.

The release was validated with the full Rust and desktop test suites, source
hygiene checks, real Microsoft Office rendering for DOCX/XLSX/PPTX, PDF
rendering, and a restart test covering actual preview persistence.

Operations Briefing live smoke tests use
`docs/templates/operations-briefing-smoke-evidence` by default. The bundled
smoke files are marked as `SMOKE SAMPLE evidence for local verification only`
and `Replace before operational use`.
The desktop seed action uses blank operator templates under
`docs/templates/operations-briefing-evidence`, not smoke or business data.

Bumps the package, desktop, Tauri, Cargo, and updater metadata to `0.5.0` /
`v0.5.0` so installed Windows clients can detect this release as newer than
`v0.4.1`.

The Windows installer remains unsigned, so Windows may show an
unknown-publisher warning.

## Installer Integrity

SHA-256: `B4618BD8D40160B97981393DE0DA2FDB8CEC4D2D4E3980C314AB343645343E6D`
