# Orchestrator - Prompt Delivery Abstraction Layer

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Decouple prompt content from shell command construction via a `PromptDelivery` enum that controls how rendered prompts reach agent processes
**Related QA**: `docs/qa/orchestrator/39-prompt-delivery.md`
**Created**: 2026-03-04
**Last Updated**: 2026-03-04

## Background

Agent commands are defined as shell templates with a `{prompt}` placeholder (e.g. `claude -p "{prompt}"`). At runtime, the entire prompt is string-substituted into the shell command (`phase_runner.rs`), creating a shell injection attack surface. Even with `shell_escape()`, embedding untrusted content in shell strings is inherently risky.

Most modern AI CLI tools (Claude Code, Codex, Gemini CLI) support stdin piping, making it possible to keep prompt content entirely out of the shell.

## Goals

- Eliminate shell injection risk for prompt content by supporting non-shell delivery mechanisms
- Provide four delivery modes: `stdin`, `file`, `env`, `arg` (legacy default)
- Maintain full backward compatibility — existing configs default to `arg` and work unchanged
- Thread delivery mode through agent selection, spawn, and execution pipeline

## Non-goals

- Removing `{prompt}` substitution entirely (backward compatibility requires it)
- Adding new CLI commands or user-facing UI
- Changing the prompt rendering/template engine itself

## Scope

- In scope: `PromptDelivery` enum definition, serde support, AgentConfig/AgentSpec integration, selection return type, runner stdin piping, phase_runner delivery dispatch, preflight validation
- Out of scope: prompt rendering changes, database schema changes, CLI surface changes

## Key Design

1. **PromptDelivery enum** (`config/agent.rs`): Four variants — `Stdin`, `File`, `Env`, `Arg` (default). Uses `#[serde(rename_all = "snake_case")]` and `#[default]` on `Arg`. Skipped from serialization when default via `skip_serializing_if`.

2. **Delivery dispatch** (`phase_runner.rs`): Before spawn, the delivery mode determines how the prompt reaches the child process:
   - `Arg`: Legacy `{prompt}` substitution in shell command string
   - `Stdin`: Prompt written to child stdin fd after spawn, then stdin closed (EOF)
   - `File`: Prompt written to temp file in logs dir, `{prompt_file}` placeholder replaced in command
   - `Env`: Prompt injected as `ORCH_PROMPT` environment variable

3. **Selection threading**: Both `select_agent_advanced()` and `select_agent_by_preference()` return `(String, String, PromptDelivery)` so the caller knows which delivery mode to use.

4. **Runner pipe_stdin**: `spawn_with_runner()` accepts a `pipe_stdin: bool` parameter. When true, `Stdio::piped()` is set on the child's stdin.

5. **Preflight validation** (`check.rs`): Warns on misconfigured combinations (e.g. `stdin` delivery with `{prompt}` in command, `file` delivery without `{prompt_file}`).

## Alternatives And Tradeoffs

- **Option A: Always stdin** — Simplest but breaks agents that don't read stdin, not backward compatible.
- **Option B: Per-agent enum (chosen)** — Each agent declares its preferred delivery mode. Backward compatible, flexible.
- **Option C: Global config** — Single mode for all agents. Too coarse — different agent CLIs have different capabilities.
- Why we chose Option B: Best balance of security improvement and backward compatibility. Agents that support stdin can opt in immediately.

## Risks And Mitigations

- Risk: TTY mode conflicts with stdin delivery (TTY redirects stdin from FIFO)
  - Mitigation: Detect TTY+Stdin conflict, log warning, fall back to Arg mode
- Risk: Large prompts via Env mode exceed OS env var limit (~128KB)
  - Mitigation: Size check with warning log suggesting File mode
- Risk: Agents configured for File mode forget `{prompt_file}` placeholder
  - Mitigation: Preflight check warns about missing `{prompt_file}`

## Observability

- Logs: `tracing::warn` emitted for TTY+Stdin fallback, placeholder-ignored cases, env size warnings, and file delivery temp path
- Metrics: No new metrics (existing step/run metrics capture success/failure)
- Tracing: Delivery mode is implicit in the `command_preview` field of `step_spawned` events — non-arg modes produce shorter commands without embedded prompts

## Operations / Release

- Config: New `promptDelivery` field on Agent spec (`stdin` | `file` | `env` | `arg`). Default `arg` requires no config change.
- Migration / rollback: No migration needed. Removing `promptDelivery` from agent spec reverts to `arg` default.
- Compatibility: Fully backward compatible. Existing agents without `promptDelivery` default to `arg`.

## Test Plan

- Unit tests: Serde roundtrip for all 4 variants, default verification, skip_serializing_if, spec↔config conversion, selection return type, preflight validation warnings (3 test cases)
- Integration tests: All 24 existing integration tests pass unchanged
- Existing runner tests updated with `pipe_stdin: false` parameter

## QA Docs

- `docs/qa/orchestrator/39-prompt-delivery.md`

## Acceptance Criteria

- `cargo build` compiles without errors
- `cargo test` — all existing tests pass (arg is default, no behavior change)
- New unit tests for serde, selection, validation
- Manual verification: change an agent to `promptDelivery: stdin`, run a step, verify agent receives prompt via stdin and command stored in DB does NOT contain prompt text
