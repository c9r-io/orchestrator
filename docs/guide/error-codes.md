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

## `secret_value_placeholder_rejected`

- **Meaning**: a `kind: SecretStore` manifest carries the redaction placeholder
  `[ENCRYPTED]` as a value instead of a real secret. Reads redact secret
  values, so this is what a manifest obtained from `get secretstore/<name>` or
  `describe secretstore/<name>` looks like — it does not carry the values it
  appears to carry.
- **Trigger**: `orchestrator apply` or `manifest validate` on such a manifest.
  The manifest is rejected; nothing is written.
- **Action**: supply the real value for every key. Apply replaces the whole
  store, so a key omitted from `spec.data` is deleted rather than preserved —
  a redacted manifest cannot be repaired by deleting the placeholder lines.
  Read commands are for inspecting which keys a store defines; they are not a
  backup of its values.

## `FILE_SHARING_GLOBAL_SKILL_UNTRUSTED`

- **Meaning**: a globally shared Skill directory failed the trust check; the
  daemon refuses to expose it to agents. The message names the directory, the
  reason, and a suggested fix.
- **Trigger**: file-sharing configuration load, when a global Skill directory
  is not owned or permissioned the way the trust policy demands.
- **Action**: apply the `suggested_fix` in the message — typically correcting
  the directory's ownership or permissions, or removing the untrusted entry
  from the shared configuration.
