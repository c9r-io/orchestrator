---
name: qa-testing
description: Execute scenario-based QA testing with optional browser automation, optional database validation, and ticket creation for failures.
---

# QA Testing Skill

Execute scenario-based QA testing driven by `docs/qa/**/*.md` documents. This skill is intentionally project-agnostic and assumes the repo was created by `project-bootstrap` (or follows similar Docker/K8s conventions).

## Prerequisites

- Local environment is runnable via Docker Compose (`docker/docker-compose.yml`).
- Reset script exists: `scripts/reset-docker.sh`.
- QA docs exist under `docs/qa/` (generate them via `qa-doc-gen` if missing).
- Optional (only if needed by the QA doc):
  - UI automation via Playwright tools.
  - Database CLI access for validation queries.

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

1. Confirm which QA document(s) to execute under `docs/qa/`.
2. Parse scenarios and required setup (test data, environment, expected DB state).
3. For each scenario:
   - Execute UI steps (if applicable) with browser automation.
   - Execute API steps using `.claude/skills/tools/qa-api-test.sh` (if applicable).
   - Validate database state (if applicable).
   - If FAIL → **Immediately create ticket** in `docs/ticket/` (DO NOT defer to end).
   - Report scenario result (PASS/FAIL) before moving to next.
4. Report a summary: pass/fail counts, created tickets, and any follow-up actions.

**Ticket creation rule**: Create the ticket the moment a scenario is confirmed as FAIL — before starting the next scenario. This ensures no failure is lost if the session is interrupted, and gives the user real-time visibility into issues as they surface.

## UI Automation (Optional)

If the QA doc requires UI steps, use the Playwright MCP tools:
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

Prefer Compose logs first:

```bash
docker compose -f docker/docker-compose.yml ps
docker compose -f docker/docker-compose.yml logs --tail 200
```

If you need a single container:

```bash
docker ps
docker logs <container> --tail 200
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

## Reset Guidance

If environment might be dirty, reset and retry:

```bash
./scripts/reset-docker.sh
```

