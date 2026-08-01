---
lifecycle: active
related_fr: FR-076
---

# Orchestrator - GUI Desktop Bundles Are Built, Signed, Notarized, And Named For Release

**Module**: Release pipeline / `crates/gui` desktop bundles
**Scope**: the `gui-build` job FR-076 requirements 2–4 added to
`.github/workflows/release.yml` — universal macOS `.dmg`, Linux
`.AppImage`/`.deb`, the `orchestrator-gui-{tag}-{platform}.{ext}` naming
scheme with per-file sha256; the conditional macOS signing/notarization
step; the Developer ID Application identity and its local custody.
**Scenarios**: 5
**Priority**: High

## Background

Requirements 2–4 of FR-076, implemented 2026-08-01 after requirement 1
(QA-203). The `gui-build` job cannot be verified by dispatching release.yml
with a test tag — that would publish a real GitHub Release and run the
homebrew and crates.io jobs — so its build steps were proven by a
byte-identical copy on a throwaway branch (`gui-bundle-proof.yml`,
dispatch/branch-push only, artifact-upload only), the same
never-touch-main shape as QA-203 scenario 3. Design record:
`docs/design_doc/orchestrator/166-gui-release-packaging.md`.

**Safety**: scenarios 1 and 5 are read-only derivations. Scenario 2 builds
and signs locally (requires the keychain identity). Scenarios 3–4 are
recorded one-time verifications on throwaway branches; re-running them is
optional and never touches `main` or publishes anything.

## Scenario 1: the gui-build job is wired outside the shipped-target derivation

Steps:

```bash
ruby scripts/lib/workflow_model.rb step-names .github/workflows/release.yml gui-build
grep -cE '^[[:space:]]+target: [a-z0-9_]+-[a-z0-9_-]+$' .github/workflows/release.yml
bash scripts/qa/test-release-publish-surface.sh; echo $?
```

Expected result: the job exists with the staging step (`Stage GUI assets
under the release naming scheme`); the literal `target:` key count in
release.yml is exactly 3 (the CLI build matrix, unchanged by the GUI job,
whose matrix key is `label`); and the publish-surface gate passes — the
CLI shipped-target set still equals install.sh and the Homebrew formula.

## Scenario 2: a locally built .dmg signs with the repository's Developer ID identity

Steps (macOS with the keychain identity installed):

```bash
cd crates/gui
APPLE_SIGNING_IDENTITY="Developer ID Application: han chen (Y8ZG3D692W)" \
  ../../gui/node_modules/.bin/tauri build --bundles dmg
MNT=$(hdiutil attach target/../../target/release/bundle/dmg/*.dmg -nobrowse -readonly | tail -1 | awk -F'\t' '{print $NF}')
codesign --verify --deep --strict "$MNT/Orchestrator GUI.app"; echo $?
codesign -dv "$MNT/Orchestrator GUI.app" 2>&1 | grep TeamIdentifier
hdiutil detach "$MNT"
```

Expected result: verify exits 0 and `TeamIdentifier=Y8ZG3D692W`, with the
full authority chain (Developer ID Application → Developer ID CA → Apple
Root CA) and the hardened-runtime flag. Recorded evidence (2026-08-01, at
`70f306d1`): signature valid; the same dmg then passed notarization
(submission `04ecdc56-cf46-426c-a090-149b5ec60032`, status Accepted), was
stapled, and both the dmg and the app inside assess as
`accepted, source=Notarized Developer ID` under `spctl`. The dmg installs
and the app launches: mounted, copied out via `ditto`, `open` — process
alive until killed.

## Scenario 3: the bundle steps produce all three artifacts with the release names (recorded)

Steps: push the `gui-bundle-proof.yml` copy of the job to a throwaway
branch (see Background), let it run, then:

```bash
gh run download <run-id> -D proof-artifacts
find proof-artifacts -type f
(cd proof-artifacts/release-gui-universal-apple-darwin && shasum -c *.sha256)
(cd proof-artifacts/release-gui-x86_64-unknown-linux-gnu && shasum -c *.sha256)
```

Expected result: exactly six files —
`orchestrator-gui-{tag}-universal-apple-darwin.dmg`,
`orchestrator-gui-{tag}-x86_64-unknown-linux-gnu.AppImage`, `…​.deb`, each
with a passing `.sha256` — and `lipo -info` on the dmg's binary reports
both `x86_64 arm64`. Recorded evidence: run `30702006709` (branch head
`3345f148`), both job conclusions `success`, all checksums OK, `file`
confirms a UDZO disk image, a static-pie x86-64 ELF AppImage, and a
format-2.0 Debian package.

## Scenario 4: the CI signing path signs and notarizes when the secrets exist (recorded)

Steps: proof round two adds release.yml's `Enable macOS signing when
credentials exist` step verbatim to the proof workflow; after the run,
download the macOS artifact, mount, and assess:

```bash
codesign -dv "$MNT/Orchestrator GUI.app" 2>&1 | grep TeamIdentifier
spctl --assess --type execute -v "$MNT/Orchestrator GUI.app"
```

Expected result: `TeamIdentifier=Y8ZG3D692W` and `accepted,
source=Notarized Developer ID` — the GITHUB_ENV export path, keychain
import, signing, notarization and stapling all executed on the runner,
not only locally. Recorded evidence: run `30702717801` (branch head
`86bbad39`) — the downloaded dmg's app assessed
`TeamIdentifier=Y8ZG3D692W`, hardened runtime (`flags=0x10000`),
`accepted, source=Notarized Developer ID`, stapler validate OK,
`lipo` reporting `x86_64 arm64`.
Negative control: round one (scenario 3's run) had no signing step and its
dmg assessed `rejected, source=no usable signature` — the pair proves the
conditional step is what signs, not ambient runner state.

## Scenario 5: unsigned fallback stays documented and the secrets stay optional

Steps: `grep -A3 'no signing certificate configured' .github/workflows/release.yml`
and `grep -rn 'xattr -d com.apple.quarantine' CHANGELOG.md docs/qa/orchestrator/204-gui-release-packaging.md`

Expected result: the signing step's else-branch logs the unsigned path
instead of failing (an empty `APPLE_CERTIFICATE` never reaches the Tauri
CLI, which treats presence as intent), and the quarantine-removal recipe
(`xattr -d com.apple.quarantine "/Applications/Orchestrator GUI.app"`)
is stated for installs of unsigned builds.

## Checklist

- [ ] release.yml is never dispatched with a test tag for verification —
      the proof workflow on a throwaway branch is the only rehearsal path
- [ ] signing evidence is a Gatekeeper assessment (`spctl`) on the built
      artifact, never the presence of the signing step in YAML
- [ ] the signed/unsigned pair (scenario 4 vs 3) is kept as
      positive/negative controls when the signing step changes
- [ ] the `label` matrix key stays; a `target:` key in the gui-build job
      breaks the publish-surface derivation (scenario 1 counts 3)
- [ ] notarization credentials are three optional secrets; their absence
      degrades to unsigned artifacts, never to a failed release
