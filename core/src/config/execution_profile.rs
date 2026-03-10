use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProfileMode {
    #[default]
    Host,
    Sandbox,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionFsMode {
    #[default]
    Inherit,
    WorkspaceReadonly,
    WorkspaceRwScoped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionNetworkMode {
    #[default]
    Inherit,
    Deny,
    Allowlist,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionProfileConfig {
    #[serde(default)]
    pub mode: ExecutionProfileMode,
    #[serde(default)]
    pub fs_mode: ExecutionFsMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writable_paths: Vec<String>,
    #[serde(default)]
    pub network_mode: ExecutionNetworkMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_allowlist: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cpu_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_processes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_open_files: Option<u64>,
}

impl Default for ExecutionProfileConfig {
    fn default() -> Self {
        Self {
            mode: ExecutionProfileMode::Host,
            fs_mode: ExecutionFsMode::Inherit,
            writable_paths: Vec::new(),
            network_mode: ExecutionNetworkMode::Inherit,
            network_allowlist: Vec::new(),
            max_memory_mb: None,
            max_cpu_seconds: None,
            max_processes: None,
            max_open_files: None,
        }
    }
}

impl ExecutionProfileConfig {
    pub fn implicit_host() -> Self {
        Self::default()
    }
}
