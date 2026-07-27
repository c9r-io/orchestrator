//! Agent session rows: creation, state transitions, and reader/writer attachment.
//!
//! Moved to `orchestrator-persistence` (FR-130 Phase A) and re-exported here.
//! The tests moved into the layer with FR-141, for the reason Phase A gave
//! for leaving them: they open connections through `crate::db`, the admin
//! facade, "which is core-side until this phase's last commit". It is not.

pub use orchestrator_persistence::session_store::*;
