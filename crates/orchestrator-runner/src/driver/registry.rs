use super::contracts::DriverCapabilities;
use anyhow::{Result, bail};
use orchestrator_config::config::{
    AgentCommandRule, AgentDriverConfig, CancelSemantics, DriverProvider, DriverTransport,
    ToolHosting,
};
use std::path::{Component, Path};

/// Stable provider/transport identifier used in diagnostics.
pub fn driver_id(config: &AgentDriverConfig) -> &'static str {
    match (config.provider, config.transport) {
        (DriverProvider::Shell, DriverTransport::Cli) => "shell/cli",
        (DriverProvider::Claude, DriverTransport::Cli) => "claude/cli",
        (DriverProvider::Codex, DriverTransport::Cli) => "codex/cli",
        (DriverProvider::Shell, DriverTransport::Sdk) => "shell/sdk",
        (DriverProvider::Claude, DriverTransport::Sdk) => "claude/sdk",
        (DriverProvider::Codex, DriverTransport::Sdk) => "codex/sdk",
    }
}

/// Capabilities for the selected driver shape, including reserved SDK transports.
pub fn driver_capabilities(config: &AgentDriverConfig) -> DriverCapabilities {
    match (config.provider, config.transport) {
        (DriverProvider::Shell, DriverTransport::Cli) => DriverCapabilities {
            multi_turn: false,
            tool_hosting: ToolHosting::None,
            session_resume: false,
            permission_events: false,
            cancel: CancelSemantics::Guaranteed,
            sandboxable: true,
            cost_reporting: false,
        },
        (DriverProvider::Claude, DriverTransport::Cli) => DriverCapabilities {
            multi_turn: true,
            tool_hosting: ToolHosting::Stdio,
            session_resume: true,
            permission_events: true,
            cancel: CancelSemantics::Guaranteed,
            sandboxable: true,
            cost_reporting: true,
        },
        (DriverProvider::Codex, DriverTransport::Cli) => DriverCapabilities {
            multi_turn: false,
            tool_hosting: ToolHosting::None,
            // Certified against codex-cli 0.144.5 and pinned by the recorded
            // resume fixture under fixtures/driver/.
            session_resume: true,
            permission_events: false,
            cancel: CancelSemantics::Guaranteed,
            sandboxable: true,
            cost_reporting: true,
        },
        (_, DriverTransport::Sdk) => DriverCapabilities {
            multi_turn: true,
            tool_hosting: ToolHosting::Http,
            session_resume: true,
            permission_events: true,
            cancel: CancelSemantics::Cooperative,
            sandboxable: false,
            cost_reporting: true,
        },
    }
}

/// Constructs the executable driver selected by an Agent manifest.
pub fn create_driver(config: &AgentDriverConfig) -> Result<Box<dyn super::AgentDriver>> {
    use super::providers::{ClaudeCliDriver, CodexCliDriver, ShellCliDriver};

    match (config.provider, config.transport) {
        (DriverProvider::Shell, DriverTransport::Cli) => Ok(Box::new(ShellCliDriver)),
        (DriverProvider::Claude, DriverTransport::Cli) => Ok(Box::new(ClaudeCliDriver)),
        (DriverProvider::Codex, DriverTransport::Cli) => Ok(Box::new(CodexCliDriver)),
        (_, DriverTransport::Sdk) => bail!(
            "driver {} is declared but unavailable in this build",
            driver_id(config)
        ),
    }
}

/// Validates one Agent driver independently of workflow requirements.
pub fn validate_driver_config(config: &AgentDriverConfig, legacy_command: &str) -> Result<()> {
    if config.transport == DriverTransport::Sdk {
        // The shape is retained for apply-time safety diagnostics, but no SDK implementation exists.
        if config.provider == DriverProvider::Shell {
            bail!("driver shell/sdk is not a meaningful provider transport");
        }
    }
    match config.provider {
        DriverProvider::Shell => {
            if legacy_command.trim().is_empty() {
                bail!("driver shell/cli requires agent.spec.command");
            }
            if config.claude.is_some() || config.codex.is_some() {
                bail!("shell driver cannot contain claude or codex configuration");
            }
        }
        DriverProvider::Claude => {
            if !legacy_command.trim().is_empty() {
                bail!("claude driver constructs its command; agent.spec.command must be omitted");
            }
            if config.codex.is_some() || config.shell.is_some() {
                bail!("claude driver cannot contain codex or shell configuration");
            }
        }
        DriverProvider::Codex => {
            if !legacy_command.trim().is_empty() {
                bail!("codex driver constructs its command; agent.spec.command must be omitted");
            }
            if config.claude.is_some() || config.shell.is_some() {
                bail!("codex driver cannot contain claude or shell configuration");
            }
        }
    }
    if !config.raw_args.is_empty() && !config.unsafe_raw_args {
        bail!("driver.rawArgs requires unsafeRawArgs: true");
    }
    if config
        .raw_args
        .iter()
        .any(|arg| arg.is_empty() || arg.chars().any(char::is_control) || arg.len() > 4096)
    {
        bail!("driver.rawArgs entries must contain 1-4096 characters without control bytes");
    }
    if let Some(cwd) = config.options.cwd.as_deref() {
        let path = Path::new(cwd);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            bail!("driver.options.cwd must be a workspace-relative path without '..'");
        }
    }
    Ok(())
}

/// Validates that conditional shell command selection is only used by the
/// provider that actually consumes those commands.
pub fn validate_driver_command_rules(
    config: &AgentDriverConfig,
    command_rules: &[AgentCommandRule],
) -> Result<()> {
    if !command_rules.is_empty() && config.provider != DriverProvider::Shell {
        bail!(
            "agent.spec.command_rules require driver shell/cli; driver {} constructs its own command",
            driver_id(config)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchestrator_config::config::{AgentDriverConfig, DriverOptions};

    fn driver(provider: DriverProvider) -> AgentDriverConfig {
        AgentDriverConfig {
            provider,
            transport: DriverTransport::Cli,
            binary: None,
            options: DriverOptions::default(),
            claude: None,
            codex: None,
            shell: None,
            raw_args: Vec::new(),
            unsafe_raw_args: false,
        }
    }

    #[test]
    fn cli_drivers_preserve_guaranteed_cancel_and_sandbox() {
        for provider in [
            DriverProvider::Shell,
            DriverProvider::Claude,
            DriverProvider::Codex,
        ] {
            let capabilities = driver_capabilities(&driver(provider));
            assert_eq!(capabilities.cancel, CancelSemantics::Guaranteed);
            assert!(capabilities.sandboxable);
        }
    }

    #[test]
    fn sdk_descriptor_is_never_workspace_sandboxable() {
        let mut config = driver(DriverProvider::Codex);
        config.transport = DriverTransport::Sdk;
        let capabilities = driver_capabilities(&config);
        assert!(!capabilities.sandboxable);
        assert_eq!(capabilities.cancel, CancelSemantics::Cooperative);
    }

    #[test]
    fn codex_resume_is_advertised_after_protocol_certification() {
        let capabilities = driver_capabilities(&driver(DriverProvider::Codex));
        assert!(capabilities.session_resume);
    }

    #[test]
    fn vendor_driver_rejects_legacy_command_and_parent_cwd() {
        let mut config = driver(DriverProvider::Claude);
        assert!(validate_driver_config(&config, "claude -p x").is_err());
        config.options.cwd = Some("../escape".to_string());
        assert!(validate_driver_config(&config, "").is_err());
    }

    #[test]
    fn command_rules_are_only_supported_by_shell_driver() {
        let rules = vec![AgentCommandRule {
            when: "true".to_string(),
            command: "echo selected".to_string(),
        }];

        assert!(validate_driver_command_rules(&driver(DriverProvider::Shell), &rules).is_ok());
        let error = validate_driver_command_rules(&driver(DriverProvider::Claude), &rules)
            .expect_err("vendor driver must reject shell command rules");
        assert!(error.to_string().contains("require driver shell/cli"));
    }
}
