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

## Workflow

1. Confirm which QA document(s) to execute under `docs/qa/`.
2. Parse scenarios and required setup (test data, environment, expected DB state).
3. For each scenario:
   - Execute UI steps (if applicable) with browser automation.
   - Execute API steps using `.claude/skills/tools/qa-api-test.sh` (if applicable).
   - Validate database state (if applicable).
   - If failure: collect evidence and create a ticket under `docs/ticket/`.
4. Report a summary: pass/fail counts, created tickets, and any follow-up actions.

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

Create a ticket file under `docs/ticket/` when a scenario fails.

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
```

## Reset Guidance

If environment might be dirty, reset and retry:

```bash
./scripts/reset-docker.sh
```

