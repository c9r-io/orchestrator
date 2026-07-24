use orchestrator_integration_tests::TestHarness;
use orchestrator_proto::TaskListRequest;

use crate::{Commands, OutputFormat, TaskCommands, commands};

#[tokio::test]
async fn task_create_crosses_real_cli_grpc_adapter_and_preserves_parameters() {
    let harness = TestHarness::start().await;
    harness.seed_qa_file();
    let mut command_client = harness.client();

    commands::dispatch(
        &mut command_client,
        Commands::Task(TaskCommands::Create {
            name: Some("CLI adapter fixture".into()),
            goal: Some("Preserve mutation parameters across tonic".into()),
            project: Some("default".into()),
            workspace: Some("default".into()),
            workflow: Some("basic".into()),
            target_file: Vec::new(),
            no_start: true,
            step: Vec::new(),
            set: Vec::new(),
        }),
    )
    .await
    .expect("CLI dispatch succeeds");

    let mut observer = harness.client();
    let tasks = observer
        .task_list(TaskListRequest {
            status_filter: Some("created".into()),
            project_filter: Some("default".into()),
        })
        .await
        .expect("observe created task")
        .into_inner()
        .tasks;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "CLI adapter fixture");
    assert_eq!(tasks[0].goal, "Preserve mutation parameters across tonic");
    assert_eq!(tasks[0].workspace_id, "default");
    assert_eq!(tasks[0].workflow_id, "basic");
}

#[tokio::test]
async fn real_cli_grpc_adapter_propagates_status_errors() {
    let harness = TestHarness::start().await;
    let mut client = harness.client();
    let error = commands::dispatch(
        &mut client,
        Commands::Task(TaskCommands::Info {
            task_id: "missing-task".into(),
            output: OutputFormat::Json,
        }),
    )
    .await
    .expect_err("missing task must fail");
    let status = error
        .downcast_ref::<tonic::Status>()
        .expect("tonic status is preserved");
    assert_eq!(status.code(), tonic::Code::NotFound);
}
