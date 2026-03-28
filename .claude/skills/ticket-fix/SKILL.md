---
name: ticket-fix
description: Fix QA tickets by reading docs/ticket/*.md, verifying the issue against real implementation, performing three-way classification (false positive / bug / feature gap), entering plan mode for user approval before acting, applying fixes or creating feature requests as needed, resetting the test environment, re-running QA steps, and deleting the ticket file once verified. Use when a user asks to fix a ticket or resolve a QA failure.
---

# Ticket Fix

Resolve QA tickets end-to-end: read the ticket, classify the issue (false positive / bug / feature gap), get user approval via plan mode, then fix or route appropriately, reset the test environment, re-run QA steps, and clean up the ticket on success.

## Workflow

### Phase 1: Investigate

1. **Discover tickets**
   - List `docs/ticket/*.md` and confirm which ticket(s) to handle if not specified.

2. **Read ticket and validate**
   - Parse: scenario, steps, expected/actual, environment, SQL checks.
   - Reproduce quickly against current implementation.
   - If issue no longer reproducible, document why and still run QA verification.

3. **Three-way classification (triage)**

   Compare the ticket’s expected behavior against the actual design intent documented in `docs/design_doc/` and `docs/qa/`, and classify the ticket:

   | Classification | Meaning | Action |
   |---|---|---|
   | **False Positive (误报)** | Implementation is correct; test expectation is wrong | Update QA docs, close ticket |
   | **Bug (缺陷)** | Implementation deviates from design intent | Fix code, verify, close ticket |
   | **Feature Gap (功能缺口)** | Design itself is incomplete; the ticket reveals missing capability | Create/update FR, update QA docs, close ticket with FR reference |

   **3a. False Positive (误报)**
   - The implementation is correct and the ticket stems from incorrect or outdated QA expectations.
   - Identify the root cause of the misreport (e.g., outdated expected values, missing preconditions, ambiguous acceptance criteria).
   - Use common false-positive patterns as a checklist:
     - Missing required authentication/signature headers in QA commands.
     - Prerequisites incomplete or ambiguous.
     - Environment assumptions wrong for default local setup.
     - Test data references entities that do not exist.
   - Action: update the relevant QA test document(s) under `docs/qa/` to correct expectations, clarify steps, or add notes that prevent future engineers from raising the same false positive.
   - Ensure updated QA commands are copy-paste-ready for the default local environment.
   - Add a troubleshooting table when the failure mode is easy to repeat.

   **3b. Bug (缺陷)**
   - The implementation does not match the design intent in `docs/design_doc/` or violates acceptance criteria.
   - Action: implement a minimal code fix aligned to the ticket scope.

   **3c. Feature Gap (功能缺口)**
   - The ticket is NOT a false positive (the failure is real), but it is also NOT a bug — the implementation correctly follows the current design, but the design itself is incomplete or does not cover this scenario.
   - Indicators:
     - The ticket expects behavior that no design doc covers.
     - The acceptance criteria in the relevant FR or design doc don’t address this scenario.
     - The implementation correctly follows the design, but the design is insufficient for real-world use.
   - Action:
     - Check `docs/feature_request/` for an existing open FR that covers this gap.
     - If an FR exists: link the ticket to it and note the gap in the FR doc.
     - If no FR exists: create a new FR document under `docs/feature_request/` with the gap description, acceptance criteria derived from the ticket’s reproduction steps, and P2 priority as default.
     - Update `docs/feature_request/README.md` index.
     - Update the relevant QA doc to note the gap as a known limitation (not a test error).
     - Do NOT attempt to fix the code — this is a design-level issue that needs FR governance.

### Phase 2: Plan Mode Gate

4. **Enter plan mode for user approval**
   - Before executing any fix or action, enter plan mode (use `EnterPlanMode`).
   - Present:
     - The classification result with supporting evidence.
     - The proposed action (QA doc update / code fix / FR creation).
     - List of files to be modified.
   - Wait for user approval before proceeding to Phase 3.

### Phase 3: Execute

5. **Fix or route**
   - **False Positive**: update QA docs as described in 3a.
   - **Bug**: implement code fix as described in 3b.
   - **Feature Gap**: create/update FR and update QA docs as described in 3c.

6. **Reset environment (always)**
   - Follow the repository’s documented reset/isolation workflow before final verification.
   - Prefer project-scoped cleanup and fixture re-apply when the repo’s QA docs require non-destructive isolation.
   - If the repo provides a dedicated reset script, use it.

7. **Re-run QA steps**
   - Follow the ticket’s steps and SQL validation.
   - Capture evidence (log snippets, DB results).

8. **Close ticket**
   - If verified, delete the ticket file from `docs/ticket/`.
   - Summarize: classification + action taken + verification evidence.
   - For feature gaps: include the FR document path in the summary.

## Rules

- Always classify before acting. Never skip the three-way triage.
- Always enter plan mode after classification and before executing changes.
- Always reset environment before final verification.
- Do not delete ticket unless QA re-test passes.
- If issue cannot be reproduced, explain why and still re-test.
- If re-test fails, keep the ticket and report remaining issue.
- When a ticket is identified as a false positive, always update the relevant QA doc under `docs/qa/` to correct the expectation, add clarifying notes, or fix preconditions so the same false positive is not raised again.
- Keep false-positive doc updates scoped to the failing scenario; avoid broad rewrites of unrelated sections.
- Feature gaps must result in an FR document, not a code fix.
- When creating an FR from a ticket, include the ticket’s reproduction steps as acceptance criteria.
- Always cross-reference `docs/design_doc/` to confirm a gap isn’t already addressed by an existing design doc before classifying as feature gap.

## Notes

- Use `docs/qa/` for any referenced test cases.
- Use `docs/design_doc/` for design intent verification.
- Use `docs/feature_request/` for feature gap routing.
- Use the repo’s runtime logs and DB queries from the ticket for evidence.
