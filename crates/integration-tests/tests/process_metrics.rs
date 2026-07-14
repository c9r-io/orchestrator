use std::collections::HashMap;

use orchestrator_integration_tests::TestHarness;
use orchestrator_proto::{
    ProcessMetricRecordRequest, ProcessMetricsGetRequest, ProcessMetricsRebuildRequest,
};
use tonic::Code;

#[tokio::test]
async fn process_metrics_rpc_is_project_scoped_bounded_and_rebuildable() {
    let harness = TestHarness::start().await;
    let mut client = harness.client();
    let inserted = client
        .process_metric_record(ProcessMetricRecordRequest {
            project_id: "default".into(),
            metric_name: "stream_reconnect_total".into(),
            dimensions: HashMap::from([
                ("page".into(), "attention".into()),
                ("result".into(), "error".into()),
            ]),
            value: 1.0,
            source_key: "integration-reconnect-1".into(),
        })
        .await
        .expect("record metric")
        .into_inner();
    assert!(inserted.inserted);

    let response = client
        .process_metrics_get(ProcessMetricsGetRequest {
            project_id: "default".into(),
            window: "24h".into(),
            bucket: "1h".into(),
        })
        .await
        .expect("query metrics")
        .into_inner();
    assert_eq!(response.schema_version, 1);
    let value: serde_json::Value =
        serde_json::from_str(&response.metrics_json).expect("metrics json");
    assert_eq!(value["project_id"], "default");
    assert!(value["metrics"].as_array().is_some_and(|metrics| {
        metrics
            .iter()
            .any(|metric| metric["name"] == "stream_reconnect_total")
    }));

    let invalid = client
        .process_metrics_get(ProcessMetricsGetRequest {
            project_id: "default".into(),
            window: "31d".into(),
            bucket: "1h".into(),
        })
        .await
        .expect_err("unbounded window must fail");
    assert_eq!(invalid.code(), Code::InvalidArgument);

    let rebuilt = client
        .process_metrics_rebuild(ProcessMetricsRebuildRequest {
            project_id: "default".into(),
        })
        .await
        .expect("rebuild metrics")
        .into_inner();
    assert_eq!(rebuilt.affected_rows, 1);
}
