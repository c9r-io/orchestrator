---
self_referential_safe: false
---

# Smoke Test: Orchestrator Core

**Marker:** ORC_SMOKE_20260227

## Checklist

- [ ] CLI binary builds successfully (`cargo build --release`)
- [ ] Database schema initializes at `data/agent_orchestrator.db`
- [ ] CLI help command works (`./target/release/orchestrator --help`)
- [ ] Daemon starts from a cold boot with an empty SQLite file and creates the UDS socket (`./target/release/orchestratord --foreground`)
- [ ] Config manifest can be validated (`manifest validate`)
- [ ] Agent listing responds (`get agents`)

(End of file - total 13 lines)
