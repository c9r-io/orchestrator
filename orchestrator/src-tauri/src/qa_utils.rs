use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

pub fn validate_workspace_rel_path(raw: &str, field: &str) -> Result<()> {
    let path = raw.trim();
    if path.is_empty() {
        anyhow::bail!("{} cannot be empty", field);
    }

    let parsed = Path::new(path);
    if parsed.is_absolute() {
        anyhow::bail!("{} must be a relative path: {}", field, raw);
    }

    if parsed
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        anyhow::bail!("{} cannot include '..': {}", field, raw);
    }

    Ok(())
}

pub fn new_ticket_diff(before: &[String], after: &[String]) -> Vec<String> {
    let before_set: HashSet<&String> = before.iter().collect();
    after
        .iter()
        .filter(|path| !before_set.contains(path))
        .cloned()
        .collect()
}

pub fn render_template(template: &str, rel_path: &str, ticket_paths: &[String]) -> String {
    template
        .replace("{rel_path}", rel_path)
        .replace("{ticket_paths}", &ticket_paths.join(" "))
}

pub fn render_template_with_context(
    template: &str,
    rel_path: &str,
    ticket_paths: &[String],
    phase: &str,
    upstream_outputs: &[serde_json::Value],
    shared_state: &HashMap<String, serde_json::Value>,
) -> String {
    let mut result = template.to_string();

    // Basic placeholders
    result = result.replace("{rel_path}", rel_path);
    result = result.replace("{ticket_paths}", &ticket_paths.join(" "));
    result = result.replace("{phase}", phase);

    // Upstream outputs (JSON serialized)
    for (i, output) in upstream_outputs.iter().enumerate() {
        let prefix = format!("upstream[{}]", i);
        if let Some(v) = output.get("exit_code").and_then(|v| v.as_i64()) {
            result = result.replace(&format!("{}.exit_code", prefix), &v.to_string());
        }
        if let Some(v) = output.get("confidence").and_then(|v| v.as_f64()) {
            result = result.replace(&format!("{}.confidence", prefix), &v.to_string());
        }
    }

    // Shared state
    for (key, value) in shared_state {
        let placeholder = format!("{{{}}}", key);
        if let Some(s) = value.as_str() {
            result = result.replace(&placeholder, s);
        } else if let Ok(s) = serde_json::to_string(value) {
            result = result.replace(&placeholder, &s);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_workspace_rel_path_accepts_normal_relative_paths() {
        assert!(validate_workspace_rel_path("docs/qa", "field").is_ok());
        assert!(validate_workspace_rel_path("config/default.yaml", "field").is_ok());
        assert!(validate_workspace_rel_path("a-b_c/1", "field").is_ok());
    }

    #[test]
    fn validate_workspace_rel_path_rejects_empty_input() {
        assert!(validate_workspace_rel_path("", "f").is_err());
        assert!(validate_workspace_rel_path("   ", "f").is_err());
    }

    #[test]
    fn validate_workspace_rel_path_rejects_absolute_path() {
        assert!(validate_workspace_rel_path("/tmp/data", "f").is_err());
    }

    #[test]
    fn validate_workspace_rel_path_rejects_parent_segments() {
        assert!(validate_workspace_rel_path("../docs", "f").is_err());
        assert!(validate_workspace_rel_path("docs/../../x", "f").is_err());
    }

    #[test]
    fn render_template_replaces_placeholders() {
        let template = "run {rel_path} --tickets {ticket_paths}";
        let tickets = vec!["a.md".to_string(), "b.md".to_string()];
        let rendered = render_template(template, "docs/qa/1.md", &tickets);
        assert_eq!(rendered, "run docs/qa/1.md --tickets a.md b.md");
    }

    #[test]
    fn render_template_handles_empty_ticket_paths() {
        let rendered = render_template("{rel_path}:{ticket_paths}", "x.md", &[]);
        assert_eq!(rendered, "x.md:");
    }

    #[test]
    fn new_ticket_diff_returns_only_new_items_with_original_order() {
        let before = vec!["a".to_string(), "b".to_string()];
        let after = vec!["b".to_string(), "c".to_string(), "d".to_string()];
        let diff = new_ticket_diff(&before, &after);
        assert_eq!(diff, vec!["c".to_string(), "d".to_string()]);
    }

    #[test]
    fn new_ticket_diff_returns_empty_when_no_new_items() {
        let before = vec!["a".to_string(), "b".to_string()];
        let after = vec!["a".to_string(), "b".to_string()];
        let diff = new_ticket_diff(&before, &after);
        assert!(diff.is_empty());
    }

    #[test]
    fn new_ticket_diff_keeps_duplicates_if_after_has_duplicates() {
        let before = vec!["a".to_string()];
        let after = vec!["b".to_string(), "b".to_string()];
        let diff = new_ticket_diff(&before, &after);
        assert_eq!(diff, vec!["b".to_string(), "b".to_string()]);
    }
}
