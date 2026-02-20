# Orchestrator - CLI Task Lifecycle

**Module**: orchestrator
**Scope**: Validate task lifecycle operations (start, pause, resume, logs, retry)
**Scenarios**: 5
**Priority**: High

---

## Background

This document tests the task lifecycle commands including starting, pausing, resuming tasks, viewing logs, and retrying failed items.

Entry point: `orchestrator task <command>`

---

## Scenario 1: Task Start

### Preconditions

- Orchestrator binary available
- Workspace configured with mock agents

### Steps

1. Create a task without auto-start:
   ```bash
   orchestrator task create --name "pause-test" --goal "Test pause/resume" --no-start
   ```

2. Verify task status is pending:
   ```bash
   orchestrator task list
   ```

3. Start the task:
   ```bash
   orchestrator task start {task_id}
   ```

4. Check task status:
   ```bash
   orchestrator task info {task_id}
   ```

### Expected

- Task starts in "running" or "completed" status
- Task details show progress

---

## Scenario 2: Task Start with --latest

### Preconditions

- At least one paused or pending task exists

### Steps

1. Create and pause a task:
   ```bash
   TASK_ID=$(orchestrator task create --name "latest-test" --goal "Test" --no-start --format json | jq -r '.id')
   ```

2. Start with --latest flag:
   ```bash
   orchestrator task start --latest
   ```

### Expected

- Latest resumable task is started automatically

---

## Scenario 3: Task Pause and Resume

### Preconditions

- A running task exists (or create one that runs for a while)

### Steps

1. Create a task that runs for some time:
   ```bash
   # First update config to use mock_sleep agent
   cat > /tmp/mock-config.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: false
   defaults:
     workspace: default
     workflow: qa_only
   workspaces:
     default:
       root_path: /path/to/project
       qa_targets:
         - docs/qa
       ticket_dir: docs/ticket
    agents:
      mock_sleep:
        metadata:
          name: mock_sleep
        capabilities:
        - qa
        templates:
          qa: "sleep 10 && echo 'done'"
    agent_groups:
      sleep_group:
        agents:
          - mock_sleep
    workflows:
      qa_only:
        steps:
          - id: qa
            required_capability: qa
            enabled: true
            repeatable: false
            agent_group_id: sleep_group
        loop:
          mode: once
        finalize:
          rules: []
    EOF
    ```

2. Start task in background:
   ```bash
   orchestrator task start {task_id} &
   sleep 1
   ```

3. Pause the task:
   ```bash
   orchestrator task pause {task_id}
   ```

4. Verify task is paused:
   ```bash
   orchestrator task info {task_id}
   ```

5. Resume the task:
   ```bash
   orchestrator task resume {task_id}
   ```

### Expected

- Task can be paused mid-execution
- Task can be resumed and continues from where it stopped

---

## Scenario 4: Task Logs

### Preconditions

- A task has been executed at least once

### Steps

1. Get task ID:
   ```bash
   orchestrator task list
   ```

2. View task logs:
   ```bash
   orchestrator task logs {task_id}
   ```

3. View last 10 lines:
   ```bash
   orchestrator task logs {task_id} --tail 10
   ```

4. View logs with timestamps:
   ```bash
   orchestrator task logs {task_id} --timestamps
   ```

### Expected

- Logs display command output
- Tail limit works correctly
- Timestamps are shown when requested

---

## Scenario 5: Task Retry

### Preconditions

- A task with failed items exists

### Steps

1. Create a task that will fail:
   ```bash
   # First create a config with failing agent
   ```

2. After task fails, identify failed item:
   ```bash
   orchestrator task info {task_id}
   ```

3. Get the task item ID:
   ```bash
   # Find failed item ID from task info
   ```

4. Retry the failed item:
   ```bash
   orchestrator task retry {task_item_id}
   ```

### Expected

- Failed task item is retried
- Retry updates the item status

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Task Start | ☐ | | | |
| 2 | Task Start --latest | ☐ | | | |
| 3 | Task Pause and Resume | ☐ | | | |
| 4 | Task Logs | ☐ | | | |
| 5 | Task Retry | ☐ | | | |
