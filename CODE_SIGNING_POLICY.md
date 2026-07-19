# Code signing policy

Last updated: 2026-07-19

## Current status

DS Agent `v1.3.0` is intentionally published unsigned. Both `ds-agent.exe` and
the Windows x64 NSIS installer are expected to report Authenticode `NotSigned`.
Windows may therefore display `Unknown publisher` or a Microsoft Defender
SmartScreen warning. Users should download only over HTTPS from the official
GitHub Release and verify the published SHA-256 before running the installer.

The SignPath Foundation application is submitted and approval is pending. No
DS Agent binary may be represented as SignPath-signed until the application is
approved and a later-version artifact independently verifies as Authenticode
`Valid`. The project will not replace the immutable `v1.1.0`, `v1.2.0`, or `v1.3.0` tag,
Release, or asset if signing becomes available later.

For releases accepted into that program: **Free code signing provided by
[SignPath.io](https://signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).**

## Team roles

DS Agent is currently maintained in the public
[`Lee-take/dsagent`](https://github.com/Lee-take/dsagent) repository by one
maintainer:

- Authors and committers: [`Lee-take`](https://github.com/Lee-take).
- Reviewers for outside contributions: [`Lee-take`](https://github.com/Lee-take).
- Signing approver: [`Lee-take`](https://github.com/Lee-take).

Every signing request requires a manual decision by the signing approver. An
API token or successful build is never sufficient approval by itself. Team
members with repository or signing access must use multi-factor authentication.

## Source and build provenance

Only DS Agent artifacts built from the project's own public source may be
signed. A signing-eligible release must:

1. originate from an exact commit in `Lee-take/dsagent`;
2. build on a GitHub-hosted Windows runner through a reviewed workflow;
3. upload the unsigned artifact to the same GitHub Actions run before it is
   submitted for origin-verified signing;
4. sign `ds-agent.exe` before it is packaged into NSIS, then separately sign
   the resulting NSIS installer;
5. keep product name and product version metadata consistent across every
   signed file; and
6. receive the required manual signing approval.

Locally built binaries, manually uploaded unsigned replacements, artifacts from
another repository, and artifacts whose source commit cannot be verified are
not eligible. Private keys and signing API tokens must never be committed to
the repository. Provider identifiers may be added only after SignPath assigns
them; guessed or placeholder identifiers are forbidden.

## Release verification

For the unsigned `v1.1.0`, `v1.2.0`, and `v1.3.0` exceptions, maintainers verify and disclose the actual
`NotSigned` status of both the application executable and installer. Evidence
must bind the exact source commit, file name, product version, byte size, and
SHA-256. The installer downloaded back from GitHub must match the reviewed
Release asset exactly. An unexpected signer, signature identity, hash, version,
source, or asset mismatch stops publication.

For any later release explicitly described as signed, maintainers must also
verify the expected signer subject, certificate chain, timestamp, and
Authenticode `Valid` status independently for both files. An invalid, missing,
expired, unexpectedly issued, or unbound signature stops that signed release.

## Project purpose and restrictions

DS Agent is a permissioned local desktop agent for ordinary office, file,
research, and automation work. Its terminal, browser, and Computer Use
capabilities are designed for user-approved work; they are not designed to
discover or exploit vulnerabilities, steal credentials, or bypass security
controls. Signing must stop if the distributed product no longer matches that
scope or the [privacy policy](PRIVACY.md).

## Incidents and policy changes

Report suspected signing misuse or a compromised release through
[GitHub Private Vulnerability Reporting](https://github.com/Lee-take/dsagent/security/advisories/new).
Do not put secrets or exploit details in a public issue. Affected publication
and signing requests must stop while the incident is investigated, and the
signing provider must be notified when revocation or other certificate action
may be required.

Changes to this policy are reviewed through the public repository history.
