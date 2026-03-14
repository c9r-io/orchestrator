use serde::{Deserialize, Serialize};

/// Supported isolation strategies for item-scoped execution.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ItemIsolationStrategy {
    /// No isolation. All items execute in the primary workspace.
    #[default]
    None,
    /// Each item executes in its own git worktree.
    GitWorktree,
}

/// Cleanup timing for isolated item workspaces.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ItemIsolationCleanup {
    /// Remove temporary worktrees and branches when the workflow finishes.
    #[default]
    AfterWorkflow,
    /// Keep worktrees and branches for manual inspection.
    Never,
}

/// Workflow-level configuration for item-scoped isolation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItemIsolationConfig {
    /// Isolation strategy to use for item-scoped execution.
    #[serde(default)]
    pub strategy: ItemIsolationStrategy,
    /// Prefix used for temporary git branches created per item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_prefix: Option<String>,
    /// Cleanup policy for temporary worktrees and branches.
    #[serde(default)]
    pub cleanup: ItemIsolationCleanup,
}

impl Default for ItemIsolationConfig {
    fn default() -> Self {
        Self {
            strategy: ItemIsolationStrategy::None,
            branch_prefix: None,
            cleanup: ItemIsolationCleanup::AfterWorkflow,
        }
    }
}

impl ItemIsolationConfig {
    /// Whether item isolation is active.
    pub fn is_enabled(&self) -> bool {
        self.strategy != ItemIsolationStrategy::None
    }
}
