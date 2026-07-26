use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A wall-clock timestamp expressed as milliseconds since the Unix epoch.
///
/// Persistent data and external APIs use this representation. Durations,
/// deadlines, and retry intervals must use `std::time::Duration` or Tokio's
/// monotonic clock instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixTimestampMillis(i64);

impl UnixTimestampMillis {
    pub fn now() -> Result<Self, TimestampError> {
        Self::from_system_time(SystemTime::now())
    }

    pub fn from_system_time(value: SystemTime) -> Result<Self, TimestampError> {
        let duration = value
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TimestampError::BeforeUnixEpoch)?;
        let milliseconds =
            i64::try_from(duration.as_millis()).map_err(|_| TimestampError::OutOfRange)?;
        Ok(Self(milliseconds))
    }

    pub const fn from_millis(value: i64) -> Result<Self, TimestampError> {
        if value < 0 {
            return Err(TimestampError::BeforeUnixEpoch);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn as_millis(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TimestampError {
    #[error("timestamp is before the Unix epoch")]
    BeforeUnixEpoch,
    #[error("timestamp cannot be represented as signed 64-bit milliseconds")]
    OutOfRange,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_negative_milliseconds() {
        assert_eq!(
            UnixTimestampMillis::from_millis(-1),
            Err(TimestampError::BeforeUnixEpoch)
        );
    }

    #[test]
    fn preserves_millisecond_value() -> Result<(), TimestampError> {
        let timestamp = UnixTimestampMillis::from_millis(1_700_000_000_123)?;
        assert_eq!(timestamp.as_millis(), 1_700_000_000_123);
        Ok(())
    }
}
