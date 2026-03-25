# Design Doc 84: Task Items & Event List CLI Commands

## FR Reference

FR-078: Task Items 与 Event List CLI 命令

## Design Decisions

### `task items` — Reuse Existing RPC

`orchestrator task items <task_id>` reuses the existing `TaskInfo` RPC which already returns all task items. The CLI formats only the items section with optional `--status` client-side filtering. This avoids a new RPC for a small dataset (<100 items per task).

### `event list` — New `TaskEvents` RPC

`orchestrator event list --task <task_id>` uses a new `TaskEvents` RPC because:
- The existing `TaskInfo` hardcodes `LIMIT 200` for events with no filtering
- Users need `--type` prefix matching (e.g., `step_skipped`, `self_restart%`)
- Users need configurable `--limit`

### Proto Changes

```protobuf
rpc TaskEvents(TaskEventsRequest) returns (TaskEventsResponse);

message TaskEventsRequest {
  string task_id = 1;
  string event_type_filter = 2;  // prefix match via SQL LIKE
  uint32 limit = 3;              // 0 = default (50)
}

message TaskEventsResponse {
  repeated Event events = 1;
}
```

### Output Formats

Both commands support `-o table` (default), `-o json`, and `-o yaml`.

**task items table:**
```
ORDER    LABEL                                    STATUS       FIXED
1        docs/qa/orchestrator/01-cli.md           resolved     yes
2        docs/qa/orchestrator/02-task.md          running      no
```

**event list table:**
```
ID       TYPE                         PAYLOAD                                                      CREATED
1234     step_started                 {"step":"qa_testing","item_id":"item-1"}...                  2026-03-25T00:01:00Z
```

## Files Modified

| File | Change |
|------|--------|
| `crates/proto/orchestrator.proto` | Added `TaskEvents` RPC + messages |
| `core/src/event_cleanup.rs` | Added `list_task_events()` query |
| `crates/daemon/src/server/system.rs` | Added `task_events` handler |
| `crates/daemon/src/server/mod.rs` | Wired `TaskEvents` RPC |
| `crates/cli/src/cli.rs` | Added `Items` to TaskCommands, `List` to EventCommands |
| `crates/cli/src/commands/task.rs` | Added items handler |
| `crates/cli/src/commands/event.rs` | Added list handler |
| `crates/cli/src/output/mod.rs` | Added `print_task_items()` and `print_event_list()` |
| `crates/integration-tests/src/lib.rs` | Added `task_events` trait impl |
