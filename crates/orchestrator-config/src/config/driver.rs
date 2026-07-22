use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Provider protocol implemented by an agent driver.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DriverProvider {
    /// Generic shell command execution.
    Shell,
    /// Anthropic Claude CLI protocol.
    Claude,
    /// OpenAI Codex CLI protocol.
    Codex,
}

/// Transport used to reach a provider implementation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DriverTransport {
    /// Spawn a provider CLI as an isolated child process.
    #[default]
    Cli,
    /// In-process SDK transport. Reserved for read-only future drivers.
    Sdk,
}

/// Provider-neutral permission behavior requested by an Agent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DriverPermissionMode {
    /// Use the provider driver's governed default.
    #[default]
    Governed,
    /// Ask the control plane before gated operations.
    Ask,
    /// Deny permission-gated operations.
    Deny,
}

/// Tool transport required or offered by a driver.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolHosting {
    /// No orchestrator-hosted tools are required or available.
    #[default]
    None,
    /// Tools are hosted over a child-process stdio protocol.
    Stdio,
    /// Tools are hosted over an authenticated local HTTP protocol.
    Http,
}

/// Cancellation guarantee exposed by a driver.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CancelSemantics {
    /// Cancellation is enforced by terminating an isolated process group.
    Guaranteed,
    /// Cancellation depends on provider cooperation.
    Cooperative,
    /// The driver cannot cancel an in-flight request.
    #[default]
    None,
}

/// Workspace access required by a workflow step.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAccess {
    /// The step does not inspect or mutate workspace files.
    None,
    /// The step only reads workspace files.
    Read,
    /// The step may mutate workspace files. This fail-closed default preserves safety.
    #[default]
    Write,
}

/// Provider-neutral execution options mapped by each driver.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DriverOptions {
    /// Optional model selector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Maximum provider turns in one run.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "maxTurns")]
    pub max_turns: Option<u32>,
    /// Optional cost ceiling in USD.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "budgetCapUsd"
    )]
    pub budget_cap_usd: Option<f64>,
    /// Provider-neutral permission mode.
    #[serde(
        default,
        skip_serializing_if = "is_governed_permission",
        rename = "permissionMode"
    )]
    pub permission_mode: DriverPermissionMode,
    /// Orchestrator tools the provider may call.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        rename = "allowedTools"
    )]
    pub allowed_tools: Vec<String>,
    /// Optional working directory relative to the governed workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Non-secret driver-local environment additions.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    /// Optional provider timeout in seconds.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "timeoutSecs"
    )]
    pub timeout_secs: Option<u64>,
}

fn is_governed_permission(value: &DriverPermissionMode) -> bool {
    *value == DriverPermissionMode::Governed
}

/// Claude-specific typed options that deliberately do not pretend to be portable.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ClaudeDriverConfig {
    /// Optional thinking budget in tokens.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "thinkingBudgetTokens"
    )]
    pub thinking_budget_tokens: Option<u32>,
}

/// Codex-specific typed options that deliberately do not pretend to be portable.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CodexDriverConfig {
    /// Optional reasoning effort selector interpreted by the Codex driver.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "reasoningEffort"
    )]
    pub reasoning_effort: Option<String>,
}

/// Shell-specific typed options.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ShellDriverConfig {
    /// Whether a shell command must contain the `{prompt}` placeholder.
    #[serde(default, rename = "requirePromptPlaceholder")]
    pub require_prompt_placeholder: bool,
}

/// Agent-scoped driver selection and provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentDriverConfig {
    /// Provider protocol.
    pub provider: DriverProvider,
    /// Provider transport. CLI is the only executable transport in FR-116.
    #[serde(default)]
    pub transport: DriverTransport,
    /// Optional executable override without provider flags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    /// Portable options mapped by the selected driver.
    #[serde(default, skip_serializing_if = "is_default_driver_options")]
    pub options: DriverOptions,
    /// Claude-only options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<ClaudeDriverConfig>,
    /// Codex-only options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<CodexDriverConfig>,
    /// Shell-only options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<ShellDriverConfig>,
    /// Explicitly unsafe provider arguments. Empty by default and gated at apply time.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "rawArgs")]
    pub raw_args: Vec<String>,
    /// Required explicit acknowledgement for `rawArgs`.
    #[serde(default, rename = "unsafeRawArgs")]
    pub unsafe_raw_args: bool,
}

fn is_default_driver_options(value: &DriverOptions) -> bool {
    *value == DriverOptions::default()
}

/// Capabilities a workflow step requires from every eligible explicit driver.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DriverRequirements {
    /// The step sends more than one user turn to the same provider process.
    #[serde(default, rename = "multiTurn")]
    pub multi_turn: bool,
    /// Required tool-hosting transport.
    #[serde(default, rename = "toolHosting")]
    pub tool_hosting: ToolHosting,
    /// The step attaches to provider context from an earlier command run.
    #[serde(default, rename = "sessionResume")]
    pub session_resume: bool,
    /// The step may request an audited human permission decision.
    #[serde(default, rename = "permissionEvents")]
    pub permission_events: bool,
    /// Workspace access intent. Defaults to write to fail closed.
    #[serde(default, rename = "workspaceAccess")]
    pub workspace_access: WorkspaceAccess,
}

impl Default for DriverRequirements {
    fn default() -> Self {
        Self {
            multi_turn: false,
            tool_hosting: ToolHosting::None,
            session_resume: false,
            permission_events: false,
            workspace_access: WorkspaceAccess::Write,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_manifest_roundtrip_keeps_provider_and_transport_orthogonal() {
        let yaml = r#"
provider: codex
transport: cli
options:
  model: gpt-test
  maxTurns: 4
codex:
  reasoningEffort: high
"#;
        let config: AgentDriverConfig = serde_yaml::from_str(yaml).expect("driver config");
        assert_eq!(config.provider, DriverProvider::Codex);
        assert_eq!(config.transport, DriverTransport::Cli);
        assert_eq!(config.options.max_turns, Some(4));
        assert_eq!(
            config.codex.and_then(|value| value.reasoning_effort),
            Some("high".to_string())
        );
    }

    #[test]
    fn driver_requirements_default_to_workspace_write() {
        let requirements = DriverRequirements::default();
        assert_eq!(requirements.workspace_access, WorkspaceAccess::Write);
        assert_eq!(requirements.tool_hosting, ToolHosting::None);
    }
}
