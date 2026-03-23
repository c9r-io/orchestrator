# Design Doc 81: Open-Source Compliance Infrastructure

## FR Reference

FR-071: 开源合规基础设施

## Design Decisions

### License Choice: MIT

Selected MIT to match the existing `license = "MIT"` declaration in `core/Cargo.toml`. MIT is permissive, well-understood, and widely adopted in the Rust ecosystem.

### CHANGELOG Format

Adopted [Keep a Changelog](https://keepachangelog.com/) format with categories (Added, Changed, Fixed, Removed). v0.1.0 serves as the initial release, summarizing all features built across 611 commits.

### AI-Native Contribution Model

CONTRIBUTING.md explicitly positions the project as AI-native developed. Key decisions:

- **Feature requests as primary contribution path** — GitHub Issues for use cases and ideas
- **Bug reports** — standard issue template
- **PRs welcome but exploratory** — non-trivial changes should start with an issue discussion to avoid duplication with the AI-native development workflow
- No traditional CODEOWNERS or mandatory review gates (development model is evolving)

### GitHub Templates

Lightweight templates that guide without over-constraining:
- Bug report: environment + reproduction steps
- Feature request: problem + proposed solution
- PR template: minimal checklist (fmt, clippy, test)

### Cargo.toml License Consistency

Added `license = "MIT"` to all 8 workspace member Cargo.toml files for consistent metadata across the workspace.

## Files Created

| File | Purpose |
|------|---------|
| `LICENSE` | MIT license text |
| `CHANGELOG.md` | Release changelog (Keep a Changelog format) |
| `CONTRIBUTING.md` | Contribution guidelines (AI-native model) |
| `.github/ISSUE_TEMPLATE/bug_report.md` | Bug report template |
| `.github/ISSUE_TEMPLATE/feature_request.md` | Feature request template |
| `.github/PULL_REQUEST_TEMPLATE.md` | PR template |
