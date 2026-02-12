use std::time::Duration;

use crate::types::{ModelId, Phase};

/// Errors from the consensus engine.
#[derive(Debug, thiserror::Error)]
pub enum ConvergeError {
    #[error("phase {phase} failed for model {model}: {source}")]
    PhaseFailure {
        phase: Phase,
        model: ModelId,
        source: ProviderError,
    },

    #[error("insufficient models in round {round}: {remaining} remaining, {minimum} required")]
    InsufficientModels {
        round: u32,
        remaining: usize,
        minimum: usize,
    },

    #[error("invalid config: {field} = {value} ({constraint})")]
    ConfigInvalid {
        field: &'static str,
        value: String,
        constraint: String,
    },

    #[error("consensus run cancelled")]
    Cancelled,
}

/// Errors from individual provider backends.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProviderError {
    #[error("model {model} timed out after {elapsed:?}")]
    Timeout { model: ModelId, elapsed: Duration },

    #[error("model {model} returned invalid JSON: {message}")]
    InvalidJson { model: ModelId, message: String },

    #[error("model {model} process failed: {message} (exit code: {exit_code:?})")]
    ProcessFailed {
        model: ModelId,
        message: String,
        exit_code: Option<i32>,
    },

    #[error("model {model} response too large: {size} bytes (max: {max})")]
    ResponseTooLarge {
        model: ModelId,
        size: usize,
        max: usize,
    },

    #[error("model {model} JSON nesting too deep: {depth} levels (max: {max})")]
    JsonTooDeep {
        model: ModelId,
        depth: usize,
        max: usize,
    },

    #[error("missing credential: {var_name} not set for {provider}")]
    MissingCredential { provider: String, var_name: String },

    #[error("CLI binary not found: {binary_name}")]
    BinaryNotFound { binary_name: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_error_display() {
        let err = ProviderError::Timeout {
            model: ModelId::new("claude"),
            elapsed: Duration::from_secs(120),
        };
        assert!(err.to_string().contains("claude"));
        assert!(err.to_string().contains("120"));
    }

    #[test]
    fn converge_error_display() {
        let err = ConvergeError::InsufficientModels {
            round: 3,
            remaining: 1,
            minimum: 2,
        };
        let msg = err.to_string();
        assert!(msg.contains("round 3"));
        assert!(msg.contains("1 remaining"));
        assert!(msg.contains("2 required"));
    }

    #[test]
    fn config_invalid_carries_structured_info() {
        let err = ConvergeError::ConfigInvalid {
            field: "max_rounds",
            value: "25".to_string(),
            constraint: "must be 1-20".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("max_rounds"));
        assert!(msg.contains("25"));
        assert!(msg.contains("must be 1-20"));
    }
}
