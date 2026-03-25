# Automated Promotion Content Generation and Distribution Execution Plan

This document is the 4th category of orchestrator showcase: **Project Promotion** — automated content creation and multi-platform distribution. Unlike the first three showcase categories (self-bootstrap, self-evolution, full QA), this workflow demonstrates the orchestrator's ability to handle **externally-facing automation tasks** rather than internal SDLC loops.

Applicable scenarios: publicity after shipping major features, periodic weekly promotion, and content distribution after reaching milestones.

---

## 1. Task Objective

> Topic name: `Automated Project Promotion Content Generation`
>
> Background:
> As an AI-native SDLC automation tool, orchestrator needs to periodically promote project progress to the technical community.
> Manually writing promotion content for multiple platforms is time-consuming and error-prone, making it a good fit for orchestrator-driven automation.
>
> Objectives for this round:
> Collect recent project changes, have AI analyze highlights, generate promotion content for
> Dev.to / Hashnode / Twitter / LinkedIn / HN platforms, automatically publish to platforms with APIs, and save drafts for manual platforms.
>
> Constraints:
> 1. Do not modify project code; only generate promotion content.
> 2. Track published content via WorkflowStore to avoid re-promoting the same commits.
> 3. HN / Reddit are draft-only; human review and manual submission required.
> 4. Dev.to publishes with `published: false` draft status; human confirmation required before publishing.

### 1.1 Expected Outputs

Produced autonomously by the orchestrator:

1. Multi-platform promotion drafts under `docs/promotion/drafts/` (JSON format with title/body/metadata).
2. Draft articles on Dev.to (if `DEVTO_API_KEY` is configured).
3. Publication records in WorkflowStore (`last_published_sha` + date index).

### 1.2 Execution Pipeline

```text
gather_updates(command) → analyze_highlights(agent) → generate_content(agent,item×N) → save_drafts(command,item×N) → publish(command,item×N) → track_results(command) → loop_guard
```

### 1.3 Non-Goals

- Do not auto-publish to Hacker News or Reddit (no safe API, and anti-spam mechanisms are strict).
- Do not generate video, podcast, or other non-text content.
- Do not evaluate promotion effectiveness (views, likes, etc.) — this can be a future iteration.

---

## 2. Prerequisites

### 2.1 Platform API Keys (Optional)

| Platform | How to Obtain | Environment Variable | Required |
|----------|--------------|---------------------|----------|
| Dev.to | https://dev.to/settings/extensions | `DEVTO_API_KEY` | No (without key, only drafts are generated) |

When no API key is configured, all platforms will only generate drafts without publishing.

### 2.2 Claude API Credits

This workflow uses 2 agent steps (`analyze_highlights` + `generate_content` x number of platforms),
all using the claude-sonnet model. A single execution is estimated to consume approximately 5-10 sonnet calls.

---

## 3. Execution Steps

### 3.1 Build and Start the Daemon

```bash
cd "$ORCHESTRATOR_ROOT"   # your orchestrator project directory

cargo build --release -p orchestratord -p orchestrator-cli

# Start the daemon (if not already running)
nohup ./target/release/orchestratord --foreground --workers 2 > /tmp/orchestratord.log 2>&1 &

# Verify the daemon is running
ps aux | grep orchestratord | grep -v grep
```

### 3.2 Load Resources

```bash
# Clean up old promotion project if it exists
orchestrator delete project/promotion --force

orchestrator init

# Load secrets (if you have a Dev.to API key, first export DEVTO_API_KEY=xxx)
orchestrator apply -f your-secrets.yaml           --project promotion

# Load the main workflow
orchestrator apply -f docs/workflow/promotion.yaml --project promotion
```

### 3.3 Verify Resources Are Loaded

```bash
orchestrator get workspaces --project promotion -o json
orchestrator get agents --project promotion -o json
```

### 3.4 Create the Task (Manual Execution)

```bash
orchestrator task create \
  -n "promotion-weekly" \
  -w promotion -W promotion \
  --project promotion \
  -g "Collect recent project updates, analyze highlights, and generate promotion content drafts for Dev.to/Hashnode/Twitter/LinkedIn/HN"
```

Record the returned `<task_id>`. The task will be immediately claimed by a worker and begin execution.

### 3.5 Enable Scheduled Triggering (Optional)

```bash
# Enable automatic triggering every Monday at 10:00 UTC
orchestrator trigger resume weekly-promotion --project promotion
```

---

## 4. Monitoring Methods

### 4.1 Status Monitoring

```bash
orchestrator task list --project promotion
orchestrator task info <task_id>
orchestrator task trace <task_id>
orchestrator task watch <task_id>
```

Key observations:

1. Whether `gather_updates` successfully collects the git log
2. Whether `analyze_highlights` outputs valid JSON and generates platform items
3. Whether `generate_content` generates content for each platform
4. Whether `save_drafts` writes drafts to the file system
5. Whether `publish` only executes for platforms with `api_publishable=true`
6. Whether `track_results` updates the WorkflowStore

### 4.2 Log Monitoring

```bash
orchestrator task logs --tail 100 <task_id>
orchestrator task logs --tail 200 <task_id>
```

### 4.3 Output Verification

```bash
# Check draft files
ls -la docs/promotion/drafts/

# View draft contents
cat docs/promotion/drafts/*.json | python3 -m json.tool

# Check WorkflowStore
orchestrator store list promotion --project promotion
orchestrator store get promotion last_published_sha --project promotion
```

---

## 5. Key Checkpoints

### 5.1 Gather Updates Checkpoint

Confirm the git log output contains meaningful commit messages:

- [ ] Output is non-empty
- [ ] If `last_published_sha` exists, only new commits are included
- [ ] Output ends with the current HEAD SHA

### 5.2 Analyze Highlights Checkpoint

Confirm the AI analysis results are reasonable:

- [ ] Output is valid JSON
- [ ] `highlights` array has 1-3 entries
- [ ] `platforms` array has 3-5 entries
- [ ] Each platform's `api_publishable` field is correct (true for devto/hashnode)
- [ ] `generate_items` post-action successfully generates items

### 5.3 Generate Content Checkpoint

Confirm content quality:

- [ ] Each platform has correctly formatted content
- [ ] Dev.to/Hashnode content is a full blog post (800+ words)
- [ ] Twitter content is a 3-7 tweet thread
- [ ] HN content is restrained in tone with no marketing language
- [ ] All content includes the project URL

### 5.4 Publish Checkpoint

- [ ] The `publish` step only executes for items with `api_publishable=true`
- [ ] If `DEVTO_API_KEY` is configured, the Dev.to API returns success
- [ ] When no API key is configured, the step degrades gracefully (shows "draft saved")

---

## 6. Success Criteria

The promotion execution is considered complete when all of the following conditions are met:

1. The orchestrator completes the full promotion pipeline and terminates normally at loop_guard.
2. At least 3 platform draft files exist under `docs/promotion/drafts/`.
3. `last_published_sha` in WorkflowStore has been updated to the current HEAD.
4. If `DEVTO_API_KEY` is configured, a draft article is visible in the Dev.to Dashboard.
5. Workflow status is completed with no abnormal failures.

---

## 7. Error Handling

| Error | Detection Method | Resolution |
|-------|-----------------|------------|
| No new changes to promote | `analyze_highlights` returns empty `highlights` | Normal termination, no content generated |
| API key not configured | `publish` step outputs "not set" | Skip publishing; drafts are already saved |
| AI generates invalid JSON | `generate_content` captures fail | Check agent logs, adjust prompt |
| Dev.to API returns 401 | curl output shows Unauthorized | Check whether `DEVTO_API_KEY` is valid |
| Dev.to API returns 422 | curl output shows Unprocessable | Check article format (title required, tag limits, etc.) |
| WorkflowStore write failure | `track_results` reports error | Manually run `orchestrator store put` to backfill |
| Agent produces no output for extended time | `task watch` step times out | Check Claude API status and network connectivity |

---

## 8. Human Role Boundaries

In this plan, the human role is strictly limited to:

1. **One-time setup**: Configure platform API keys.
2. **Launch**: Create a task or enable a cron trigger.
3. **Monitoring**: Observe execution status and output quality.
4. **Review and publish**:
   - Dev.to: Change the draft from unpublished to published in the Dashboard.
   - Hacker News: Read the draft, manually submit a Show HN post.
   - Twitter: Read the tweet thread draft, manually publish.
   - LinkedIn: Read the short-form draft, manually publish.
5. **Error handling**: Interrupt and adjust when content quality is insufficient.

Humans do not pre-write promotion content for the orchestrator and do not prescribe platform selection. Content strategy is determined autonomously by AI after analyzing project changes; humans only review the final output.
