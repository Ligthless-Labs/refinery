use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::ProviderError;
use crate::strategy::ClosingDecision;

/// Per-round trajectory entry: the model's own proposal and reviews received as `(label, text)`.
pub type RoundHistory = Vec<(String, Vec<(String, String)>)>;

/// Unique identifier for a model participating in consensus.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelId(String);

impl ModelId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Role in a conversation message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

/// A single message in a conversation.
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// A score in the range 1-10 (inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score(u8);

impl Score {
    /// Create a new score, returning an error if the value is outside 1-10.
    pub fn new(value: u8) -> Result<Self, ScoreError> {
        if (1..=10).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ScoreError::OutOfRange { value })
        }
    }

    #[must_use]
    pub fn value(self) -> u8 {
        self.0
    }
}

/// Error for invalid score values.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ScoreError {
    #[error("score {value} is out of range (must be 1-10)")]
    OutOfRange { value: u8 },
}

/// A qualitative review of another model's answer.
#[derive(Debug, Clone)]
pub struct Review {
    pub strengths: Vec<String>,
    pub weaknesses: Vec<String>,
    pub suggestions: Vec<String>,
    pub overall_assessment: String,
}

/// A combined evaluation: qualitative review + numeric score.
#[derive(Debug, Clone)]
pub struct Evaluation {
    pub review: Review,
    pub score: Score,
    pub rationale: String,
}

/// Output of the PROPOSE phase.
#[derive(Debug, Clone)]
pub struct ProposalSet {
    pub proposals: HashMap<ModelId, String>,
    pub dropped: Vec<(ModelId, ProviderError)>,
}

/// Output of the EVALUATE phase (merged review + score).
#[derive(Debug, Clone)]
pub struct EvaluationSet {
    /// (evaluator, evaluatee) -> evaluation
    pub evaluations: HashMap<(ModelId, ModelId), Evaluation>,
    pub dropped: Vec<(ModelId, ModelId, ProviderError)>,
}

/// The complete output of one round, returned by `Session::next_round()`.
#[derive(Debug, Clone)]
pub struct RoundOutcome {
    pub round: u32,
    pub proposals: ProposalSet,
    pub evaluations: EvaluationSet,
    pub closing_decision: ClosingDecision,
    pub elapsed: Duration,
    pub call_count: u32,
}

/// Read-only view of round data for closing strategies.
#[derive(Debug)]
pub struct RoundData {
    /// Current round number (1-indexed).
    pub round: u32,
    /// Mean scores per model from the current round (self-scores excluded).
    pub mean_scores: HashMap<ModelId, f64>,
    /// The model that had the highest mean score in the previous round, if any.
    pub previous_winner: Option<ModelId>,
    /// How many consecutive rounds the current leader has been on top.
    pub stable_rounds: u32,
}

/// Phases of the consensus loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Propose,
    Evaluate,
    Close,
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Propose => write!(f, "propose"),
            Self::Evaluate => write!(f, "evaluate"),
            Self::Close => write!(f, "close"),
        }
    }
}

/// Status of the consensus outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConvergenceStatus {
    /// Closing strategy reported convergence.
    Converged,
    /// Hit `max_rounds` without convergence; best answer returned.
    MaxRoundsExceeded,
    /// N=1 short-circuit; no consensus loop ran.
    SingleModel,
    /// Fell below N=2 mid-run; best available returned.
    InsufficientModels,
    /// Run was cancelled via `Session::cancel()`.
    Cancelled,
}

/// The final output of a consensus run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusOutcome {
    pub status: ConvergenceStatus,
    pub winner: ModelId,
    pub answer: String,
    pub final_round: u32,
    pub all_answers: Vec<ModelAnswer>,
    pub total_calls: u32,
    pub elapsed: Duration,
}

/// A single model's final answer with scores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAnswer {
    pub model_id: ModelId,
    pub answer: String,
    pub mean_score: f64,
}

/// Configuration for the consensus engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub models: Vec<ModelId>,
    pub max_rounds: u32,
    pub threshold: f64,
    pub stability_rounds: u32,
    pub timeout: Duration,
    pub max_concurrent: usize,
}

impl EngineConfig {
    pub fn new(
        models: Vec<ModelId>,
        max_rounds: u32,
        threshold: f64,
        stability_rounds: u32,
        timeout: Duration,
        max_concurrent: usize,
    ) -> Result<Self, crate::ConvergeError> {
        let n = models.len();
        if n == 0 || n > 7 {
            return Err(crate::ConvergeError::ConfigInvalid {
                field: "models",
                value: n.to_string(),
                constraint: "must have 1-7 models".to_string(),
            });
        }
        if !(1..=20).contains(&max_rounds) {
            return Err(crate::ConvergeError::ConfigInvalid {
                field: "max_rounds",
                value: max_rounds.to_string(),
                constraint: "must be 1-20".to_string(),
            });
        }
        if !(1.0..=10.0).contains(&threshold) {
            return Err(crate::ConvergeError::ConfigInvalid {
                field: "threshold",
                value: threshold.to_string(),
                constraint: "must be 1.0-10.0".to_string(),
            });
        }
        Ok(Self {
            models,
            max_rounds,
            threshold,
            stability_rounds,
            timeout,
            max_concurrent,
        })
    }

    /// Estimate total API calls for a full run.
    #[must_use]
    pub fn estimate_calls_per_round(&self) -> u32 {
        let n = u32::try_from(self.models.len()).expect("model count fits in u32");
        if n == 1 {
            return 1; // single-model short-circuit: 1 PROPOSE call, no loop
        }
        // N (propose) + N*(N-1) (evaluate) = N²
        n * n
    }
}

/// Optional overrides for inter-round agent intervention.
#[derive(Debug, Clone, Default)]
pub struct RoundOverrides {
    /// Additional context injected into all prompts this round.
    pub additional_context: Option<String>,
    /// Models to exclude from this round.
    pub drop_models: Vec<ModelId>,
}

/// Cost estimate returned by `Engine::estimate()`.
#[derive(Debug, Clone, Serialize)]
pub struct CostEstimate {
    pub calls_per_round: u32,
    pub total_calls: u32,
    pub model_count: usize,
    pub max_rounds: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_new_valid_range() {
        assert!(Score::new(0).is_err());
        assert!(Score::new(1).is_ok());
        assert_eq!(Score::new(1).unwrap().value(), 1);
        assert!(Score::new(10).is_ok());
        assert_eq!(Score::new(10).unwrap().value(), 10);
        assert!(Score::new(11).is_err());
    }

    #[test]
    fn model_id_as_hashmap_key() {
        let mut map = HashMap::new();
        let id = ModelId::new("claude");
        map.insert(id.clone(), "hello");
        assert_eq!(map.get(&id), Some(&"hello"));
    }

    #[test]
    fn engine_config_validation() {
        let models = vec![ModelId::new("a"), ModelId::new("b")];

        // Valid config
        let config = EngineConfig::new(models.clone(), 5, 8.0, 2, Duration::from_secs(120), 10);
        assert!(config.is_ok());

        // Too many models
        let too_many: Vec<_> = (0..8).map(|i| ModelId::new(format!("m{i}"))).collect();
        assert!(EngineConfig::new(too_many, 5, 8.0, 2, Duration::from_secs(120), 10).is_err());

        // No models
        assert!(EngineConfig::new(vec![], 5, 8.0, 2, Duration::from_secs(120), 10).is_err());

        // max_rounds out of range
        assert!(
            EngineConfig::new(models.clone(), 0, 8.0, 2, Duration::from_secs(120), 10).is_err()
        );
        assert!(
            EngineConfig::new(models.clone(), 21, 8.0, 2, Duration::from_secs(120), 10).is_err()
        );

        // threshold out of range
        assert!(
            EngineConfig::new(models.clone(), 5, 0.5, 2, Duration::from_secs(120), 10).is_err()
        );
        assert!(EngineConfig::new(models, 5, 10.5, 2, Duration::from_secs(120), 10).is_err());
    }

    #[test]
    fn convergence_status_serialization() {
        assert_eq!(
            serde_json::to_string(&ConvergenceStatus::Converged).unwrap(),
            "\"converged\""
        );
        assert_eq!(
            serde_json::to_string(&ConvergenceStatus::MaxRoundsExceeded).unwrap(),
            "\"max_rounds_exceeded\""
        );
        assert_eq!(
            serde_json::to_string(&ConvergenceStatus::SingleModel).unwrap(),
            "\"single_model\""
        );
        assert_eq!(
            serde_json::to_string(&ConvergenceStatus::InsufficientModels).unwrap(),
            "\"insufficient_models\""
        );
        assert_eq!(
            serde_json::to_string(&ConvergenceStatus::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }

    #[test]
    fn estimate_calls_per_round() {
        let models =
            |n: usize| -> Vec<ModelId> { (0..n).map(|i| ModelId::new(format!("m{i}"))).collect() };

        // N=2: 2² = 4
        let config = EngineConfig::new(models(2), 5, 8.0, 2, Duration::from_secs(120), 10).unwrap();
        assert_eq!(config.estimate_calls_per_round(), 4);

        // N=3: 3² = 9
        let config = EngineConfig::new(models(3), 5, 8.0, 2, Duration::from_secs(120), 10).unwrap();
        assert_eq!(config.estimate_calls_per_round(), 9);

        // N=5: 5² = 25
        let config = EngineConfig::new(models(5), 5, 8.0, 2, Duration::from_secs(120), 10).unwrap();
        assert_eq!(config.estimate_calls_per_round(), 25);

        // N=7: 7² = 49
        let config = EngineConfig::new(models(7), 5, 8.0, 2, Duration::from_secs(120), 10).unwrap();
        assert_eq!(config.estimate_calls_per_round(), 49);
    }

    #[test]
    fn estimate_calls_single_model_is_one() {
        let models = vec![ModelId::new("solo")];
        let config = EngineConfig::new(models, 5, 8.0, 2, Duration::from_secs(120), 10).unwrap();
        assert_eq!(config.estimate_calls_per_round(), 1);
    }

    #[test]
    fn proposal_set_carries_failures() {
        let set = ProposalSet {
            proposals: HashMap::from([(ModelId::new("a"), "answer".to_string())]),
            dropped: vec![(
                ModelId::new("b"),
                ProviderError::Timeout {
                    model: ModelId::new("b"),
                    elapsed: Duration::from_secs(120),
                },
            )],
        };
        assert_eq!(set.proposals.len(), 1);
        assert_eq!(set.dropped.len(), 1);
    }
}
