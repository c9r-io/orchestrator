---
self_referential_safe: true
---

# QA 130: Integration Manifest Packages

## FR Reference

FR-082

## Scenario 1: Integrations repo exists

**Steps:**
1. `gh repo view c9r-io/orchestrator-integrations --json name`

**Expected:** Repository exists.

## Scenario 2: Slack package completeness

**Steps:**
1. `ls orchestrator-integrations/slack/`

**Expected:** Contains `secrets-template.yaml`, `trigger-message.yaml`, `trigger-slash-command.yaml`, `step-template-parse.yaml`, `README.md`.

## Scenario 3: GitHub package completeness

**Steps:**
1. `ls orchestrator-integrations/github/`

**Expected:** Contains `secrets-template.yaml`, `trigger-push.yaml`, `trigger-pr-opened.yaml`, `trigger-issue-comment.yaml`, `README.md`.

## Scenario 4: LINE package completeness

**Steps:**
1. `ls orchestrator-integrations/line/`

**Expected:** Contains `secrets-template.yaml`, `trigger-message.yaml`, `README.md`.

## Scenario 5: Manifest syntax valid

**Steps:**
1. For each trigger YAML, verify it parses as valid YAML with correct `apiVersion: orchestrator.dev/v2`

**Expected:** All manifests have correct apiVersion, kind, metadata, spec structure.

## Scenario 6: Secret rotation showcase exists

**Steps:**
1. `cat docs/showcases/secret-rotation-workflow.md | head -5`

**Expected:** File exists with Agent Collaboration header.

## Scenario 7: Each README has setup steps

**Steps:**
1. Check each integration README for: Prerequisites, Setup steps, Apply commands

**Expected:** All READMEs are complete.
