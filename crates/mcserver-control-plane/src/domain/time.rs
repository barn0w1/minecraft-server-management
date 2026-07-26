use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnixTimestampMillis(i64);

impl UnixTimestampMillis {
    pub fn from_millis(value: i64) -> Result<Self, TimestampError> {
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

pub trait Clock: Send + Sync {
    fn now(&self) -> Result<UnixTimestampMillis, TimestampError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Result<UnixTimestampMillis, TimestampError> {
        let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let milliseconds =
            i64::try_from(elapsed.as_millis()).map_err(|_| TimestampError::OutOfRange)?;
        UnixTimestampMillis::from_millis(milliseconds)
    }
}

#[derive(Debug, Error)]
pub enum TimestampError {
    #[error("system clock is before the Unix epoch")]
    BeforeUnixEpoch,
    #[error("Unix timestamp is outside the supported signed 64-bit millisecond range")]
    OutOfRange,
    #[error("system clock operation failed")]
    SystemTime(#[from] SystemTimeError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_negative_timestamp() {
        assert!(matches!(
            UnixTimestampMillis::from_millis(-1),
            Err(TimestampError::BeforeUnixEpoch)
        ));
    }
}
