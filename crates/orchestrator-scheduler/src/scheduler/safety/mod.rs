//! Safety subsystem: binary snapshots, git checkpoints, self-restart, and self-test.
//!
//! This module provides the safety mechanisms used during self-evolution cycles:
//! - **snapshot**: Binary snapshot lifecycle (create, verify, restore)
//! - **checkpoint**: Git tag-based checkpoint and rollback
//! - **restart**: Self-restart orchestration with post-restart verification
//! - **self_test**: Self-test step execution (cargo check/test + manifest validate)

mod checkpoint;
mod restart;
mod self_test;
mod snapshot;

pub use checkpoint::{create_checkpoint, rollback_to_checkpoint};
pub use restart::{
    EXIT_RESTART, RestartRequestedError, SelfRestartOutcome, execute_self_restart_step,
    verify_post_restart_binary,
};
pub use self_test::{SelfTestResult, execute_self_test_step};
pub use snapshot::{
    BinaryVerificationResult, SnapshotManifest, restore_binary_snapshot, snapshot_binary,
    verify_binary_snapshot,
};

#[cfg(test)]
mod tests;
