---
name: qa-testing
description: Execute scenario-based QA testing with optional browser automation, optional database validation, and ticket creation for failures.
---

# QA Testing Skill

Execute scenario-based QA testing driven by `docs/qa/**/*.md` documents.

## Prerequisites

**CRITICAL: Always rebuild CLI before testing to ensure you have the latest version.**

1. **Rebuild CLI** (REQUIRED - ensures latest code):
   ```bash
   cd core && cargo build --release
   ```

2. **Initialize orchestrator if needed** — only when the database does not exist yet:
   ```bash
   # DANGER: NEVER run `rm -f data/agent_orchestrator.db` or `db reset --include-config`
   # during an active workflow run. Doing so destroys all in-flight task state.
   # Only initialize when no database exists at all.
   test -f data/agent_orchestrator.db || ./scripts/orchestrator.sh init
   ```

3. **For QA scenario isolation** — use project-scoped reset instead of destroying global state:
   ```bash
   # Reset a specific project's QA state (task data + config + auto-tickets)
   ./scripts/orchestrator.sh qa project reset <project> --force
   # Deploy fixtures into project scope (global config untouched)
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/<fixture>.yaml --project <project>
   ```

3. QA docs exist under `docs/qa/`.
4. Optional: UI automation via browser tools.

## Conventions / Configuration

API helper script: `.claude/skills/tools/qa-api-test.sh`

Environment variables:
- `API_BASE_URL` (default: `http://localhost:8080`)
- `API_TOKEN` (optional)
- `API_TOKEN_CMD` (optional command that prints a token to stdout)

Notes:
- If the API does not require auth, omit `API_TOKEN` and `API_TOKEN_CMD`.
- If the API requires auth, prefer providing a project-specific `API_TOKEN_CMD` (for example, a script under `scripts/`).

## Test Scripts Directory (`scripts/qa/`)

**IMPORTANT**: A collection of reusable QA test scripts may exist in `scripts/qa/`. Before writing any new test script, **always check `scripts/qa/` first** for an existing script that covers the same or similar scenario.

- **Reuse first**: Run `ls scripts/qa/` or `Glob: scripts/qa/*` to find existing scripts. If a matching script exists, use it directly (or adapt it) instead of creating a new one.
- **Create in `scripts/qa/`**: When a new test script is needed, always place it under `scripts/qa/` — never in the project root or other ad-hoc locations.
- **Naming convention**: Follow existing patterns — e.g., `test-{feature}.{js,mjs,py,sh}` or `{feature}_test.py`.

## Workflow

**IMPORTANT: This skill is strictly for testing and reporting. NEVER attempt to fix, patch, or modify any source code during QA testing. If a test fails, create a ticket immediately and move on to the next scenario.**

1. Confirm which QA document(s) to execute under `docs/qa/` (never assume when request is ambiguous).
2. Parse scenarios and required setup (test data, environment, expected DB state).
3. For each scenario:
   - Execute UI steps (if applicable) with browser automation.
   - Execute API steps using `.claude/skills/tools/qa-api-test.sh` (if applicable).
   - Validate database state (if applicable).
   - If FAIL → **Immediately create ticket** in `docs/ticket/` (DO NOT defer to end).
   - Report scenario result (PASS/FAIL) before moving to next.
4. Report a summary: pass/fail counts, created tickets, and any follow-up actions.

**Ticket creation rule**: Create the ticket the moment a scenario is confirmed as FAIL — before starting the next scenario. This ensures no failure is lost if the session is interrupted, and gives the user real-time visibility into issues as they surface.

## Step 1: Discover and Confirm Test Document

1. Discover QA docs:
   - `rg --files docs/qa | rg '\.md$'`
2. Exclude index/meta docs (`docs/qa/README.md`, `docs/qa/_*.md`, manifest-like files).
3. If user request is vague, present concise options and ask which doc/module to execute.
4. Only start execution after the target doc(s) are explicit.

## UI Automation (Optional)

If the QA doc requires UI steps, use the browser tools:
- Always take a snapshot before interactions.
- Use short incremental waits (1-3s) after navigation and major actions.
- Capture a final snapshot for expected state verification.

## API Testing Helper

Examples:

```bash
.claude/skills/tools/qa-api-test.sh GET /health
.claude/skills/tools/qa-api-test.sh GET /api/v1/items
.claude/skills/tools/qa-api-test.sh POST /api/v1/items '{"name":"example"}'
```

## Logs / Troubleshooting

For orchestrator CLI, check logs in `data/logs/` directory:

```bash
ls -la data/logs/
# or check specific task logs
./scripts/orchestrator.sh task logs <task-id>
```

## Database Validation (Optional)

Use the QA doc as the source of truth for queries and expected outcomes.

Examples:

```bash
psql "$DB_DSN" -c "SELECT 1"
mysql -h 127.0.0.1 -P 3306 -u root mydb -e "SELECT 1"
```

## Ticket Creation for Failures

**CRITICAL: Create the ticket RIGHT NOW, before moving to the next scenario.** Do NOT accumulate failures for batch ticket creation later. Each failed scenario gets its own ticket written to `docs/ticket/` immediately upon confirmation of failure.

Workflow per failure:
1. Gather all evidence (error message, logs, DB state, screenshots)
2. Write ticket file to `docs/ticket/` using the naming and structure below
3. Inform user: "❌ Scenario #N FAIL — ticket created: `docs/ticket/{filename}.md`"
4. Only then proceed to the next scenario

This ensures:
- No failures are lost if the session is interrupted or context is compressed
- User has real-time visibility into each issue as it surfaces
- Ticket evidence is freshest at the moment of failure (logs, DB state haven't been polluted by subsequent tests)

UI visibility/accessibility failures must also create tickets for UIUX test documents, even if backend behavior is correct.
Examples:
- Hidden or unreachable controls
- Missing or misleading ARIA semantics
- Keyboard navigation/focus failures
- Truncated critical content
- Touch targets too small for mobile use

Naming:
- `{module}_{document}_scenario{N}_{YYMMDD_HHMMSS}.md`

Template:

```markdown
# Ticket: {Scenario Title}

**Created**: {YYYY-MM-DD HH:mm:ss}
**QA Document**: `docs/qa/{module}/{document}.md`
**Scenario**: #{number}
**Status**: FAILED

---

## Test Content
{Brief description}

---

## Expected Result
{Expected outcome}

---

## Actual Result
{What happened}

---

## Repro Steps
1. ...
2. ...

---

## Evidence

**UI/CLI Output**:
```
{key output}
```

**Service Logs**:
```
{relevant log lines}
```

**DB Checks (if applicable)**:
```sql
{queries and results}
```

---

## Analysis

**Root Cause**: {Analysis of likely cause}
**Severity**: High / Medium / Low
**Related Components**: Frontend / Backend / Database / Cache / External Service
```

## Batch Execution: Lessons Learned

When running QA across many documents (e.g., 19 docs, 80+ scenarios), the following hard-won lessons apply.

### 1. Ticket Preservation Across Batches

**CRITICAL: Never delete `docs/ticket/` contents between batches.**

When splitting QA into sequential batches (e.g., docs 00-04, 05-09, 10-14, 15-17), each batch's environment reset **must not** delete tickets created by previous batches. Specifically:

- ❌ **NEVER** include `find docs/ticket -name '*.md' ! -name 'README.md' -delete` as a cross-batch step. This destroys tickets from earlier batches.
- ✅ Only clean tickets **within a single QA doc's setup** if that doc's precondition explicitly requires an empty ticket directory — and even then, back up existing tickets first or scope the deletion narrowly.
- ✅ If a QA doc says "clean ticket dir", only remove tickets created by **that specific doc's previous run**, not all tickets.

**Root cause of past incident**: Batch 2 instructions included a blanket ticket cleanup that wiped Batch 1's 3 tickets. Batch 3 wiped Batch 2's 18 tickets. Batch 4 wiped Batch 3's 10 tickets. Only Batch 4's 5 tickets survived — 31 tickets had to be recreated from session transcripts.

### 2. SQLite Concurrency — WAL Mode

The orchestrator uses WAL mode with connection pooling (r2d2, pool size 20, busy_timeout 5s).
Item-scoped steps can run in parallel when `max_parallel > 1` is configured in the workflow.

- Parallel items share the same DB safely — WAL allows concurrent readers, writes are serialized with retry.
- Do NOT bypass the orchestrator to run raw parallel `task create` + `task start` against the same DB.
- Use `max_parallel` in workflow config to control concurrency; the engine handles semaphore gating.

### 3. Environment Reset Between Batches

The correct reset sequence between batches uses **project-scoped isolation** instead of destroying the global DB:

```bash
# Reset project state (task data + project config + auto-tickets)
./scripts/orchestrator.sh qa project reset <project> --force

# Apply fixture into project scope (global config untouched)
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/<relevant>.yaml --project <project>

# DO NOT delete docs/ticket/*.md — preserve prior batch tickets
# DO NOT run `rm -f data/agent_orchestrator.db` — this destroys all state including bootstrap
```

**CRITICAL**: Never use `rm -f data/agent_orchestrator.db` for QA environment resets. Use `qa project reset` + `apply --project` to isolate each QA batch into its own project scope without affecting global/bootstrap agents.

### 4. `init` vs `qa project reset` vs `apply --project`

- `init` creates the DB schema and a minimal default config. It does **not** load fixture data.
- `qa project reset <project> --force` resets task data, project config, and auto-tickets for the named project — without touching global config or other projects.
- `apply -f <fixture> --project <project>` deploys agents/workflows/workspaces into the project scope. Only project-scoped agents participate in selection for that project's tasks.
- Most QA scenarios should use `qa project reset` + `apply --project` instead of `init` + `config bootstrap`.

### 5. Delegation Prompt Design for Batch Agents

When delegating QA batches to subagents:

- **Explicitly state**: "Do NOT delete any files in `docs/ticket/` that were not created by your own batch"
- **Grant exclusive access**: "YOU HAVE EXCLUSIVE DB ACCESS — no other agents are running"
- **List fixture mapping**: Tell each agent exactly which fixture file to use for each QA doc
- **Include the full ticket template**: Subagents may not have access to this skill doc

### 6. Session Transcripts as Recovery Source

If tickets are lost, they can be reconstructed from subagent session transcripts:
- Each subagent session records all tool calls (including file writes via `apply_patch` or `write`)
- Use `session_search(query="ticket", session_id="ses_xxx")` to find ticket-related actions
- Use `session_read(session_id="ses_xxx")` to read the full final summary with PASS/FAIL details
- Recreated tickets should use a reconstructed timestamp (batch timeframe) rather than current time

## Reset Guidance

For **QA scenario isolation** (recommended — preserves global/bootstrap state):

```bash
# Reset a specific project's QA state
./scripts/orchestrator.sh qa project reset <project> --force

# Deploy fixtures into project scope
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/<fixture>.yaml --project <project>
```

For **full runtime re-initialization** (only when DB is missing or schema needs upgrade):

```bash
# Only if DB does not exist or is corrupted
./scripts/orchestrator.sh init --force
```

**CRITICAL**: Do NOT use `rm -f data/agent_orchestrator.db` during routine QA. This destroys all in-flight task state, bootstrap config, and event history.

## Troubleshooting Configuration Issues

### Orchestrator: CLI Initialization

The orchestrator no longer has hardcoded defaults. For QA scenarios, prefer project-scoped isolation:

```bash
# Preferred: project-scoped reset (preserves global state)
./scripts/orchestrator.sh qa project reset <project> --force
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/<fixture>.yaml --project <project>

# Only if DB does not exist at all:
./scripts/orchestrator.sh init --force

# Or with custom root path
./scripts/orchestrator.sh init --root /path/to/project
```

**CRITICAL**: Do NOT use `rm -f data/agent_orchestrator.db` for QA resets. This destroys all state including bootstrap config, in-flight tasks, and event history. Use `qa project reset` for per-project isolation.

This creates:
- 1 workspace (`default`)
- 1 agent (`echo`) with qa capability
- 1 workflow (`basic`) with qa step

### When CLI fails immediately with config errors:

1. **Run the actual command first** — don't try to guess the problem:
   ```bash
   ./scripts/orchestrator.sh task list
   ```
   Read the actual error message before making any changes.

2. **Common config errors and solutions**:

   - `loop.guard enabled but no agent has loop_guard template`:
     - Set `loop.guard.enabled: false` in the workflow, OR
     - Add `loop_guard` template to an agent

   - `config.workspaces cannot be empty`:
     - Check if there's a top-level `workspaces:` field (not nested under `projects.*`)
     - Remove empty `workspaces: {}` at the end of YAML file

   - `config.agents cannot be empty`:
     - Check if there's a top-level `agents:` field (not nested under `projects.*`)
     - Remove empty `agents: {}` at the end of YAML file

   - `failed to parse config`:
     - Try: `git checkout HEAD~2 -- config/default.yaml` to restore original
     - Then re-validate incrementally

3. **Git restore is faster than debugging**:
   - If config is corrupted, `git checkout HEAD~2 -- path/to/config.yaml` is often faster than iterative fixes
   - Always check git history to understand what changed

4. **Project-specific entry points**:
   - Not all projects use Docker — check `scripts/orchestrator.sh` for CLI tools
   - Look for `scripts/*.sh` files in the project root
   - Check `package.json` scripts for test commands

## Optional Reference Cookbook

If the repository includes `qa-testing/references/api-testing-cookbook.md`, use it as the first reference for repeatable API/gRPC/webhook recipes and known pitfalls.
