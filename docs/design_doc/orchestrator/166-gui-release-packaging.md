---
lifecycle: active
related_fr: FR-076
---

# GUI Release Packaging And Signing

**Status**: Released (FR-076 requirements 2–4; the FR's final acceptance —
GUI assets on a real Release page — lands with the next version tag)
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

## Known limits

- **No Windows `.msi`**: no Windows runner job exists and the workspace
  has never compiled on Windows; the FR marks it "如支持" and this record
  marks it not attempted.
- **The release page acceptance is unverified until the next real tag**:
  the proof workflow validates the job's steps, not the
  `publish`-job wiring (`needs: gui-build`, the `dist/*` glob). That wiring
  is one derived hop (`workflow_model.rb`) and gets its behavioral proof
  on the next release; the FR stays open until then.
- **Notarization rides an app-specific password**, which the account
  owner can revoke at any time; a revoked password fails the notarize
  step, not the signing. Rotation is a secret update, no code change.
- **The proof branch must be deleted after each use** — a stale copy of
  the gui-build steps is exactly the drift QA-203's checklist warns about
  for duplicated enforcement text.
