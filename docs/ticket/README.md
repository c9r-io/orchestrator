# QA Testing Tickets

This directory contains individual ticket files for failed QA test scenarios.

## Ticket Format

Each ticket file represents a single failed test scenario with the following information:

- **Test Content**: What was being tested
- **Expected Result**: What should have happened
- **Reproduction Steps**: How to reproduce the issue
- **Actual Result**: What actually happened, including error logs and database state

## Naming Convention

Ticket files follow this naming pattern:

```
{module}_{document}_scenario{N}_{YYMMDD_HHMMSS}.md
```

Examples:
- `user_01-crud_scenario2_260203_143052.md`
- `tenant_01-crud_scenario5_260203_143125.md`
- `rbac_02-role_scenario3_260203_143201.md`

## Workflow

1. **QA Testing Skill** automatically creates tickets when scenarios fail
2. Development team reviews tickets to understand issues
3. Developers fix the issues based on ticket details
4. Re-run QA tests to verify fixes
5. After verification, delete the resolved ticket; git history preserves its evidence

## Ticket Lifecycle

```
[FAILED] → [IN_PROGRESS] → [FIXED] → [VERIFIED] → [CLOSED]
```

Update ticket status by editing the **Status** field in the ticket file. Active
ticket Markdown files are intentionally tracked so another agent or checkout can
resume the failure investigation. Use `git status` after ticket creation to prove
the ticket is not ignored.

## Related Directories

- `docs/qa/` - QA test scenarios organized by module
 
Notes:
- Tickets are the input for the `ticket-fix` skill.
- A ticket is deleted only after its original QA scenario passes. The deletion and
  the fixing commit retain the audit trail; there is no separate `closed/` archive
  command or automatic move.
