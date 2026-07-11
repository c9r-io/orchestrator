use anyhow::{Result, bail};

use super::model::TIMELINE_PROJECTION_VERSION;

/// Decoded opaque pagination cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimelineCursor {
    pub(crate) snapshot_max_event_id: i64,
    pub(crate) source_order: u64,
    pub(crate) entry_id: String,
}

pub(crate) fn encode_cursor(cursor: &TimelineCursor) -> String {
    format!(
        "v{}.{:x}.{:x}.{}",
        TIMELINE_PROJECTION_VERSION,
        cursor.snapshot_max_event_id,
        cursor.source_order,
        cursor.entry_id
    )
}

pub(crate) fn decode_cursor(raw: &str) -> Result<TimelineCursor> {
    let mut parts = raw.splitn(4, '.');
    let version = parts.next().unwrap_or_default();
    let watermark = parts.next().unwrap_or_default();
    let order = parts.next().unwrap_or_default();
    let entry_id = parts.next().unwrap_or_default();
    if version != format!("v{TIMELINE_PROJECTION_VERSION}")
        || watermark.is_empty()
        || order.is_empty()
        || entry_id.is_empty()
    {
        bail!("invalid timeline cursor");
    }
    let snapshot_max_event_id = i64::from_str_radix(watermark, 16)
        .map_err(|_| anyhow::anyhow!("invalid timeline cursor watermark"))?;
    let source_order = u64::from_str_radix(order, 16)
        .map_err(|_| anyhow::anyhow!("invalid timeline cursor order"))?;
    if !entry_id.bytes().all(|value| value.is_ascii_hexdigit()) {
        bail!("invalid timeline cursor entry id");
    }
    Ok(TimelineCursor {
        snapshot_max_event_id,
        source_order,
        entry_id: entry_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trip() {
        let cursor = TimelineCursor {
            snapshot_max_event_id: 42,
            source_order: 123,
            entry_id: "abcdef0123".to_string(),
        };
        assert_eq!(decode_cursor(&encode_cursor(&cursor)).unwrap(), cursor);
    }

    #[test]
    fn invalid_cursor_is_rejected() {
        assert!(decode_cursor("bad").is_err());
        assert!(decode_cursor("v1.1.2.not-hex").is_err());
    }
}
