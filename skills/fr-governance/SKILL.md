---
name: fr-governance
description: Govern feature request (FR) documents through their full lifecycle — from planning to implementation to closure. Use when the user asks to govern/治理 a feature request, close an FR, check FR status, or says "治理FR", "/fr-governance". Scans docs/feature_request/ for open FR docs, plans implementation, executes governance, and self-checks closure.
---

# FR Governance Workflow

Execute feature request governance in three phases: Select, Plan, Govern & Close.

## Phase 1: Select FR

1. Scan `docs/feature_request/` for `FR-*.md` files (exclude `README.md`)
2. If no FR files found, inform the user and stop
3. If exactly one FR file, auto-select it and confirm with user
4. If multiple FR files, present a numbered list with ID, title, priority, and status, then ask the user which one to govern
5. Read the selected FR document fully to understand scope, requirements, and acceptance criteria

## Phase 2: Plan (enter plan mode)

Enter plan mode and produce a governance plan that covers:

1. **Current state audit** — grep/search the codebase to understand what has already been implemented vs what remains
2. **Implementation plan** — concrete steps to fulfill each requirement in the FR, ordered by dependency
3. **Acceptance mapping** — map each acceptance criterion to specific implementation actions and verification methods
4. **Risk mitigation** — address risks listed in the FR with concrete countermeasures
5. **Artifact plan** — identify design docs (`docs/design_doc/`) and QA docs (`docs/qa/`) to create upon closure

Present the plan to the user for approval before proceeding.

## Phase 3: Implement & Close

After plan approval, execute the implementation. When done:

### Self-check procedure

1. Re-read the FR document
2. For each acceptance criterion, verify implementation by searching code, running tests, or inspecting artifacts
3. Classify the FR:
   - **Closed**: all acceptance criteria met, tests pass, no open items
   - **Partially done**: some criteria met, others remain

### If closed (all criteria met):

1. Create design doc under `docs/design_doc/orchestrator/` documenting the design decisions
2. Create QA doc under `docs/qa/orchestrator/` with verification scenarios
3. Delete the FR file from `docs/feature_request/`
4. Update `docs/feature_request/README.md`:
   - Remove the FR row from the table (or mark status)
   - Add a closure note following the existing pattern: `FR-XXX 已闭环删除；其设计与验证信息现由 docs/design_doc/... 与 docs/qa/... 承载`

### If partially done:

1. Update the FR document to reflect current implementation status
2. Mark completed requirements with checkmarks
3. Update status from `Proposed` to `In Progress`
4. Update `docs/feature_request/README.md` table status to `In Progress`
5. Summarize remaining work to the user
