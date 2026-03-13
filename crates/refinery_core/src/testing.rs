use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::ModelProvider;
use crate::error::ProviderError;
use crate::strategy::{ClosingDecision, ClosingStrategy};
use crate::types::{Message, ModelId, RoundData};

/// A mock provider that returns fixed text responses.
#[derive(Debug)]
pub struct EchoProvider {
    model_id: ModelId,
    responses: Arc<Mutex<VecDeque<String>>>,
    default_response: String,
}

impl EchoProvider {
    /// Create an echo provider that always returns a default text response.
    pub fn new(name: &str) -> Self {
        Self {
            model_id: ModelId::new(name),
            responses: Arc::new(Mutex::new(VecDeque::new())),
            default_response: format!("Echo response from {name}"),
        }
    }

    /// Create a provider whose default response is a valid evaluation JSON.
    ///
    /// This makes the provider work in evaluate phases without parse errors.
    pub fn with_json_eval(name: &str, score: u8) -> Self {
        let json_response = format!(
            r#"```json
{{
  "strengths": ["good"],
  "weaknesses": ["could be better"],
  "suggestions": ["improve"],
  "overall_assessment": "Decent answer.",
  "rationale": "Solid but room for improvement.",
  "score": {score}
}}
```"#
        );
        Self {
            model_id: ModelId::new(name),
            responses: Arc::new(Mutex::new(VecDeque::new())),
            default_response: json_response,
        }
    }

    /// Queue a specific response (FIFO). Falls back to default when queue is empty.
    pub fn queue_response(&self, response: String) {
        self.responses.lock().unwrap().push_back(response);
    }
}

#[async_trait]
impl ModelProvider for EchoProvider {
    async fn send_message(&self, _messages: &[Message]) -> Result<String, ProviderError> {
        let mut queue = self.responses.lock().unwrap();
        Ok(queue
            .pop_front()
            .unwrap_or_else(|| self.default_response.clone()))
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }
}

/// A mock provider that always fails.
#[derive(Debug)]
pub struct FailingProvider {
    model_id: ModelId,
}

impl FailingProvider {
    pub fn new(name: &str) -> Self {
        Self {
            model_id: ModelId::new(name),
        }
    }
}

#[async_trait]
impl ModelProvider for FailingProvider {
    async fn send_message(&self, _messages: &[Message]) -> Result<String, ProviderError> {
        Err(ProviderError::ProcessFailed {
            model: self.model_id.clone(),
            message: "mock failure".to_string(),
            exit_code: Some(1),
        })
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }
}

/// A mock provider that succeeds N times, then fails.
#[derive(Debug)]
pub struct FailAfterNProvider {
    model_id: ModelId,
    max_successes: usize,
    call_count: AtomicUsize,
}

impl FailAfterNProvider {
    pub fn new(name: &str, max_successes: usize) -> Self {
        Self {
            model_id: ModelId::new(name),
            max_successes,
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ModelProvider for FailAfterNProvider {
    async fn send_message(&self, _messages: &[Message]) -> Result<String, ProviderError> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        if count < self.max_successes {
            Ok(format!("Response {} from {}", count, self.model_id))
        } else {
            Err(ProviderError::ProcessFailed {
                model: self.model_id.clone(),
                message: format!("failed after {count} calls"),
                exit_code: Some(1),
            })
        }
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }
}

// --- Mock Strategies ---

/// A strategy that always says "refinery" starting from round N.
pub struct AlwaysConvergeAfterN {
    min_rounds: u32,
}

impl AlwaysConvergeAfterN {
    pub fn new(min_rounds: u32) -> Self {
        Self { min_rounds }
    }
}

#[async_trait]
impl ClosingStrategy for AlwaysConvergeAfterN {
    async fn check(&self, round_data: &RoundData) -> ClosingDecision {
        if round_data.round >= self.min_rounds {
            let winner = round_data
                .mean_scores
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| ModelId::from_parts("mock", "winner"));
            ClosingDecision::Converged {
                winner,
                explanation: "Mock convergence".to_string(),
            }
        } else {
            ClosingDecision::Continue
        }
    }

    fn name(&self) -> &'static str {
        "mock-converge-after-n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn echo_provider_returns_default() {
        let provider = EchoProvider::new("test/echo");
        let result = provider.send_message(&[]).await.unwrap();
        assert!(result.contains("Echo response from test/echo"));
    }

    #[tokio::test]
    async fn echo_provider_returns_queued() {
        let provider = EchoProvider::new("test/echo");
        provider.queue_response("first".to_string());
        provider.queue_response("second".to_string());

        assert_eq!(provider.send_message(&[]).await.unwrap(), "first");
        assert_eq!(provider.send_message(&[]).await.unwrap(), "second");
        assert!(provider.send_message(&[]).await.unwrap().contains("Echo"));
    }

    #[tokio::test]
    async fn failing_provider_always_fails() {
        let provider = FailingProvider::new("test/fail");
        assert!(provider.send_message(&[]).await.is_err());
    }

    #[tokio::test]
    async fn fail_after_n_succeeds_then_fails() {
        let provider = FailAfterNProvider::new("test/countdown", 2);
        assert!(provider.send_message(&[]).await.is_ok());
        assert!(provider.send_message(&[]).await.is_ok());
        assert!(provider.send_message(&[]).await.is_err());
    }

    #[tokio::test]
    async fn trait_object_safety() {
        let provider: Box<dyn ModelProvider> = Box::new(EchoProvider::new("test/echo"));
        assert_eq!(provider.model_id(), &ModelId::new("test/echo"));
        assert!(provider.send_message(&[]).await.is_ok());
    }
}
