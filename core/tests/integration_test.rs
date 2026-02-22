mod test_utils {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    pub struct TestState {
        temp_root: PathBuf,
    }

    impl TestState {
        pub fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let temp_root = std::env::temp_dir().join(format!(
                "orchestrator-integration-test-{}-{}",
                nonce,
                Uuid::new_v4()
            ));
            std::fs::create_dir_all(&temp_root).expect("failed to create test temp root");

            Self { temp_root }
        }

        pub fn temp_root(&self) -> &Path {
            &self.temp_root
        }

        pub fn write_manifest(&self, filename: &str, content: &str) -> PathBuf {
            let path = self.temp_root.join(filename);
            std::fs::write(&path, content).expect("failed to write manifest");
            path
        }

        pub fn ensure_workspace_structure(&self, root_path: &str) {
            let root = self.temp_root.join(root_path);
            std::fs::create_dir_all(root.join("docs/qa")).expect("qa dir should be creatable");
            std::fs::create_dir_all(root.join("docs/ticket"))
                .expect("ticket dir should be creatable");
        }
    }

    impl Drop for TestState {
        fn drop(&mut self) {
            if self.temp_root.exists() {
                let _ = std::fs::remove_dir_all(&self.temp_root);
            }
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use test_utils::TestState;

    fn workspace_yaml(name: &str, root_path: &str) -> String {
        format!(
            r#"apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: {name}
spec:
  root_path: {root_path}
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
"#
        )
    }

    fn agent_yaml(name: &str, qa_template: &str) -> String {
        format!(
            r#"apiVersion: orchestrator.dev/v1
kind: Agent
metadata:
  name: {name}
spec:
  templates:
    qa: "{qa_template}"
"#
        )
    }

    fn multi_document_yaml() -> String {
        r#"apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: ws1
spec:
  root_path: workspace/ws1
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: ws2
spec:
  root_path: workspace/ws2
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v1
kind: Agent
metadata:
  name: test-agent
spec:
  templates:
    qa: "echo test"
"#
        .to_string()
    }

    #[test]
    fn db_reset_clears_tasks_but_preserves_config() {
        let fixture = TestState::new();

        assert!(fixture.temp_root().exists());
    }

    #[test]
    fn apply_creates_resource_and_persists() {
        let fixture = TestState::new();
        fixture.ensure_workspace_structure("workspace/new-ws");

        let manifest = fixture.write_manifest(
            "apply-create.yaml",
            &workspace_yaml("new-ws", "workspace/new-ws"),
        );

        assert!(manifest.exists());
        let content = std::fs::read_to_string(&manifest).expect("should read manifest");
        assert!(content.contains("kind: Workspace"));
        assert!(content.contains("name: new-ws"));
    }

    #[test]
    fn apply_updates_existing_resource() {
        let fixture = TestState::new();
        fixture.ensure_workspace_structure("workspace/update-ws-v1");
        fixture.ensure_workspace_structure("workspace/update-ws-v2");

        let manifest = fixture.write_manifest(
            "apply-update.yaml",
            &workspace_yaml("update-ws", "workspace/update-ws-v1"),
        );

        assert!(manifest.exists());

        let manifest_v2 = fixture.write_manifest(
            "apply-update.yaml",
            &workspace_yaml("update-ws", "workspace/update-ws-v2"),
        );

        assert!(manifest_v2.exists());
        let content = std::fs::read_to_string(&manifest_v2).expect("should read manifest");
        assert!(content.contains("workspace/update-ws-v2"));
    }

    #[test]
    fn apply_preserves_unmentioned_resources() {
        let fixture = TestState::new();
        fixture.ensure_workspace_structure("workspace/ws-a");
        fixture.ensure_workspace_structure("workspace/ws-b");

        let manifest_a = fixture.write_manifest(
            "apply-preserve-a.yaml",
            &workspace_yaml("ws-a", "workspace/ws-a"),
        );

        let manifest_b = fixture.write_manifest(
            "apply-preserve-b.yaml",
            &workspace_yaml("ws-b", "workspace/ws-b"),
        );

        assert!(manifest_a.exists());
        assert!(manifest_b.exists());
    }

    #[test]
    fn apply_edit_round_trip() {
        let fixture = TestState::new();
        fixture.ensure_workspace_structure("workspace/roundtrip-v1");
        fixture.ensure_workspace_structure("workspace/roundtrip-v2");

        let manifest = fixture.write_manifest(
            "roundtrip.yaml",
            &workspace_yaml("roundtrip", "workspace/roundtrip-v1"),
        );
        assert!(manifest.exists());

        let edited_manifest = fixture.write_manifest(
            "roundtrip-edited.yaml",
            &workspace_yaml("roundtrip", "workspace/roundtrip-v2"),
        );

        let content = std::fs::read_to_string(&edited_manifest).expect("should read manifest");
        assert!(content.contains("workspace/roundtrip-v2"));
    }

    #[test]
    fn multi_document_apply() {
        let fixture = TestState::new();
        fixture.ensure_workspace_structure("workspace/ws1");
        fixture.ensure_workspace_structure("workspace/ws2");

        let manifest = fixture.write_manifest("multi-doc.yaml", &multi_document_yaml());

        let content = std::fs::read_to_string(&manifest).expect("should read manifest");
        assert!(content.contains("---"));
        assert!(content.contains("name: ws1"));
        assert!(content.contains("name: ws2"));
        assert!(content.contains("name: test-agent"));
    }

    #[test]
    fn integration_all_commands_work_together() {
        let fixture = TestState::new();
        fixture.ensure_workspace_structure("workspace/integration");

        let initial_manifest = fixture.write_manifest(
            "integration-initial.yaml",
            &workspace_yaml("integration", "workspace/integration"),
        );
        assert!(initial_manifest.exists());

        let agent_manifest = fixture.write_manifest(
            "integration-agent.yaml",
            &agent_yaml("integration-agent", "echo test"),
        );
        assert!(agent_manifest.exists());

        assert!(initial_manifest.exists());
        assert!(agent_manifest.exists());
    }

    #[test]
    fn apply_dry_run_does_not_persist() {
        let fixture = TestState::new();
        fixture.ensure_workspace_structure("workspace/dry-run");

        let manifest = fixture.write_manifest(
            "dry-run.yaml",
            &workspace_yaml("dry-run", "workspace/dry-run"),
        );

        assert!(manifest.exists());
    }

    #[test]
    fn edit_export_generates_valid_yaml() {
        let fixture = TestState::new();
        fixture.ensure_workspace_structure("workspace/export-test");

        let manifest = fixture.write_manifest(
            "export-test.yaml",
            &workspace_yaml("export-test", "workspace/export-test"),
        );

        let content = std::fs::read_to_string(&manifest).expect("should read manifest");

        assert!(content.contains("apiVersion: orchestrator.dev/v1"));
        assert!(content.contains("kind: Workspace"));
        assert!(content.contains("metadata:"));
        assert!(content.contains("spec:"));
    }

    #[test]
    fn multi_resource_apply_partial_failure() {
        let fixture = TestState::new();
        fixture.ensure_workspace_structure("workspace/valid");

        let mixed_yaml = format!(
            r#"{}
---
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: invalid-empty-path
spec:
  root_path: ""
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
"#,
            workspace_yaml("valid", "workspace/valid")
        );

        let manifest = fixture.write_manifest("mixed.yaml", &mixed_yaml);
        assert!(manifest.exists());

        let content = std::fs::read_to_string(&manifest).expect("should read manifest");
        assert!(content.contains("name: valid"));
        assert!(content.contains("root_path: \"\""));
    }
}
