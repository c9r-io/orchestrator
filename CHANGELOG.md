# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Agent Process Console v1** (FR-095 through FR-105) — deterministic process timelines and evidence, cross-task Attention Inbox, immutable handoff briefings and reviewed safe resume, governed Session re-entry/control, provider-neutral source bindings with a Slack adapter, canonical mutation audit, and local privacy-safe operational metrics
- **Agent Process Console UI** (FR-100) — Attention-first navigation, integrated Process Workspace, global Session re-entry, stable hash deep links, keyboard triage, role-sensitive actions, responsive/reduced-transparency fallbacks, and request-ID error correlation
- Console release acceptance with populated schema-26 upgrade coverage, nine independently owned slice gates, a real Tauri-to-gRPC recovery flow, release performance fixtures, and the [operator runbook](docs/guide/agent-process-console-v1-operations.md)
- Frontend Vitest and Playwright coverage for route migration, Attention reconciliation, semantic evidence, read-only gates, narrow navigation, accessibility, and visual fallbacks
- **Slack Reaction Skill Automation** (FR-107 through FR-113) — authenticated `reaction_added` ingestion, versioned Skill task templates, exact badge bindings, Slack permalink resolution, canonical task creation, durable retries/Attention replay, and Sources → Automations management
- Slack automation release acceptance with two badges selecting distinct Skill/workflow tasks, concurrent identity convergence, rate-limit restart recovery, real Tauri provenance, populated migration, compatible previous-binary rollback, and the [setup/operations guide](docs/guide/slack-reaction-skill-automation.md)
- **Managed Slack Connections** (FR-114) — one-consent installation of an official shared Orchestrator Slack App, independent internet-facing OAuth/Events Gateway, project-scoped SourceConnection lifecycle, outbound durable delivery, bounded permalink proxy, target-side two-phase ownership transfer, and Sources → Connections management with a [deployment and user guide](docs/guide/slack-managed-connections.md)
- **Dedicated Slack App Provisioning** (FR-115) — an advanced per-workspace private App path with a fixed reviewed manifest, local-only short-lived Configuration Token custody, one-time receipt-gated credential import, per-connection encrypted App identity, exact-App OAuth/events, provisioning Attention recovery, reviewed shared↔dedicated migration, semantic manifest upgrade with suspension/reauthorization, separate typed App deletion, CLI stdin, and a [dedicated setup guide](docs/guide/slack-dedicated-app-provisioning.md)
- **Agent Driver Abstraction** (FR-116) — per-Agent `shell/cli`, `claude/cli`, and `codex/cli` adapters; typed provider-neutral options and workflow requirements; structured apply diagnostics; direct event-stream folding; complete tool/usage/permission event projection; opaque in-memory session attachment; and run-scoped private MCP configuration. See the [driver guide](docs/guide/agent-driver-model.md).
- **Non-code Workspaces and Global File Sharing** (FR-117) — `task` workspaces with an optional persistent `work_dir`, one implicit process item, provider-neutral convergence, private per-task HOME/cwd allocation, operator-owned `fileSharing` ceilings, read-only global Skills, Process Console task semantics, and a reproducible Slack inventory pilot. See the [user guide](docs/guide/non-code-workspace.md).
- **QA gate enforcement surface** (FR-127) — `config/governance/qa-gate-surface.json` classifies every `scripts/qa` gate as `ci-required`, `manual-runbook`, or `scheduled`, and `scripts/qa/test-qa-gate-surface.sh` verifies the classification against the repository: disk and manifest must agree in both directions, every non-CI gate must carry a reason and an owner document, every CI gate must actually be *executed* by the workflow job it claims, every CI gate must have a provider-isolation mechanism that has been executed and shown to reject its own absence, and no tracked Markdown file may claim CI enforcement a gate does not have. `--fixture-test` proves each check rejects an injected defect and that the defect trips only its own check. (The first version of each of those three assertions read text describing the fact rather than observing it, and FR-134 reproduced a defect against each; the wording here is the repaired form.) See [DD-139](docs/design_doc/orchestrator/139-qa-gate-enforcement-surface.md) and [QA 177](docs/qa/orchestrator/177-qa-gate-enforcement-surface.md)
- **Governance ledger regeneration and review tooling** (FR-128) — `scripts/qa/coordination-governance.rb` gains `--emit-inventory` and `--emit-baseline`, which print regenerated candidates for the reviewed sections of `config/governance/coordination-collapse-ledger.json`, and `--write`, which applies a candidate and refuses to run when `CI` is set. The inventory emitter is the same expression the gate compares, so a candidate cannot diverge from the checked value. A mismatch now names the added, removed, and changed Agents together with the differing top-level `spec` keys, recovered from `git show HEAD:<file>`; when the spec was committed without its ledger update the report says so instead of guessing. `scripts/qa/test-governance-ledger-tooling.sh` covers all of it in the `governance` job. See [DD-140](docs/design_doc/orchestrator/140-governance-ledger-regeneration.md) and [QA 178](docs/qa/orchestrator/178-governance-ledger-regeneration.md)
- **Skill single source and mirror integrity** (FR-129) — `config/governance/skill-mirrors.json` declares `.claude/skills` as the sole authoritative source, the mirror roots that must carry it, the entries that are not skills, and the exemptions; `scripts/qa/test-skill-mirror-integrity.sh` verifies coverage in both directions per root, canonical symlink shape, policy freshness, the absence of any tracked `SKILL.md` outside the source tree, and — the check no structural test substitutes for — that every `<root>/<name>/SKILL.md` opens as a non-empty regular file. All 29 skills are now mirrored into both `.agents/skills` and `.cursor/skills`, with no exemptions. `--fixture-test` proves each corruption shape is caught, including one case where shape is perfect and only the read fails, and asserts that no check can be dropped from the registry or left without a negative fixture. See [DD-141](docs/design_doc/orchestrator/141-skill-mirror-integrity.md) and [QA 179](docs/qa/orchestrator/179-skill-mirror-integrity.md)
- **Core crate boundary freeze and migration schema baseline** (FR-130, requirements 1 and 3) — `config/governance/core-boundary-ledger.json` records `core`'s public surface and every `rusqlite` reference per file; it froze at 52 top-level `pub mod`, 924 public items across 143 files and 200 references across 37 files, and FR-130 Phase A has since moved it to 50 / 665 / 129 and 86 across 20 (the point of the ledger is that the figures move only through a reviewed diff, so they are not quoted here as fixed); `scripts/qa/core-boundary.rb` compares it by exact equality in both directions, with `--emit-baseline` / `--write` for reviewed regeneration and a refusal to write under `CI`. `config/governance/schema-snapshot.sql` records the 46 tables and 92 indexes the 37 registered migrations produce, and `core/src/persistence/schema_snapshot.rs` holds the chain to it: one-shot equivalence, idempotency, and resume-to-identical-schema after an interruption at every one of the 37 steps. (FR-130 originally recorded 74 here and in four other documents; the chain has 37 entries, and 74 is what `grep -c m00` over `migration.rs` returns, because each entry names its step twice — once in `name:` and once in `up:`. The sweep always ran the right number; only the prose was wrong, and FR-130 Phase A's new extent assertion is what surfaced it.) Every migration from now on arrives with its schema delta in the same diff, which was previously unreviewable. `scripts/lib/rust_source.rb` is the single Rust source scanner both governance ledgers use. `scripts/qa/test-core-boundary.sh` covers all of it in the `governance` job, with every case confirmed against a targeted mutation. See [DD-142](docs/design_doc/orchestrator/142-core-boundary-freeze.md) and [QA 180](docs/qa/orchestrator/180-core-boundary-freeze.md)
- A `governance` job in `.github/workflows/ci.yml` executes the gate surface check, `qa-doc-lint.sh`, and six previously unexecuted gates on every push and pull request, behind `claude`/`codex` stubs that turn an accidental real-provider invocation into a build failure instead of silent credential and quota spend. (FR-134 later found that being wired into a job is not the same as being able to run in it: three gates in two *other* jobs were exiting on a missing `rg` before their first assertion, and the stubs existed only here)
- **Documentation publishing single source and link integrity** (FR-131) — `config/governance/docs-publishing.json` declares the published collections, their per-locale sources, and the translation gaps that a page is allowed to have; `scripts/sync-docs.mjs` reads it instead of a hardcoded table, so the generator and the gate cannot disagree about what is published. `scripts/qa/test-docs-publishing-integrity.sh` verifies policy freshness, source inventory, that no generated page is tracked, that the declared and produced sets match per locale in both directions, that two syncs of unchanged sources are byte-identical, and — the pair nothing has ever checked — that every navigation link resolves to a produced page and every produced page is linked. It proves the publish set by running the generator into `$TMPDIR` and diffing trees, never by comparing filenames, and derives the expected set independently of the generator. `scripts/qa/test-markdown-link-integrity.sh` resolves every relative link in 567 tracked markdown files, with ten positive fixtures asserting the shapes it must *not* report: site routes, anchors, extensionless targets, symlinked directories, code spans, and fenced blocks. Both run in the `governance` job and assert that no check can be dropped from their registry or left without a negative fixture. See [DD-143](docs/design_doc/orchestrator/143-docs-publishing-integrity.md) and [QA 181](docs/qa/orchestrator/181-docs-publishing-integrity.md)
- **Design doc and QA doc lifecycle governance** (FR-132) — every file under `docs/design_doc/` and `docs/qa/` now declares `lifecycle: active | superseded` in YAML frontmatter, with `superseded_by` naming the successor and an optional `related_fr`. All 378 documents that existed at the time were backfilled in one pass — the tree now holds 380, of which 377 are active, 3 superseded, and 244 carry a feature-request attribution recorded rather than guessed — so there is no exemption list and no ratchet to defeat. `scripts/qa/doc-lifecycle.rb` parses the frontmatter with a real YAML parser (the block sequences and `#` comments already in `docs/qa` defeat a `key: value` regex), walks coverage from the filesystem rather than a roster, rejects a `superseded_by` that dangles, points at its own document, or forms a cycle, and generates `config/governance/doc-lifecycle-index.json` — the reverse index giving document → feature request and superseded → successor, the directions `docs/feature_request/README.md` cannot express. The index is compared by exact equality in both directions and `--write` refuses to run under `CI`. `scripts/qa/test-doc-lifecycle.sh` covers all of it in the `governance` job, with each of its twelve cases confirmed against a targeted mutation. The field is not called `status` because 71 design docs already carry a `**Status**:` header meaning implementation maturity — an independent axis, since DD-101 is `Released` *and* superseded. See [DD-144](docs/design_doc/orchestrator/144-doc-lifecycle-governance.md) and [QA 182](docs/qa/orchestrator/182-doc-lifecycle-governance.md)
- **bash 3.2 compatibility gate** (FR-135) — `scripts/qa/bash32-compat.rb` scans every file `git ls-files '*.sh'` reports, with no roster and no exemption list, for the seven constructs bash 3.2 rejects: empty-array expansion under `set -u`, `declare -A`, `mapfile`/`readarray`, `${x^^}`, `local -n`, `wait -n`, and `shopt -s globstar`. Because `BASH_COMPAT=3.2` restores none of those semantics — measured against bash 5.3 for each — the static scan cannot be the whole check and cannot run anywhere useful on Linux. `scripts/qa/test-bash32-compat.sh` pairs it with a fixture corpus *executed* under the real `/bin/bash`, where each hazardous form must fail and its prescribed replacement must succeed; on a bash 4+ host those cases report `SKIPPED` with a warning rather than passing, and one case reads the parsed workflow to assert some macOS job actually runs the gate, so the skip cannot become a silent hole. `scripts/qa/test-coverage-governance-mainpath.sh` runs the coverage script's shell main path with cargo, node, npm, npx, rustc and rg shadowed by stubs, asserting the exact `cargo llvm-cov` argv in both the empty and non-empty branch cases and that `--fixture-test` still never reaches it. Eighteen mutations, including restoring the original defect, each caught. Both gates run on the fixtures job's ubuntu and macOS legs. See [DD-146](docs/design_doc/orchestrator/146-bash32-compatibility.md) and [QA 184](docs/qa/orchestrator/184-bash32-compatibility.md)
- **Persistence dependency chokepoint** (FR-136) — the decision about who may hold the SQLite driver, made before `orchestrator-persistence` is extracted rather than during it, and recorded in `config/governance/persistence-dependency-ledger.json`. `core` (later `orchestrator-persistence`) is the layer; `orchestrator-scheduler` and `daemon` are forbidden and carry a frozen residual until FR-130 pays it down, `task_state.rs` included; `orchestrator-security` is exempt with its reason (core depends on *it*, so it cannot route through a layer above itself); `slack-gateway` is out of scope because `GatewayStore::open` opens its own `SLACK_GATEWAY_DATABASE` and its 56 SQL statements never touch `agent_orchestrator.db`; `integration-tests` is frozen to `[dev-dependencies]`. `scripts/qa/persistence-dependency.rb` holds it with two independent conditions, because a manifest check is a proxy for the rule it claims to enforce: `AsyncDatabase::writer()` returns `&tokio_rusqlite::Connection` and `conn.execute(sql, [])` names no driver at all, which is why `orchestrator-security/src/secret_store_crypto.rs` runs four production SQL statements with zero `rusqlite` references and a manifest gate reports it clean. So who may *declare* the driver is read from the workspace `members` list with each manifest parsed by section, and who may *use* it is frozen per file as a count of SQL statements and driver references compared by exact equality in both directions. `scripts/qa/test-persistence-dependency.sh` covers all of it in the `governance` job with eighteen assertions, including two the gate must **not** reject — without them, "fails whenever a manifest changes" and "enforces a per-crate chokepoint" have the same green record. FR-136's own inventory did not survive verification: it reported 75 references across 23 files, counted by a `grep` that includes test code, where the scanner its cited ledger uses finds 55 across 15. See [DD-147](docs/design_doc/orchestrator/147-persistence-dependency-chokepoint.md) and [QA 185](docs/qa/orchestrator/185-persistence-dependency-chokepoint.md)
- **Persistence crate extraction, Phase A** (FR-130, requirement 2 Phase A) — `crates/orchestrator-persistence` now owns `agent_orchestrator.db`: the connection helpers, the 37-step migration chain, the task/session/scheduler/workflow-store repositories, the async write coordinator, the admin facade, and the row DTOs. Four independently revertible commits took `core` from 143 to 129 scanned files, 52 to 50 top-level `pub mod`, 924 to 665 public items, and 200 to 86 `rusqlite` references across 37 to 20 files, with `config/governance/schema-snapshot.sql` byte-identical throughout and both governance ledgers re-frozen in the same commit as each move. Every `agent_orchestrator::{db, db_write, dto, task_repository, session_store, async_database, persistence::*}` path still resolves through re-exports, so no consumer changed. The structural half of that is held by the two existing ledger gates and would all be equally true of a layer that persists nothing, so `scripts/qa/test-persistence-extraction.sh` adds the half they cannot see: core is compiled with the dependency *commented out* and must fail; the migration resume sweep now asserts its own extent against the rows the database records as applied, so a `step_by` inserted for speed cannot leave it passing over a fifth of the chain; a real write/read round trip crosses every moved module, reading each write back through a different module than wrote it, paired with the same calls against an unmigrated database which must error rather than return a plausible nothing; and cargo's resolved tree must contain no path from the layer back to core. Two files changed phase against the FR's own list, both forced by structure rather than preference: `persistence/repository/config.rs` stayed in `core` (it reads `crd`, which calls back into `db`, so sinking both closes a cycle) and `session_store.rs` moved into Phase A (Phase A's `repository/session.rs` delegates to it). 114 of Phase A's 115 references converged; the remaining one is named and explained rather than rounded away. See [DD-148](docs/design_doc/orchestrator/148-persistence-crate-extraction.md) and [QA 186](docs/qa/orchestrator/186-persistence-crate-extraction.md)
- **Persistence crate extraction, Phase C and part of Phase B** (FR-130) — `impl From<rusqlite::Error> for OrchestratorError` is **removed**, so core's canonical error type no longer names the SQLite driver. FR-130 posed Phase C as a choice between a port-layer error type and accepting the coupling; measurement answered it instead — deleting the impl failed to compile with exactly three errors, all in one seven-line SQL block, which moved to `orchestrator_persistence::db::delete_project_resources`. The `ExternalDependency` category the blanket impl guaranteed is on the wire, so the call site now maps explicitly through a named function pinned by a test using a real error from a real unmigrated database; the near-miss is recorded, because the first version used the message-based classifier and SQLite's phrase for a missing table is `no such table`, which that classifier reads as `NotFound`. Six of Phase B's eighteen files are disposed of — a resource delete moved out, task auto-cleanup split so its retention query and cascade delete go through the repository while filesystem cleanup stays, the config deletion guards given a `db::DeletionGuardQueries` port instead of a raw connection (and now unit-testable with no database), and `events.rs` split at the seam already in it, with the eleven step-event type literals moving *up* into core as a policy constant passed down. Two more files were ledger artefacts rather than refactors and are reported as such: five stale module doc comments, and a `#[cfg(test)]` import at file scope. Every moved statement carries an assertion about its contract rather than about `Ok` — a `DELETE` with a non-matching predicate succeeds and affects nothing, and a retention query returning everything would let auto-cleanup delete running work. That round took core to 75 `rusqlite` references across 13 files, down from 200 across 37 (the second round below carries it further), with a per-file disposition table naming the conclusion for all eighteen files and the blocking reason for the twelve then still open. Recorded alongside it: 61% of Phase B's references are driver-error plumbing rather than SQL, which makes a ~39-reference reduction available with no SQL moved at all — refused, and the refusal is written down. See [DD-148](docs/design_doc/orchestrator/148-persistence-crate-extraction.md) and [QA 186](docs/qa/orchestrator/186-persistence-crate-extraction.md)
- **Persistence crate extraction, Phase B second round** (FR-130) — five more of Phase B's eighteen files and the Phase A residual are disposed of, one file per commit, ledgers re-frozen in each. `service/bootstrap.rs`'s six blank-scope backfill statements and its SecretStore key probe, `action_audit.rs`'s `control_action_audit` table, `task_ops.rs`'s two duplicated task-creation paths (now one transaction), `event_cleanup.rs`'s retention queries, and `source.rs`'s four tables and 24 statements all moved into `orchestrator-persistence`; validation, canonical hashing, deterministic id derivation, retry backoff, lifecycle allowlists and every conflict rule stayed in core. `core::migration`'s three compatibility wrappers were **removed** — zero production callers workspace-wide, and deleting them exposed the drift they were hiding, since the wrapper returned a bare count where the real API returns a summary of what was applied. Where a write, a conditional read, and a rule about what the read means were interleaved, the store now returns a *named case* — `Reservation::{Claimed, PriorByRetryIdentity, PriorByRequestId}`, `CommandActionStart::{Started, Restarted, AlreadySucceeded, RequestMismatch}` — so the statement stays below the boundary and its meaning stays above it. The refused AsyncDatabase closure API is still refused: these moves carry the `writer().call` closure and its error adapter *down with the SQL*, and `AsyncDatabase` gained no methods. Two error shapes went with them: the JSONL event archive no longer writes files inside the SQLite writer's callback, where a full disk was reported as a driver type-conversion failure, and `source.rs`'s stored payload now crosses the boundary as text rather than being parsed inside a row mapper that can only fail as a column conversion. **The finding worth reading is from `source.rs`**: five moved safety guards were each confirmed to be pinned by no test at all — core's 96 `source::` tests stayed green when every one of them was mutated in place. Without them a late worker overwrites another's routing decision, a delivery nobody is routing is handed to a second owner, a poison message retries forever, a retry key reused under a different request is silently restarted rather than refused, and a duplicate delivery is routed twice. All five are now pinned. core is at 44 `rusqlite` references across 7 files, down from 200 across 37. Fourteen of the eighteen files now have a written disposition — eleven moved or split, two kept with the reason recorded and pointed at FR-141, one blocked on `crd` sinking first — and the four still open (`trigger_engine.rs`, `source_automation.rs`, `source_connection.rs`, `handoff.rs`) are named with their shape rather than left as "the rest of Phase B". See [DD-148](docs/design_doc/orchestrator/148-persistence-crate-extraction.md) and [QA 186](docs/qa/orchestrator/186-persistence-crate-extraction.md)
- **Gate surface execution truth** (FR-134) — the enforcement surface gate now decides four things by observing them instead of by reading text that describes them. Wiring is read from parsed workflow steps, so a `run:` commented out with an explanation beside it, a step behind `if: false`, a script named in a `name:` field, and a script mentioned inside a heredoc all stop counting as enforcement. Fixture pinning is associated per agent, because "every claude/codex agent in the bundle declares `binary: fake-*`" is a property of each object that a count over a file cannot express. Path-shadow isolation keeps its text conditions — with comments stripped, which is what had broken them — but they are no longer alone: `scripts/lib/provider_isolation.sh` resolves the provider through the PATH the run will actually use, the parity gate asserts its own isolation on every run, and the surface gate *executes* that assertion against a synthetic PATH in both directions. Each of these was a reproduced defect: all four applied together previously left the gate reporting `5 passed, 0 failed`. See [DD-145](docs/design_doc/orchestrator/145-gate-surface-execution-truth.md) and [QA 183](docs/qa/orchestrator/183-gate-surface-execution-truth.md)
- **CI job liveness** (FR-134) — `config/governance/ci-job-liveness.json` records the last real conclusion of every job in every push-triggered workflow, and `scripts/qa/ci-liveness.rb` verifies it offline. The job list is discovered by parsing `.github/workflows/*.yml`, not enumerated: `qa-gate-surface.json` classifies `scripts/qa/*` and so never contained `boundary-coverage`, `test`, `clippy`, `miri` or `cross-compile`, and `boundary-coverage` had been red for six consecutive runs with nothing obliged to say so. A job present in a workflow and absent from the ledger fails; a non-success record without a written reference and reason fails; an annotation left on a job that has recovered fails; and a record taken before its workflow last changed is stale, because it describes a pipeline that no longer exists. `--refresh` pulls real outcomes from `gh run`, collapses matrix legs to their worst, and refuses to run unattended
- **CI environment parity** (FR-134) — `scripts/qa/test-ci-environment-parity.sh` runs every in-scope `ci-required` gate with the CI variables set and cleared and requires the same exit code. Nothing structural sees this class of defect: `test-governance-ledger-tooling.sh` was wired, its dependencies were present, its assertions were sound, it passed 8/8 on every developer machine, and it had never once succeeded in the job it ran in. Scope is derived from each gate's declared `command -v` preamble through the shared `scripts/lib/gate_preamble.sh`; the cargo-bearing gates are excluded on cost and the limit is stated in the script rather than left to be found
- **Coverage by discovery** (FR-134) — four checks that read a hand-maintained list now derive it. The stale-claim scan reads `git ls-files '*.md'` minus declared exemptions instead of `docs` plus `.claude/skills`, which had left 41 tracked files unread including `README.md`, `CHANGELOG.md` and every crate README. Gate classification recurses `scripts/qa/**`, with a `supportFiles[]` array naming non-gates by role and reason instead of expressing exemption through a glob that fails to match. Mirror-root coverage comes from tracked symlinks pointing into `.claude/skills`, found in the git index rather than trusted from `mirrorRoots`. And `check_job_dependencies`, `check_workspace_scope`, `check_diagnostics_preserved` and `check_provider_stub_coverage` assert that a gate can run in the job that claims it, not merely that the job mentions it
- **Orchestrator-Owned Coordination Tools** (FR-118) — authenticated per-run loopback tool hosting for `run_tests`, `mark_item`, `create_ticket`, `scan_tickets`, and `generate_items`; a transport-only stdio MCP shim; complete daemon/tool event evidence; and a parity pilot that removes all measured CEL/capture/post-action coordination lines. See the [coordination tools guide](docs/guide/coordination-tools.md).

### Changed
- The workspace minimum supported Rust version is now 1.88, allowing the patched `plist`/`quick-xml` dependency chain required by current Tauri releases
- Wish Pool and Progress Observer are now presented as New Process and Processes; resource administration remains reachable through System and raw diagnostics through Process Expert
- Session read and control rollout is globally authoritative from the `_system` RuntimePolicy; ordinary project policies cannot override the fail-closed control gate
- Process Console mutations support `action_audit_mode=compatibility|enforced`; rollout begins in compatibility mode and moves to enforced only after clients send canonical action context
- Explicit driver phases use `setup → start → consume → fold → record`; provider execution is selected only by `Agent.spec.driver`, and every production Agent declares a typed `shell/cli`, `claude/cli`, or `codex/cli` driver
- Structured run signals (`tools_called`, `tool_error_count`, `num_tool_calls`, `agent_reported_error`, `run_cost_usd`) are derived from typed `driver_terminal` and normalized tool artifacts, and item-level signals are promoted into task convergence state before guard evaluation
- Codex CLI cross-step session attachment is certified against `codex-cli 0.144.5` with same-thread context continuity, a sanitized recorded JSONL fixture, an offline replay gate, and an isolated live recertification script
- Workspace manifests serialize the canonical `work_dir` field while continuing to accept legacy `root_path`; existing omitted `kind` manifests retain `code_repo` behavior
- Tool-capable Claude driver runs now receive a private mode-`0600` MCP configuration that forwards stdio JSON-RPC to an authenticated, ephemeral daemon callback; legacy shell/CEL workflows remain compatible
- Showcase documentation is single-sourced like the guide (FR-131): `docs/showcases/*.md` is the English source and `docs/showcases/zh/*.md` the Chinese, and `site/{en,zh}/showcases/**` is generated. The English text was recovered out of `site/en/showcases/`, which held the only copy of it; the top-level paths did not move, so every existing reference still resolves. `scripts/sync-docs.mjs` now empties the destination before writing, so a renamed or deleted source no longer leaves its page on the site forever. `docs.yml` gained `docs/showcases/**` and `scripts/sync-docs.mjs` in its `paths:` filter — a showcase edit previously matched no trigger and could never deploy — and runs the publishing gate before the build
- The `coordination-strangler` and `slack-certification-recorded` jobs install ripgrep (FR-134). Neither runner image ships it, so `test-coordination-strangler.sh`, `test-slack-live-certification.sh` and `certify-slack-managed-live.sh` had been exiting on their own `command -v` preamble — asserting nothing — on every push since they were wired. FR-127's case was "46 gates and only 3 in CI"; at least two of those three were dead. The provider stubs move into a `./.github/actions/provider-stubs` composite action and are installed in `coordination-strangler` too, whose gate rests entirely on the fixture pinning that this FR's third defect defeated
- `test-filesystem-trigger.sh` excludes `orchestrator-gui` and keeps cargo output (FR-134). No job installs the Tauri and webkit dependencies, so the unexcluded workspace was not a duplicate of the sibling `test` and `clippy` jobs but a superset whose extra member cannot build on Linux; DD-139 recorded it as accepted duplication, which was wrong, and it passed locally because macOS supplies those frameworks as system libraries. The gate previously ran cargo as `>/dev/null 2>&1`, so the CI log read `FAIL: cargo test --workspace` and nothing else
- The `governance` job's steps report independently and a final step fails the job (FR-134). A serial job stops at its first failure and reports nothing after it, which is how the workspace-scope defect stayed invisible behind the ledger tooling's self-lock for two runs
- Fourteen VitePress sidebar entries added (FR-131): the previously unpublished typed-driver showcase in both locales, plus twelve pages the site had been generating with nothing linking them — eight EN guide chapters, three ZH, and one ZH showcase

### Removed
- `rusqliteDependentCrates` from `config/governance/core-boundary-ledger.json` (FR-136) — the list of crates taking the SQLite driver directly now lives in `config/governance/persistence-dependency-ledger.json`, so the rule is expressed once. It was a fact about the workspace rather than about core's boundary, and both halves of how `core-boundary.rb` computed it were wrong for the question being asked: the crate list came from a `Dir["crates/*/Cargo.toml"]` glob, so a member declared anywhere else was invisible to it, and the match read the whole manifest, so `crates/integration-tests` sat in the frozen list beside four production crates although its declaration is a `[dev-dependency]`. It also had no negative fixture in `test-core-boundary.sh` — it was frozen, and nothing had ever shown the freeze could fail. `scripts/lib/rust_source.rb` gains `rust_files_under(repo_root, roots)` so a caller that discovers its own roots does not have to restate the exclusion rules; `rust_source_files` keeps its `core/src` plus `crates/*/src` scope, which is correct for the two ledgers that count core
- The unproduced third copy of the orchestrator-guide skill (FR-129) — `skills/orchestrator-guide/**` and `skills/orchestrator-guide.skill` are deleted. `scripts/package-skills.sh` reads `.claude/skills/` and writes `dist/`; it never produced `skills/`, which had no producer, no consumer, and had already drifted ~32KB from the authoritative source. The release package is unaffected, and both user guides already install from `.claude/skills/orchestrator-guide`. Because a deletion is not a rule, the gate's `check_no_content_copies` now rejects any tracked `SKILL.md` outside `.claude/skills/`, so an equivalent copy cannot reappear
- The 36 hand-maintained documentation-site showcase pages (FR-131) — `site/{en,zh}/showcases/**` is untracked and gitignored, joining the guide pages it had been governed differently from since the site existed. Each page was compared against its generated replacement individually before removal: 13 were byte-identical, 19 differed only in link form, and the four with prose differences were the source being newer, except one Chinese section that existed only in the tracked page and was ported back to its source first. `scripts/qa-doc-lint.sh` loses its narrower "site guide files must not be tracked" assertion, which `check_generated_not_tracked` now covers from the policy rather than from two hardcoded paths
- Two unreferenced QA scripts (FR-127) — `scripts/qa/test-qa83-mixed-text.sh`, which wrote to the legacy `data/agent_orchestrator.db` path that `qa-doc-lint.sh` bans in documentation, and `scripts/qa/auto-regress.sh`, an unmaintained generic runner with no callers. The scenarios they covered remain in QA 83 and in the per-topic gates
- **Global runner backend selection** (FR-126) — `RunnerExecutorKind`, the `RunnerExecutor` trait, `ShellRunnerExecutor`, `StreamingAgentRunner`, and the provider-session compatibility bridge are deleted. Agent execution has a single model: the typed driver named by `Agent.spec.driver`. The shared spawn substrate that enforces runner policy, sandbox profiles, resource limits, process groups, environment filtering, and redaction is retained and still serves both drivers and engine-owned Step commands. See [DD-138](docs/design_doc/orchestrator/138-agent-driver-execution-migration.md)
- **Legacy coordination extraction** (FR-125) — the production capture and JSONPath post-action paths are removed; `goal` and the three sandbox-denial fields moved out of the generic string map into preserved execution channels. Deterministic governance CEL, builtins, and public manifest compatibility are retained. See [DD-137](docs/design_doc/orchestrator/137-legacy-coordination-decommission.md)

### Fixed
- The persistence chokepoint gate had an assertion nothing could fail, a missing SQL verb, and a scan narrower than its own scope prose (FR-139). `classification_errors` summed the categorised driver references and required the total to equal `totals["rusqlite"]` — but that total *is* the same reduction over the same hash, so the branch compared the scan to itself; on a `git archive` copy with the other branches disabled it passed on a file carrying one driver reference and no category. DD-147 and QA 185 both stated it as a live guarantee, which is why the removal is recorded rather than done quietly; coverage was never actually missing, since an unledgered file fails the reference freeze. `SQL_STATEMENT` gained `PRAGMA` and nothing else: the ledger moves 112 → 114 with the delta being exactly `orchestrator-security/src/lib.rs` +1 and `slack-gateway/src/store.rs` +1, and that two-sided figure — not a green gate — is the evidence the verb set was corrected rather than relaxed. `VACUUM`, `BEGIN`, `COMMIT` and `WITH` were measured and rejected because every hit outside core is prose, and a case-insensitive match reads 20 help strings in `crates/cli/src/commands/guide.rs` as SQL; a fixture asserts the non-counting direction in the same file a control statement proves is scanned, so its green cannot come from the file being unread. The quote anchor now steps over a leading escape sequence, closing `"\n            SELECT …"` before any such literal exists. And the scan, which read only `<member>/src` while `SCOPE` said "its non-test Rust source", now reads each member's Cargo build script too — five members ship one and `daemon` and `orchestrator-scheduler`, the two `forbidden` crates, are among them, while condition 1 was already counting `[build-dependencies]` as a production declaration. The ledger gained `scanRoots`, the roots the walk actually visited, frozen and compared both ways; the pre-existing scope check compares the ledger's copy of the prose to the constant, which is prose against prose and agreed throughout the period the constant was wrong. `scripts/lib/rust_source.rb` accepts a single file as a root, with `core-boundary.rb` still at `200 / 37` and `52 / 924 / 143` and the coordination ratchets still `53 / 30 / 9 / 0` as the two-sided test that the change is additive. `test-persistence-dependency.sh` goes from twelve assertions to eighteen. See [DD-147](docs/design_doc/orchestrator/147-persistence-dependency-chokepoint.md) and [QA 185](docs/qa/orchestrator/185-persistence-dependency-chokepoint.md)
- The `boundary-coverage` job had never reached a coverage comparison (FR-135). `scripts/coverage-governance.sh` expanded an empty array under `set -u`; bash 4.4+ expands that to zero words and bash 3.2 rejects it as `branch_args[@]: unbound variable`. The job is `runs-on: macos-latest`, where the runner image ships bash 3.2.57 as `/bin/bash` and nothing newer on PATH — confirmed from the runner, not assumed — so it died on its first real command on every run from the commit that created it, 77 commits. Two things hid it: the upload step ran `if: always()` with `if-no-files-found: error`, making "No files were found" the only `##[error]` in the summary while the real failure sat one step above; and the sibling `coverage-policy-fixtures` job, green throughout, runs `--fixture-test`, which `exec`s node on line 16 and shares no line with the failing path. Every expansion of a possibly-empty array across all 95 tracked shell files is now guarded, the one `declare -A` is a `case` lookup and the four `mapfile` calls are read loops, and `if-no-files-found` is `warn` so a generation failure is the first error a reader sees. See [DD-146](docs/design_doc/orchestrator/146-bash32-compatibility.md) and [QA 184](docs/qa/orchestrator/184-bash32-compatibility.md)
- `scripts/qa/test-governance-ledger-tooling.sh` had never once succeeded in CI (FR-134). Its second case verifies that `--write` refuses when `CI` is set; its third case then called `--write`, was refused by the mechanism the case above had just confirmed, and killed the whole gate at `set -e`. The gate's positive path was mutually exclusive with its own safety mechanism, and only in the environment where it actually ran — so it reported `8 passed, 0 failed` on every developer machine while being dead on every push since FR-128 wired it. The two recovery cases now run with the CI variables cleared, and `test-ci-environment-parity.sh` generalises the check to every in-scope gate
- The unattended-write guard on the governance ledgers only recognised `CI` (FR-134). That is a GitHub and Travis convention, not a universal one: a self-hosted runner or a cron job that does not export it wrote the reviewed ledger with no human present, which is the single barrier keeping the review gate from being decoration. `scripts/lib/ci_env.rb` replaces three separate copies of the test — a fourth had appeared while this FR was open — and recognises `GITHUB_ACTIONS`, `GITLAB_CI` and the other common indicators, while treating `CI=false` as interactive
- A brace inside a string literal could hide production code from both governance ratchets (FR-134). `strip_test_modules` counted `{` and `}` textually, so `.body("{")` inside a `#[cfg(test)]` module left the depth counter above zero, the module's range ran to end of file, and every production line after it left the scan — silently, because the hidden lines simply stop being counted and no baseline moves. `scripts/lib/rust_lexer.rb` carries string, char, raw-string and nested-comment state across lines. The obvious fix is worse than the defect: a per-line matcher cannot see the multi-line `r#"{"items": [` at `item_generate.rs:199`, closes that module 245 lines early, and moves `capturesOrJsonPath` from 53 to 60 by handing test fixtures to the ratchet. All four coordination ratchets remain `53 / 30 / 9 / 0` and the core boundary remains `200 / 37` and `52 / 924 / 143`, which is the two-sided test that the fix is right; a new check asserts no `cfg(test)` module fails to close
- The surface gate could report a false failure under load (FR-134). `producer | grep -q` with `set -o pipefail` is a race: `grep -q` exits at the first match, the producer takes SIGPIPE, and the pipeline reports the producer's death as its own status. `check_wiring_truth` announced that two gates were not called by their declared invoker while printing `sed: couldn't write 80 items to stdout: Broken pipe` beside the accusation. Every such pipeline in the governance scripts is now a here-string
- FR-126's retirement-parity evidence had never once been verified in CI (FR-134). `test-agent-driver-production-parity.sh` proves the removal with `git cat-file`, `git merge-base --is-ancestor` and a reverse `git apply` of the removal patch — the recorded baseline commit is reachable, the compatibility window is an ordered interval, and the patch is mechanically revertible — and `actions/checkout` fetches a single commit unless told otherwise, so all three failed on every run while passing on every developer machine. The governance job now checks out with `fetch-depth: 0`, and `check_git_history_available` generalises the rule to any gate whose source queries history. This was found by the step-level reporting added in the same FR, on its first run: it had been sitting behind two earlier failures in a job that stopped at the first
- The Slack certification library read permission bits wrongly on Linux (FR-134). `slack_cert_file_mode` was `stat -f '%Lp' "$1" 2>/dev/null || stat -c '%a' "$1"`, and on GNU coreutils `-f` is `--file-system` and takes no format: it prints the filesystem block for the file on *stdout*, fails on the leftover `'%Lp'` operand, and the `||` fallback then appends the real mode to that output. The caller compared the result against `600` and never matched, so the private-file assertion could not pass on Linux. It went unnoticed for the library's whole life because the job running it had no ripgrep and exited before reaching any assertion — repairing that gap is what surfaced this. The wrapper now validates that whatever answers looks like an octal mode, rather than trusting which platform's `stat` is tried first
- A tracked symlink inside `.claude/skills/` pointed at itself and resolved nowhere (FR-134). `.claude/skills/orchestrator-guide/orchestrator-guide -> ../../.claude/skills/orchestrator-guide` resolves to a `.claude/.claude` that has never existed; committed in `1f5af317`, referenced by nothing, and invisible to all six mirror checks because every one of them read the declared `mirrorRoots`. Deleted, and mirror coverage is now discovered from the git index
- Eight statements still described the source ratchets as monotonic (FR-134). FR-128 tightened them to exact equality because a decrease passed silently — and a decrease is the event those baselines exist to record. FR-134 listed six of the eight and had five of the line numbers wrong; the three it missed were DD-137's governance summary, its row in the design doc index, and QA-175's "the ledger remains exact and monotonic", which asserted two rules at once after one of them stopped being true
- The `fr-governance` skill was unreadable through the `.agents/skills` mirror for its entire life (FR-129). Its entry was shaped `<name>/SKILL.md -> <directory>` rather than `<name> -> <skill directory>`, so any runtime opening `.agents/skills/fr-governance/SKILL.md` received `EISDIR`. Every structural property was satisfied — the entry existed, the symlink resolved, the target was present, git recorded mode `120000` — which is why nothing caught it; only opening the file fails. The mirrors also carried arbitrary subsets of the source: `.agents/skills` held 20 of 29 skills and `.cursor/skills` held 16, with the gaps concentrated in the governance skills. Both roots are now complete
- The typed-driver convergence showcase was never on the documentation site (FR-131). `docs/showcases/streaming-mark-done-convergence.md` was the one source with no published page, and both CEL guides send readers to it. Nothing caught it because the guides name it as an inline code span rather than a markdown link, so there was no broken link to find — and because `site/*/showcases/` was outside the sync entirely. It now renders at `/en/showcases/streaming-mark-done-convergence` and `/zh/...`
- `core/README.md` linked `core/src/runner.rs`, deleted by the runner refactor (FR-131). The link was also repo-relative inside a file-relative context, so it resolved to `core/core/src/runner.rs` even before the file went away. It now points at `crates/orchestrator-runner/src/runner/resource_limits.rs`, which holds the `setrlimit` ABI it describes. It was the repository's only broken relative markdown link; the other two FR-131 named are a code span and a fenced block, and are correct as written
- `SKILLS.md` documented a `.gemini/skills/` mirror that did not exist on disk (FR-129). The line is removed, and the file now points at `config/governance/skill-mirrors.json` as the authority on mirror roots and exemptions
- `scripts/qa/test-legacy-coordination-decommission.sh` asserted exactly 4 legacy command-only Agents. FR-126 migrated all of them and drove the count to 0, so the gate had been failing for an entire FR cycle without anyone noticing, because no workflow ran it. The ratchet now asserts 0 and only tightens; FR-127 found it while wiring the gate into CI
- The coordination ledger's four source ratchets no longer accept a count below their baseline (FR-128). `capturesOrJsonPath` had been sitting at 54 against a reviewed 55 with the gate green, because the comparison rejected only increases. It is now exact in both directions, with `--emit-baseline` as the recovery
- The source scan now excludes every inline `#[cfg(test)]` module, as `sourceBaseline.scope` always claimed (FR-128). Only a single trailing `mod tests { … }` was stripped per file, so ten test-only lines — nine `PipelineVariables` uses in the scheduler item executor and one `output_json_path` in the task repository — were counted as production coordination debt. The reviewed baseline is retightened from `55 / 39 / 9 / 0` to `53 / 30 / 9 / 0`
- Typed driver runs bind structured convergence signals again: `stream_signal_vars` accepts a `driver_terminal` artifact as a terminal marker, and item-level typed signals reach task convergence state, so a `mark_done` workflow converges on the cycle the tool is called instead of running to its cycle ceiling
- Slack source automation now permits different reviewed badge bindings on the same message to create distinct tasks, while preserving one route/task for retries of the same message/reaction/binding identity
- Task-scoped driver completion and `mark_done` events now participate in implicit-item convergence, and successful low-confidence Slack replies create Attention records without converting the step into a failure

### Security
- Remediated all open Dependabot advisories across Cargo, the GUI, the documentation site, and the project-bootstrap portal template
- Global Skill directories now fail closed unless owned by the daemon effective user, free of group/world write bits, and disjoint from every task Workspace and writable ExecutionProfile path; unsupported platforms reject configured global Skills with `FILE_SHARING_GLOBAL_SKILL_UNTRUSTED`

### Compatibility And Migrations
- Migrations 27-32 add Attention/change feeds, handoff/resume state, Session control fencing, source events/bindings, canonical action audit, and Process Console metric observations/rollups. They are additive, forward-only, restart-safe, and preserve existing task and Session identity.
- Migrations 33-34 add durable source automation routes, frozen template/binding generations, optimistic route versions, bounded retry leases, attempt/change history, and Attention correlation. They are additive and forward-only; normal binary rollback keeps their tables and disables reaction writers before using a verified compatible binary.
- Daemon migration 35 adds SourceConnection, OAuth intent, and monotonic connection-change persistence. Slack Gateway schema versions 1-2 independently add encrypted app/install credentials, normalized delivery/audit state, and target-side transfer handoffs. Both stores are additive, forward-only, and backed up/restored with their own encryption keys.
- Daemon migration 36 adds safe dedicated App projections and provisioning checkpoints. Slack Gateway schema 3 adds per-App encryption contexts, one-time import capabilities, signed receipt metadata, dedicated OAuth identity, and exact event endpoint mapping; populated shared/manual state remains compatible.
- Daemon migration 37 adds reviewed dedicated-provisioning migration targets. Slack Gateway schema 4 adds installation/version/source-mode OAuth fences so stale or unreviewed App-mode callbacks fail closed; older binaries reject newer stores rather than guessing rollback compatibility.
- **Breaking (manifests): `runner.executor: streaming` is rejected.** The field remains in the public RuntimePolicy schema as a parse-only compatibility field; `shell` is accepted solely for round-trip compatibility, and `streaming` fails Apply with `[legacy_runner_executor_removed]`. Configure provider execution on the Agent via `spec.driver` (`shell/cli`, `claude/cli`, or `codex/cli`) instead.
- Agent manifests that still carry `spec.command` without `spec.driver` remain accepted during the compatibility window: Apply emits `[legacy_agent_command_deprecated]` and persists the Agent as `shell/cli`, preserving the command, sandbox, prompt-delivery, `command_rules`, and TTY semantics. The scheduler has no non-driver Agent branch left and fails closed with `[legacy_agent_execution_removed]`. Rollback is a reviewed explicit `shell/cli` Agent, not a legacy executor.
- Workflow manifests authored with legacy capture or JSONPath post-action coordination are rejected at Apply with `[legacy_coordination_removed]` and `[legacy_json_path_removed]` before a task can run.
- Existing task, trace, log, watch, CLI, and additive gRPC clients remain compatible, and no database migration is required for the execution changes above. No persisted `Task` rename or destructive schema conversion is included.
- No database migration is required for task workspaces. Existing Workspace manifests remain compatible through the default `code_repo` kind and `root_path` input alias; exported manifests use `work_dir`.
- Normal rollback disables source/session/resume writers and optional projectors before deploying the previous binaries; it retains migrations 27-32 and all Console tables. Database restore is reserved for migration failure or corruption.

### Slack Permissions, Secrets, And Privacy
- Slack Events API configuration requires `reaction_added` delivery and the `reactions:read` scope. Inbound requests use Slack Signing Secret verification; outbound `chat.getPermalink` uses a separately referenced installation token from SecretStore.
- Slack message bodies, attachments, and thread transcripts are not ingested. Tasks contain the configured Skill invocation plus the protected message permalink; safe source/route/UI projections omit credentials, raw payloads, rendered goals, and permalinks unless an Operator explicitly opens the protected route.
- Managed shared mode keeps official app and installation tokens encrypted only in the Slack Gateway. The daemon holds an encrypted installation-scoped pairing; OAuth state/code, tokens, raw Slack bodies, private workspace names, and provider URLs are excluded from safe connection projections, browser storage, tasks, metrics, and routine logs.
- Dedicated mode keeps the Configuration Token only in zeroizing daemon memory and clears it before UI review completes. Newly created App credentials move once into connection-context encrypted Gateway storage; safe state exposes only digests, manifest version, and stable provisioning errors.

### Known Non-goals
- Desktop application packaging/distribution (FR-076), hosted multi-tenant SaaS, down migrations, arbitrary checkpoint rollback, and unreviewed non-idempotent replay are not part of Console v1.
- Marketplace distribution, Enterprise Grid/GovSlack, outbound Slack progress messages, `reaction_removed` task cancellation, message-body ingestion, production Slack release testing, in-place Slack Signing/Client Secret rotation, and destructive automation down migrations are not included. FR-114/FR-115 live certification is limited to a controlled non-production Slack sandbox.

## [0.3.1] - 2026-04-06

### Security
- **UDS trust boundary hardening** — fix RPC role map, enrich audit metadata, add daemon startup checks
- **Least-privilege UDS default** — default UDS max role changed from Admin to Operator

### Added
- **Benchmark evaluation 6-dimension scoring** — upgraded from simple pass/fail to 0-60 composite score

### Fixed
- **Trigger firing chain** (P1) — eliminate duplicate tasks, bypass, and cross-project leakage in unified fire path
- **Sandbox capability matrix** — Linux does not support non-inherit `fs_mode`; corrected capability reporting
- **Loop-guard builtin step** — skip agent capability check when builtin step is present

## [0.3.0] - 2026-04-05

### Added
- **Self-describing CLI reference** — `orchestrator guide` command for built-in documentation

### Changed
- **Core module decomposition** — split oversized dispatch, resource service, and workflow convert modules for maintainability

## [0.2.8] - 2026-04-04

### Added
- **Lightweight step run** (FR-090) — `orchestrator run` command for ad-hoc single-step execution without full workflow scaffolding
- **Design-first workflow skills** — `design-brief-gen` and `design-governance` skills for structured design-first development
- 195 new unit tests — coverage improved from 80.9% to 82.3%

### Fixed
- **CRD plugin process-group isolation** (P1) — plugin child processes now run in dedicated process groups with correct async execution semantics
- **Cross-platform sandbox capability gaps** (P2) — sandbox capability mismatches are now surfaced at manifest validate time rather than failing silently at runtime
- **Log read-path per-project secret redaction** (P2) — defense-in-depth redaction now resolves the task's actual project_id instead of hardcoding the default project; prevents cross-project secret leakage on fallback
- Documentation drift in README and architecture reference
- Replaced 'operator' terminology with 'user' in plugin policy docs

## [0.2.7] - 2026-04-02

### Added
- **Plugin policy governance** (P0-SEC) — layered defense against CRD plugin privilege escalation:
  - `PluginPolicy` with three modes: `deny`, `allowlist` (default), `audit`
  - Command allowlist with prefix matching; built-in denied patterns (curl, wget, nc, eval, base64, /dev/tcp)
  - Timeout cap enforcement (default 30s max per plugin)
  - Hook command policy enforcement (`enforce_on_hooks: true` by default)
  - Admin role elevation for CRDs containing plugins or hooks (`ApplyPluginCrd` RPC)
  - `plugin_audit` SQLite table for immutable audit trail (migration m0022)
  - Audit logging on CRD apply (allowed/denied) and plugin execution
  - Policy loaded from `{data_dir}/plugin-policy.yaml`; absent file = Allowlist with empty allowlist (secure-by-default)
- QA doc 137: plugin policy governance verification (5 scenarios)
- Integration tests for plugin policy enforcement (6 tests)

## [0.2.6] - 2026-04-01

### Added
- **CRD plugin system** (FR-083) — generic custom resource definition plugin framework with three plugin types: interceptor, transformer, cron; `webhook.authenticate`/`webhook.transform` extension points; `crdRef` trigger association; built-in orchestrator tool library
- **QA doctor CLI** (FR-088) — `orchestrator qa doctor` command exposing task execution metrics for observability
- **SecretStore emergency recovery** (FR-089) — `secret key bootstrap` command for encryption key emergency recovery
- **Health policy CLI fixtures** (FR-087) — automated QA script for verifying custom health policy display via `orchestrator check`
- **Dependabot governance skill** — dependency PR lifecycle management

### Fixed
- Key rotation crash safety — prevent data loss during SecretStore key rotation
- Mark QA-64/135 as self-referential unsafe
- Clippy errors — unused gid field and redundant i32 cast
- SecretStore write-blocked error message when encryption keys revoked
- Resolved 30+ QA tickets — doc drift, triage, test alignment, feature gap routing

### Changed
- **Dependency upgrades** — sha2 0.10→0.11, hmac 0.12→0.13, notify 7→8.2, notify-debouncer-full 0.4→0.7, cron 0.15→0.16, picomatch 4.0.3→4.0.4 (CVE fix)

## [0.2.5] - 2026-03-29

### Fixed
- **SafetySpec derived Default** stored zeros instead of proper defaults — now correctly initializes all safety fields
- **Block-style YAML arrays** in frontmatter parser — suppressed false `orphan_command` warnings for multi-line list syntax
- **FR-086 daemon config hot reload** confirmed already implemented via ArcSwap — closed as no-op
- **FR-086 agent selection threshold** closed via Option 2 (unit-test verification) — added `test_diseased_agent_with_passing_capability_threshold_is_selected` integration test proving diseased agents with custom `capability_success_threshold` remain selectable
- **QA-106 inflight wait test fixture** — 3 integration tests verify heartbeat reset (S1), timeout reap (S2), and diagnostic events (S4)
- Resolved all 18 QA tickets — fmt drift, doc date corrections, lint fixes, and feature gap FRs

### Changed
- Removed unused `MessageBus` mechanism (dead code cleanup)
- Added scenario-level self-referential safety annotations to QA docs

## [0.2.4] - 2026-03-28

### Changed
- Extended panic-safety deny lints (`clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`) to all production crates
- Resolved clippy errors and formatting drift across core crates after crate decomposition

## [0.2.3] - 2026-03-28

### Changed
- **Core crate decomposition** — extracted 3 leaf crates from the 60K-LOC monolithic `agent-orchestrator` core:
  - `orchestrator-collab` (1,935 LOC) — agent collaboration types, message bus, shared context, DAG primitives
  - `orchestrator-security` (1,895 LOC) — SecretStore encryption, key lifecycle, audit, secure file helpers
  - `orchestrator-runner` (2,305 LOC) — command runner, sandbox, output capture, network allowlist
- **TaskRepository sub-trait split** — decomposed the 38-method `TaskRepository` trait into 7 domain-aligned sub-traits (`TaskQueryRepository`, `TaskItemQueryRepository`, `TaskStateRepository`, `TaskItemMutRepository`, `CommandRunRepository`, `EventRepository`, `TaskGraphRepository`) with a blanket supertrait for backward compatibility
- All existing import paths preserved via re-export facades — zero downstream breakage

## [0.2.2] - 2026-03-26

### Added
- Filesystem trigger — `event.source: filesystem` for native file system change detection (macOS FSEvents / Linux inotify via `notify` crate)
- Lazy watcher lifecycle — zero filesystem triggers = zero overhead; watcher created/released on demand
- Filesystem event payload — `payload_path`, `payload_filename`, `payload_dir`, `payload_event_type`, `payload_timestamp` available in CEL filter
- Path safety constraints — watched paths must be within workspace `root_path`; `.git/` and daemon data dir auto-excluded
- Workflow template library — 5 progressive templates (hello-world, qa-loop, plan-execute, scheduled-scan, fr-watch) with echo agents for zero-cost tryout
- Doc site "Templates" section — 5 beginner-friendly entries in EN/ZH Showcases sidebar
- Agent `command_rules` — CEL conditional command selection per agent; first matching rule overrides default `command`
- Step `step_vars` — per-step temporary pipeline variable overlay (isolated from other steps)
- `command_rule_index` audit column in `command_runs` table for rule traceability
- `integration-authoring` skill for managing companion integrations repo

## [0.2.1] - 2026-03-26

### Added
- Per-trigger webhook authentication — `webhook.secret.fromRef` resolves signing keys from SecretStore with multi-key rotation support
- Custom signature header per trigger — `webhook.signatureHeader` (default: `X-Webhook-Signature`)
- CEL payload filtering — `filter.condition` evaluates CEL expressions against webhook JSON body
- Integration manifest packages — companion repo `c9r-io/orchestrator-integrations` with Slack, GitHub, LINE pre-configured triggers
- `integration-authoring` skill for creating new integration packages
- Secret rotation showcase (`docs/showcases/secret-rotation-workflow.md`)

### Changed
- Webhook auth fallback chain: per-trigger secret → global `--webhook-secret` → no verification

## [0.2.0] - 2026-03-25

### Added
- HTTP webhook endpoint — `--webhook-bind <ADDR>` runs axum HTTP server alongside gRPC
- Webhook trigger source — `event.source: webhook` for external event ingestion
- HMAC-SHA256 signature verification — `--webhook-secret` with `X-Webhook-Signature` header
- `orchestrator trigger fire --payload` — simulate webhook payloads via CLI
- `orchestrator task items <task_id>` — list task item status
- `orchestrator event list --task <task_id>` — list task events with type filter
- `orchestrator db vacuum` — reclaim SQLite disk space
- `orchestrator db cleanup --older-than N` — manual log file cleanup
- `orchestrator db status` — shows DB, logs, and archive sizes
- Automatic log file TTL cleanup — `--log-retention-days 30` (default enabled)
- Optional task auto-cleanup — `--task-retention-days N` (default disabled)

### Changed
- Webhook payload included in trigger goal for context
- `db status` output now includes disk usage information

## [0.1.6] - 2026-03-25

### Changed
- Dependencies upgraded: clap 4.6, nix 0.31, cron 0.15, arc-swap 1.9, tracing-subscriber 0.3.23, clap_complete 4.6
- Fix nix 0.31 breaking change: `dup2()` API migration to `AsFd` + `OwnedFd`
- CI clippy and fmt fixes

## [0.1.5] - 2026-03-25

### Changed
- Documentation site launched at docs.c9r.io (VitePress + Cloudflare Pages)
- 9 showcase execution plans with EN/ZH translations
- Multi-model benchmark showcase for comparing LLM shells and models
- README slimmed from 371 to 74 lines with agent-first vision
- Project identity: "Built for agents, by agents"

## [0.1.3] - 2026-03-25

### Fixed
- Supply chain: rustls-webpki 0.103.9 → 0.103.10 (RUSTSEC-2026-0049)
- Supply chain: migrate serde_yml → serde_yaml (RUSTSEC-2025-0067/0068)

## [0.1.2] - 2026-03-24

### Fixed
- `orchestrator get` returns empty results instead of error for missing projects
- Full CLI/daemon documentation alignment (20+ stale references fixed)

### Changed
- Showcases sanitized with developer-friendly placeholders
- sqlite workarounds replaced with CLI commands

## [0.1.1] - 2026-03-24

### Added
- Homebrew tap: `brew install c9r-io/tap/orchestrator`
- crates.io publishing with Trusted Publishers (OIDC)
- crate READMEs for crates.io display

### Changed
- Release workflow: Homebrew formula auto-push + crates.io auto-publish

## [0.1.0] - 2026-03-24

Initial release of the Agent Orchestrator platform.

### Added

#### Core Engine
- DAG execution engine with topological sort, cycle detection, and conditional edges
- CEL (Common Expression Language) prehooks: conditional step execution via bool expressions
- Capability-driven agent selection with health scoring and load balancing
- Dynamic step pools with runtime step selection based on context and priority
- Pipeline variables with CEL expression interpolation

#### Architecture
- Client/server model: `orchestratord` daemon + `orchestrator` CLI over gRPC/UDS
- Configurable worker pool (`--workers N`) for concurrent task execution
- Proper daemonization with PID file, log rotation, and crash recovery
- Fixed data directory at `~/.orchestratord/` with database-level project isolation

#### Workflow Engine
- Declarative YAML manifests (v2 resource model: `orchestrator.dev/v2`)
- Loop control: `once` / `infinite` modes with `max_cycles` limits
- Guard steps for workflow termination (`loop_guard`, convergence expressions)
- Repeatable steps with per-cycle execution control
- Step templates for reusable step definitions
- Item-scoped git worktree isolation for parallel execution

#### Resource Model
- 11 built-in resource kinds: Workspace, Agent, Workflow, StepTemplate, ExecutionProfile, SecretStore, EnvStore, WorkflowStore, Trigger, RuntimePolicy, CustomResourceDefinition
- Custom Resource Definitions (CRD) with JSON Schema + CEL validation
- Resource versioning and audit trail

#### Security
- mTLS control plane with auto-generated PKI (CA, server, client certificates)
- RBAC authorization (read_only, operator, admin roles)
- SecretStore encryption (AES-256-GCM-SIV) with key rotation support
- Control plane audit logging
- Sandbox enforcement: resource limits, network isolation, writable paths
- Daemon PID guard against subprocess kill attempts

#### Triggers
- Cron-based scheduled task creation
- Event-driven task creation (workflow completion, step events)

#### Observability
- Structured logging with JSON and pretty formats
- Event system with TTL cleanup and JSONL archival
- Agent health metrics, success rates, and latency tracking
- Task execution metrics sampling

#### CLI
- kubectl-style interface with aliases (`t` for `task`, `g` for `get`)
- Output formats: table, JSON, YAML
- Shell completion support (via `clap_complete`)
- Daemon lifecycle commands: stop, status, maintenance mode

#### GUI (Alpha)
- Tauri 2.x desktop application with gRPC client
- Wish pool UI with real-time progress observation
- Theme toggle, i18n framework, responsive layout

#### Distribution
- Multi-platform binaries: Linux (x86_64, aarch64) + macOS (x86_64, aarch64)
- Automated release pipeline with SHA256 checksums
- One-line installer: `curl -fsSL .../install.sh | sh`

#### Documentation
- 7-chapter user guide (English + Simplified Chinese)
- Architecture reference documentation
- 70+ design documents with QA verification
