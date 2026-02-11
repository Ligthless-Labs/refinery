pub mod engine;
pub mod error;
pub mod phases;
pub mod prompts;
pub mod strategy;
pub mod types;

pub use engine::{Engine, Session};
pub use error::{ConvergeError, ProviderError};
pub use strategy::{ClosingDecision, ClosingStrategy, VoteThreshold};
pub use types::{
    ConsensusOutcome, ConvergenceStatus, CostEstimate, EngineConfig, Message, ModelAnswer, ModelId,
    Role, RoundOutcome, RoundOverrides,
};

use async_trait::async_trait;

/// A model provider that can send messages and receive responses.
#[async_trait]
pub trait ModelProvider: Send + Sync + std::fmt::Debug {
    /// Send a sequence of messages and return the model's text response.
    async fn send_message(&self, messages: &[Message]) -> Result<String, ProviderError>;

    /// The unique identifier for this model.
    fn model_id(&self) -> &ModelId;
}

/// Testing utilities for mock providers and strategies.
#[cfg(any(test, feature = "testing"))]
pub mod testing;
