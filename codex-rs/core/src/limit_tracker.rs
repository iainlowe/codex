use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

/// Time in seconds after which we should attempt to switch back to ChatGPT auth
/// when a usage limit was previously reached.
const LIMIT_RETRY_DELAY: Duration = Duration::from_secs(5 * 60 * 60); // 5 hours

#[derive(Debug, Serialize, Deserialize)]
struct LimitState {
    /// Unix timestamp when the usage limit was first reached
    hit_at: u64,
}

/// Manages tracking of when ChatGPT usage limits were hit to enable
/// automatic fallback to API key and eventual retry of ChatGPT auth.
#[derive(Debug)]
pub struct LimitTracker {
    limit_file: PathBuf,
}

impl LimitTracker {
    /// Create a new LimitTracker for the given codex home directory.
    pub fn new(codex_home: &Path) -> Self {
        Self {
            limit_file: codex_home.join("limit"),
        }
    }

    /// Record that a usage limit was reached at the current time.
    pub fn record_limit_hit(&self) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get current time")?
            .as_secs();

        let state = LimitState { hit_at: now };
        let content = serde_json::to_string(&state).context("Failed to serialize limit state")?;

        fs::write(&self.limit_file, content).context("Failed to write limit file")?;

        Ok(())
    }

    /// Check if enough time has passed since the last recorded limit hit
    /// to attempt switching back to ChatGPT auth.
    pub fn should_retry_chatgpt(&self) -> bool {
        match self.read_limit_state() {
            Ok(Some(state)) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                now >= state.hit_at + LIMIT_RETRY_DELAY.as_secs()
            }
            _ => true, // If no limit recorded or error reading, allow retry
        }
    }

    /// Clear the recorded limit state, typically called after successfully
    /// switching back to ChatGPT auth.
    pub fn clear_limit(&self) -> Result<()> {
        if self.limit_file.exists() {
            fs::remove_file(&self.limit_file).context("Failed to remove limit file")?;
        }
        Ok(())
    }

    /// Check if there's a recorded limit that hasn't expired yet.
    pub fn has_active_limit(&self) -> bool {
        !self.should_retry_chatgpt()
    }

    fn read_limit_state(&self) -> Result<Option<LimitState>> {
        if !self.limit_file.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.limit_file).context("Failed to read limit file")?;

        let state: LimitState =
            serde_json::from_str(&content).context("Failed to parse limit file")?;

        Ok(Some(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_no_limit_recorded() {
        let dir = tempdir().unwrap();
        let tracker = LimitTracker::new(dir.path());

        assert!(tracker.should_retry_chatgpt());
        assert!(!tracker.has_active_limit());
    }

    #[test]
    fn test_record_and_check_recent_limit() {
        let dir = tempdir().unwrap();
        let tracker = LimitTracker::new(dir.path());

        tracker.record_limit_hit().unwrap();

        assert!(!tracker.should_retry_chatgpt());
        assert!(tracker.has_active_limit());
    }

    #[test]
    fn test_clear_limit() {
        let dir = tempdir().unwrap();
        let tracker = LimitTracker::new(dir.path());

        tracker.record_limit_hit().unwrap();
        assert!(tracker.has_active_limit());

        tracker.clear_limit().unwrap();
        assert!(!tracker.has_active_limit());
        assert!(tracker.should_retry_chatgpt());
    }

    #[test]
    fn test_old_limit_should_retry() {
        let dir = tempdir().unwrap();
        let tracker = LimitTracker::new(dir.path());

        // Manually create an old limit state (6 hours ago)
        let six_hours_ago = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - (6 * 60 * 60);

        let state = LimitState {
            hit_at: six_hours_ago,
        };
        let content = serde_json::to_string(&state).unwrap();
        fs::write(&tracker.limit_file, content).unwrap();

        assert!(tracker.should_retry_chatgpt());
        assert!(!tracker.has_active_limit());
    }
}
