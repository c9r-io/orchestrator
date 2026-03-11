---
name: ticket-fix
description: Fix QA tickets by reading docs/ticket/*.md, verifying the issue against real implementation, analyzing false positives and updating QA docs to prevent recurrence, applying fixes when needed, resetting the test environment, re-running QA steps from the ticket, and deleting the ticket file once verified. Use when a user asks to fix a ticket or resolve a QA failure.
---

# Ticket Fix

Resolve QA tickets end-to-end: read the ticket, confirm the issue, fix or dismiss, reset the test environment, re-run QA steps, and clean up the ticket on success.

## Workflow

1. **Discover tickets**
   - List `docs/ticket/*.md` and confirm which ticket(s) to handle if not specified.

2. **Read ticket and validate**
   - Parse: scenario, steps, expected/actual, environment, SQL checks.
   - Reproduce quickly against current implementation.
   - If issue no longer reproducible, document why and still run QA verification.

3. **Analyze whether ticket is a false positive (误报)**
   - Compare ticket's expected behavior against the actual design intent documented in `docs/design_doc/` and `docs/qa/`.
   - If the implementation is correct and the ticket stems from incorrect or outdated QA expectations:
     - Mark the ticket as a false positive.
     - Identify the root cause of the misreport (e.g., outdated expected values, missing preconditions, ambiguous acceptance criteria).
     - Use common false-positive patterns as a checklist:
       - Missing required authentication/signature headers in QA commands.
       - Prerequisites incomplete or ambiguous.
       - Environment assumptions wrong for default local setup.
       - Test data references entities that do not exist.
     - Update the relevant QA test document(s) under `docs/qa/` to correct expectations, clarify steps, or add notes that prevent future engineers from raising the same false positive.
     - Ensure updated QA commands are copy-paste-ready for the default local environment.
     - Add a troubleshooting table when the failure mode is easy to repeat.
     - Proceed to step 4 (reset) and step 5 (re-run) to confirm the implementation is indeed correct.
     - In step 6, delete the ticket and summarize why it was a false positive and what QA docs were updated.

4. **Fix if needed**
   - If confirmed as a real issue, implement code fix.
   - Keep change scope minimal and aligned to ticket.

5. **Reset environment (always)**
   - Follow the repository's documented reset/isolation workflow before final verification.
   - Prefer project-scoped cleanup and fixture re-apply when the repo's QA docs require non-destructive isolation.
   - If the repo provides a dedicated reset script, use it.

6. **Re-run QA steps**
   - Follow the ticket’s steps and SQL validation.
   - Capture evidence (log snippets, DB results).

7. **Close ticket**
   - If verified, delete the ticket file from `docs/ticket/`.
   - Summarize fix + verification in response.

## Rules

- Always reset environment before final verification.
- Do not delete ticket unless QA re-test passes.
- If issue cannot be reproduced, explain why and still re-test.
- If re-test fails, keep the ticket and report remaining issue.
- When a ticket is identified as a false positive, always update the relevant QA doc under `docs/qa/` to correct the expectation, add clarifying notes, or fix preconditions so the same false positive is not raised again.
- Keep false-positive doc updates scoped to the failing scenario; avoid broad rewrites of unrelated sections.

## Notes

- Use `docs/qa/` for any referenced test cases.
- Use the repo's runtime logs and DB queries from the ticket for evidence.
