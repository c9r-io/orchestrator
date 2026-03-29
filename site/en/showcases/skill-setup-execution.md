# AI-Native SDLC Skills Initialization Execution Plan

> **Harness Engineering execution plan**: this is an agent-executable scenario that shows how the control plane coordinates environment, workflow, guardrails, and feedback loops rather than a one-off agent call.
>
> **Agent Collaboration**: This document is an agent-executable plan. Open this project in an AI coding agent (Claude Code, OpenCode, Codex, etc.). After reading this plan, the agent orchestrates other agents via the orchestrator CLI to collaboratively complete the task — from resource deployment and task execution to result verification, all done autonomously.

## 1. Goal

Initialize AI-native SDLC skills for the current project. The agent analyzes the project's language, framework, and directory structure, selects appropriate skills from skill templates, customizes them, and installs them to `.claude/skills/`.

## 2. Skill Template Location

Templates are installed at `~/.orchestratord/skill-templates/`, organized into three categories:

```
skill-templates/
├── generic/              # Generic (any project)
│   ├── performance-testing/
│
│   └── project-bootstrap/
├── framework/            # Framework-specific (selected based on project tech stack)
│   ├── align-tests/
│   ├── deploy-gh-k8s/
│   ├── e2e-testing/
│   ├── grpc-regression/
│   ├── ops/
│   ├── project-readiness/
│   ├── reset-local-env/
│   ├── rust-conventions/
│   ├── test-authoring/
│   └── test-coverage/
└── sdlc-patterns/        # SDLC patterns (suitable for projects requiring governance processes)
    ├── fr-governance/
    ├── qa-testing/
    ├── ticket-fix/
    ├── qa-doc-gen/
    └── security-test-doc-gen/
```

## 3. Execution Flow

### 3.1 Analyze the Project

The agent should first check:

```bash
# Check language and framework
ls Cargo.toml 2>/dev/null && echo "Rust project"
ls package.json 2>/dev/null && echo "Node.js project"
ls go.mod 2>/dev/null && echo "Go project"
ls docker-compose.yml docker/docker-compose.yml 2>/dev/null && echo "Docker Compose found"
ls k8s/ deploy/ 2>/dev/null && echo "Kubernetes found"
ls .github/workflows/ 2>/dev/null && echo "GitHub Actions found"
ls docs/qa/ 2>/dev/null && echo "QA docs found"
```

### 3.2 Select Skills

Based on the analysis results, the agent decides which skills to install:

| Condition | Skills to Install |
|-----------|-------------------|
| Any project | `performance-testing` |
| Has `Cargo.toml` | `rust-conventions`, `align-tests`, `test-coverage`, `test-authoring` |
| Has `package.json` | `e2e-testing`, `test-authoring` |
| Has `docker-compose.yml` | `ops`, `reset-local-env` |
| Has `k8s/` or `deploy/` | `deploy-gh-k8s`, `project-readiness` |
| Has `.github/workflows/` | `project-readiness` |
| Has `docs/qa/` | `qa-testing`, `ticket-fix`, `qa-doc-gen` |
| Has `docs/feature_request/` | `fr-governance` |
| Has `docs/security/` | `security-test-doc-gen` |

### 3.3 Customize Templates

For each selected skill:

1. Copy from `~/.orchestratord/skill-templates/<category>/<skill>/` to `.claude/skills/<skill>/`
2. Read the `SKILL.md` in the template
3. Replace placeholders based on the actual project structure:
   - `<project-root>` -> actual project root path
   - `core/` -> actual backend source directory
   - `portal/` -> actual frontend directory
   - `docker/docker-compose.yml` -> actual compose file path
   - `docs/qa/<project>/` -> actual QA documentation directory

### 3.4 Verification

```bash
# Confirm skills are installed
ls .claude/skills/

# Each skill should have a SKILL.md
for d in .claude/skills/*/; do
  [[ -f "$d/SKILL.md" ]] && echo "OK: $d" || echo "MISSING: $d/SKILL.md"
done
```

## 4. Notes

- The agent should only install skills relevant to the current project — do not install all of them
- If templates do not exist (`~/.orchestratord/skill-templates/` is empty), prompt the user to run `install.sh` or download from GitHub Releases
- Existing `.claude/skills/` should not be overwritten — skip skills that are already present
- When customizing, preserve the SKILL.md frontmatter format (`---` delimited name/description)
