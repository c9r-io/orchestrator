# QA 123: Open-Source Compliance Infrastructure

## FR Reference

FR-071

## Verification Scenarios

### Scenario 1: LICENSE file

**Steps:**
1. `cat LICENSE`
2. Verify MIT license text with copyright `2026 c9r-io contributors`

**Expected:** Valid MIT license text present at repo root.

### Scenario 2: Cargo.toml license consistency

**Steps:**
1. `grep -r 'license = "MIT"' --include='Cargo.toml' | wc -l`

**Expected:** 8 (all workspace members).

### Scenario 3: CHANGELOG format

**Steps:**
1. `head -20 CHANGELOG.md`
2. Verify Keep a Changelog header and `[0.1.0]` entry

**Expected:** Valid changelog with v0.1.0 section containing Added subsections.

### Scenario 4: CONTRIBUTING.md content

**Steps:**
1. `grep -c 'AI-native' CONTRIBUTING.md`
2. Verify sections: Feature Requests, Bug Reports, Pull Requests, Development Setup

**Expected:** AI-native positioning present; all sections exist.

### Scenario 5: GitHub templates

**Steps:**
1. `ls .github/ISSUE_TEMPLATE/`
2. `ls .github/PULL_REQUEST_TEMPLATE.md`

**Expected:** `bug_report.md` and `feature_request.md` in ISSUE_TEMPLATE/; PR template exists.

### Scenario 6: v0.1.0 release (manual)

**Steps:**
1. `git tag v0.1.0 && git push origin v0.1.0`
2. Check GitHub Actions release workflow completes
3. Verify 4 platform tarballs + checksums on Releases page
4. `curl -fsSL https://raw.githubusercontent.com/c9r-io/orchestrator/main/install.sh | sh`

**Expected:** Binaries downloadable and installable.
