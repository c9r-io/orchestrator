use crate::config::TaskRuntimeContext;
use crate::config_load::resolve_workspace_path;
use crate::dto::{TicketPreviewData, UNASSIGNED_QA_FILE_PATH};
use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use walkdir::WalkDir;

pub fn normalize_rel_path_for_match(raw: &str) -> String {
    let value = raw.trim().trim_matches('`').replace('\\', "/");
    if value.is_empty() {
        return String::new();
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in value.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return String::new();
        }
        parts.push(part);
    }
    parts.join("/")
}

pub fn is_active_ticket_status(status: &str) -> bool {
    let normalized = status.trim().to_ascii_uppercase();
    normalized.is_empty() || matches!(normalized.as_str(), "FAILED" | "OPEN")
}

pub fn parse_ticket_preview_content(content: &str) -> TicketPreviewData {
    let mut status = String::new();
    let mut qa_doc = String::new();
    for line in content.lines().take(80) {
        if line.starts_with("**Status**:") {
            status = line.trim_start_matches("**Status**:").trim().to_string();
        } else if line.starts_with("**QA Document**:") {
            qa_doc = line
                .trim_start_matches("**QA Document**:")
                .trim()
                .trim_matches('`')
                .to_string();
        }
    }
    TicketPreviewData {
        status,
        qa_document: qa_doc,
    }
}

pub fn read_ticket_preview_from_workspace(
    workspace_root: &Path,
    rel_path: &str,
) -> TicketPreviewData {
    let abs = match resolve_workspace_path(workspace_root, rel_path, "ticket preview path") {
        Ok(value) => value,
        Err(_) => {
            return TicketPreviewData {
                status: String::new(),
                qa_document: String::new(),
            };
        }
    };
    let content = std::fs::read_to_string(abs).unwrap_or_default();
    parse_ticket_preview_content(&content)
}

pub fn list_ticket_files_in_workspace(
    workspace_root: &Path,
    ticket_dir: &str,
) -> Result<Vec<String>> {
    let ticket_dir = resolve_workspace_path(workspace_root, ticket_dir, "task.ticket_dir")?;
    if !ticket_dir.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in WalkDir::new(ticket_dir)
        .min_depth(1)
        .into_iter()
        .filter_map(|value| value.ok())
    {
        if !entry.path().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case("README.md")
        {
            continue;
        }
        let rel = pathdiff::diff_paths(entry.path(), workspace_root)
            .unwrap_or_else(|| entry.path().to_path_buf())
            .to_string_lossy()
            .to_string();
        result.push(rel);
    }
    result.sort();
    Ok(result)
}

pub fn list_ticket_files(task_ctx: &TaskRuntimeContext) -> Result<Vec<String>> {
    list_ticket_files_in_workspace(&task_ctx.workspace_root, &task_ctx.ticket_dir)
}

pub fn list_existing_tickets_for_item(
    task_ctx: &TaskRuntimeContext,
    qa_file_path: &str,
) -> Result<Vec<String>> {
    let normalized_target = normalize_rel_path_for_match(qa_file_path);
    let mut matched = Vec::new();
    for ticket in list_ticket_files(task_ctx)? {
        let preview = read_ticket_preview_from_workspace(&task_ctx.workspace_root, &ticket);
        if !is_active_ticket_status(&preview.status) {
            continue;
        }
        let normalized_doc = normalize_rel_path_for_match(&preview.qa_document);
        if qa_file_path == UNASSIGNED_QA_FILE_PATH {
            if normalized_doc.is_empty() {
                matched.push(ticket);
            }
            continue;
        }
        if normalized_doc == normalized_target {
            matched.push(ticket);
        }
    }
    matched.sort();
    Ok(matched)
}

pub fn scan_active_tickets_for_task_items(
    task_ctx: &TaskRuntimeContext,
    task_item_paths: &[String],
) -> Result<HashMap<String, Vec<String>>> {
    let mut item_path_by_normalized: HashMap<String, String> = HashMap::new();
    for path in task_item_paths {
        let normalized = normalize_rel_path_for_match(path);
        if !normalized.is_empty() {
            item_path_by_normalized.insert(normalized, path.clone());
        }
    }

    let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
    for ticket in list_ticket_files(task_ctx)? {
        let preview = read_ticket_preview_from_workspace(&task_ctx.workspace_root, &ticket);
        if !is_active_ticket_status(&preview.status) {
            continue;
        }
        let normalized_doc = normalize_rel_path_for_match(&preview.qa_document);
        let bucket = if normalized_doc.is_empty() {
            UNASSIGNED_QA_FILE_PATH.to_string()
        } else {
            item_path_by_normalized
                .get(&normalized_doc)
                .cloned()
                .unwrap_or_else(|| UNASSIGNED_QA_FILE_PATH.to_string())
        };
        grouped.entry(bucket).or_default().push(ticket);
    }
    for paths in grouped.values_mut() {
        paths.sort();
        paths.dedup();
    }
    Ok(grouped)
}

pub fn read_ticket_preview(task_ctx: &TaskRuntimeContext, rel_path: &str) -> serde_json::Value {
    let preview = read_ticket_preview_from_workspace(&task_ctx.workspace_root, rel_path);
    json!({
        "path": rel_path,
        "status": preview.status,
        "qa_document": preview.qa_document
    })
}

pub fn collect_target_files(
    workspace_root: &Path,
    qa_targets: &[String],
    input: Option<Vec<String>>,
) -> Result<Vec<String>> {
    if let Some(list) = input {
        let mut result = Vec::new();
        for entry in list {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                continue;
            }
            let abs = resolve_workspace_path(workspace_root, trimmed, "target_files")?;
            if abs.is_file() {
                result.push(trimmed.to_string());
            }
        }
        result.sort();
        result.dedup();
        return Ok(result);
    }

    let mut files = Vec::new();
    for target in qa_targets {
        let base = resolve_workspace_path(workspace_root, target, "qa_targets")?;
        if !base.exists() {
            continue;
        }
        for entry in WalkDir::new(base)
            .into_iter()
            .filter_map(|value| value.ok())
        {
            if !entry.path().is_file() {
                continue;
            }
            if entry.path().extension().and_then(|v| v.to_str()) != Some("md") {
                continue;
            }
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("README.md")
            {
                continue;
            }
            let rel = pathdiff::diff_paths(entry.path(), workspace_root)
                .unwrap_or_else(|| entry.path().to_path_buf())
                .to_string_lossy()
                .to_string();
            files.push(rel);
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

pub fn create_ticket_for_qa_failure(
    workspace_root: &Path,
    ticket_dir: &str,
    task_name: &str,
    qa_file_path: &str,
    exit_code: i64,
    stdout_path: &str,
    stderr_path: &str,
) -> Result<Option<String>> {
    let abs_ticket_dir = resolve_workspace_path(workspace_root, ticket_dir, "ticket_dir")?;
    if !abs_ticket_dir.exists() {
        std::fs::create_dir_all(&abs_ticket_dir)?;
    }

    let now = Utc::now();
    let timestamp = now.format("%y%m%d_%H%M%S").to_string();

    let qa_stem = Path::new(qa_file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let filename = format!("auto_{qa_stem}_{timestamp}.md");
    let ticket_path = abs_ticket_dir.join(&filename);

    if ticket_path.exists() {
        return Ok(None);
    }

    let stdout_snippet = std::fs::read_to_string(stdout_path)
        .unwrap_or_default()
        .lines()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");

    let stderr_snippet = std::fs::read_to_string(stderr_path)
        .unwrap_or_default()
        .lines()
        .rev()
        .take(10)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");

    let created = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let content = format!(
        r#"# Ticket: QA Failure - {qa_stem}

**Created**: {created}
**QA Document**: `{qa_file_path}`
**Status**: FAILED

---

## Test Content
Automated ticket created by orchestrator when QA phase failed for task "{task_name}".

---

## Expected Result
QA phase exits with code 0 (success).

---

## Actual Result
QA phase exited with code {exit_code}.

---

## Evidence

**stdout** (last 20 lines):
```text
{stdout_snippet}
```

**stderr** (last 10 lines):
```text
{stderr_snippet}
```

---

## Analysis

**Root Cause**: Auto-generated ticket; investigate QA output above.
**Severity**: Medium
**Related Components**: Backend
"#
    );

    std::fs::write(&ticket_path, content)?;

    let rel = pathdiff::diff_paths(&ticket_path, workspace_root)
        .unwrap_or_else(|| ticket_path.clone())
        .to_string_lossy()
        .to_string();
    Ok(Some(rel))
}

pub fn collect_target_files_from_active_tickets(
    workspace_root: &Path,
    ticket_dir: &str,
) -> Result<Vec<String>> {
    let ticket_files = list_ticket_files_in_workspace(workspace_root, ticket_dir)?;
    let mut targets: HashSet<String> = HashSet::new();
    let mut include_unassigned = false;

    for ticket in ticket_files {
        let preview = read_ticket_preview_from_workspace(workspace_root, &ticket);
        if !is_active_ticket_status(&preview.status) {
            continue;
        }
        let normalized_doc = normalize_rel_path_for_match(&preview.qa_document);
        if normalized_doc.is_empty() {
            include_unassigned = true;
            continue;
        }
        let qa_abs = resolve_workspace_path(workspace_root, &normalized_doc, "ticket qa_document");
        if qa_abs.map(|path| path.is_file()).unwrap_or(false) {
            targets.insert(normalized_doc);
        } else {
            include_unassigned = true;
        }
    }

    let mut result: Vec<String> = targets.into_iter().collect();
    result.sort();
    if include_unassigned {
        result.push(UNASSIGNED_QA_FILE_PATH.to_string());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_rel_path_for_match_trims_whitespace() {
        let result = normalize_rel_path_for_match("  foo/bar  ");
        assert_eq!(result, "foo/bar");
    }

    #[test]
    fn test_normalize_rel_path_for_match_removes_backslashes() {
        let result = normalize_rel_path_for_match("foo\\bar\\baz");
        assert_eq!(result, "foo/bar/baz");
    }

    #[test]
    fn test_normalize_rel_path_for_match_filters_empty_parts() {
        let result = normalize_rel_path_for_match("foo//bar");
        assert_eq!(result, "foo/bar");
    }

    #[test]
    fn test_normalize_rel_path_for_match_filters_dot() {
        let result = normalize_rel_path_for_match("foo/./bar");
        assert_eq!(result, "foo/bar");
    }

    #[test]
    fn test_normalize_rel_path_for_match_rejects_parent_traversal() {
        let result = normalize_rel_path_for_match("foo/../bar");
        assert!(result.is_empty());
    }

    #[test]
    fn test_is_active_ticket_status_failed() {
        assert!(is_active_ticket_status("FAILED"));
        assert!(is_active_ticket_status("failed"));
        assert!(is_active_ticket_status("FAILED "));
    }

    #[test]
    fn test_is_active_ticket_status_open() {
        assert!(is_active_ticket_status("OPEN"));
        assert!(is_active_ticket_status("open"));
    }

    #[test]
    fn test_is_active_ticket_status_empty() {
        assert!(is_active_ticket_status(""));
        assert!(is_active_ticket_status("   "));
    }

    #[test]
    fn test_is_active_ticket_status_closed() {
        assert!(!is_active_ticket_status("PASSED"));
        assert!(!is_active_ticket_status("FIXED"));
    }

    #[test]
    fn test_parse_ticket_preview_content_extracts_title() {
        let content = r#"# Ticket: Test Issue

**Status**: FAILED
**QA Document**: `docs/qa/test.md`
"#;
        let result = parse_ticket_preview_content(content);
        assert_eq!(result.status, "FAILED");
        assert_eq!(result.qa_document, "docs/qa/test.md");
    }

    #[test]
    fn test_parse_ticket_preview_content_handles_empty() {
        let result = parse_ticket_preview_content("no content here");
        assert!(result.status.is_empty());
    }

    #[test]
    fn test_parse_ticket_preview_content_extracts_status_and_qa_doc() {
        let content = "**Status**: OPEN\n**QA Document**: `docs/qa/auth.md`\n";
        let result = parse_ticket_preview_content(content);
        assert_eq!(result.status, "OPEN");
        assert_eq!(result.qa_document, "docs/qa/auth.md");
    }

    #[test]
    fn test_parse_ticket_preview_content_only_status() {
        let content = "# Title\n**Status**: FAILED\nSome details";
        let result = parse_ticket_preview_content(content);
        assert_eq!(result.status, "FAILED");
        assert!(result.qa_document.is_empty());
    }

    #[test]
    fn test_normalize_rel_path_for_match_backtick_wrapping() {
        let result = normalize_rel_path_for_match("`docs/qa/test.md`");
        assert_eq!(result, "docs/qa/test.md");
    }

    #[test]
    fn test_normalize_rel_path_for_match_empty() {
        let result = normalize_rel_path_for_match("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_normalize_rel_path_for_match_whitespace_only() {
        let result = normalize_rel_path_for_match("   ");
        assert!(result.is_empty());
    }

    #[test]
    fn test_is_active_ticket_status_various_closed() {
        assert!(!is_active_ticket_status("CLOSED"));
        assert!(!is_active_ticket_status("RESOLVED"));
        assert!(!is_active_ticket_status("SKIPPED"));
        assert!(!is_active_ticket_status("PASSED"));
    }

    #[test]
    fn test_list_ticket_files_in_workspace() {
        let dir = std::env::temp_dir().join(format!("ticket-test-{}", uuid::Uuid::new_v4()));
        let ticket_dir = dir.join("docs/ticket");
        std::fs::create_dir_all(&ticket_dir).expect("create ticket dir");

        // Create ticket files
        std::fs::write(ticket_dir.join("auto_test_001.md"), "# Ticket").expect("write ticket 1");
        std::fs::write(ticket_dir.join("auto_test_002.md"), "# Ticket 2")
            .expect("write ticket 2");
        // README should be excluded
        std::fs::write(ticket_dir.join("README.md"), "# Readme").expect("write readme");
        // Non-md files should be excluded
        std::fs::write(ticket_dir.join("notes.txt"), "notes").expect("write notes");

        let result = list_ticket_files_in_workspace(&dir, "docs/ticket")
            .expect("list ticket files in workspace");
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|p| p.ends_with(".md")));
        assert!(!result.iter().any(|p| p.contains("README")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_ticket_files_in_workspace_missing_dir() {
        let dir = std::env::temp_dir().join(format!("ticket-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create workspace dir");

        let result = list_ticket_files_in_workspace(&dir, "docs/ticket")
            .expect("list ticket files in missing dir");
        assert!(result.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_create_ticket_for_qa_failure() {
        let dir = std::env::temp_dir().join(format!("ticket-create-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let stdout_path = dir.join("stdout.log");
        let stderr_path = dir.join("stderr.log");
        std::fs::write(&stdout_path, "test output line 1\ntest output line 2")
            .expect("write stdout log");
        std::fs::write(&stderr_path, "error detail").expect("write stderr log");

        let result = create_ticket_for_qa_failure(
            &dir,
            "docs/ticket",
            "test-task",
            "docs/qa/auth.md",
            1,
            stdout_path.to_str().expect("stdout path should be utf-8"),
            stderr_path.to_str().expect("stderr path should be utf-8"),
        )
        .expect("create ticket for qa failure");

        assert!(result.is_some());
        let ticket_path = result.expect("ticket path should be returned");
        assert!(ticket_path.starts_with("docs/ticket/auto_auth_"));
        assert!(ticket_path.ends_with(".md"));

        // Verify content
        let abs_path = dir.join(&ticket_path);
        let content = std::fs::read_to_string(&abs_path).expect("read generated ticket");
        assert!(content.contains("**Status**: FAILED"));
        assert!(content.contains("**QA Document**: `docs/qa/auth.md`"));
        assert!(content.contains("exited with code 1"));
        assert!(content.contains("test output line"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_collect_target_files_with_explicit_list() {
        let dir = std::env::temp_dir().join(format!("target-files-{}", uuid::Uuid::new_v4()));
        let qa_dir = dir.join("docs/qa");
        std::fs::create_dir_all(&qa_dir).expect("create qa dir");

        std::fs::write(qa_dir.join("test1.md"), "# Test 1").expect("write test1");
        std::fs::write(qa_dir.join("test2.md"), "# Test 2").expect("write test2");

        let input = vec![
            "docs/qa/test1.md".to_string(),
            "docs/qa/test2.md".to_string(),
            "docs/qa/nonexistent.md".to_string(), // should be filtered
            "".to_string(),                       // should be filtered
        ];

        let result =
            collect_target_files(&dir, &[], Some(input)).expect("collect explicit target files");
        assert_eq!(result.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_collect_target_files_from_directory_scan() {
        let dir = std::env::temp_dir().join(format!("target-scan-{}", uuid::Uuid::new_v4()));
        let qa_dir = dir.join("docs/qa");
        std::fs::create_dir_all(&qa_dir).expect("create qa dir");

        std::fs::write(qa_dir.join("auth.md"), "# Auth QA").expect("write auth qa");
        std::fs::write(qa_dir.join("README.md"), "# README").expect("write readme");
        std::fs::write(qa_dir.join("data.json"), "{}").expect("write data json");

        let result = collect_target_files(&dir, &["docs/qa".to_string()], None)
            .expect("collect scanned target files");
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("auth.md"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_ticket_preview_from_workspace() {
        let dir = std::env::temp_dir().join(format!("preview-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create preview dir");
        std::fs::write(
            dir.join("ticket.md"),
            "**Status**: FAILED\n**QA Document**: `docs/qa/test.md`\n",
        )
        .expect("write ticket preview file");

        let preview = read_ticket_preview_from_workspace(&dir, "ticket.md");
        assert_eq!(preview.status, "FAILED");
        assert_eq!(preview.qa_document, "docs/qa/test.md");

        // Non-existent file returns empty preview
        let preview = read_ticket_preview_from_workspace(&dir, "nonexistent.md");
        assert!(preview.status.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_ticket_preview() {
        use crate::config::TaskRuntimeContext;

        let dir = std::env::temp_dir().join(format!("read-preview-{}", uuid::Uuid::new_v4()));
        let ticket_dir = dir.join("docs/ticket");
        std::fs::create_dir_all(&ticket_dir).expect("create nested ticket dir");
        std::fs::write(
            ticket_dir.join("t1.md"),
            "**Status**: OPEN\n**QA Document**: `docs/qa/a.md`\n",
        )
        .expect("write nested ticket");

        let task_ctx = TaskRuntimeContext {
            workspace_id: "ws".to_string(),
            workspace_root: dir.clone(),
            ticket_dir: "docs/ticket".to_string(),
            execution_plan: crate::config::TaskExecutionPlan {
                steps: vec![],
                loop_policy: crate::config::WorkflowLoopConfig::default(),
                finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
            },
            current_cycle: 0,
            init_done: false,
            dynamic_steps: vec![],
            pipeline_vars: crate::config::PipelineVariables::default(),
            safety: crate::config::SafetyConfig::default(),
            self_referential: false,
            consecutive_failures: 0,
        };

        let result = read_ticket_preview(&task_ctx, "docs/ticket/t1.md");
        assert_eq!(result["status"], "OPEN");
        assert_eq!(result["qa_document"], "docs/qa/a.md");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
