---
lifecycle: active
related_fr: FR-076
---

# GUI Release Packaging And Signing

**Status**: Released (FR-076 requirements 2–4; final acceptance verified
at v0.5.0 — GUI assets on the Release page, all platforms)
**FR**: FR-076（GUI 正式发布 — 需求 2–4：打包、Release 集成、签名）
**QA**: docs/qa/orchestrator/204-gui-release-packaging.md
**Predecessor**: docs/design_doc/orchestrator/165-gui-ci-integration.md
(requirement 1) — this record completes the same FR's release half.

## The problem

After requirement 1 put the GUI crate under CI, nothing shipped it: no
bundles, no release job, no signing. The FR asked for a universal macOS
`.dmg`, a Linux `.AppImage`/`.deb`, GitHub Release assets named
`orchestrator-gui-{version}-{platform}.{ext}`, and either Apple signing or
a documented quarantine workaround.

## Decisions

### The gui-build job lives outside the CLI build matrix

`test-release-publish-surface.sh` derives the CLI/daemon shipped-target
set from release.yml's literal `target:` matrix keys and requires it to
equal install.sh's `SUPPORTED_TARGETS` and the Homebrew formula's url
stanzas. The GUI ships through neither channel, so its job uses a `label`
matrix key and writes target triples only inside `run:` lines, which the
anchored extraction cannot see. The alternative — teaching the gate a GUI
exemption — would have widened a deliberately narrow derivation for zero
enforcement gain.

### Verification never dispatches release.yml

release.yml's dispatch input publishes a real GitHub Release and runs the
homebrew and crates.io jobs; a rehearsal tag would either publish garbage
or attach main-built artifacts to an existing tag's release page — the
exact provenance lie FR-151 recorded (crates.io 0.4.0 built one commit
after its tag). The gui-build steps were proven instead by a byte-identical
copy in `gui-bundle-proof.yml` on a throwaway branch, artifact-upload only:
run `30702006709` (unsigned round: bundles, naming, universal binary,
checksums) and run `30702717801` (signing round: the conditional
signing/notarization step executed on the runner). The two rounds form a
negative/positive control pair for the signing step — round one's dmg
assessed `rejected, source=no usable signature`, round two's
`accepted, source=Notarized Developer ID`.

### Signing is conditional on secret presence, and empty never reaches Tauri

The Tauri CLI treats the presence of `APPLE_CERTIFICATE` as intent and
fails on an empty value rather than skipping. The workflow therefore
exports the signing and notarization variable groups to `GITHUB_ENV` only
when their secrets are non-empty, logging which path it took. Absent
credentials degrade to an unsigned artifact and the documented
`xattr -d com.apple.quarantine` install recipe — never to a failed
release. Signing (three secrets) and notarization (three more) degrade
independently.

### Identity custody

The Developer ID Application certificate (`HH48FMK8YB`, G2 Sub-CA, expires
2031-08-02) was issued from a locally generated CSR; the private key never
left the machine. Local custody: `~/.orchestrator-signing/` (mode 700;
key, cert, legacy-cipher `.p12`, its password file, all mode 600), the
identity imported to the login keychain. CI custody: `APPLE_CERTIFICATE`
(base64 p12) / `APPLE_CERTIFICATE_PASSWORD` / `APPLE_SIGNING_IDENTITY`
plus `APPLE_ID` / `APPLE_PASSWORD` (app-specific) / `APPLE_TEAM_ID`
repository secrets. The `.p12` uses legacy PKCS12 ciphers deliberately:
OpenSSL 3's AES/PBKDF2 default fails macOS `security import` with "MAC
verification failed".

### The stale Tauri version pin and the placeholder icon went with this change

`tauri.conf.json` pinned `version: 0.1.0` against a 0.4.0 crate; the field
is gone, the config now inherits the crate version, so bundle names track
releases without a second version to forget. The 32×32 `icon.png` — whose
upscaled artifacts would have shipped inside every bundle — was replaced
by a 1024px source (SVG-rendered) and a regenerated `.icns`/`.ico`/PNG
set; the Windows-store `Square*` outputs are not tracked because no
Windows bundle exists.

## What the v0.5.0 release proved, and what it caught

The limit this record originally carried — "the publish-job wiring gets
its behavioral proof on the next release" — was exact. At v0.5.0 (tag on
`58166a9f`, run `30703915075`) both gui-build legs and the publish job
concluded success, and the release page carried the three GUI `.sha256`
files **without the binaries they describe**: the publish glob list
(`dist/*.tar.gz`, `dist/*sha256*`) predated the GUI job and matched the
checksums but not the `.dmg`/`.AppImage`/`.deb`. A green job and a
checksum on the page are each proxies for "the artifact ships" (§4.4
shapes 1 and 2 — two enumerations of one surface, never compared). The
gap was closed by uploading the same run's artifacts (checksum-verified
against the already-published `.sha256` files) and the enumeration is now
compared mechanically: `check_gui_asset_globs` in
`test-release-publish-surface.sh` derives the staged set from the staging
step's cp destinations and requires a covering publish glob for each
artifact and its checksum, with fixture 6 commenting out `dist/*.dmg` and
the check reproducing the historical defect verbatim against the
release.yml recorded at the v0.5.0 tag.

The released assets themselves were behaviorally verified: the released
dmg assesses `accepted, source=Notarized Developer ID` with a valid
staple and a universal (x86_64+arm64) binary; the released `.deb`
installs in a clean ubuntu:24.04 container and its binary runs under
Xvfb; the released `.AppImage` runs the same way after the standard
binfmt magic-byte workaround required under QEMU emulation (see QA-204
scenario 3 notes).

## Known limits

- **No Windows `.msi`**: no Windows runner job exists and the workspace
  has never compiled on Windows; the FR marks it "如支持" and this record
  marks it not attempted.
- **crates.io Trusted Publishing covers 5 of 12 crates**: at v0.5.0 the
  OIDC token published proto, config, collab, security and runner, then
  403'd on `orchestrator-persistence` ("token is not valid for crate"),
  leaving 7 crates to a local-token publish from the exact tag commit —
  the FR-151 bootstrap precedent, with clean provenance this time. Until
  Trusted Publishing configurations are added on crates.io for the
  remaining seven (persistence, agent-orchestrator, scheduler, client,
  cli, slack-gateway, orchestratord — a crate-owner web-UI action), every
  release's crates-io job fails at the same crate and the tail must be
  published locally.
- **Notarization rides an app-specific password**, which the account
  owner can revoke at any time; a revoked password fails the notarize
  step, not the signing. Rotation is a secret update, no code change.
- **The proof branch must be deleted after each use** — a stale copy of
  the gui-build steps is exactly the drift QA-203's checklist warns about
  for duplicated enforcement text.
