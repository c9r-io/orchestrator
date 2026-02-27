use crate::cli::{OutputFormat, TaskCommands, WorkspaceCommands};
use crate::cli_types::ResourceMetadata;
use crate::config_load::{persist_config_and_reload, read_active_config};
use anyhow::{Context, Result};
use serde_json::json;

use super::parse::{
    matches_selector, parse_label_selector, parse_resource_selector, string_map_to_csv,
};
use super::CliHandler;

impl CliHandler {
    pub(super) fn handle_get(
        &self,
        resource: &str,
        output: OutputFormat,
        selector: Option<&str>,
    ) -> Result<i32> {
        if resource.contains('/') {
            if selector.is_some() {
                anyhow::bail!("--selector/-l is only supported for list queries");
            }
            return self.handle_get_single(resource, output);
        }

        self.handle_get_list(resource, output, selector)
    }

    fn handle_get_single(&self, resource: &str, output: OutputFormat) -> Result<i32> {
        let (kind, name) = parse_resource_selector(resource)?;

        match kind {
            "ws" | "workspace" => self.handle_workspace(&WorkspaceCommands::Info {
                workspace_id: name.to_string(),
                output,
            }),
            "wf" | "workflow" => {
                let active = read_active_config(&self.state)?;
                if let Some(wf) = active.config.workflows.get(name) {
                    match output {
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string_pretty(wf)?);
                        }
                        OutputFormat::Yaml => {
                            println!("{}", serde_yaml::to_string(wf)?);
                        }
                        OutputFormat::Table => {
                            let step_types: Vec<String> = wf
                                .steps
                                .iter()
                                .map(|s| format!("{:?}", s.step_type))
                                .collect();
                            println!("{:<20} {:<40}", name, step_types.join(", "));
                        }
                    }
                    Ok(0)
                } else {
                    anyhow::bail!("workflow not found: {}", name)
                }
            }
            "agent" => {
                let active = read_active_config(&self.state)?;
                if let Some(agent) = active.config.agents.get(name) {
                    match output {
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string_pretty(agent)?);
                        }
                        OutputFormat::Yaml => {
                            println!("{}", serde_yaml::to_string(agent)?);
                        }
                        OutputFormat::Table => {
                            let templates: Vec<&str> = [
                                agent.templates.get("init_once").map(|s| s.as_str()),
                                agent.templates.get("qa").map(|s| s.as_str()),
                                agent.templates.get("fix").map(|s| s.as_str()),
                                agent.templates.get("retest").map(|s| s.as_str()),
                                agent.templates.get("loop_guard").map(|s| s.as_str()),
                            ]
                            .into_iter()
                            .flatten()
                            .collect();
                            println!("{:<20} {:?}", name, templates);
                        }
                    }
                    Ok(0)
                } else {
                    anyhow::bail!("agent not found: {}", name)
                }
            }
            "task" | "t" => self.handle_task(&TaskCommands::Info {
                task_id: name.to_string(),
                output,
            }),
            _ => anyhow::bail!(
                "unknown resource type: {} (supported: ws/workspace, wf/workflow, agent, task)",
                kind
            ),
        }
    }

    fn handle_get_list(
        &self,
        resource_type: &str,
        output: OutputFormat,
        selector: Option<&str>,
    ) -> Result<i32> {
        let selector_terms = selector
            .map(parse_label_selector)
            .transpose()?
            .unwrap_or_default();
        let active = read_active_config(&self.state)?;

        match resource_type {
            "ws" | "workspace" | "workspaces" => {
                let rows: Vec<_> = active
                    .config
                    .workspaces
                    .iter()
                    .filter_map(|(name, ws)| {
                        let metadata = ResourceMetadata {
                            name: name.clone(),
                            project: None,
                            labels: active
                                .config
                                .resource_meta
                                .workspaces
                                .get(name)
                                .and_then(|m| m.labels.clone()),
                            annotations: active
                                .config
                                .resource_meta
                                .workspaces
                                .get(name)
                                .and_then(|m| m.annotations.clone()),
                        };
                        if !matches_selector(&metadata.labels, &selector_terms) {
                            return None;
                        }
                        Some(json!({
                            "name": name,
                            "root_path": ws.root_path,
                            "qa_targets": ws.qa_targets,
                            "ticket_dir": ws.ticket_dir,
                            "labels": metadata.labels,
                            "annotations": metadata.annotations,
                        }))
                    })
                    .collect();
                self.print_resource_rows("WORKSPACE", rows, output, |row| {
                    let labels = row
                        .get("labels")
                        .and_then(|v| v.as_object())
                        .map(string_map_to_csv)
                        .unwrap_or_else(|| "-".to_string());
                    format!(
                        "{:<20} {:<40} {:<30}",
                        row["name"].as_str().unwrap_or_default(),
                        row["root_path"].as_str().unwrap_or_default(),
                        labels
                    )
                })
            }
            "agent" | "agents" => {
                let rows: Vec<_> = active
                    .config
                    .agents
                    .iter()
                    .filter_map(|(name, agent)| {
                        let metadata = ResourceMetadata {
                            name: name.clone(),
                            project: None,
                            labels: active
                                .config
                                .resource_meta
                                .agents
                                .get(name)
                                .and_then(|m| m.labels.clone()),
                            annotations: active
                                .config
                                .resource_meta
                                .agents
                                .get(name)
                                .and_then(|m| m.annotations.clone()),
                        };
                        if !matches_selector(&metadata.labels, &selector_terms) {
                            return None;
                        }
                        Some(json!({
                            "name": name,
                            "capabilities": agent.capabilities,
                            "labels": metadata.labels,
                            "annotations": metadata.annotations,
                        }))
                    })
                    .collect();
                self.print_resource_rows("AGENT", rows, output, |row| {
                    let capabilities = row["capabilities"]
                        .as_array()
                        .map(|caps| {
                            caps.iter()
                                .filter_map(|c| c.as_str())
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_default();
                    let labels = row
                        .get("labels")
                        .and_then(|v| v.as_object())
                        .map(string_map_to_csv)
                        .unwrap_or_else(|| "-".to_string());
                    format!(
                        "{:<20} {:<30} {:<30}",
                        row["name"].as_str().unwrap_or_default(),
                        capabilities,
                        labels
                    )
                })
            }
            "wf" | "workflow" | "workflows" => {
                let rows: Vec<_> = active
                    .config
                    .workflows
                    .iter()
                    .filter_map(|(name, workflow)| {
                        let metadata = ResourceMetadata {
                            name: name.clone(),
                            project: None,
                            labels: active
                                .config
                                .resource_meta
                                .workflows
                                .get(name)
                                .and_then(|m| m.labels.clone()),
                            annotations: active
                                .config
                                .resource_meta
                                .workflows
                                .get(name)
                                .and_then(|m| m.annotations.clone()),
                        };
                        if !matches_selector(&metadata.labels, &selector_terms) {
                            return None;
                        }
                        let steps: Vec<String> = workflow
                            .steps
                            .iter()
                            .filter_map(|s| s.step_type.as_ref().map(|t| t.as_str().to_string()))
                            .collect();
                        Some(json!({
                            "name": name,
                            "steps": steps,
                            "labels": metadata.labels,
                            "annotations": metadata.annotations,
                        }))
                    })
                    .collect();
                self.print_resource_rows("WORKFLOW", rows, output, |row| {
                    let steps = row["steps"]
                        .as_array()
                        .map(|steps| {
                            steps
                                .iter()
                                .filter_map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_default();
                    let labels = row
                        .get("labels")
                        .and_then(|v| v.as_object())
                        .map(string_map_to_csv)
                        .unwrap_or_else(|| "-".to_string());
                    format!(
                        "{:<20} {:<30} {:<30}",
                        row["name"].as_str().unwrap_or_default(),
                        steps,
                        labels
                    )
                })
            }
            _ => anyhow::bail!(
                "unknown list resource type: {} (supported: workspaces, agents, workflows)",
                resource_type
            ),
        }
    }

    pub(super) fn handle_describe(&self, resource: &str, output: OutputFormat) -> Result<i32> {
        let parts: Vec<&str> = resource.split('/').collect();
        if parts.len() != 2 {
            anyhow::bail!(
                "invalid resource format: {} (use format: resource/name)",
                resource
            );
        }
        let (kind, name) = (parts[0], parts[1]);

        match kind {
            "ws" | "workspace" => self.handle_workspace(&WorkspaceCommands::Info {
                workspace_id: name.to_string(),
                output,
            }),
            "wf" | "workflow" => {
                let active = read_active_config(&self.state)?;
                if let Some(wf) = active.config.workflows.get(name) {
                    match output {
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string_pretty(wf)?);
                        }
                        OutputFormat::Yaml => {
                            println!("{}", serde_yaml::to_string(wf)?);
                        }
                        OutputFormat::Table => {
                            let step_types: Vec<String> = wf
                                .steps
                                .iter()
                                .map(|s| format!("{:?}", s.step_type))
                                .collect();
                            println!("{:<20} {:<40}", name, step_types.join(", "));
                        }
                    }
                    Ok(0)
                } else {
                    anyhow::bail!("workflow not found: {}", name)
                }
            }
            "agent" => {
                let active = read_active_config(&self.state)?;
                if let Some(agent) = active.config.agents.get(name) {
                    match output {
                        OutputFormat::Json => {
                            let mut obj = serde_json::to_value(agent)?;
                            if let Some(map) = obj.as_object_mut() {
                                map.insert("output_schema".to_string(), json!({
                                    "type": "AgentOutput",
                                    "fields": {
                                        "exit_code": "i64",
                                        "stdout": "String",
                                        "stderr": "String",
                                        "artifacts": "[Artifact]",
                                        "confidence": "f32 (0.0-1.0)",
                                        "quality_score": "f32 (0.0-1.0)"
                                    },
                                    "artifact_kinds": ["ticket", "code_change", "test_result", "analysis", "decision"]
                                }));
                            }
                            println!("{}", serde_json::to_string_pretty(&obj)?);
                        }
                        OutputFormat::Yaml => {
                            println!("{}", serde_yaml::to_string(agent)?);
                        }
                        OutputFormat::Table => {
                            println!("Agent: {}", name);
                            println!("  Cost: {:?}", agent.metadata.cost);
                            println!("  Capabilities: {:?}", agent.capabilities);
                            println!("  Strategy: {:?}", agent.selection.strategy);
                            println!("  Templates:");
                            for (phase, tmpl) in &agent.templates {
                                println!("    {}: {}", phase, tmpl);
                            }
                            println!("  Output Schema: AgentOutput {{ exit_code, stdout, artifacts, confidence, quality_score }}");
                        }
                    }
                    Ok(0)
                } else {
                    anyhow::bail!("agent not found: {}", name)
                }
            }
            "task" | "t" => self.handle_task(&TaskCommands::Info {
                task_id: name.to_string(),
                output,
            }),
            _ => anyhow::bail!(
                "unknown resource type: {} (supported: ws/workspace, wf/workflow, agent, task)",
                kind
            ),
        }
    }

    pub(super) fn handle_delete(&self, resource: &str, force: bool) -> Result<i32> {
        let parts: Vec<&str> = resource.split('/').collect();
        if parts.len() != 2 {
            anyhow::bail!(
                "invalid resource format: {} (use format: kind/name, e.g., workspace/my-ws)",
                resource
            );
        }
        let (kind, name) = (parts[0], parts[1]);

        if !force {
            println!("Use --force to confirm deletion of {}/{}", kind, name);
            return Ok(0);
        }

        let mut config = {
            let active = read_active_config(&self.state)?;
            active.config.clone()
        };

        if (kind == "ws" || kind == "workspace") && config.defaults.workspace == name {
            anyhow::bail!(
                "cannot delete workspace '{}': it is the current default workspace",
                name
            );
        }
        if (kind == "wf" || kind == "workflow") && config.defaults.workflow == name {
            anyhow::bail!(
                "cannot delete workflow '{}': it is the current default workflow",
                name
            );
        }

        let deleted = crate::resource::delete_resource_by_kind(&mut config, kind, name)?;
        if !deleted {
            anyhow::bail!("{}/{} not found", kind, name);
        }

        let yaml = serde_yaml::to_string(&config)
            .context("failed to serialize configuration after delete")?;
        persist_config_and_reload(&self.state, config, yaml, "cli")?;
        println!("{}/{} deleted", kind, name);
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::super::CliHandler;
    use crate::cli::{Cli, Commands, OutputFormat};
    use crate::config_load::read_active_config;
    use crate::test_utils::TestState;

    #[test]
    fn delete_requires_force_flag() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Delete {
                resource: "workspace/default".to_string(),
                force: false,
            },
            verbose: false,
        };

        let code = handler
            .execute(&cli)
            .expect("should succeed without deleting");
        assert_eq!(code, 0);

        let active = read_active_config(&state).expect("config should be readable");
        assert!(active.config.workspaces.contains_key("default"));
    }

    #[test]
    fn delete_rejects_default_workspace() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Delete {
                resource: "workspace/default".to_string(),
                force: true,
            },
            verbose: false,
        };

        let result = handler.execute(&cli);
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(err_msg.contains("default workspace"));
    }

    #[test]
    fn delete_rejects_default_workflow() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Delete {
                resource: "workflow/basic".to_string(),
                force: true,
            },
            verbose: false,
        };

        let result = handler.execute(&cli);
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(err_msg.contains("default workflow"));
    }

    #[test]
    fn delete_nonexistent_resource_returns_error() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Delete {
                resource: "workspace/nonexistent".to_string(),
                force: true,
            },
            verbose: false,
        };

        let result = handler.execute(&cli);
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(err_msg.contains("not found"));
    }

    #[test]
    fn get_single_resource_rejects_selector_flag() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let cli = Cli {
            command: Commands::Get {
                resource: "workspace/default".to_string(),
                output: OutputFormat::Table,
                selector: Some("env=dev".to_string()),
            },
            verbose: false,
        };

        let err = handler
            .execute(&cli)
            .expect_err("selector should fail for single get");
        assert!(err.to_string().contains("only supported for list queries"));
    }
}
