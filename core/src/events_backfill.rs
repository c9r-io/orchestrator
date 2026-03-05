use anyhow::Result;
use std::path::Path;

pub struct BackfillStats {
    pub scanned: u64,
    pub updated: u64,
    pub skipped: u64,
}

/// No-op: step_scope backfill logic has been moved to schema migration m0002.
/// This function is retained for compilation compatibility with the `config backfill-events` CLI.
pub fn backfill_event_step_scope(_db_path: &Path) -> Result<BackfillStats> {
    Ok(BackfillStats {
        scanned: 0,
        updated: 0,
        skipped: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn backfill_is_noop_and_returns_zero_stats() {
        // backfill_event_step_scope is now a no-op — logic moved to migration m0002.
        // It should always return zero stats regardless of input.
        let dummy_path = PathBuf::from("/nonexistent/db");
        let stats = backfill_event_step_scope(&dummy_path).expect("noop backfill");
        assert_eq!(stats.scanned, 0);
        assert_eq!(stats.updated, 0);
        assert_eq!(stats.skipped, 0);
    }
}
