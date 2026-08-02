# Error Codes

Machine-readable error codes the orchestrator prints in square brackets, e.g.
`[driver_config_invalid]`. They appear in `apply` output, validation
diagnostics, and task logs — concentrated on the first-run path. This page is
the glossary: what each code means, what triggers it, and what to do.

The entry set below is compared against the source-derived set in CI
(`scripts/qa/test-error-code-glossary.sh`, via `qa-doc-lint`): a code added to
the product without a glossary entry fails the build, and so does an entry
whose code no longer exists. Run `orchestrator guide error-codes` for the CLI
pointer to this page.

## `legacy_agent_command_deprecated`

- **Meaning**: a `kind: Agent` manifest sets `spec.command` but omits
  `spec.driver`. This is the deprecated pre-driver authoring form.
- **Trigger**: `orchestrator apply` on such a manifest. The apply succeeds:
  the warning announces that the Agent is promoted to an explicit `shell/cli`
  driver when persisted.
- **Action**: add the typed driver to the manifest so the promotion is
  explicit:

  ```yaml
  spec:
    driver:
      provider: shell
      transport: cli
  ```

  See [Agent Driver Model](agent-driver-model.md).

## `legacy_agent_execution_removed`

- **Meaning**: the scheduler was asked to execute an Agent whose stored record
  has no typed driver — a record persisted before driver promotion existed.
- **Trigger**: task execution selects such an Agent.
- **Action**: re-apply the Agent manifest. Apply promotes command-only
  configuration to `shell/cli` and the stored record gains its driver.

## `legacy_coordination_removed`

- **Meaning**: a Workflow step uses `behavior.captures`, part of the
  removed CEL/capture coordination mechanism (DD-137).
- **Trigger**: `orchestrator apply` or `manifest validate` on a Workflow
  carrying `behavior.captures`. The manifest is rejected.
- **Action**: delete the `captures` block and use typed driver/tool results
  instead. See [Coordination Tools](coordination-tools.md).

## `legacy_json_path_removed`

- **Meaning**: a Workflow step uses a JSONPath-backed post-action
  (`spawn_tasks` / `generate_items`), removed with the same coordination
  collapse.
- **Trigger**: `orchestrator apply` or `manifest validate` on such a Workflow.
  The manifest is rejected.
- **Action**: replace the post-action with typed daemon tools. See
  [Coordination Tools](coordination-tools.md).

## `legacy_pipeline_variables_removed`

- **Meaning**: a Workflow step authors a pipeline variable through one of the
  four retired step-level constructs — `store_inputs`, `store_outputs`,
  `step_vars`, or a `store_put` post-action. All four routed an author-chosen
  value through the generic pipeline-variable map retired by the coordination
  collapse (DD-169).
- **Trigger**: `orchestrator apply` or `manifest validate` on such a Workflow,
  including when the construct sits inside `chain_steps`. The manifest is
  rejected and the diagnostic names which of the four you used.
- **Action**: have the step address the store itself, which needs no binding:

  ```yaml
  command: >-
    LAST_SHA="$(orchestrator store get promotion last_published_sha
    --project {project_id} 2>/dev/null || true)" && ...
  ```

  `{project_id}` renders from the task context, so nothing has to be
  pre-substituted into the step. For an agent step, put the same command in the
  prompt and let the agent run it. For `step_vars`, write the value directly into
  the step's own command or prompt. See [Coordination Tools](coordination-tools.md).

## `legacy_runner_executor_removed`

- **Meaning**: the manifest sets `runner.executor: streaming`, a removed
  execution mode. `runner.executor` survives only as a parse-only
  compatibility field for the historical `shell` value.
- **Trigger**: `orchestrator apply` on a manifest with
  `runner.executor: streaming`. The manifest is rejected.
- **Action**: delete `runner.executor` and configure each Agent's
  `spec.driver` instead (`shell/cli`, `claude/cli`, or `codex/cli`).

## `driver_config_invalid`

- **Meaning**: an Agent's `spec.driver` block contradicts itself or its Agent.
  The message carries the specific reason — for example `driver shell/cli
  requires agent.spec.command`, a provider given configuration belonging to a
  different provider, or `claude driver constructs its command;
  agent.spec.command must be omitted`.
- **Trigger**: `orchestrator apply` or workflow validation touching the
  Agent.
- **Action**: fix the named field. The rules: `shell` drivers keep
  `spec.command`; `claude`/`codex` drivers must not have one; each provider
  accepts only its own sub-block (`shell:`, `claude:`, `codex:`).

## `driver_raw_args_unsafe_mode_required`

- **Meaning**: an Agent driver sets `rawArgs`, which bypasses provider flag
  construction and is only honored when the daemon runs in unsafe mode.
- **Trigger**: `orchestrator apply` of such an Agent against a daemon not in
  unsafe mode.
- **Action**: remove `driver.rawArgs` (preferred), or run the daemon in
  unsafe mode when the raw flags are genuinely required.

## `driver_multi_turn_required`

- **Meaning**: a Workflow step declares multi-turn driver requirements, and a
  candidate Agent's driver cannot hold a multi-turn session.
- **Trigger**: workflow validation at apply time; the pairing is rejected.
- **Action**: give the step an Agent with a multi-turn-capable driver (Claude
  CLI), or drop the multi-turn requirement from the step.

## `driver_tool_hosting_required`

- **Meaning**: the step requires hosted tool transport that the candidate
  Agent's driver does not provide.
- **Trigger**: workflow validation at apply time.
- **Action**: select a driver with the requested tool transport, or set the
  step's `toolHosting` requirement to one the driver supports.

## `driver_session_resume_required`

- **Meaning**: the step requires session resume and the candidate driver
  cannot resume provider sessions.
- **Trigger**: workflow validation at apply time.
- **Action**: use a session-resume-capable driver (Claude or Codex CLI), or
  remove the `sessionResume` requirement.

## `driver_permission_events_required`

- **Meaning**: the step carries an approval gate that needs the driver to
  emit permission request events, and the candidate driver cannot.
- **Trigger**: workflow validation at apply time.
- **Action**: use a permission-event-capable driver, or remove the approval
  gate from the step.

## `driver_workspace_sandbox_required`

- **Meaning**: the step declares workspace access, which requires a
  sandboxable driver, and the candidate driver is not sandboxable.
- **Trigger**: workflow validation at apply time.
- **Action**: use a sandboxable CLI driver, or set the step's
  `workspaceAccess` to `none`.

## `driver_guaranteed_cancel_required`

- **Meaning**: the step is classified `nonIdempotentExternal`, which demands
  a driver with guaranteed cancel semantics, and the candidate driver only
  offers best-effort cancellation.
- **Trigger**: workflow validation at apply time.
- **Action**: use a guaranteed-cancel driver, or make the external operation
  idempotent and reclassify the step.

## `driver_transport_unavailable`

- **Meaning**: the Agent driver declares `transport: sdk`. The SDK transport
  is a reserved shape with no implementation; `cli` is the only executable
  transport.
- **Trigger**: workflow validation at apply time.
- **Action**: change the driver's `transport` to `cli`.

## `empty_change_check`

- **Meaning**: the post-implement safety self-test found no repository
  changes after an implement step — `git diff --stat HEAD` came back empty,
  so running the check suite would certify nothing.
- **Trigger**: task execution, after an implement-class step completes; the
  item fails with this code in `task logs`.
- **Action**: inspect the implement agent's output. The agent finished
  without producing changes — usually a goal the agent judged already done,
  or a prompt that did not reach the working directory it was meant to edit.

## `FILE_SHARING_GLOBAL_SKILL_UNTRUSTED`

- **Meaning**: a globally shared Skill directory failed the trust check; the
  daemon refuses to expose it to agents. The message names the directory, the
  reason, and a suggested fix.
- **Trigger**: file-sharing configuration load, when a global Skill directory
  is not owned or permissioned the way the trust policy demands.
- **Action**: apply the `suggested_fix` in the message — typically correcting
  the directory's ownership or permissions, or removing the untrusted entry
  from the shared configuration.
