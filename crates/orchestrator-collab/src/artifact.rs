use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Artifact produced by an agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Stable artifact identifier generated at creation time.
    pub id: Uuid,
    /// Semantic category of the artifact payload.
    pub kind: ArtifactKind,
    /// Optional filesystem path associated with the artifact.
    pub path: Option<String>,
    /// Optional structured payload embedded directly in the artifact.
    pub content: Option<serde_json::Value>,
    /// Optional checksum used to deduplicate or verify the artifact body.
    pub checksum: String,
    /// Timestamp when the artifact was created.
    pub created_at: DateTime<Utc>,
}

impl Artifact {
    /// Creates a new artifact with a generated identifier and current timestamp.
    pub fn new(kind: ArtifactKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            path: None,
            content: None,
            checksum: String::new(),
            created_at: Utc::now(),
        }
    }

    /// Attaches a filesystem path to the artifact.
    pub fn with_path(mut self, path: String) -> Self {
        self.path = Some(path);
        self
    }

    /// Attaches structured JSON content to the artifact.
    pub fn with_content(mut self, content: serde_json::Value) -> Self {
        self.content = Some(content);
        self
    }

    /// Records a checksum for the artifact.
    pub fn with_checksum(mut self, checksum: String) -> Self {
        self.checksum = checksum;
        self
    }
}

/// Types of artifacts an agent can produce.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactKind {
    /// A QA or governance ticket raised by an agent.
    Ticket {
        /// Severity assigned to the ticket.
        severity: Severity,
        /// Logical category such as `bug`, `qa`, or `security`.
        category: String,
    },
    /// A code change touching one or more files.
    CodeChange {
        /// Repository-relative files affected by the change.
        files: Vec<String>,
    },
    /// A summarized test execution result.
    TestResult {
        /// Number of passing tests.
        passed: u32,
        /// Number of failing tests.
        failed: u32,
    },
    /// An analytical artifact containing findings.
    Analysis {
        /// Findings captured during analysis.
        findings: Vec<Finding>,
    },
    /// A persisted decision with rationale.
    Decision {
        /// Selected option or action.
        choice: String,
        /// Reasoning that explains the choice.
        rationale: String,
    },
    /// A generic data payload with a declared schema name.
    Data {
        /// Schema or format identifier for the payload.
        schema: String,
    },
    /// A custom artifact type not covered by builtin variants.
    Custom {
        /// User-defined artifact name.
        name: String,
    },
}

/// Severity level used by tickets and findings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Severity {
    /// Requires immediate operator attention.
    Critical,
    /// High-priority issue with significant impact.
    High,
    /// Medium-priority issue that should be addressed soon.
    Medium,
    /// Low-priority issue or minor defect.
    Low,
    /// Informational note without immediate action required.
    Info,
}

/// Structured finding emitted by an analysis artifact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Finding {
    /// Short finding title.
    pub title: String,
    /// Detailed description of the finding.
    pub description: String,
    /// Severity assigned to the finding.
    pub severity: Severity,
    /// Optional source location or file reference.
    pub location: Option<String>,
    /// Optional remediation guidance.
    pub suggestion: Option<String>,
}

/// Registry of artifacts available in the current execution context.
#[derive(Debug, Default)]
pub struct ArtifactRegistry {
    artifacts: HashMap<String, Vec<Artifact>>,
}

impl Clone for ArtifactRegistry {
    fn clone(&self) -> Self {
        Self {
            artifacts: self.artifacts.clone(),
        }
    }
}

impl ArtifactRegistry {
    /// Registers a new artifact under the given phase key.
    pub fn register(&mut self, phase: String, artifact: Artifact) {
        self.artifacts.entry(phase).or_default().push(artifact);
    }

    /// Returns all artifacts recorded for a single phase.
    pub fn get_by_phase(&self, phase: &str) -> Vec<&Artifact> {
        self.artifacts
            .get(phase)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Returns all artifacts whose kind matches the requested variant.
    pub fn get_by_kind(&self, kind: &ArtifactKind) -> Vec<&Artifact> {
        self.artifacts
            .values()
            .flatten()
            .filter(|a| &a.kind == kind)
            .collect()
    }

    /// Returns the most recent artifact recorded for a phase.
    pub fn get_latest(&self, phase: &str) -> Option<&Artifact> {
        self.artifacts.get(phase).and_then(|v| v.last())
    }

    /// Counts all artifacts across all phases.
    pub fn count(&self) -> usize {
        self.artifacts.values().map(|v| v.len()).sum()
    }

    /// Returns a phase-keyed view of the entire registry.
    pub fn all(&self) -> HashMap<String, Vec<&Artifact>> {
        self.artifacts
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().collect()))
            .collect()
    }
}

/// Key-value store for shared state shared between collaborating agents.
#[derive(Debug, Default, Clone)]
pub struct SharedState {
    data: HashMap<String, serde_json::Value>,
}

impl SharedState {
    /// Stores or replaces a value under the provided key.
    pub fn set(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.data.insert(key.into(), value);
    }

    /// Returns a shared reference to a stored value.
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.data.get(key)
    }

    /// Removes a stored value and returns it if present.
    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        self.data.remove(key)
    }

    /// Replaces `{key}` placeholders in the template using stored values.
    pub fn render_template(&self, template: &str) -> String {
        let mut result = template.to_string();
        for (key, value) in &self.data {
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

/// Parses artifact payloads from agent stdout or stderr output.
pub fn parse_artifacts_from_output(output: &str) -> Vec<Artifact> {
    let mut artifacts = Vec::new();

    if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(output) {
        for value in parsed {
            if let Some(kind) = extract_artifact_kind(&value) {
                let mut artifact = Artifact::new(kind);
                if let Some(path) = value.get("path").and_then(|v| v.as_str()) {
                    artifact = artifact.with_path(path.to_string());
                }
                if let Some(content) = value.get("content") {
                    artifact = artifact.with_content(content.clone());
                }
                artifacts.push(artifact);
            }
        }
        return artifacts;
    }

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(output) {
        if let Some(kind) = extract_artifact_kind(&parsed) {
            let mut artifact = Artifact::new(kind);
            if let Some(path) = parsed.get("path").and_then(|v| v.as_str()) {
                artifact = artifact.with_path(path.to_string());
            }
            if let Some(content) = parsed.get("content") {
                artifact = artifact.with_content(content.clone());
            }
            artifacts.push(artifact);
        }

        if artifacts.is_empty() {
            if let Some(arr) = parsed.get("artifacts").and_then(|v| v.as_array()) {
                for value in arr {
                    if let Some(kind) = extract_artifact_kind(value) {
                        let mut artifact = Artifact::new(kind);
                        if let Some(path) = value.get("path").and_then(|v| v.as_str()) {
                            artifact = artifact.with_path(path.to_string());
                        }
                        if let Some(content) = value.get("content") {
                            artifact = artifact.with_content(content.clone());
                        }
                        artifacts.push(artifact);
                    }
                }
            }
        }
    }

    for line in output.lines() {
        if let Some(ticket) = parse_ticket_from_line(line) {
            artifacts.push(ticket);
        }
    }

    artifacts
}

fn extract_artifact_kind(value: &serde_json::Value) -> Option<ArtifactKind> {
    let kind = value.get("kind")?.as_str()?;

    match kind {
        "ticket" => {
            let severity = value
                .get("severity")
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "critical" => Severity::Critical,
                    "high" => Severity::High,
                    "medium" => Severity::Medium,
                    "low" => Severity::Low,
                    _ => Severity::Info,
                })
                .unwrap_or(Severity::Info);

            let category = value
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("general")
                .to_string();

            Some(ArtifactKind::Ticket { severity, category })
        }
        "code_change" => {
            let files = value
                .get("files")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|f| f.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            Some(ArtifactKind::CodeChange { files })
        }
        "test_result" => {
            let passed = value.get("passed").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let failed = value.get("failed").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

            Some(ArtifactKind::TestResult { passed, failed })
        }
        "analysis" => {
            let findings = value
                .get("findings")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|f| {
                            Some(Finding {
                                title: f.get("title")?.as_str()?.to_string(),
                                description: f
                                    .get("description")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                severity: f
                                    .get("severity")
                                    .and_then(|v| v.as_str())
                                    .map(|s| match s {
                                        "critical" => Severity::Critical,
                                        "high" => Severity::High,
                                        "medium" => Severity::Medium,
                                        "low" => Severity::Low,
                                        _ => Severity::Info,
                                    })
                                    .unwrap_or(Severity::Info),
                                location: f
                                    .get("location")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                                suggestion: f
                                    .get("suggestion")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some(ArtifactKind::Analysis { findings })
        }
        "decision" => {
            let choice = value
                .get("choice")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let rationale = value
                .get("rationale")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            Some(ArtifactKind::Decision { choice, rationale })
        }
        _ => None,
    }
}

fn parse_ticket_from_line(line: &str) -> Option<Artifact> {
    if !line.contains("[TICKET:") {
        return None;
    }

    let severity = if line.contains("severity=critical") {
        Severity::Critical
    } else if line.contains("severity=high") {
        Severity::High
    } else if line.contains("severity=medium") {
        Severity::Medium
    } else if line.contains("severity=low") {
        Severity::Low
    } else {
        Severity::Info
    };

    let category = if line.contains("category=bug") {
        "bug".to_string()
    } else if line.contains("category=security") {
        "security".to_string()
    } else if line.contains("category=performance") {
        "performance".to_string()
    } else {
        "general".to_string()
    };

    Some(Artifact::new(ArtifactKind::Ticket { severity, category }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_builder() {
        let artifact = Artifact::new(ArtifactKind::CodeChange {
            files: vec!["main.rs".to_string()],
        })
        .with_path("/tmp/diff.patch".to_string())
        .with_content(serde_json::json!({"lines_added": 10}))
        .with_checksum("abc123".to_string());

        assert_eq!(
            artifact.path.expect("artifact path should be populated"),
            "/tmp/diff.patch"
        );
        assert!(artifact.content.is_some());
        assert_eq!(artifact.checksum, "abc123");
    }

    #[test]
    fn test_artifact_registry() {
        let mut registry = ArtifactRegistry::default();

        let artifact = Artifact::new(ArtifactKind::Ticket {
            severity: Severity::High,
            category: "bug".to_string(),
        });

        registry.register("qa".to_string(), artifact);

        assert_eq!(registry.count(), 1);
        assert!(registry.get_latest("qa").is_some());
    }

    #[test]
    fn test_artifact_registry_get_by_phase() {
        let mut registry = ArtifactRegistry::default();
        registry.register(
            "qa".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "a".to_string(),
            }),
        );
        registry.register(
            "implement".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "b".to_string(),
            }),
        );

        assert_eq!(registry.get_by_phase("qa").len(), 1);
        assert_eq!(registry.get_by_phase("implement").len(), 1);
        assert_eq!(registry.get_by_phase("nonexistent").len(), 0);
    }

    #[test]
    fn test_artifact_registry_get_by_kind() {
        let mut registry = ArtifactRegistry::default();
        let kind = ArtifactKind::TestResult {
            passed: 10,
            failed: 2,
        };
        registry.register("qa".to_string(), Artifact::new(kind.clone()));
        registry.register(
            "qa".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "x".to_string(),
            }),
        );

        let results = registry.get_by_kind(&kind);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_artifact_registry_get_latest() {
        let mut registry = ArtifactRegistry::default();
        assert!(registry.get_latest("qa").is_none());

        registry.register(
            "qa".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "first".to_string(),
            }),
        );
        registry.register(
            "qa".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "second".to_string(),
            }),
        );

        let latest = registry
            .get_latest("qa")
            .expect("latest qa artifact should exist");
        if let ArtifactKind::Custom { name } = &latest.kind {
            assert_eq!(name, "second");
        }
    }

    #[test]
    fn test_artifact_registry_all() {
        let mut registry = ArtifactRegistry::default();
        registry.register(
            "qa".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "a".to_string(),
            }),
        );
        registry.register(
            "plan".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "b".to_string(),
            }),
        );

        let all = registry.all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_shared_state_template() {
        let mut state = SharedState::default();
        state.set("name", serde_json::json!("test"));
        state.set("count", serde_json::json!(42));

        let result = state.render_template("Hello {name}, count is {count}");
        assert_eq!(result, "Hello test, count is 42");
    }

    #[test]
    fn test_shared_state_operations() {
        let mut state = SharedState::default();
        assert!(state.get("key").is_none());

        state.set("key", serde_json::json!("value"));
        assert_eq!(
            state.get("key").expect("shared state key should exist"),
            &serde_json::json!("value")
        );

        let removed = state.remove("key");
        assert!(removed.is_some());
        assert!(state.get("key").is_none());
    }

    #[test]
    fn test_shared_state_render_non_string_json() {
        let mut state = SharedState::default();
        state.set("data", serde_json::json!({"nested": true}));

        let result = state.render_template("result: {data}");
        assert!(result.contains("nested"));
    }

    #[test]
    fn test_parse_artifacts_from_output_json_object() {
        let input = r#"{"kind":"ticket","severity":"high","category":"bug"}"#;
        let artifacts = parse_artifacts_from_output(input);
        assert_eq!(artifacts.len(), 1);
        if let ArtifactKind::Ticket { severity, category } = &artifacts[0].kind {
            assert_eq!(*severity, Severity::High);
            assert_eq!(category, "bug");
        } else {
            assert!(
                matches!(&artifacts[0].kind, ArtifactKind::Ticket { .. }),
                "expected Ticket"
            );
        }
    }

    #[test]
    fn test_parse_artifacts_from_output_nested_artifacts_array() {
        let input = r#"{"confidence":0.4,"quality_score":0.25,"artifacts":[{"kind":"ticket","severity":"high","category":"capability","content":{"title":"qa-from-agent"}}]}"#;
        let artifacts = parse_artifacts_from_output(input);
        assert_eq!(artifacts.len(), 1);
        if let ArtifactKind::Ticket { severity, category } = &artifacts[0].kind {
            assert_eq!(*severity, Severity::High);
            assert_eq!(category, "capability");
        } else {
            assert!(
                matches!(&artifacts[0].kind, ArtifactKind::Ticket { .. }),
                "expected Ticket from nested artifacts array"
            );
        }
    }

    #[test]
    fn test_parse_artifacts_from_output_json_array() {
        let input = r#"[{"kind":"test_result","passed":5,"failed":1},{"kind":"code_change","files":["a.rs"]}]"#;
        let artifacts = parse_artifacts_from_output(input);
        assert_eq!(artifacts.len(), 2);
    }

    #[test]
    fn test_parse_artifacts_from_output_ticket_marker() {
        let input = "some output\n[TICKET: severity=high, category=bug]\nmore output";
        let artifacts = parse_artifacts_from_output(input);
        assert_eq!(artifacts.len(), 1);
        if let ArtifactKind::Ticket { severity, category } = &artifacts[0].kind {
            assert_eq!(*severity, Severity::High);
            assert_eq!(category, "bug");
        }
    }

    #[test]
    fn test_parse_artifacts_from_output_ticket_severity_levels() {
        let levels = [
            ("severity=critical", Severity::Critical),
            ("severity=medium", Severity::Medium),
            ("severity=low", Severity::Low),
            ("severity=unknown", Severity::Info),
        ];
        for (marker, expected) in levels {
            let input = format!("[TICKET: {}, category=bug]", marker);
            let artifacts = parse_artifacts_from_output(&input);
            assert_eq!(artifacts.len(), 1);
            if let ArtifactKind::Ticket { severity, .. } = &artifacts[0].kind {
                assert_eq!(*severity, expected, "failed for marker: {}", marker);
            }
        }
    }

    #[test]
    fn test_parse_artifacts_from_output_ticket_categories() {
        let categories = [
            ("category=security", "security"),
            ("category=performance", "performance"),
            ("category=other", "general"),
        ];
        for (marker, expected) in categories {
            let input = format!("[TICKET: severity=high, {}]", marker);
            let artifacts = parse_artifacts_from_output(&input);
            if let ArtifactKind::Ticket { category, .. } = &artifacts[0].kind {
                assert_eq!(category, expected);
            }
        }
    }

    #[test]
    fn test_parse_artifacts_from_output_no_artifacts() {
        let artifacts = parse_artifacts_from_output("plain text with no markers");
        assert!(artifacts.is_empty());
    }

    #[test]
    fn test_extract_artifact_kind_decision() {
        let value = serde_json::json!({
            "kind": "decision",
            "choice": "option_a",
            "rationale": "better performance"
        });
        let kind = extract_artifact_kind(&value).expect("decision artifact should parse");
        if let ArtifactKind::Decision { choice, rationale } = &kind {
            assert_eq!(choice, "option_a");
            assert_eq!(rationale, "better performance");
        } else {
            assert!(
                matches!(&kind, ArtifactKind::Decision { .. }),
                "expected Decision"
            );
        }
    }

    #[test]
    fn test_extract_artifact_kind_analysis() {
        let value = serde_json::json!({
            "kind": "analysis",
            "findings": [
                {"title": "Issue 1", "severity": "high", "description": "desc"}
            ]
        });
        let kind = extract_artifact_kind(&value).expect("analysis artifact should parse");
        if let ArtifactKind::Analysis { findings } = kind {
            assert_eq!(findings.len(), 1);
            assert_eq!(findings[0].title, "Issue 1");
            assert_eq!(findings[0].severity, Severity::High);
        }
    }

    #[test]
    fn test_extract_artifact_kind_unknown_returns_none() {
        let value = serde_json::json!({"kind": "unknown_type"});
        assert!(extract_artifact_kind(&value).is_none());
    }

    #[test]
    fn test_extract_artifact_kind_missing_kind_returns_none() {
        let value = serde_json::json!({"data": "value"});
        assert!(extract_artifact_kind(&value).is_none());
    }
}
