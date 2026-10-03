//! Reproducible-build helpers.
//!
//! Honors the [`SOURCE_DATE_EPOCH`] convention: when the environment variable
//! is set to a Unix timestamp, generated metadata and tar entries use it
//! instead of the current wall-clock time. Without the variable the builder
//! keeps its historical behavior.
//!
//! [`SOURCE_DATE_EPOCH`]: https://reproducible-builds.org/specs/source-date-epoch/

use std::time::{SystemTime, UNIX_EPOCH};

/// Parse a `SOURCE_DATE_EPOCH` value.
///
/// Accepts a non-negative integer number of seconds since the Unix epoch.
/// Values that are not valid timestamps return `None`.
pub fn parse_source_date_epoch(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok()
}

/// Read `SOURCE_DATE_EPOCH` from the environment, if set and valid.
pub fn source_date_epoch() -> Option<u64> {
    match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(value) => parse_source_date_epoch(&value),
        Err(_) => None,
    }
}

/// Timestamp to embed in generated metadata and archive entries.
///
/// Uses `SOURCE_DATE_EPOCH` when set, falling back to the current time.
pub fn build_timestamp() -> u64 {
    source_date_epoch().unwrap_or_else(current_timestamp)
}

/// Current Unix timestamp in seconds.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_source_date_epoch_valid() {
        assert_eq!(parse_source_date_epoch("1700000000"), Some(1_700_000_000));
        assert_eq!(parse_source_date_epoch(" 42 "), Some(42));
        assert_eq!(parse_source_date_epoch("0"), Some(0));
    }

    #[test]
    fn test_parse_source_date_epoch_invalid() {
        assert_eq!(parse_source_date_epoch(""), None);
        assert_eq!(parse_source_date_epoch("yesterday"), None);
        assert_eq!(parse_source_date_epoch("-1"), None);
        assert_eq!(parse_source_date_epoch("1.5"), None);
    }
}
