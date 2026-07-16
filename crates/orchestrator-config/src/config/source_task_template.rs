use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Trusted skill invocation attached to a source task template.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceTaskTemplateSkillConfig {
    /// Stable skill identifier used for governance and provenance.
    pub name: String,
    /// Trusted invocation text, for example `$docs`.
    pub invocation: String,
    /// Ordered, bounded arguments supplied by trusted configuration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// Task action produced after a source event matches this template.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceTaskTemplateActionConfig {
    /// Workflow to use when a later binding creates the task.
    pub workflow: String,
    /// Workspace to use when a later binding creates the task.
    pub workspace: String,
    /// Whether a later binding should immediately start the task.
    #[serde(default)]
    pub start: bool,
    /// Trusted initial task variables. Source-controlled values cannot overwrite these.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub initial_vars: BTreeMap<String, String>,
}

/// Runtime representation of a project-scoped source-to-task template.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceTaskTemplateConfig {
    /// Trusted skill configuration.
    pub skill: SourceTaskTemplateSkillConfig,
    /// Trusted task action configuration.
    pub action: SourceTaskTemplateActionConfig,
    /// Goal template rendered from explicitly allowlisted source variables.
    pub goal_template: String,
    /// Exact variable names permitted in `goal_template`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_variables: Vec<String>,
}
