# CLAUDE.md

## Forbidden Operations

### Never delete `data/agent_orchestrator.db`

Do NOT run `rm -f data/agent_orchestrator.db`, `rm data/agent_orchestrator.db`, or any command that deletes or truncates the runtime database.

If you encounter a scenario that seems to require deleting the database, it indicates a bug — the system should provide proper isolation (e.g., project-scoped operations) without destructive resets.

- **During QA testing**: Create a ticket under `docs/ticket/` documenting the scenario and the missing isolation mechanism. Adjust the QA doc to work without database deletion, noting the known issue.
- **During interactive work**: Inform the user that the operation would require deleting the database, explain why this suggests a bug, and let the user decide.
