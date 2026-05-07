//! Count successful OpenAI HTTP responses (2xx) for visibility during indexing and queries.

use std::sync::atomic::{AtomicU64, Ordering};

static OPENAI_OK: AtomicU64 = AtomicU64::new(0);

/// Log each successful OpenAI API response (one line per HTTP round-trip).
pub fn log_openai_success(operation: &'static str) {
    if !debug_openai_ok_enabled() {
        return;
    }
    let n = OPENAI_OK.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("ctx: openai 200 #{} ({})", n, operation);
}

fn debug_openai_ok_enabled() -> bool {
    std::env::var("CTX_DEBUG_OPENAI_OK")
        .ok()
        .map(|value| {
            let value = value.trim();
            !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
        })
        .unwrap_or(false)
}
