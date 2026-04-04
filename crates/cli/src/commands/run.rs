use anyhow::{Result, bail};
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

/// FR-090: Dispatch the `orchestrator run` command.
///
/// In workflow mode: creates a task with optional step filter and follows logs.
/// In direct assembly mode (--template): creates a single-step ephemeral task.
/// With --detach: creates the task and returns immediately.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    workflow: Option<String>,
    step: Vec<String>,
    set: Vec<(String, String)>,
    project: Option<String>,
    workspace: Option<String>,
    target_file: Vec<String>,
    detach: bool,
    template: Option<String>,
    agent_capability: Option<String>,
    profile: Option<String>,
) -> Result<()> {
    let initial_vars: std::collections::HashMap<String, String> = set.into_iter().collect();

    // Phase 3: Direct assembly mode (no workflow)
    let task_id = if let Some(ref tmpl) = template {
        let cap = agent_capability
            .as_deref()
            .unwrap_or(tmpl.as_str())
            .to_string();
        let resp = client
            .run_step(orchestrator_proto::RunStepRequest {
                project_id: project,
                workspace_id: workspace,
                template: tmpl.clone(),
                agent_capability: cap,
                execution_profile: profile,
                initial_vars,
                target_files: target_file,
                no_start: false,
            })
            .await?
            .into_inner();

        if detach {
            println!("{}", resp.message);
            return Ok(());
        }
        resp.task_id
    } else if workflow.is_some() {
        // Phase 1/2: Workflow mode with optional step filter
        let resp = client
            .task_create(orchestrator_proto::TaskCreateRequest {
                name: None,
                goal: None,
                project_id: project,
                workspace_id: workspace,
                workflow_id: workflow,
                target_files: target_file,
                no_start: false,
                step_filter: step,
                initial_vars,
            })
            .await?
            .into_inner();

        if detach {
            println!("{}", resp.message);
            return Ok(());
        }
        resp.task_id
    } else {
        bail!("either --workflow or --template is required for `orchestrator run`");
    };

    // Synchronous mode: follow task logs until completion
    eprintln!("Following task {task_id} ...");

    // Stream live logs via TaskFollow
    let mut follow_stream = client
        .task_follow(orchestrator_proto::TaskFollowRequest {
            task_id: task_id.clone(),
        })
        .await?
        .into_inner();

    while let Some(line) = follow_stream.message().await? {
        print!("{}", line.line);
    }

    // Fetch final status
    let info = client
        .task_info(orchestrator_proto::TaskInfoRequest {
            task_id: task_id.clone(),
        })
        .await?
        .into_inner();

    let task_status = info
        .task
        .as_ref()
        .map(|t| t.status.as_str())
        .unwrap_or("unknown");
    let exit_code = if task_status == "completed" { 0 } else { 1 };
    eprintln!("\nTask {} finished with status: {}", task_id, task_status);
    std::process::exit(exit_code);
}
