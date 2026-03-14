# Design Doc 52: Prehook Self-Referential Safe Filter

**FR**: FR-040
**Status**: Implemented
**Date**: 2026-03-14

## Problem

FR-034 implemented a Daemon PID Guard at the runner/spawn layer that detects kill commands targeting the daemon. However, when QA testing spawns an LLM agent (e.g., `claude -p "..."`), the top-level command passes the guard, but the agent internally executes `pkill -9 -f orchestratord` — a sub-command the guard never sees.

This caused a real incident during self-bootstrap monitoring: the QA agent testing `53-client-server-architecture` killed the daemon, aborting all in-flight steps and leaving 3 items unresolved.

## Solution: Prehook-Level Frontmatter Filter

### Data Flow

```
QA doc frontmatter: self_referential_safe: false
  → ticket.rs: parse_qa_doc_self_referential_safe()
    → accumulator.rs: populates StepPrehookContext.self_referential_safe
      → prehook/context.rs: adds `self_referential_safe` CEL variable
        → self-bootstrap.yaml prehook: `&& self_referential_safe`
          → step_skipped event (item not spawned at all)
```

### Change

Single-line enhancement to `docs/workflow/self-bootstrap.yaml` qa_testing step prehook:

```yaml
# Before
when: "is_last_cycle && qa_file_path.startsWith(\"docs/qa/\") && qa_file_path.endsWith(\".md\")"

# After
when: "is_last_cycle && qa_file_path.startsWith(\"docs/qa/\") && qa_file_path.endsWith(\".md\") && self_referential_safe"
```

### Why This Works

- **Prevention over detection**: Instead of trying to catch kill commands inside LLM agent sessions, the unsafe QA doc is never dispatched. No agent is spawned, so no kill command can be issued.
- **Existing infrastructure reused**: The `self_referential_safe` field was already parsed (ticket.rs), populated (accumulator.rs), and exposed to CEL (prehook/context.rs) — only the prehook expression was missing.
- **Default-safe**: `is_self_referential_safe()` returns `true` when `self_referential == false` or when frontmatter is absent, so non-self-referential workflows are unaffected.

### Relationship to FR-034

FR-034 provides runtime guard at the spawn layer (last line of defense). FR-040 adds a prehook filter that prevents the problematic agent from being spawned in the first place. The two mechanisms are complementary: the guard catches direct kill commands in step scripts, while the prehook filter handles the case where an LLM agent autonomously issues kill commands that bypass spawn-level inspection.

### Design Decisions

- **Prehook over prompt injection**: Chose deterministic prehook filtering (方案 A) over injecting constraints into agent system prompts (方案 B), which cannot guarantee LLM compliance.
- **No sandbox changes needed**: Process-level sandboxing (方案 C) would require kernel integration; the prehook approach solves the immediate problem with zero Rust code changes.
