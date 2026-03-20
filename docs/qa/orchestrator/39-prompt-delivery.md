---
self_referential_safe: true
---

# Orchestrator - Prompt Delivery Abstraction

**Module**: orchestrator
**Scope**: PromptDelivery enum configuration, serde behavior, delivery dispatch, preflight validation
**Scenarios**: 5
**Priority**: High

---

## Background

Agents can now declare a `promptDelivery` mode in their spec to control how the rendered prompt reaches the spawned process. Four modes exist:

- **arg** (default): Legacy `{prompt}` substitution in the shell command string
- **stdin**: Prompt written to the child process stdin, then stdin closed (EOF)
- **file**: Prompt written to a temp file, `{prompt_file}` placeholder replaced in command
- **env**: Prompt injected as the `ORCH_PROMPT` environment variable

The mode is threaded through agent selection → phase runner → spawn.

---

## Scenario 1: Default Prompt Delivery Is Arg

### Goal
Verify that agents without an explicit `promptDelivery` field default to `arg` mode.

### Steps
1. **Unit test** — 运行默认值测试：
   ```bash
   cargo test -p orchestrator-config -- agent::tests::prompt_delivery_default_is_arg --nocapture
   ```

### Expected
- `PromptDelivery::default()` 返回 `Arg`
- 测试通过

---

## Scenario 2: Serde Round-Trip Preserves Delivery Mode

### Goal
Verify that all four `promptDelivery` modes correctly serialize and deserialize through YAML/JSON.

### Steps
1. **Unit test** — 运行 serde round-trip 测试：
   ```bash
   cargo test -p orchestrator-config -- agent::tests::prompt_delivery_serde_roundtrip --nocapture
   ```
2. **Unit test** — 运行 skip-serializing-default 测试：
   ```bash
   cargo test -p orchestrator-config -- agent::tests::prompt_delivery_skip_serializing_default --nocapture
   ```

### Expected
- `stdin`/`file`/`env` 模式经序列化-反序列化后保持不变
- 默认 `arg` 模式在序列化时被省略（`skip_serializing_if`）

---

## Scenario 3: Preflight Warns on Stdin with Prompt Placeholder

### Goal
Verify that the preflight check system warns when `promptDelivery: stdin` but command contains `{prompt}`.

### Steps
1. **Unit test** — 运行 stdin 冲突 warning 测试：
   ```bash
   cargo test -p orchestrator-scheduler -- check::tests::prompt_delivery_stdin_warns_on_prompt_placeholder --nocapture
   ```

### Expected
- Warning 指出 `{prompt}` placeholder 在 stdin 模式下不被替换
- Warning 为 informational，不阻止 apply

---

## Scenario 4: Preflight Warns on File Without Prompt File Placeholder

### Goal
Verify that the preflight check warns when `promptDelivery: file` but command lacks `{prompt_file}`.

### Steps
1. **Unit test** — 运行 file 缺失 placeholder warning 测试：
   ```bash
   cargo test -p orchestrator-scheduler -- check::tests::prompt_delivery_file_warns_missing_prompt_file_placeholder --nocapture
   ```

### Expected
- Warning 指出 command 缺少 `{prompt_file}` placeholder
- Warning 为 informational，不阻止 apply

---

## Scenario 5: Arg Mode Shell-Escapes Full Prompt

### Goal
Verify that `apply_prompt_delivery` in arg mode correctly shell-escapes the full prompt text to prevent injection.

### Steps
1. **Unit test** — 运行 shell 转义测试：
   ```bash
   cargo test -p orchestrator-scheduler -- phase_runner::tests::cases::apply_prompt_delivery_arg_shell_escapes_full_prompt --nocapture
   ```
2. **Code review** — 确认 `apply_prompt_delivery` 函数存在且对 `Arg` 模式执行 shell-safe 转义：
   ```bash
   rg -n "fn apply_prompt_delivery" crates/orchestrator-scheduler/src/scheduler/phase_runner/mod.rs
   ```

### Expected
- Prompt 中的反引号、`$()` 等 shell 元字符被正确转义
- 测试通过

---

## General: Preflight Arg Mode No Warning

### Steps
1. **Unit test** — 运行 arg 模式无 warning 测试：
   ```bash
   cargo test -p orchestrator-scheduler -- check::tests::prompt_delivery_arg_no_warning --nocapture
   ```

### Expected
- `promptDelivery: arg` 配合 `{prompt}` placeholder 时不产生 warning

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Default prompt delivery is arg | ✅ | 2026-03-20 | Claude | |
| 2 | Serde round-trip preserves delivery mode | ✅ | 2026-03-20 | Claude | |
| 3 | Preflight warns on stdin with prompt placeholder | ✅ | 2026-03-20 | Claude | |
| 4 | Preflight warns on file without prompt_file | ✅ | 2026-03-20 | Claude | |
| 5 | Arg mode shell-escapes full prompt | ✅ | 2026-03-20 | Claude | shell_escape called at line 78 |
| G | Arg mode no warning | ✅ | 2026-03-20 | Claude | |
