#[cfg(test)]
mod cases {
    use super::super::*;
    use clap::Parser;

    macro_rules! assert_variant {
        ($value:expr, $pattern:pat, $message:literal) => {
            assert!(matches!($value, $pattern), $message)
        };
    }

    #[test]
    fn parse_apply_file_and_dry_run_flags() {
        let cli = Cli::parse_from(["orchestrator", "apply", "-f", "resources.yaml", "--dry-run"]);

        match cli.command {
            Commands::Apply { file, dry_run, .. } => {
                assert_eq!(file, "resources.yaml");
                assert!(dry_run);
            }
            other => assert_variant!(other, Commands::Apply { .. }, "expected apply command"),
        }
    }

    #[test]
    fn parse_apply_defaults_dry_run_to_false() {
        let cli = Cli::parse_from(["orchestrator", "apply", "-f", "resources.yaml"]);

        match cli.command {
            Commands::Apply { file, dry_run, .. } => {
                assert_eq!(file, "resources.yaml");
                assert!(!dry_run);
            }
            other => assert_variant!(other, Commands::Apply { .. }, "expected apply command"),
        }
    }

    #[test]
    fn parse_edit_export_command() {
        let cli = Cli::parse_from(["orchestrator", "edit", "export", "workspace/default"]);

        match cli.command {
            Commands::Edit(EditCommands::Export { selector }) => {
                assert_eq!(selector, "workspace/default");
            }
            other => assert_variant!(
                other,
                Commands::Edit(EditCommands::Export { .. }),
                "expected edit export command"
            ),
        }
    }

    #[test]
    fn parse_edit_export_with_agent_selector() {
        let cli = Cli::parse_from(["orchestrator", "edit", "export", "agent/opencode"]);

        match cli.command {
            Commands::Edit(EditCommands::Export { selector }) => {
                assert_eq!(selector, "agent/opencode");
            }
            other => assert_variant!(
                other,
                Commands::Edit(EditCommands::Export { .. }),
                "expected edit export command"
            ),
        }
    }

    #[test]
    fn parse_edit_open_command() {
        let cli = Cli::parse_from(["orchestrator", "edit", "open", "workspace/default"]);

        match cli.command {
            Commands::Edit(EditCommands::Open { selector }) => {
                assert_eq!(selector, "workspace/default");
            }
            other => assert_variant!(
                other,
                Commands::Edit(EditCommands::Open { .. }),
                "expected edit open command"
            ),
        }
    }

    #[test]
    fn parse_workspace_info_with_positional_arg() {
        let cli = Cli::parse_from(["orchestrator", "workspace", "info", "new-workspace"]);

        match cli.command {
            Commands::Workspace(WorkspaceCommands::Info {
                workspace_id,
                output,
            }) => {
                assert_eq!(workspace_id, "new-workspace");
                assert_eq!(output, OutputFormat::Table);
            }
            other => assert_variant!(
                other,
                Commands::Workspace(WorkspaceCommands::Info { .. }),
                "expected workspace info command"
            ),
        }
    }

    #[test]
    fn parse_workspace_info_with_output_format() {
        let cli = Cli::parse_from(["orchestrator", "workspace", "info", "my-ws", "-o", "json"]);

        match cli.command {
            Commands::Workspace(WorkspaceCommands::Info {
                workspace_id,
                output,
            }) => {
                assert_eq!(workspace_id, "my-ws");
                assert_eq!(output, OutputFormat::Json);
            }
            other => assert_variant!(
                other,
                Commands::Workspace(WorkspaceCommands::Info { .. }),
                "expected workspace info command"
            ),
        }
    }

    #[test]
    fn parse_workspace_create_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "workspace",
            "create",
            "new-ws",
            "--root-path",
            "workspace/new",
            "--qa-target",
            "docs/qa",
            "--label",
            "env=dev",
            "--dry-run",
            "-o",
            "json",
        ]);

        match cli.command {
            Commands::Workspace(WorkspaceCommands::Create {
                name,
                root_path,
                qa_target,
                labels,
                dry_run,
                output,
                ..
            }) => {
                assert_eq!(name, "new-ws");
                assert_eq!(root_path, "workspace/new");
                assert_eq!(qa_target, vec!["docs/qa"]);
                assert_eq!(labels, vec!["env=dev"]);
                assert!(dry_run);
                assert_eq!(output, OutputFormat::Json);
            }
            other => assert_variant!(
                other,
                Commands::Workspace(WorkspaceCommands::Create { .. }),
                "expected workspace create command"
            ),
        }
    }

    #[test]
    fn parse_agent_create_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "agent",
            "create",
            "qa-agent",
            "--command",
            "glmcode -p \"{prompt}\"",
            "--capability",
            "qa",
        ]);

        match cli.command {
            Commands::Agent(AgentCommands::Create {
                name,
                command,
                capability,
                ..
            }) => {
                assert_eq!(name, "qa-agent");
                assert_eq!(command, "glmcode -p \"{prompt}\"");
                assert_eq!(capability, vec!["qa"]);
            }
            other => assert_variant!(
                other,
                Commands::Agent(AgentCommands::Create { .. }),
                "expected agent create command"
            ),
        }
    }

    #[test]
    fn parse_task_worker_start_workers_flag() {
        let cli = Cli::parse_from([
            "orchestrator",
            "task",
            "worker",
            "start",
            "--poll-ms",
            "250",
            "--workers",
            "6",
        ]);

        match cli.command {
            Commands::Task(TaskCommands::Worker(TaskWorkerCommands::Start {
                poll_ms,
                workers,
            })) => {
                assert_eq!(poll_ms, 250);
                assert_eq!(workers, 6);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Worker(TaskWorkerCommands::Start { .. })),
                "expected task worker start command"
            ),
        }
    }

    #[test]
    fn parse_task_worker_start_workers_default() {
        let cli = Cli::parse_from(["orchestrator", "task", "worker", "start"]);

        match cli.command {
            Commands::Task(TaskCommands::Worker(TaskWorkerCommands::Start {
                poll_ms,
                workers,
            })) => {
                assert_eq!(poll_ms, 1000);
                assert_eq!(workers, 1);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Worker(TaskWorkerCommands::Start { .. })),
                "expected task worker start command"
            ),
        }
    }

    #[test]
    fn parse_workflow_create_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "workflow",
            "create",
            "qa-flow",
            "--step",
            "qa",
            "--step",
            "fix",
            "--loop-mode",
            "infinite",
            "--max-cycles",
            "5",
        ]);

        match cli.command {
            Commands::Workflow(WorkflowCommands::Create {
                name,
                step,
                loop_mode,
                max_cycles,
                ..
            }) => {
                assert_eq!(name, "qa-flow");
                assert_eq!(step, vec!["qa", "fix"]);
                assert_eq!(loop_mode, "infinite");
                assert_eq!(max_cycles, Some(5));
            }
            other => assert_variant!(
                other,
                Commands::Workflow(WorkflowCommands::Create { .. }),
                "expected workflow create command"
            ),
        }
    }

    #[test]
    fn parse_init_command() {
        let cli = Cli::parse_from(["orchestrator", "init"]);

        match cli.command {
            Commands::Init { root, force } => {
                assert_eq!(root, None);
                assert!(!force);
            }
            other => assert_variant!(other, Commands::Init { .. }, "expected init command"),
        }
    }

    #[test]
    fn parse_init_command_with_options() {
        let cli = Cli::parse_from(["orchestrator", "init", "--root", "/tmp/test", "--force"]);

        match cli.command {
            Commands::Init { root, force } => {
                assert_eq!(root, Some("/tmp/test".to_string()));
                assert!(force);
            }
            other => assert_variant!(other, Commands::Init { .. }, "expected init command"),
        }
    }

    #[test]
    fn parse_get_command() {
        let cli = Cli::parse_from(["orchestrator", "get", "workspace/default"]);

        match cli.command {
            Commands::Get {
                resource,
                output,
                selector,
            } => {
                assert_eq!(resource, "workspace/default");
                assert_eq!(output, OutputFormat::Table);
                assert_eq!(selector, None);
            }
            other => assert_variant!(other, Commands::Get { .. }, "expected get command"),
        }
    }

    #[test]
    fn parse_get_command_yaml() {
        let cli = Cli::parse_from(["orchestrator", "get", "agent/echo", "-o", "yaml"]);

        match cli.command {
            Commands::Get {
                resource,
                output,
                selector,
            } => {
                assert_eq!(resource, "agent/echo");
                assert_eq!(output, OutputFormat::Yaml);
                assert_eq!(selector, None);
            }
            other => assert_variant!(other, Commands::Get { .. }, "expected get command"),
        }
    }

    #[test]
    fn parse_get_list_with_selector() {
        let cli = Cli::parse_from(["orchestrator", "get", "workspaces", "-l", "env=prod"]);

        match cli.command {
            Commands::Get {
                resource,
                output,
                selector,
            } => {
                assert_eq!(resource, "workspaces");
                assert_eq!(output, OutputFormat::Table);
                assert_eq!(selector, Some("env=prod".to_string()));
            }
            other => assert_variant!(other, Commands::Get { .. }, "expected get command"),
        }
    }

    #[test]
    fn parse_describe_command() {
        let cli = Cli::parse_from(["orchestrator", "describe", "workflow/basic"]);

        match cli.command {
            Commands::Describe { resource, output } => {
                assert_eq!(resource, "workflow/basic");
                assert_eq!(output, OutputFormat::Yaml);
            }
            other => assert_variant!(
                other,
                Commands::Describe { .. },
                "expected describe command"
            ),
        }
    }

    #[test]
    fn parse_delete_command() {
        let cli = Cli::parse_from(["orchestrator", "delete", "workspace/old-ws"]);

        match cli.command {
            Commands::Delete { resource, force } => {
                assert_eq!(resource, "workspace/old-ws");
                assert!(!force);
            }
            other => assert_variant!(other, Commands::Delete { .. }, "expected delete command"),
        }
    }

    #[test]
    fn parse_delete_force() {
        let cli = Cli::parse_from(["orchestrator", "delete", "agent/old", "--force"]);

        match cli.command {
            Commands::Delete { resource, force } => {
                assert_eq!(resource, "agent/old");
                assert!(force);
            }
            other => assert_variant!(other, Commands::Delete { .. }, "expected delete command"),
        }
    }

    #[test]
    fn parse_delete_alias_rm() {
        let cli = Cli::parse_from(["orchestrator", "rm", "workflow/old-wf", "-f"]);

        match cli.command {
            Commands::Delete { resource, force } => {
                assert_eq!(resource, "workflow/old-wf");
                assert!(force);
            }
            other => assert_variant!(
                other,
                Commands::Delete { .. },
                "expected delete command via rm alias"
            ),
        }
    }

    #[test]
    fn parse_db_command() {
        let cli = Cli::parse_from(["orchestrator", "db", "reset"]);

        match cli.command {
            Commands::Db(DbCommands::Reset {
                force,
                include_history,
                include_config,
            }) => {
                assert!(!force);
                assert!(!include_history);
                assert!(!include_config);
            }
            other => assert_variant!(
                other,
                Commands::Db(DbCommands::Reset { .. }),
                "expected db reset command"
            ),
        }
    }

    #[test]
    fn parse_db_reset_force() {
        let cli = Cli::parse_from(["orchestrator", "db", "reset", "--force"]);

        match cli.command {
            Commands::Db(DbCommands::Reset {
                force,
                include_history,
                include_config,
            }) => {
                assert!(force);
                assert!(!include_history);
                assert!(!include_config);
            }
            other => assert_variant!(
                other,
                Commands::Db(DbCommands::Reset { .. }),
                "expected db reset command"
            ),
        }
    }

    #[test]
    fn parse_db_reset_include_history() {
        let cli = Cli::parse_from([
            "orchestrator",
            "db",
            "reset",
            "--force",
            "--include-history",
        ]);

        match cli.command {
            Commands::Db(DbCommands::Reset {
                force,
                include_history,
                include_config,
            }) => {
                assert!(force);
                assert!(include_history);
                assert!(!include_config);
            }
            other => assert_variant!(
                other,
                Commands::Db(DbCommands::Reset { .. }),
                "expected db reset command"
            ),
        }
    }

    #[test]
    fn parse_db_reset_include_config() {
        let cli = Cli::parse_from(["orchestrator", "db", "reset", "--force", "--include-config"]);

        match cli.command {
            Commands::Db(DbCommands::Reset {
                force,
                include_history,
                include_config,
            }) => {
                assert!(force);
                assert!(!include_history);
                assert!(include_config);
            }
            other => assert_variant!(
                other,
                Commands::Db(DbCommands::Reset { .. }),
                "expected db reset command"
            ),
        }
    }

    #[test]
    fn parse_completion_command() {
        let cli = Cli::parse_from(["orchestrator", "completion", "bash"]);

        match cli.command {
            Commands::Completion(CompletionCommands::Bash) => {}
            other => assert_variant!(
                other,
                Commands::Completion(CompletionCommands::Bash),
                "expected completion bash command"
            ),
        }
    }

    #[test]
    fn parse_qa_project_create_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "qa",
            "project",
            "create",
            "qa-run-1",
            "--workspace",
            "ws-a",
            "--workflow",
            "qa_only",
            "--force",
        ]);

        match cli.command {
            Commands::Qa(QaCommands::Project(QaProjectCommands::Create {
                project_id,
                workspace,
                workflow,
                force,
                ..
            })) => {
                assert_eq!(project_id, "qa-run-1");
                assert_eq!(workspace, Some("ws-a".to_string()));
                assert_eq!(workflow, Some("qa_only".to_string()));
                assert!(force);
            }
            other => assert_variant!(
                other,
                Commands::Qa(QaCommands::Project(QaProjectCommands::Create { .. })),
                "expected qa project create command"
            ),
        }
    }

    #[test]
    fn parse_qa_project_reset_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "qa",
            "project",
            "reset",
            "qa-run-1",
            "--keep-config",
            "--force",
        ]);

        match cli.command {
            Commands::Qa(QaCommands::Project(QaProjectCommands::Reset {
                project_id,
                keep_config,
                force,
            })) => {
                assert_eq!(project_id, "qa-run-1");
                assert!(keep_config);
                assert!(force);
            }
            other => assert_variant!(
                other,
                Commands::Qa(QaCommands::Project(QaProjectCommands::Reset { .. })),
                "expected qa project reset command"
            ),
        }
    }

    #[test]
    fn parse_qa_doctor_command() {
        let cli = Cli::parse_from(["orchestrator", "qa", "doctor", "-o", "json"]);

        match cli.command {
            Commands::Qa(QaCommands::Doctor { output }) => {
                assert_eq!(output, OutputFormat::Json);
            }
            other => assert_variant!(
                other,
                Commands::Qa(QaCommands::Doctor { .. }),
                "expected qa doctor command"
            ),
        }
    }

    #[test]
    fn parse_task_info_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "info", "task-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Info { task_id, output }) => {
                assert_eq!(task_id, "task-123");
                assert_eq!(output, OutputFormat::Table);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Info { .. }),
                "expected task info command"
            ),
        }
    }

    #[test]
    fn parse_task_start_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "start", "task-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Start {
                task_id, latest, ..
            }) => {
                assert_eq!(task_id, Some("task-123".to_string()));
                assert!(!latest);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Start { .. }),
                "expected task start command"
            ),
        }
    }

    #[test]
    fn parse_task_start_latest() {
        let cli = Cli::parse_from(["orchestrator", "task", "start", "--latest"]);

        match cli.command {
            Commands::Task(TaskCommands::Start {
                task_id, latest, ..
            }) => {
                assert_eq!(task_id, None);
                assert!(latest);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Start { .. }),
                "expected task start command"
            ),
        }
    }

    #[test]
    fn parse_task_create_with_project_flag() {
        let cli = Cli::parse_from([
            "orchestrator",
            "task",
            "create",
            "--project",
            "default",
            "--name",
            "test",
            "--goal",
            "goal",
            "--no-start",
        ]);

        match cli.command {
            Commands::Task(TaskCommands::Create {
                project,
                name,
                goal,
                no_start,
                ..
            }) => {
                assert_eq!(project, Some("default".to_string()));
                assert_eq!(name, Some("test".to_string()));
                assert_eq!(goal, Some("goal".to_string()));
                assert!(no_start);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Create { .. }),
                "expected task create command"
            ),
        }
    }

    #[test]
    fn parse_task_list_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "list"]);

        match cli.command {
            Commands::Task(TaskCommands::List {
                status,
                output,
                verbose,
            }) => {
                assert_eq!(status, None);
                assert_eq!(output, OutputFormat::Table);
                assert!(!verbose);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::List { .. }),
                "expected task list command"
            ),
        }
    }

    #[test]
    fn parse_task_list_with_options() {
        let cli = Cli::parse_from([
            "orchestrator",
            "task",
            "list",
            "--status",
            "running",
            "-o",
            "json",
            "-v",
        ]);

        match cli.command {
            Commands::Task(TaskCommands::List {
                status,
                output,
                verbose,
            }) => {
                assert_eq!(status, Some("running".to_string()));
                assert_eq!(output, OutputFormat::Json);
                assert!(verbose);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::List { .. }),
                "expected task list command"
            ),
        }
    }

    #[test]
    fn parse_task_delete_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "delete", "task-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Delete { task_id, force }) => {
                assert_eq!(task_id, "task-123");
                assert!(!force);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Delete { .. }),
                "expected task delete command"
            ),
        }
    }

    #[test]
    fn parse_task_delete_force() {
        let cli = Cli::parse_from(["orchestrator", "task", "delete", "task-123", "--force"]);

        match cli.command {
            Commands::Task(TaskCommands::Delete { task_id, force }) => {
                assert_eq!(task_id, "task-123");
                assert!(force);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Delete { .. }),
                "expected task delete command"
            ),
        }
    }

    #[test]
    fn parse_task_retry_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "retry", "item-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Retry { task_item_id, .. }) => {
                assert_eq!(task_item_id, "item-123");
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Retry { .. }),
                "expected task retry command"
            ),
        }
    }

    #[test]
    fn parse_task_pause_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "pause", "task-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Pause { task_id }) => {
                assert_eq!(task_id, "task-123");
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Pause { .. }),
                "expected task pause command"
            ),
        }
    }

    #[test]
    fn parse_task_resume_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "resume", "task-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Resume { task_id, .. }) => {
                assert_eq!(task_id, "task-123");
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Resume { .. }),
                "expected task resume command"
            ),
        }
    }

    #[test]
    fn parse_task_edit_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "task",
            "edit",
            "task-123",
            "--insert-before",
            "qa",
            "--step",
            "plan",
            "--tty",
        ]);

        match cli.command {
            Commands::Task(TaskCommands::Edit {
                task_id,
                insert_before,
                step,
                tty,
                repeatable,
                ..
            }) => {
                assert_eq!(task_id, "task-123");
                assert_eq!(insert_before, "qa");
                assert_eq!(step, "plan");
                assert!(tty);
                assert!(!repeatable);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Edit { .. }),
                "expected task edit command"
            ),
        }
    }

    #[test]
    fn parse_task_session_list_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "session", "list", "task-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Session(TaskSessionCommands::List {
                task_id,
                output,
            })) => {
                assert_eq!(task_id, "task-123");
                assert_eq!(output, OutputFormat::Table);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Session(TaskSessionCommands::List { .. })),
                "expected task session list command"
            ),
        }
    }

    #[test]
    fn parse_task_session_info_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "task",
            "session",
            "info",
            "sess-123",
            "-o",
            "json",
        ]);

        match cli.command {
            Commands::Task(TaskCommands::Session(TaskSessionCommands::Info {
                session_id,
                output,
            })) => {
                assert_eq!(session_id, "sess-123");
                assert_eq!(output, OutputFormat::Json);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Session(TaskSessionCommands::Info { .. })),
                "expected task session info command"
            ),
        }
    }

    #[test]
    fn parse_task_session_close_force_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "task",
            "session",
            "close",
            "sess-123",
            "--force",
        ]);

        match cli.command {
            Commands::Task(TaskCommands::Session(TaskSessionCommands::Close {
                session_id,
                force,
            })) => {
                assert_eq!(session_id, "sess-123");
                assert!(force);
            }
            other => assert_variant!(
                other,
                Commands::Task(TaskCommands::Session(TaskSessionCommands::Close { .. })),
                "expected task session close command"
            ),
        }
    }

    #[test]
    fn parse_exec_interactive_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "exec",
            "-it",
            "task/task-123/step/plan-1",
            "--",
            "echo",
            "hello",
        ]);

        match cli.command {
            Commands::Exec {
                stdin,
                tty,
                target,
                command,
            } => {
                assert!(stdin);
                assert!(tty);
                assert_eq!(target, "task/task-123/step/plan-1");
                assert_eq!(command, vec!["echo".to_string(), "hello".to_string()]);
            }
            other => assert_variant!(other, Commands::Exec { .. }, "expected exec command"),
        }
    }

    #[test]
    fn parse_manifest_export_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "manifest",
            "export",
            "-o",
            "json",
            "-f",
            "/tmp/out.json",
        ]);

        match cli.command {
            Commands::Manifest(ManifestCommands::Export { output, file }) => {
                assert_eq!(output, OutputFormat::Json);
                assert_eq!(file, Some("/tmp/out.json".to_string()));
            }
            other => assert_variant!(
                other,
                Commands::Manifest(ManifestCommands::Export { .. }),
                "expected manifest export command"
            ),
        }
    }

    #[test]
    fn parse_manifest_validate_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "manifest",
            "validate",
            "-f",
            "/tmp/input.yaml",
        ]);

        match cli.command {
            Commands::Manifest(ManifestCommands::Validate { file }) => {
                assert_eq!(file, "/tmp/input.yaml");
            }
            other => assert_variant!(
                other,
                Commands::Manifest(ManifestCommands::Validate { .. }),
                "expected manifest validate command"
            ),
        }
    }

    #[test]
    fn parse_verify_binary_snapshot_default() {
        let cli = Cli::parse_from(["orchestrator", "verify", "binary-snapshot"]);

        match cli.command {
            Commands::Verify(VerifyCommands::BinarySnapshot { root }) => {
                assert_eq!(root, None);
            }
            other => assert_variant!(
                other,
                Commands::Verify(VerifyCommands::BinarySnapshot { .. }),
                "expected verify binary-snapshot command"
            ),
        }
    }

    #[test]
    fn parse_verify_binary_snapshot_with_root() {
        let cli = Cli::parse_from([
            "orchestrator",
            "verify",
            "binary-snapshot",
            "--root",
            "/path/to/workspace",
        ]);

        match cli.command {
            Commands::Verify(VerifyCommands::BinarySnapshot { root }) => {
                assert_eq!(root, Some("/path/to/workspace".to_string()));
            }
            other => assert_variant!(
                other,
                Commands::Verify(VerifyCommands::BinarySnapshot { .. }),
                "expected verify binary-snapshot command"
            ),
        }
    }

    #[test]
    fn parse_verify_binary_snapshot_short_flag() {
        let cli = Cli::parse_from([
            "orchestrator",
            "verify",
            "binary-snapshot",
            "-r",
            "/another/path",
        ]);

        match cli.command {
            Commands::Verify(VerifyCommands::BinarySnapshot { root }) => {
                assert_eq!(root, Some("/another/path".to_string()));
            }
            other => assert_variant!(
                other,
                Commands::Verify(VerifyCommands::BinarySnapshot { .. }),
                "expected verify binary-snapshot command"
            ),
        }
    }

    #[test]
    fn parse_check_default() {
        let cli = Cli::parse_from(["orchestrator", "check"]);

        match cli.command {
            Commands::Check { workflow, output } => {
                assert_eq!(workflow, None);
                assert_eq!(output, OutputFormat::Table);
            }
            other => assert_variant!(other, Commands::Check { .. }, "expected check command"),
        }
    }

    #[test]
    fn parse_check_with_workflow() {
        let cli = Cli::parse_from(["orchestrator", "check", "--workflow", "self-bootstrap"]);

        match cli.command {
            Commands::Check { workflow, output } => {
                assert_eq!(workflow, Some("self-bootstrap".to_string()));
                assert_eq!(output, OutputFormat::Table);
            }
            other => assert_variant!(other, Commands::Check { .. }, "expected check command"),
        }
    }

    #[test]
    fn parse_check_with_json_output() {
        let cli = Cli::parse_from(["orchestrator", "check", "-o", "json"]);

        match cli.command {
            Commands::Check { workflow, output } => {
                assert_eq!(workflow, None);
                assert_eq!(output, OutputFormat::Json);
            }
            other => assert_variant!(other, Commands::Check { .. }, "expected check command"),
        }
    }

    #[test]
    fn parse_check_alias_ck() {
        let cli = Cli::parse_from(["orchestrator", "ck"]);

        match cli.command {
            Commands::Check { .. } => {}
            other => assert_variant!(
                other,
                Commands::Check { .. },
                "expected check command via alias"
            ),
        }
    }

    #[test]
    fn parse_unsafe_flag_sets_unsafe_mode_true() {
        let cli = Cli::parse_from(["orchestrator", "--unsafe", "task", "list"]);
        assert!(
            cli.unsafe_mode,
            "--unsafe flag should set unsafe_mode to true"
        );
    }

    #[test]
    fn parse_default_unsafe_mode_is_false() {
        let cli = Cli::parse_from(["orchestrator", "task", "list"]);
        assert!(!cli.unsafe_mode, "unsafe_mode should default to false");
    }

    #[test]
    fn parse_unsafe_flag_works_after_subcommand() {
        let cli = Cli::parse_from(["orchestrator", "task", "--unsafe", "list"]);
        assert!(
            cli.unsafe_mode,
            "--unsafe global flag should be accepted after subcommand"
        );
    }
}
