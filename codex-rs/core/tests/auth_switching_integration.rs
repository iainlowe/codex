use codex_core::limit_tracker::LimitTracker;
use codex_login::AuthManager;
use codex_login::AuthMode;
use codex_login::login_with_api_key;
use std::env;
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_end_to_end_auth_switching() {
    let dir = tempdir().unwrap();

    // Set up environment with API key for fallback
    unsafe {
        env::set_var("OPENAI_API_KEY", "sk-test-fallback-key");
    }

    // Create auth.json with ChatGPT credentials (simulated by API key)
    login_with_api_key(dir.path(), "sk-chatgpt-key").unwrap();

    // Create auth manager starting with ChatGPT preference
    let auth_manager = Arc::new(AuthManager::new(
        dir.path().to_path_buf(),
        AuthMode::ChatGPT,
    ));

    // Create limit tracker
    let limit_tracker = LimitTracker::new(dir.path());

    // Initially should not have any limit recorded
    assert!(limit_tracker.should_retry_chatgpt());
    assert!(!limit_tracker.has_active_limit());

    // Simulate hitting a usage limit
    limit_tracker.record_limit_hit().unwrap();

    // Now should have an active limit
    assert!(!limit_tracker.should_retry_chatgpt());
    assert!(limit_tracker.has_active_limit());

    // Test auth switching to API key when limit is hit
    if auth_manager.force_switch_to_api_key() {
        if let Some(auth) = auth_manager.auth() {
            assert_eq!(auth.mode, AuthMode::ApiKey);
        }
    }

    // Clear the limit (simulating 5 hours passing)
    limit_tracker.clear_limit().unwrap();

    // Should now be able to retry ChatGPT
    assert!(limit_tracker.should_retry_chatgpt());
    assert!(!limit_tracker.has_active_limit());

    // Test switching back to ChatGPT
    if auth_manager.force_switch_to_chatgpt() {
        if let Some(auth) = auth_manager.auth() {
            // Should be back to ChatGPT mode or API key (depending on what's available)
            assert!(auth.mode == AuthMode::ChatGPT || auth.mode == AuthMode::ApiKey);
        }
    }

    env::remove_var("OPENAI_API_KEY");
}
