---
lifecycle: active
related_fr: FR-116
---

# Orchestrator Runner - Codex Session Resume Conformance

**Module**: Orchestrator Runner / Agent Driver
**Status**: Released
**Related Plan**: FR-116-A Codex Driver session resume verification
**Related QA**: `docs/qa/orchestrator/166-codex-session-resume-conformance.md`
**Created**: 2026-07-22
**Last Updated**: 2026-07-22

## Background

FR-116 introduced a `codex/cli` driver and advertised cross-step session attachment, but its resume command and JSONL mapping were inferred rather than exercised against a real Codex CLI. A false-positive capability claim could allow apply-time validation and then restart context or fail only after the step had started.

The follow-up certifies the complete boundary against `codex-cli 0.144.5`: command grammar, session identifier source, resumed context continuity, provider-event normalization, fixture replay, and credential/session cleanup.

## Goals

- Prove that `codex exec resume` attaches to the exact prior thread and retains prior-turn context.
- Pin the provider fields consumed by `ProcessSession` to a sanitized recorded fixture.
- Keep the default test suite deterministic and offline while retaining an explicit live certification path.
- Advertise `session_resume: true` only after the conformance evidence passes.

## Non-goals

- Supporting live multi-turn stdin, Orchestrator-hosted tools, or permission events in `codex/cli`.
- Persisting provider session IDs across daemon restarts.
- Treating every future Codex CLI version as certified without fixture renewal.
- Mapping provider-only reasoning or cached-token details that have no `DriverEvent` field.

## Scope

- In scope: Codex CLI resume command construction, `thread.started`, `item.completed`, `turn.completed`, failure records, session redaction, versioned fixture replay, and controlled live certification.
- Out of scope: SDK transport, public session-token APIs, database migrations, GUI changes, and provider account setup.

## Key Design

1. The certified resume grammar is:

   ```text
   codex exec resume <SESSION_ID> --json [typed options] -- <PROMPT>
   ```

   `build_codex_command` keeps the session ID inside the provider adapter and shell-quotes both the ID and prompt.

2. `thread.started.thread_id` is the sole Codex session reference consumed by Orchestrator. A resumed process emits the same thread ID before its new turn.

3. The recorded `0.144.5` protocol maps as follows:

   | Codex JSONL | Driver behavior |
   |---|---|
   | `thread.started.thread_id` | internal `SessionRef` plus `DriverEvent::Started` |
   | `item.completed` / `agent_message` | `DriverEvent::AssistantText` |
   | `item.completed` / `mcp_tool_call` | `DriverEvent::ToolUse` when present |
   | `turn.completed.usage.input_tokens/output_tokens` | `DriverEvent::Usage`; cached/reasoning counts remain provider-only |
   | `turn.failed` or `error` | failed `DriverEvent::Finished` |
   | successful process exit | successful `DriverEvent::Finished` |

4. `fixtures/driver/codex-cli-0.144.5-resume.json` is a sanitized capture of an initial turn and resumed turn. The live UUID and dynamic item IDs are replaced with stable placeholders; recorded messages and numeric usage fields remain protocol evidence.

5. `scripts/qa/test-codex-session-resume.sh` is the offline gate. `scripts/qa/certify-codex-session-resume.sh` is an explicit network/token-consuming certification that requires exactly `codex-cli 0.144.5`.

6. Live certification creates a temporary `CODEX_HOME`, copies only `auth.json` at mode `0600`, ignores user configuration and rules, uses a read-only scratch workspace, and deletes the entire temporary tree on every exit path.

## Alternatives And Tradeoffs

- Keep `session_resume: false` permanently: safest, but discards a provider feature that the installed CLI supports and that workflows can use safely inside one daemon lifetime.
- Advertise resume with an experimental warning: preserves availability but permits a silent protocol mismatch after apply-time validation.
- Selected approach: switch to fail-closed during the spike, restore the capability only after thread identity and contextual continuity pass, and retain exact-version drift detection.

## Risks And Mitigations

- Risk: Codex CLI changes its command or JSONL schema.
  - Mitigation: exact version metadata, deterministic fixture replay, event-sequence assertions, and a live script that rejects a different version until recertified.
- Risk: command acceptance is mistaken for contextual resume.
  - Mitigation: the second turn must return both a new anchor and the exact first-turn anchor while emitting the same thread ID.
- Risk: live certification leaks OAuth material or a thread ID.
  - Mitigation: disposable private home, no raw stderr/stdout printing, placeholder substitution, UUID rejection in the committed fixture, and unconditional cleanup.
- Risk: a successful Codex stream has no explicit provider success record.
  - Mitigation: `turn.completed` contributes usage; the shared child-process wait path remains the authoritative success terminal event.

## Observability

- Logs: provider JSONL continues through the shared redacted stdout artifact; `thread_id` is replaced with `[REDACTED]`.
- Events: the scheduler receives only normalized session availability, assistant text, usage, and terminal outcome.
- Metrics: no new metrics are introduced; provider-reported input/output token counts continue through `driver_usage`.
- Tracing: no new span is required because the command run and step events already identify the driver and execution boundary.

## Operations / Release

- Offline verification: `./scripts/qa/test-codex-session-resume.sh`.
- Controlled live verification: `./scripts/qa/certify-codex-session-resume.sh`.
- `CODEX_RESUME_SOURCE_HOME` selects the authenticated source home; `CODEX_RESUME_EXPECTED_VERSION` is available only for an intentional recertification workflow.
- When upgrading Codex, run the live script, review schema differences, regenerate the sanitized fixture, update mappings/tests, and only then change the pinned version.
- Emergency rollback is code-only: set Codex `session_resume` back to `false`. No manifest or database migration is required; affected workflow requirements then fail during apply with `driver_session_resume_required`.

## Test Plan

- Unit: exact resume command grammar and certified capability descriptor.
- Conformance: replay both recorded streams and assert session equality, assistant anchors, and usage mapping.
- Security: reject a live UUID in the fixture and preserve recursive `thread_id` redaction.
- Live: execute one initial and one resumed Codex turn in an isolated home/workspace.
- Regression: workspace tests, strict Clippy, formatting, documentation lint, and the FR-116 driver suite.

## QA Docs

- `docs/qa/orchestrator/166-codex-session-resume-conformance.md`

## Acceptance Criteria

- A real `codex-cli 0.144.5` initial/resume conformance run passes and proves contextual continuity.
- `build_codex_command` exactly matches the certified resume grammar.
- Codex JSONL fields consumed by `ProcessSession` are covered by a sanitized recorded fixture.
- DD-127 and the user guide advertise only the verified capability and document the versioned recertification path.
