use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::artifact::{ArtifactRegistry, SharedState};
use super::escape_for_bash_dquote;
use super::output::AgentOutput;

/// Lightweight reference to agent context for serialized message payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContextRef {
    /// Parent task identifier.
    pub task_id: String,
    /// Current task-item identifier.
    pub item_id: String,
    /// Execution cycle number.
    pub cycle: u32,
    /// Optional phase name when known.
    pub phase: Option<String>,
    /// Workspace root serialized as a string path.
    pub workspace_root: String,
    /// Workspace identifier from configuration.
    pub workspace_id: String,
}

/// Full agent context available during phase execution.
#[derive(Debug, Clone)]
pub struct AgentContext {
    /// Parent task identifier.
    pub task_id: String,
    /// Current task-item identifier.
    pub item_id: String,
    /// Execution cycle number.
    pub cycle: u32,
    /// Current phase name.
    pub phase: String,
    /// Absolute workspace root used for command execution.
    pub workspace_root: PathBuf,
    /// Workspace identifier from configuration.
    pub workspace_id: String,
    /// Historical phase executions accumulated so far.
    pub execution_history: Vec<PhaseRecord>,
    /// Outputs produced by upstream phases.
    pub upstream_outputs: Vec<AgentOutput>,
    /// Artifact registry accumulated across phases.
    pub artifacts: ArtifactRegistry,
    /// Shared key-value state available to templates and follow-up steps.
    pub shared_state: SharedState,
}

impl AgentContext {
    /// Creates a fresh execution context for an agent phase.
    pub fn new(
        task_id: String,
        item_id: String,
        cycle: u32,
        phase: String,
        workspace_root: PathBuf,
        workspace_id: String,
    ) -> Self {
        Self {
            task_id,
            item_id,
            cycle,
            phase,
            workspace_root,
            workspace_id,
            execution_history: Vec::new(),
            upstream_outputs: Vec::new(),
            artifacts: ArtifactRegistry::default(),
            shared_state: SharedState::default(),
        }
    }

    /// Adds an upstream output and merges its artifacts into the registry.
    pub fn add_upstream_output(&mut self, output: AgentOutput) {
        self.upstream_outputs.push(output.clone());

        for artifact in output.artifacts {
            self.artifacts.register(self.phase.clone(), artifact);
        }
    }

    /// Renders a template using context variables only.
    ///
    /// Note: Pipeline variable values are escaped for safe use inside
    /// bash double-quoted strings. This prevents content like markdown
    /// backticks from triggering shell command substitution.
    pub fn render_template(&self, template: &str) -> String {
        self.render_template_with_pipeline(template, None)
    }

    /// Renders a template using context variables and optional pipeline values.
    pub fn render_template_with_pipeline(
        &self,
        template: &str,
        pipeline: Option<&orchestrator_config::config::PipelineVariables>,
    ) -> String {
        let mut result = template.to_string();

        result = result.replace("{task_id}", &self.task_id);
        result = result.replace("{item_id}", &self.item_id);
        result = result.replace("{cycle}", &self.cycle.to_string());
        result = result.replace("{phase}", &self.phase);
        result = result.replace("{workspace_root}", &self.workspace_root.to_string_lossy());
        result = result.replace("{source_tree}", &self.workspace_root.to_string_lossy());

        if let Some(pipeline) = pipeline {
            result = result.replace(
                "{build_output}",
                &escape_for_bash_dquote(&pipeline.prev_stdout),
            );
            result = result.replace(
                "{test_output}",
                &escape_for_bash_dquote(&pipeline.prev_stdout),
            );
            result = result.replace("{diff}", &escape_for_bash_dquote(&pipeline.diff));

            if !pipeline.build_errors.is_empty() {
                let errors_json = serde_json::to_string(&pipeline.build_errors).unwrap_or_default();
                result = result.replace("{build_errors}", &errors_json);
            } else {
                result = result.replace("{build_errors}", "[]");
            }

            if !pipeline.test_failures.is_empty() {
                let failures_json =
                    serde_json::to_string(&pipeline.test_failures).unwrap_or_default();
                result = result.replace("{test_failures}", &failures_json);
            } else {
                result = result.replace("{test_failures}", "[]");
            }

            for (key, value) in &pipeline.vars {
                result = result.replace(&format!("{{{}}}", key), &escape_for_bash_dquote(value));
            }
        }

        for (i, output) in self.upstream_outputs.iter().enumerate() {
            let prefix = format!("upstream[{}]", i);

            result = result.replace(
                &format!("{}.exit_code", prefix),
                &output.exit_code.to_string(),
            );
            result = result.replace(
                &format!("{}.confidence", prefix),
                &output.confidence.to_string(),
            );
            result = result.replace(
                &format!("{}.quality_score", prefix),
                &output.quality_score.to_string(),
            );
            result = result.replace(
                &format!("{}.duration_ms", prefix),
                &output.metrics.duration_ms.to_string(),
            );

            for (j, artifact) in output.artifacts.iter().enumerate() {
                if let Some(content) = &artifact.content {
                    let key = format!("{}.artifacts[{}].content", prefix, j);
                    result = result.replace(
                        &format!("{{{}}}", key),
                        &serde_json::to_string(content).unwrap_or_default(),
                    );
                }
            }
        }

        result = self.shared_state.render_template(&result);

        result = result.replace("{artifacts.count}", &self.artifacts.count().to_string());

        result
    }

    /// Converts the full context into a lightweight serializable reference.
    pub fn to_ref(&self) -> AgentContextRef {
        AgentContextRef {
            task_id: self.task_id.clone(),
            item_id: self.item_id.clone(),
            cycle: self.cycle,
            phase: Some(self.phase.clone()),
            workspace_root: self.workspace_root.to_string_lossy().to_string(),
            workspace_id: self.workspace_id.clone(),
        }
    }
}

/// Record of a single completed or attempted phase execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseRecord {
    /// Phase identifier.
    pub phase: String,
    /// Agent identifier selected for the phase.
    pub agent_id: String,
    /// Run identifier for the phase execution.
    pub run_id: Uuid,
    /// Process exit code returned by the agent command.
    pub exit_code: i64,
    /// Optional structured output captured from the run.
    pub output: Option<AgentOutput>,
    /// Start timestamp.
    pub started_at: DateTime<Utc>,
    /// End timestamp.
    pub ended_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Artifact, ArtifactKind};

    #[test]
    fn test_agent_context_template() {
        let ctx = AgentContext::new(
            "task1".to_string(),
            "item1".to_string(),
            1,
            "qa".to_string(),
            PathBuf::from("/workspace"),
            "ws1".to_string(),
        );

        let result = ctx.render_template("Task: {task_id}, Item: {item_id}, Cycle: {cycle}");
        assert_eq!(result, "Task: task1, Item: item1, Cycle: 1");
    }

    #[test]
    fn test_agent_context_to_ref() {
        let ctx = AgentContext::new(
            "t1".to_string(),
            "i1".to_string(),
            2,
            "qa".to_string(),
            PathBuf::from("/ws"),
            "ws1".to_string(),
        );
        let r = ctx.to_ref();
        assert_eq!(r.task_id, "t1");
        assert_eq!(r.item_id, "i1");
        assert_eq!(r.cycle, 2);
        assert_eq!(r.phase, Some("qa".to_string()));
    }

    #[test]
    fn test_agent_context_add_upstream_output() {
        let mut ctx = AgentContext::new(
            "t1".to_string(),
            "i1".to_string(),
            1,
            "qa".to_string(),
            PathBuf::from("/ws"),
            "ws1".to_string(),
        );

        let output = AgentOutput::new(
            Uuid::new_v4(),
            "plan_agent".to_string(),
            "plan".to_string(),
            0,
            "plan output".to_string(),
            "".to_string(),
        )
        .with_artifacts(vec![Artifact::new(ArtifactKind::Custom {
            name: "plan_doc".to_string(),
        })]);

        ctx.add_upstream_output(output);
        assert_eq!(ctx.upstream_outputs.len(), 1);
        assert_eq!(ctx.artifacts.count(), 1);
    }

    #[test]
    fn test_agent_context_render_source_tree_alias() {
        let ctx = AgentContext::new(
            "t1".to_string(),
            "i1".to_string(),
            1,
            "qa".to_string(),
            PathBuf::from("/workspace"),
            "ws1".to_string(),
        );
        let result = ctx.render_template("root={source_tree}");
        assert_eq!(result, "root=/workspace");
    }

    #[test]
    fn test_pipeline_vars_escaped_in_template() {
        let ctx = AgentContext::new(
            "task1".to_string(),
            "item1".to_string(),
            1,
            "plan".to_string(),
            PathBuf::from("/workspace"),
            "ws1".to_string(),
        );

        let mut pipeline = orchestrator_config::config::PipelineVariables::default();
        pipeline.vars.insert(
            "plan_output".to_string(),
            "Split `resource.rs` into `mod.rs` and `api.rs`".to_string(),
        );

        let template = r#"claude "Plan: {plan_output}""#;
        let rendered = ctx.render_template_with_pipeline(template, Some(&pipeline));

        assert!(rendered.contains("\\`resource.rs\\`"));
        assert!(rendered.contains("\\`mod.rs\\`"));
        assert!(rendered.contains("\\`api.rs\\`"));
        assert!(!rendered.contains(" `resource.rs` "));
    }
}
