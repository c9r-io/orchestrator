---
name: dependabot-governance
description: "Govern Dependabot dependency upgrade PRs — audit CI, diagnose failures, combine breaking upgrades, rebase, merge, and close. Use when the user asks to handle Dependabot PRs, govern deps, or says '治理依赖', '/dependabot-governance'."
---

# Dependabot PR Governance

Audit, remediate, and merge all open Dependabot PRs in one pass.

## Phase 1: Audit

1. List open Dependabot PRs:
   ```
   gh pr list --state open --author 'app/dependabot'
   ```
2. For each PR, fetch CI and mergeability:
   ```
   gh pr view <N> --json title,mergeable,mergeStateStatus,statusCheckRollup,headRefName
   ```
3. Record: PR number, package, ecosystem (npm/rust), version bump, CI pass/fail per job, mergeable state.

## Phase 2: Classify

| Category | Criteria | Action |
|----------|----------|--------|
| **green** | All CI pass, mergeable | Merge in Phase 4 |
| **rebase-needed** | CI fails on fmt/lint only (pre-existing on main), or has merge conflict | `@dependabot rebase` |
| **breaking-combo** | CI fails on build/test — shared backend requires combined upgrade | Create combined branch in Phase 3 |

### Shared-backend detection

Dependabot upgrades crates independently, but some must be upgraded together because they share a transitive dependency with breaking trait changes. Common Rust groups:

- `sha2` + `hmac` + `pbkdf2` → `digest` backend
- `aes` + `ctr` → `cipher` backend
- `notify` + `notify-debouncer-full` → `notify-types` backend

Detection method:
1. If a PR's CI shows trait-mismatch or version-conflict compile errors, inspect `Cargo.toml` for the shared transitive dep
2. Check if another open Dependabot PR bumps the counterpart crate
3. If so, classify both as **breaking-combo**

## Phase 3: Remediate breaking-combo PRs

1. Investigate CI failure logs:
   ```
   gh run list --branch <branch> --limit 1 --json databaseId -q '.[0].databaseId'
   gh run view <run-id> --log-failed 2>&1 | head -200
   ```
2. Find affected source files: `grep -r "use <crate>" --include="*.rs" -l`
3. Create combined branch from main: `deps/<combined-name>`
4. Apply all version bumps, fix compile errors
5. Common migration patterns:
   - `hmac` 0.13: add `use hmac::KeyInit` (no longer re-exported via `Mac`)
   - `sha2` 0.11: `Digest::Output` drops `LowerHex` — use `hash.iter().map(|b| format!("{b:02x}")).collect()`
6. Verify: `cargo check && cargo fmt --check && cargo clippy && cargo test`
7. Commit, push, create PR referencing superseded Dependabot PRs
8. Close individual Dependabot PRs: `gh pr close <N> --comment "Superseded by #<combined>."`

## Phase 4: Merge

Order matters — merging one Dependabot PR often causes Cargo.lock conflicts in others.

1. Merge **green** PRs one at a time: `gh pr merge <N> --merge --delete-branch`
2. After each merge, check remaining PRs for new conflicts
3. If conflict appears, `gh pr comment <N> --body "@dependabot rebase"` and wait
4. Once rebase completes and CI passes, merge
5. Merge combined PRs last (they tend to touch more of Cargo.lock)

## Phase 5: Closure Check

All Dependabot PRs must reach a terminal state before governance is complete. Terminal states: **merged**, **closed**.

### Closure loop

1. After Phase 4, list remaining open Dependabot PRs:
   ```
   gh pr list --state open --author 'app/dependabot'
   ```
2. If any remain (e.g., waiting for `@dependabot rebase`):
   - Check mergeability: `gh pr view <N> --json mergeable,mergeStateStatus`
   - If `MERGEABLE` + CI green → merge immediately
   - If `CONFLICTING` or CI pending → wait and re-check (poll up to 3 times with 30s intervals)
   - If still not resolvable after retries → report as blocked with reason
3. Repeat until no open Dependabot PRs remain or all remaining are reported as blocked

### Completion criteria

Governance is **complete** only when:
- Zero open Dependabot PRs, OR
- All remaining PRs are explicitly reported as blocked with actionable next steps

### Final report

Present summary table:

| PR | Package | Action | Result |
|----|---------|--------|--------|
| #N | crate x.y→z.w | merged / closed / blocked | status |

Include:
- Total PRs processed
- Blocked PRs with reason and suggested follow-up
- If all closed: "Dependabot governance complete — zero open PRs."
