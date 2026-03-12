//! Integration tests for agent lifecycle: cordon → drain → uncordon.

mod common;

use orchestrator_integration_tests::TestHarness;
use orchestrator_proto::*;
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::test]
async fn agent_cordon_drain_uncordon() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("echo-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        let mut client = harness.client();

        // List agents — echo should be Active
        let list_resp = client
            .agent_list(AgentListRequest {
                project_id: None,
            })
            .await
            .expect("agent_list failed")
            .into_inner();

        let echo_agent = list_resp
            .agents
            .iter()
            .find(|a| a.name == "echo")
            .expect("echo agent not found");
        assert_eq!(echo_agent.lifecycle_state, "Active");

        // Cordon
        client
            .agent_cordon(AgentCordonRequest {
                agent_name: "echo".into(),
                project_id: None,
            })
            .await
            .expect("agent_cordon failed");

        let list_resp = client
            .agent_list(AgentListRequest {
                project_id: None,
            })
            .await
            .expect("agent_list after cordon failed")
            .into_inner();
        let echo_agent = list_resp
            .agents
            .iter()
            .find(|a| a.name == "echo")
            .expect("echo agent not found after cordon");
        assert_eq!(echo_agent.lifecycle_state, "Cordoned");

        // Drain (no in-flight items, should go straight to Drained)
        let drain_resp = client
            .agent_drain(AgentDrainRequest {
                agent_name: "echo".into(),
                timeout_secs: Some(5),
                project_id: None,
            })
            .await
            .expect("agent_drain failed")
            .into_inner();
        assert_eq!(drain_resp.lifecycle_state, "Drained");

        // Uncordon
        client
            .agent_uncordon(AgentUncordonRequest {
                agent_name: "echo".into(),
                project_id: None,
            })
            .await
            .expect("agent_uncordon failed");

        let list_resp = client
            .agent_list(AgentListRequest {
                project_id: None,
            })
            .await
            .expect("agent_list after uncordon failed")
            .into_inner();
        let echo_agent = list_resp
            .agents
            .iter()
            .find(|a| a.name == "echo")
            .expect("echo agent not found after uncordon");
        assert_eq!(echo_agent.lifecycle_state, "Active");
    })
    .await
    .expect("test timed out");
}
