use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

/// Unified template engine trait for all template rendering
/// This provides a consistent interface for rendering templates with various placeholders
pub trait TemplateEngine {
    /// Render a template with the given context
    fn render(&self, template: &str) -> String;
}

/// Basic template context for simple replacements
pub struct BasicTemplateContext {
    pub rel_path: Option<String>,
    pub ticket_paths: Option<Vec<String>>,
    pub phase: Option<String>,
    pub task_id: Option<String>,
    pub cycle: Option<u32>,
    pub unresolved_items: Option<i64>,
}

impl BasicTemplateContext {
    pub fn new() -> Self {
        Self {
            rel_path: None,
            ticket_paths: None,
            phase: None,
            task_id: None,
            cycle: None,
            unresolved_items: None,
        }
    }

    pub fn with_rel_path(mut self, path: impl Into<String>) -> Self {
        self.rel_path = Some(path.into());
        self
    }

    pub fn with_ticket_paths(mut self, paths: Vec<String>) -> Self {
        self.ticket_paths = Some(paths);
        self
    }

    pub fn with_phase(mut self, phase: impl Into<String>) -> Self {
        self.phase = Some(phase.into());
        self
    }

    pub fn with_task_id(mut self, id: impl Into<String>) -> Self {
        self.task_id = Some(id.into());
        self
    }

    pub fn with_cycle(mut self, cycle: u32) -> Self {
        self.cycle = Some(cycle);
        self
    }

    pub fn with_unresolved_items(mut self, count: i64) -> Self {
        self.unresolved_items = Some(count);
        self
    }
}

impl Default for BasicTemplateContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateEngine for BasicTemplateContext {
    fn render(&self, template: &str) -> String {
        let mut result = template.to_string();

        if let Some(ref rel_path) = self.rel_path {
            result = result.replace("{rel_path}", rel_path);
        }
        if let Some(ref ticket_paths) = self.ticket_paths {
            result = result.replace("{ticket_paths}", &ticket_paths.join(" "));
        }
        if let Some(ref phase) = self.phase {
            result = result.replace("{phase}", phase);
        }
        if let Some(ref task_id) = self.task_id {
            result = result.replace("{task_id}", task_id);
        }
        if let Some(cycle) = self.cycle {
            result = result.replace("{cycle}", &cycle.to_string());
        }
        if let Some(unresolved) = self.unresolved_items {
            result = result.replace("{unresolved_items}", &unresolved.to_string());
        }

        result
    }
}

/// Advanced template context with upstream outputs and shared state
pub struct AdvancedTemplateContext {
    basic: BasicTemplateContext,
    pub upstream_outputs: Vec<serde_json::Value>,
    pub shared_state: HashMap<String, serde_json::Value>,
}

impl AdvancedTemplateContext {
    pub fn new() -> Self {
        Self {
            basic: BasicTemplateContext::new(),
            upstream_outputs: Vec::new(),
            shared_state: HashMap::new(),
        }
    }

    pub fn with_basic(mut self, basic: BasicTemplateContext) -> Self {
        self.basic = basic;
        self
    }

    pub fn with_upstream_outputs(mut self, outputs: Vec<serde_json::Value>) -> Self {
        self.upstream_outputs = outputs;
        self
    }

    pub fn with_shared_state(mut self, state: HashMap<String, serde_json::Value>) -> Self {
        self.shared_state = state;
        self
    }
}

impl Default for AdvancedTemplateContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateEngine for AdvancedTemplateContext {
    fn render(&self, template: &str) -> String {
        // First apply basic context replacements
        let mut result = self.basic.render(template);

        // Upstream outputs - collect all replacements first
        let mut replacements: Vec<(String, String)> = Vec::new();
        for (i, output) in self.upstream_outputs.iter().enumerate() {
            let prefix = format!("upstream[{}]", i);
            if let Some(v) = output.get("exit_code").and_then(|v| v.as_i64()) {
                replacements.push((format!("{}.exit_code", prefix), v.to_string()));
            }
            if let Some(v) = output.get("confidence").and_then(|v| v.as_f64()) {
                replacements.push((format!("{}.confidence", prefix), v.to_string()));
            }
            if let Some(v) = output.get("quality_score").and_then(|v| v.as_f64()) {
                replacements.push((format!("{}.quality_score", prefix), v.to_string()));
            }
        }

        // Sort by length descending to replace longer patterns first
        replacements.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        for (key, value) in replacements {
            result = result.replace(&format!("{{{}}}", key), &value);
        }

        // Shared state
        for (key, value) in &self.shared_state {
            let placeholder = format!("{{{}}}", key);
            if let Some(s) = value.as_str() {
                result = result.replace(&placeholder, s);
            } else if let Ok(s) = serde_json::to_string(value) {
                result = result.replace(&placeholder, &s);
            }
        }

        result
    }
}

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

    #[test]
    fn basic_template_context_render() {
        let ctx = BasicTemplateContext::new()
            .with_rel_path("docs/qa/test.md")
            .with_ticket_paths(vec!["ticket1.md".to_string()]);

        let result = ctx.render("qa {rel_path} --tickets {ticket_paths}");
        assert_eq!(result, "qa docs/qa/test.md --tickets ticket1.md");
    }

    #[test]
    fn basic_template_context_all_fields() {
        let ctx = BasicTemplateContext::new()
            .with_rel_path("test.md")
            .with_phase("qa")
            .with_task_id("task-123")
            .with_cycle(5)
            .with_unresolved_items(3);

        let result = ctx.render("{rel_path} {phase} {task_id} c{cycle} u{unresolved_items}");
        assert_eq!(result, "test.md qa task-123 c5 u3");
    }

    #[test]
    fn advanced_template_context_with_upstream() {
        let mut shared = HashMap::new();
        shared.insert("key".to_string(), serde_json::json!("value"));

        let upstream = vec![serde_json::json!({"exit_code": 0, "confidence": 0.9})];

        let ctx = AdvancedTemplateContext::new()
            .with_basic(BasicTemplateContext::new().with_rel_path("test.md"))
            .with_upstream_outputs(upstream)
            .with_shared_state(shared);

        let result = ctx.render(
            "{rel_path} exit:{upstream[0].exit_code} conf:{upstream[0].confidence} key:{key}",
        );
        assert_eq!(result, "test.md exit:0 conf:0.9 key:value");
    }

    #[test]
    fn advanced_template_context_with_json_value() {
        let mut shared = HashMap::new();
        shared.insert("data".to_string(), serde_json::json!({"foo": "bar"}));

        let ctx = AdvancedTemplateContext::new().with_shared_state(shared);

        let result = ctx.render("data: {data}");
        assert!(result.contains("foo"));
        assert!(result.contains("bar"));
    }
}
