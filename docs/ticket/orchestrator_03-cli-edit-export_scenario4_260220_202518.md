# Ticket: Edit Export Agent Group Fails

**Created**: 2026-02-20 20:25:18
**QA Document**: `docs/qa/orchestrator/03-cli-edit-export.md`
**Scenario**: #4
**Status**: FAILED

---

## Test Content
Export agent group configuration using `edit export` command

---

## Expected Result
- Export shows agent group configuration with member list in YAML manifest format
- Command: `orchestrator edit export agentgroup/qa_group` should succeed
- Output should contain apiVersion, kind: AgentGroup, metadata, spec with agents list

---

## Actual Result
Command failed with error: `cli execution failed: resource not found: agentgroup/qa_group`

The agent group `qa_group` exists in the configuration file (`orchestrator/config/default.yaml`):
```yaml
agent_groups:
  qa_group:
    agents:
    - opencode
```

But the `edit export` command cannot find it.

---

## Repro Steps
1. Run: `./orchestrator/src-tauri/target/release/agent-orchestrator edit export agentgroup/qa_group`
2. Also tried: `./orchestrator/src-tauri/target/release/agent-orchestrator edit export agent_group/qa_group`
3. Both fail with "resource not found"

---

## Evidence

**CLI Output**:
```
$ ./orchestrator/src-tauri/target/release/agent-orchestrator edit export agentgroup/qa_group
cli execution failed: resource not found: agentgroup/qa_group

$ ./orchestrator/src-tauri/target/release/agent-orchestrator edit export agent_group/qa_group
cli execution failed: resource not found: agent_group/qa_group
```

**Config File Verification**:
```bash
$ grep -A 10 "agent_groups:" orchestrator/config/default.yaml
agent_groups:
  qa_group:
    agents:
    - opencode
  fix_group:
    agents:
    - claudecode
```

**Working Examples** (for comparison):
- `edit export workspace/default` ✓ works
- `edit export agent/opencode` ✓ works
- `edit export workflow/qa_only` ✓ works
- `edit export agentgroup/qa_group` ✗ fails

---

## Analysis

**Root Cause**: The `edit export` command does not support the `agentgroup` or `agent_group` resource kind. The command successfully exports `workspace`, `agent`, and `workflow` resources, but agent groups are not implemented.

This could be:
1. Missing implementation in the resource export logic
2. Agent groups may not be designed as exportable resources
3. Incorrect resource kind name (though both `agentgroup` and `agent_group` were tried)

**Severity**: Medium

**Related Components**: Backend / CLI / Resource Management
