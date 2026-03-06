use serde::{Deserialize, Serialize};

/// WP02: Action to spawn a single child task from a step's output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnTaskAction {
    /// Goal template for the spawned task. Supports `{var}` substitution.
    pub goal: String,
    /// Optional workflow ID. If omitted, inherits parent's workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    /// What to inherit from the parent task.
    #[serde(default)]
    pub inherit: SpawnInherit,
}

/// WP02: Action to spawn multiple child tasks from a JSON array in pipeline vars.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnTasksAction {
    /// Pipeline variable name containing the JSON array.
    pub from_var: String,
    /// JSON path to the array within the variable value (e.g. `$.goals`).
    pub json_path: String,
    /// How to map each array element to a task.
    pub mapping: SpawnMapping,
    /// What to inherit from the parent task.
    #[serde(default)]
    pub inherit: SpawnInherit,
    /// Maximum number of tasks to spawn (default: 5).
    #[serde(default = "default_max_tasks")]
    pub max_tasks: usize,
    /// Whether to queue tasks for background execution (default: true).
    #[serde(default = "default_true")]
    pub queue: bool,
}

fn default_max_tasks() -> usize {
    5
}

fn default_true() -> bool {
    true
}

/// Mapping from a JSON array element to task fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnMapping {
    /// JSON path to the goal field within each element.
    pub goal: String,
    /// Optional JSON path to a workflow ID field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    /// Optional JSON path to a task name field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// What to inherit from the parent task when spawning children.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnInherit {
    /// Inherit workspace (default: true).
    #[serde(default = "default_inherit_true")]
    pub workspace: bool,
    /// Inherit project (default: true).
    #[serde(default = "default_inherit_true")]
    pub project: bool,
    /// Inherit target files (default: false).
    #[serde(default)]
    pub target_files: bool,
}

fn default_inherit_true() -> bool {
    true
}

impl Default for SpawnInherit {
    fn default() -> Self {
        Self {
            workspace: true,
            project: true,
            target_files: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_task_action_minimal() {
        let json = r#"{"goal": "improve {area}"}"#;
        let action: SpawnTaskAction =
            serde_json::from_str(json).expect("deserialize spawn task");
        assert_eq!(action.goal, "improve {area}");
        assert!(action.workflow.is_none());
        assert!(action.inherit.workspace);
        assert!(action.inherit.project);
        assert!(!action.inherit.target_files);
    }

    #[test]
    fn test_spawn_tasks_action_defaults() {
        let json = r#"{
            "from_var": "goals_output",
            "json_path": "$.goals",
            "mapping": {"goal": "$.description"}
        }"#;
        let action: SpawnTasksAction =
            serde_json::from_str(json).expect("deserialize spawn tasks");
        assert_eq!(action.max_tasks, 5);
        assert!(action.queue);
        assert!(action.inherit.workspace);
    }

    #[test]
    fn test_spawn_inherit_default() {
        let inherit = SpawnInherit::default();
        assert!(inherit.workspace);
        assert!(inherit.project);
        assert!(!inherit.target_files);
    }

    #[test]
    fn test_spawn_tasks_action_full() {
        let json = r#"{
            "from_var": "analysis",
            "json_path": "$.tasks",
            "mapping": {
                "goal": "$.goal",
                "workflow": "$.workflow",
                "name": "$.name"
            },
            "inherit": {"workspace": true, "project": false, "target_files": true},
            "max_tasks": 10,
            "queue": false
        }"#;
        let action: SpawnTasksAction =
            serde_json::from_str(json).expect("deserialize full spawn tasks");
        assert_eq!(action.max_tasks, 10);
        assert!(!action.queue);
        assert!(!action.inherit.project);
        assert!(action.inherit.target_files);
    }
}
