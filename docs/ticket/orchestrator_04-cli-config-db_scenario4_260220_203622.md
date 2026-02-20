# Ticket: Database Reset Does Not Clear Workspace Data

**Created**: 2026-02-20 20:36:22
**QA Document**: `docs/qa/orchestrator/04-cli-config-db.md`
**Scenario**: #4
**Status**: FAILED

---

## Test Content
Verify that `orchestrator db reset --force` clears both task and workspace data from the database.

---

## Expected Result
- Without `--force`, command prompts for confirmation
- With `--force`, database is cleared
- **Task and workspace data is removed**

---

## Actual Result
- Without `--force`: ✅ Command correctly requires `--force` flag
- With `--force`: ✅ Database reset completed
- Task data: ✅ Cleared (task list is empty)
- Workspace data: ❌ **Still present in database**

---

## Repro Steps
1. Add workspaces to configuration:
   ```bash
   orchestrator config set /tmp/add-workspace.yaml
   ```

2. Verify workspaces exist:
   ```bash
   orchestrator workspace list
   # Shows: default, new-workspace
   ```

3. Reset database with force:
   ```bash
   orchestrator db reset --force
   # Output: Database reset completed
   ```

4. Check workspace data:
   ```bash
   orchestrator workspace list
   # Still shows: new-workspace, default
   ```

---

## Evidence

**Before Reset**:
```
ID                   ROOT PATH                               
-------------------- ----------------------------------------
default              /Volumes/Yotta/ai_native_sdlc           
new-workspace        /tmp/test-new-workspace                 
```

**Task List Before**:
```
ID                                     NAME         STATUS     FINISHED FAILED  
-------------------------------------- ------------ ---------- -------- --------
1de1fb8b                               Test task fo pending    0        0       
```

**Reset Command**:
```bash
$ orchestrator db reset --force
Database reset completed
```

**Task List After** (correctly cleared):
```
ID                                     NAME         STATUS     FINISHED FAILED  
-------------------------------------- ------------ ---------- -------- --------
```

**Workspace List After** (NOT cleared):
```
ID                   ROOT PATH                               
-------------------- ----------------------------------------
new-workspace        /tmp/test-new-workspace                 
default              /Volumes/Yotta/ai_native_sdlc           
```

---

## Analysis

**Root Cause**: The `db reset --force` command appears to clear the tasks table but not the workspaces table. Workspaces may be stored in configuration rather than purely in the database, or the reset logic only targets task-related tables.

**Ambiguity in Spec**: The QA document states "Task and workspace data is removed", but it's unclear whether:
1. Workspaces should be removed from the database (if stored there)
2. Workspaces should be removed from the configuration file
3. Both should be cleared

**Possible Implementations**:
- If workspaces are config-derived and synced to DB on startup, `db reset` might not affect them
- If workspaces are runtime-only DB records, they should be cleared
- Design decision needed: should `db reset` also clear configuration or just runtime state?

**Severity**: Medium
**Related Components**: Database / CLI / Configuration Management
