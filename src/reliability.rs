use std::time::Duration;

pub const IO_TIMEOUT_MS: u64 = 800;
pub const RETRY_ATTEMPTS: usize = 3;
pub const BACKOFF_BASE_MS: u64 = 50;

pub fn backoff_delay(attempt: usize) -> Duration {
    Duration::from_millis(BACKOFF_BASE_MS * (1u64 << attempt.min(6)))
}
