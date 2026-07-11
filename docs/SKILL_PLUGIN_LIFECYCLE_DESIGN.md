# DS Agent Skill and Plugin Lifecycle Design

Status: implemented design for the v0.3.0 feature line.

## 1. Product outcome

DS Agent must turn a natural-language instruction containing a GitHub or
Hugging Face repository URL into a working, persistent capability. The user
does not choose an installer path, package format, version, manifest, or update
strategy. DS Agent discovers those details, validates them, installs the
capability, refreshes the runtime catalog, and uses it automatically when a
later task needs it.

The same lifecycle must cover capabilities created by DS Agent itself:

1. install from a GitHub or Hugging Face URL;
2. create a Skill or plugin from a user request;
3. activate it automatically from ordinary chat;
4. check remote sources on every app launch and upgrade in the background;
5. disable or uninstall it from the plugin manager or an explicit chat request;
6. preserve provenance, integrity, audit, rollback, and the DeepSeek/DS Agent
   ownership boundary throughout.

An explicit user instruction such as "install this repository", "make a Skill
for this workflow", or "uninstall this plugin" authorizes that requested
lifecycle operation. DS Agent must not insert format-selection dialogs,
preview/confirm steps, or repeated approvals. If a package cannot pass local
policy, DS Agent leaves the prior state unchanged and reports the exact blocked
reason instead of asking the user to make a security decision.

## 2. Terminology and UI taxonomy

- **System Skill**: shipped with DS Agent and required for a core product
  behavior. It can be updated or repaired but not uninstalled. The built-in
  Skill/Plugin Builder belongs here.
- **Skill**: one hash-verified declarative instruction entry that DeepSeek may
  select for a matching task. Its later tool actions remain independently
  validated by DS Agent.
- **Plugin**: an installable package that owns one or more Skills plus metadata,
  capabilities, and allowed DS Agent action contracts. A plugin is not
  permission to execute arbitrary native code.
- **Scenario template**: a first-party workflow shortcut. Operations Briefing
  is a scenario template, not an installed plugin, and must not be rendered in
  the installed plugin catalog.

The plugin manager presents separate groups for System Skills, Installed
Skills, Installed Plugins, and Scenario Templates. Only installed Skills and
plugins expose lifecycle controls. Scenario templates remain outside the
installed catalog.

## 3. Architecture boundary

DeepSeek owns:

- understanding whether the user wants installation, creation, removal, or a
  task that benefits from an installed capability;
- proposing `skill_install`, `skill_create`, `skill_activate`, or
  `skill_uninstall` in the structured agent envelope;
- generating declarative Skill content when the user asks DS Agent to create a
  capability.

DS Agent owns:

- URL recognition, source resolution, download, archive limits, path safety,
  format discovery, deterministic adaptation, manifest validation, integrity,
  permissions, installation, version comparison, upgrade, rollback, removal,
  audit, and runtime catalog refresh;
- deciding whether a package is compatible with the safe runtime;
- exact binding between the current user instruction and the lifecycle action;
- preventing unknown scripts, binaries, hooks, or dependencies from executing.

The model can request a lifecycle action. Only DS Agent can commit it.

## 4. Supported source contract

### 4.1 Canonical source identity

Every remote installation records:

- provider: `github` or `huggingface`;
- canonical repository URL;
- requested reference, if present;
- resolved immutable revision (Git commit or Hugging Face revision);
- package subpath when the URL targets a directory;
- source format discovered by the adapter;
- content SHA-256 for every installed entry;
- automatic update policy, default `automatic`;
- last check, last success, and last failure summaries.

The canonical identity is provider + repository + package subpath. A revision
change upgrades the same installation; it must not create a duplicate catalog
item.

### 4.2 Discovery order

For a repository or directory URL DS Agent downloads one bounded source archive
at the resolved revision and discovers, in order:

1. a DS Agent package manifest (`plugin.json`, `skill.json`, or
   `manifest.json`) that declares `ds-agent.skill.v1` or the new plugin bundle
   schema;
2. standard Skill directories containing `SKILL.md`;
3. Claude plugin metadata plus referenced Skills;
4. a root `SKILL.md`;
5. a root `CLAUDE.md` as a single prompt-pack compatibility fallback.

Multiple discovered Skills are installed as one plugin bundle. Deterministic
compatibility adapters synthesize DS Agent manifests but never rewrite the
instruction bodies with local model judgment.

Repositories containing only executable code, binaries, lifecycle hooks, or
unsupported dependency installers are rejected as incompatible. The archive is
never executed during discovery.

### 4.3 Network behavior

The source fetcher accepts only HTTPS GitHub and Hugging Face endpoints, resolves
redirects one hop at a time, revalidates every destination, limits response and
expanded archive size, rejects non-public destinations, and ignores accidental
loopback proxy routing for direct public repository downloads. A failure cannot
fall back to ordinary `browser_browse`; installation has its own source tool.

## 5. Persistent lifecycle model

The existing append-only event store remains authoritative. Add lifecycle
events instead of mutating old installation rows:

- `skill.installed`: first committed version with stable installation ID;
- `skill.updated`: same stable ID, previous/new revision and complete new
  manifest/entry snapshot;
- `skill.update_checked`: remote revision comparison result;
- `skill.update_failed`: bounded failure and retained-version evidence;
- `skill.enablement_changed`: disabled/enabled state;
- `skill.uninstalled`: removal tombstone;
- `skill.generated`: provenance for a DS Agent-created package.

`list_skill_records` folds those events into the latest active version. Each
record exposes kind, system protection, source identity, installed revision,
latest revision, update state, update timestamps, entry count, and rollback
revision.

An update is committed only after the complete candidate package passes the
same preflight as a first install. The previous snapshot remains available for
rollback. A failed candidate writes failure evidence but does not change the
active catalog.

## 6. Automatic lifecycle flows

### 6.1 Install from URL

1. DeepSeek proposes `skill_install` with the exact URL from the user message.
2. DS Agent binds the proposal to that message and canonicalizes the source.
3. The provider adapter resolves the current immutable revision.
4. DS Agent downloads and discovers the bounded package without executing it.
5. The compatibility adapter builds one validated package candidate.
6. DS Agent commits the install atomically and refreshes the in-memory/runtime
   catalog.
7. The visible result reports installed names, version/revision, and source.

There is no preview dialog or confirmation step.

### 6.2 Create a Skill or plugin

The built-in System Skill "Skill/Plugin Builder" teaches DeepSeek the supported
schemas and asks it for a bounded declarative package proposal. DS Agent then:

1. validates schema, entry size, capability names, and permission declarations;
2. creates deterministic local provenance and content hashes;
3. runs manifest and activation self-tests without invoking later task tools;
4. commits and enables the package;
5. refreshes the runtime catalog.

Generated packages use local semantic versions and changelog events. They have
no remote update check unless the user later publishes and links a source.

### 6.3 Startup update

After app state is available, DS Agent starts one non-blocking update sweep.
The main window and chat remain usable. For each active remote installation:

1. resolve the latest remote revision;
2. skip when it matches the installed revision;
3. download, discover, and validate the candidate when it differs;
4. commit the update atomically and refresh the runtime catalog;
5. retain the current version and record a failure if any step fails.

Update checks are deduplicated per launch and bounded in concurrency. Offline
startup is successful and uses installed capabilities. A candidate that adds
unsupported executable content or expands permissions is rejected automatically
without replacing the current version.

### 6.4 Automatic activation

Only trusted, installed, entry-complete records enter the compact runtime
catalog sent to DeepSeek. The catalog contains stable IDs, names, descriptions,
capabilities, and entry kinds. DeepSeek selects a matching Skill and proposes
`skill_activate`; DS Agent verifies the stored hash and injects the declarative
instructions as evidence. Subsequent tools remain independently validated.

### 6.5 Disable and uninstall

Disable removes the capability from the runtime catalog while preserving its
data. Uninstall:

1. rejects protected System Skills;
2. writes a tombstone for the stable installation ID;
3. removes it from runtime selection immediately;
4. removes package-owned staged files and private cache entries;
5. preserves only bounded audit/provenance and rollback evidence;
6. never deletes shared dependencies or user artifacts.

The plugin manager exposes one-click Disable/Enable and Uninstall actions. An
explicit click or chat request performs the action directly; no second modal is
shown.

## 7. Security invariants

- Remote content is data until validated; discovery never executes repository
  code.
- Only known declarative entry kinds enter the first runtime.
- Archive traversal, symlinks, device paths, hidden executables, dependency
  installers, and oversized content are rejected.
- Source URLs and every redirect remain public HTTPS GitHub/Hugging Face
  destinations.
- Installed entries are hash-verified again on every activation.
- A remote update cannot silently expand permissions or change package identity.
- All lifecycle operations are append-only audited and exactly bound to a user
  request or the startup automatic-update policy.
- A failed install/update/uninstall leaves the previously committed catalog
  state usable.
- DeepSeek never approves its own local execution authority.

## 8. UI behavior

The left plugin hub becomes a lifecycle view rather than a read-only demo:

- a compact automatic-update status line;
- System Skills with a protected badge;
- installed plugin/Skill cards with name, type, version or short revision,
  source, update status, description, Disable/Enable, and Uninstall;
- empty-state guidance that tells users to paste a GitHub/Hugging Face URL into
  chat;
- no manual manifest textarea, package-format picker, update button, or security
  decision dialog.

Operations Briefing moves to Scenario Templates and is no longer injected as a
fake plugin card.

## 9. Implementation slices and verification gates

### Slice A - lifecycle kernel

- Add canonical source identity, package kind, update policy/state, stable
  update events, event folding, system protection, and rollback metadata.
- Regression: a newer revision replaces the visible version under the same ID;
  failed update keeps the previous version; protected entries cannot uninstall.

### Slice B - repository adapters and automatic install

- Add GitHub/Hugging Face URL parsing, immutable revision resolution, bounded
  archive fetch, repository discovery, and deterministic compatibility adapters.
- Regression fixtures cover a DS manifest, `skills/*/SKILL.md`, Claude plugin,
  root `SKILL.md`, root `CLAUDE.md`, multi-Skill bundle, blocked executable, bad
  redirect, path traversal, and duplicate install.
- Live verification uses the referenced
  `https://github.com/multica-ai/andrej-karpathy-skills` repository.

### Slice C - chat actions and builder

- Add `skill_install`, `skill_create`, and `skill_uninstall` action contracts,
  validation, executors, audit evidence, and DeepSeek prompt instructions.
- Ship the protected Skill/Plugin Builder System Skill.
- Regression proves one user message completes installation/creation without an
  intermediate decision and a later matching task activates the capability.

### Slice D - startup updater and manager UI

- Start one background update sweep during app initialization and refresh state
  after completion.
- Replace the read-only catalog with lifecycle cards and move Operations
  Briefing to Scenario Templates.
- Regression proves startup remains usable offline, newer safe content updates,
  unsafe content does not replace the active version, and uninstall removes the
  capability from automatic selection.

### Slice E - completion audit

- Run focused Rust and JavaScript regressions, production frontend build,
  broader Rust suite, source/secret hygiene, and `git diff --check`.
- Visually verify the real plugin hub and chat-driven install/create/uninstall
  flow in the desktop app.
- Verify the installed v0.2.3 app only as the before-state reference. Do not
  publish or install a new DS Agent version without explicit user instruction.

## 10. Definition of done

The feature is complete only when evidence proves all of the following:

- a user can paste a supported GitHub or Hugging Face repository URL into chat
  and DS Agent installs compatible Skills automatically;
- the reference Karpathy repository is discovered without requiring a ZIP URL
  or DS Agent-specific manifest;
- installed capabilities persist across restart and are selected automatically
  for later matching tasks;
- DS Agent checks remote revisions on launch and safely auto-upgrades without
  blocking chat;
- update failure or policy expansion preserves the last working version;
- the plugin manager can disable, enable, and uninstall ordinary installations;
- protected System Skills cannot be uninstalled;
- DS Agent can create, validate, install, and later use a new declarative Skill
  or plugin bundle from a natural-language request;
- Operations Briefing is classified as a Scenario Template rather than an
  installed plugin;
- no lifecycle step asks the user to choose an implementation format or repeat
  an approval already contained in the explicit instruction;
- tests, build, visual runtime evidence, and requirement-by-requirement audit
  all pass.
