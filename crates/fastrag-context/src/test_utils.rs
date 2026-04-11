//! Test doubles for the fastrag-context crate. Available under
//! `#[cfg(test)]` within the crate and under the `test-utils` feature for
//! external integration tests.

use std::sync::Mutex;

use crate::ContextError;
use crate::contextualizer::{Contextualizer, ContextualizerMeta};

/// A deterministic contextualizer suitable for tests. It can be configured to
/// succeed (returning a deterministic context string) or to fail with a given
/// [`ContextError`] every `fail_every` calls.
pub struct MockContextualizer {
    model_id: String,
    prompt_version: u32,
    fail_every: usize,
    panic_on_call: bool,
    counter: Mutex<usize>,
}

impl MockContextualizer {
    /// Build a mock that always succeeds.
    pub fn always_ok() -> Self {
        Self {
            model_id: "mock-ok".to_string(),
            prompt_version: 1,
            fail_every: 0,
            panic_on_call: false,
            counter: Mutex::new(0),
        }
    }

    /// Build a mock that fails on every Nth call (1-indexed). `fail_every = 3`
    /// means calls 3, 6, 9, ... fail.
    pub fn fail_every(n: usize) -> Self {
        Self {
            model_id: "mock-flaky".to_string(),
            prompt_version: 1,
            fail_every: n,
            panic_on_call: false,
            counter: Mutex::new(0),
        }
    }

    /// Build a mock that panics on any call. Useful for asserting that a
    /// cache hit short-circuits the stage before the contextualizer is
    /// invoked.
    pub fn panicking() -> Self {
        Self {
            model_id: "mock".to_string(),
            prompt_version: 1,
            fail_every: 0,
            panic_on_call: true,
            counter: Mutex::new(0),
        }
    }
}

impl ContextualizerMeta for MockContextualizer {
    fn model_id(&self) -> &str {
        &self.model_id
    }
    fn prompt_version(&self) -> u32 {
        self.prompt_version
    }
}

impl Contextualizer for MockContextualizer {
    fn contextualize(&self, doc_title: &str, raw_chunk: &str) -> Result<String, ContextError> {
        if self.panic_on_call {
            panic!("MockContextualizer::panicking was called unexpectedly");
        }
        let mut count = self.counter.lock().expect("mock counter poisoned");
        *count += 1;
        if self.fail_every > 0 && (*count).is_multiple_of(self.fail_every) {
            return Err(ContextError::EmptyCompletion);
        }
        // Deterministic, short, clearly synthetic context for assertions.
        Ok(format!(
            "ctx({doc_title}): {}",
            raw_chunk.chars().take(16).collect::<String>()
        ))
    }
}
