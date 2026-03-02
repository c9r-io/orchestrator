use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunnerPolicy {
    #[default]
    Legacy,
    Allowlist,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunnerExecutorKind {
    #[default]
    Shell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerConfig {
    pub shell: String,
    #[serde(default = "default_shell_arg")]
    pub shell_arg: String,
    #[serde(default)]
    pub policy: RunnerPolicy,
    #[serde(default)]
    pub executor: RunnerExecutorKind,
    #[serde(default = "default_allowed_shells")]
    pub allowed_shells: Vec<String>,
    #[serde(default = "default_allowed_shell_args")]
    pub allowed_shell_args: Vec<String>,
    #[serde(default = "default_env_allowlist")]
    pub env_allowlist: Vec<String>,
    #[serde(default = "default_redaction_patterns")]
    pub redaction_patterns: Vec<String>,
}

fn default_shell_arg() -> String {
    "-lc".to_string()
}

fn default_allowed_shells() -> Vec<String> {
    vec![
        "/bin/bash".to_string(),
        "/bin/zsh".to_string(),
        "/bin/sh".to_string(),
    ]
}

fn default_allowed_shell_args() -> Vec<String> {
    vec!["-lc".to_string(), "-c".to_string()]
}

fn default_env_allowlist() -> Vec<String> {
    vec![
        "PATH".to_string(),
        "HOME".to_string(),
        "USER".to_string(),
        "LANG".to_string(),
        "TERM".to_string(),
    ]
}

fn default_redaction_patterns() -> Vec<String> {
    vec![
        "token".to_string(),
        "password".to_string(),
        "secret".to_string(),
        "api_key".to_string(),
        "authorization".to_string(),
    ]
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            shell: "/bin/bash".to_string(),
            shell_arg: default_shell_arg(),
            policy: RunnerPolicy::Legacy,
            executor: RunnerExecutorKind::Shell,
            allowed_shells: default_allowed_shells(),
            allowed_shell_args: default_allowed_shell_args(),
            env_allowlist: default_env_allowlist(),
            redaction_patterns: default_redaction_patterns(),
        }
    }
}

/// Resume behavior configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeConfig {
    pub auto: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runner_config_default() {
        let cfg = RunnerConfig::default();
        assert_eq!(cfg.shell, "/bin/bash");
        assert_eq!(cfg.shell_arg, "-lc");
        assert_eq!(cfg.policy, RunnerPolicy::Legacy);
        assert_eq!(cfg.executor, RunnerExecutorKind::Shell);
        assert_eq!(cfg.allowed_shells.len(), 3);
        assert!(cfg.allowed_shells.contains(&"/bin/bash".to_string()));
        assert_eq!(cfg.allowed_shell_args, vec!["-lc", "-c"]);
        assert!(cfg.env_allowlist.contains(&"PATH".to_string()));
        assert!(cfg.env_allowlist.contains(&"HOME".to_string()));
        assert!(cfg.redaction_patterns.contains(&"token".to_string()));
        assert!(cfg.redaction_patterns.contains(&"secret".to_string()));
    }

    #[test]
    fn test_runner_policy_default() {
        let policy = RunnerPolicy::default();
        assert_eq!(policy, RunnerPolicy::Legacy);
    }

    #[test]
    fn test_runner_executor_kind_default() {
        let kind = RunnerExecutorKind::default();
        assert_eq!(kind, RunnerExecutorKind::Shell);
    }

    #[test]
    fn test_runner_config_serde_round_trip() {
        let cfg = RunnerConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let cfg2: RunnerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg2.shell, cfg.shell);
        assert_eq!(cfg2.policy, cfg.policy);
    }

    #[test]
    fn test_runner_config_deserialize_minimal() {
        let json = r#"{"shell": "/bin/sh"}"#;
        let cfg: RunnerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.shell, "/bin/sh");
        // defaults should kick in
        assert_eq!(cfg.shell_arg, "-lc");
        assert_eq!(cfg.policy, RunnerPolicy::Legacy);
        assert!(!cfg.allowed_shells.is_empty());
    }
}
