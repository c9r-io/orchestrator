---
lifecycle: active
related_fr: FR-175
---

# DD-194: The Guide Documented An Egress Boundary The Code Never Had

**Status**: Released

## The problem

SecretStore values are encrypted at rest and redacted in persisted snapshots. Two
authorized read commands serialized the live in-memory config and emitted them in
cleartext:

| Egress | Site (at `44328020`) | Least role that reaches it |
|---|---|---|
| `manifest export -o yaml` | `core/src/service/resource/mod.rs:527` | **read-only** |
| `manifest export -o json` | same fn, shared `builtin_docs` | **read-only** |
| `debug --component config` | `core/src/service/system.rs:38` | admin |
| GUI `manifest_export` | `crates/gui/src/commands/manifest.rs:73` → same RPC | read-only |

The in-memory config holds decrypted values by design — the load path runs
`decrypt_resource_spec_json` over every resource, because env injection needs
them. So every surface that renders a whole config is an egress, and whether it
leaked depended entirely on whether its author remembered to redact first. Two
authors did and two did not.

`ManifestExport` sits in the read-only role's allowed set
(`crates/daemon/src/control_plane.rs:748`); `ConfigDebug` requires admin (`:774`).
The wider of the two leaks was the one reachable by the least-privileged role.

## What step 0 found that the FR did not

FR-175 was filed on the premise that the egress boundary had never been named —
that DD-17 scoped redaction to logs and export was simply outside anyone's
statement of the contract. Two of its premises did not survive verification.

### The contract *was* written down, in the user guide, and it was false

- `docs/guide/02-resource-model.md:331`, the EnvStore/SecretStore comparison
  table: *"Export and overview | Values shown | **Values replaced with a
  placeholder before leaving the daemon**"*
- `docs/guide/05-advanced-features.md:96`: *"a SecretStore spec is encrypted at
  rest, **redacted on export**, …"*

Both were introduced by `92f0ef26` (FR-166, 2026-08-13) — the FR whose entire
purpose was to write down the three behaviours that distinguish two kinds with
byte-identical specs. It stated the difference *behaviourally*, exactly as the
authoring rule requires, and never measured it. One of the three named behaviours
had never existed.

That reclassifies this work. It is not "adding a missing concept"; it is making a
shipped promise true. And it means the guide had been telling a read-only user
that `manifest export` output was safe to share.

**The generalizable half**: FR-166's own lesson, now §4.4 shape 11, is about
choosing a *measure* rather than a proxy. The failure here is upstream of that —
the behaviour was described correctly and never executed. A behavioural claim
written into a guide is an assertion; it earns the same treatment as an assertion
in code, which is to be run against the system before it ships. Prefer, for any
"kind A does X and kind B does not" table: name the command that demonstrates the
difference, and run it.

### The redactor already existed twice, and only one copy was tested

`sanitized_config_snapshot` had two byte-identical private definitions —
`core/src/config_load/persist.rs:30` and
`core/src/persistence/repository/config.rs:100` — each with its own private
`serialize_config_snapshot` wrapper. Only the first was covered by
`persist_raw_config_encrypts_secret_store_resources_and_redacts_snapshots`.

FR-175's requirement 3 said "reuse it, don't write a second one, because two
copies would drift and only one would be under test". That was correct reasoning
about a state the repository was already in. The requirement therefore became
*collapse to one*, not *relax the visibility of the one*.

### Two other claims, checked and confirmed rather than assumed

- **The CRD half of export is clean.** `export_crd_documents` reads
  `custom_resources`, which cannot hold a SecretStore: the loader populates it
  only for non-builtin CRD kinds under an `is_builtin_kind` guard
  (`persistence/repository/config.rs:388-397`), and the apply dispatcher routes a
  `kind: SecretStore` document to `ParsedManifest::Builtin`, never to `Custom`.
  The FR's table asserted nothing either way here; the redactor's doc comment now
  records why the field is skipped, so the next reader does not have to re-derive
  it.
- **No crate boundary blocks reuse.** All four sites are in `agent-orchestrator`,
  and the three export helpers, though `pub`, have zero consumers outside it — so
  their signatures were free to change. Requirement 3's escape hatch was not
  needed.

## The design

### One redactor, behind a type

`core/src/config_load/redact.rs` holds `RedactedConfig`: a newtype over
`OrchestratorConfig` with one constructor, and the constructor redacts. It is
`#[serde(transparent)]`, so a caller that needs the whole document gets the shape
it had before, minus the values.

The three export helpers in `core/src/resource/export.rs` —
`export_manifest_resources`, `export_manifest_documents`, `export_crd_documents` —
now take `&RedactedConfig`. That is the mechanism, not the redaction call itself:
a future export path **cannot be written** that forgets, because there is nothing
else to hand them and no second way to build one.

Both duplicate `sanitized_config_snapshot` bodies and both
`serialize_config_snapshot` wrappers are gone; the four call sites (two snapshot
writers, `export_manifests`, `debug_info`) go through the one type.

### Both stores, and why neither half is redundant

`RedactedConfig::new` walks two places, because different egress paths read
different ones:

- typed `projects[].secret_stores` — what `export_manifest_resources` reads, and
  the only place it looks;
- `resource_store` SecretStore CRs — the mirror `crd::writeback` maintains, which
  a whole-config serialization renders and `export_manifest_resources` never
  touches.

Redacting only the typed map leaves `debug --component config` leaking through the
mirror. Redacting only the mirror leaves `manifest export` leaking through the
typed map. Each half has its own test, at both the unit and the service layer.

### The residue, named rather than implied

Types close the manifest-export family. They do not close raw serialization:
`debug_info` reaches its output through `serde_yaml::to_string`, which serializes
whatever it is handed. That site redacts by call-site rule, held by
`service::resource::tests::debug_component_config_redacts_secret_store_values` and
by the QA gate's sentinel sweep — not by the compiler.

**This is a known limit and not a closed one.** A *new* surface that serializes
`read_active_config(state).config` directly would be caught by neither until
someone adds a check for it. Closing it would mean making `ActiveConfig` itself
non-serializable at the boundary, which is a larger change than this FR carried.
Recorded here so the next author finds it stated rather than discovers it.

## Verification

Seven acceptance criteria, mapped in
[QA 232](../../qa/orchestrator/232-secret-egress-redaction.md).

Every assertion is a **pair**: the value is absent *and* `[ENCRYPTED]` is present.
Absence alone is satisfied by an output that dropped the SecretStore entirely,
which is a different bug wearing this bug's colour.

Three mutations were run, each **commented out rather than deleted** — deletion
is the case the author had in mind, so it proves less. Each produced a verdict
with named surfaces, never a timeout and never a silent abort:

| Mutation | Verdict | What it showed |
|---|---|---|
| redaction removed from `export_manifests` | 9 passed, 4 failed | both formats named independently; sweep named both files |
| redaction removed from the `debug` arm | 11 passed, 2 failed | surface named; sweep named `debug-config.txt` |
| `store.data.clear()` instead of the placeholder | 9 passed, 4 failed | **the sweep passed** |

The third is the one worth keeping. Under it the secret is genuinely absent from
every output, so the whole-run sentinel sweep — the most thorough-looking check in
the gate — reports clean. Only the placeholder-present half fails. A gate built on
the sweep alone would have certified an operator's total loss of visibility into
which keys a store defines as a successful redaction.

### Why the gate is manual-runbook

`scripts/qa/test-secret-egress-redaction.sh` starts a daemon and holds a cleartext
test secret on disk for the run's duration, which is the shape every comparable
daemon gate in this repository carries (`test-expert-resources-governed-editing`,
`test-control-plane-action-audit`, `test-attention-inbox` are all
manual-runbook). The security property itself is **CI-enforced** at the service
boundary by six behavioural tests that `cargo test --workspace` runs on every
push. The E2E gate adds the real-daemon, real-CLI evidence that acceptance
criterion 7 asks for, and it exists because the originating ticket was first
probed at the pure-function layer — which is where the leak looked closed and was
not.

The gate announces its own truncation: an EXIT trap prints `TRUNCATED:` when the
summary line was never reached, because an early abort otherwise reads exactly
like a completed run to anyone holding the exit code.

The sweep carries a **control**. It plants the secret in a file and confirms the
sweep finds it before reading the real outputs, because a sweep that cannot see a
secret it is standing on reports clean for the same reason a genuinely clean tree
does.

## Consequences for users

`manifest export` is no longer a restorable backup. It was one, accidentally, and
only for anyone who did not mind their secrets being in it. An exported manifest
re-applied is refused by `[secret_value_placeholder_rejected]`, naming the key —
the refusal FR-171 added, which is what makes redaction safe: without it, a
redacted export applied back would overwrite real secrets with the literal
placeholder, silently and irreversibly.

Read commands are for inspecting which keys a store defines. They are not a backup
of the values, and now nothing is.

## Known limits

Carried forward from FR-175, unresolved rather than silently closed:

- **The backup capability has no replacement.** Whether operators need a separate
  encrypted export paired with a restore command was not evaluated, and nobody
  measured whether anyone was using `manifest export` as a backup.
- **Existing exported artifacts were not inventoried.** Anything exported before
  this change — archived, committed, pasted into an issue — carries cleartext
  secrets. No remediation was assessed and no count was taken.
- **Non-CLI/GUI egress is unverified.** Event payloads, task logs and
  Slack/webhook projections run a separate `redaction_patterns` mechanism
  (DD-17:140, DD-127:142). Single-method, unverified.
- **EnvStore is unchanged and unaudited.** It stores non-sensitive values by
  design and is neither encrypted nor redacted. Nobody checked whether anyone has
  put a secret in one; if they have, that is a different problem.
- **The raw-serde egress is a rule, not a type**, as described above.

## Related

- [DD-17](17-envstore-secretstore-agent-env.md) — the original design, whose
  redaction scope was logs and task output.
- [DD-189](189-resource-observability-tiers.md) — FR-171, which recorded this leak
  as an untouched known limit.
- [QA 232](../../qa/orchestrator/232-secret-egress-redaction.md).
