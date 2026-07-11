# DS Agent v0.4.1 Release Notes

Status: Windows-first repository-cleanliness patch release. The `v0.4.1`
release supersedes `v0.4.0` for ordinary downloads.

Repository: https://github.com/Lee-take/dsagent

This patch removes the tracked `.codegraph` directory marker from the public
source tree. CodeGraph is a maintainer-side repository analysis tool and is not
part of DS Agent, its source code, dependencies, runtime, or installer.

The repository root now ignores `.codegraph/` completely, including local
databases, logs, process metadata, and any future tool-generated files. The
published source tree therefore no longer displays a `.codegraph` directory.

DS Agent v0.4.1 otherwise preserves the v0.4.0 native Subagent parallel work
and automatic Skill/Plugin catalog refresh behavior.

Operations Briefing live smoke tests use
`docs/templates/operations-briefing-smoke-evidence` by default. The bundled
smoke files are marked as `SMOKE SAMPLE evidence for local verification only`
and `Replace before operational use`.
The desktop seed action uses blank operator templates under
`docs/templates/operations-briefing-evidence`, not smoke or business data.

Bumps the package, desktop, Tauri, Cargo, and updater metadata to `0.4.1` /
`v0.4.1` so installed Windows clients can detect this release as newer than
`v0.4.0`.

The Windows installer remains unsigned, so Windows may show an
unknown-publisher warning.
