use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::types::{ModelId, RoundData};

/// Decision made by a closing strategy after evaluating round data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClosingDecision {
    Converged {
        winner: ModelId,
        explanation: String,
    },
    Continue,
}

/// A strategy for deciding when consensus has been reached.
#[async_trait]
pub trait ClosingStrategy: Send + Sync {
    /// Check whether consensus has been reached given the current round data.
    async fn check(&self, round_data: &RoundData) -> ClosingDecision;

    /// Human-readable name for this strategy.
    fn name(&self) -> &'static str;
}

/// Vote Threshold closing strategy.
///
/// Converges when:
/// - The top-scoring answer has a mean score >= `threshold`, AND
/// - The top-scoring answer has been the same model for `stability_rounds` consecutive rounds.
#[derive(Debug, Clone)]
pub struct VoteThreshold {
    pub threshold: f64,
    pub stability_rounds: u32,
}

impl VoteThreshold {
    #[must_use]
    pub fn new(threshold: f64, stability_rounds: u32) -> Self {
        Self {
            threshold,
            stability_rounds,
        }
    }
}

#[async_trait]
impl ClosingStrategy for VoteThreshold {
    async fn check(&self, round_data: &RoundData) -> ClosingDecision {
        // Find the model with the highest mean score
        let top = round_data
            .mean_scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal));

        let Some((top_model, &top_score)) = top else {
            return ClosingDecision::Continue;
        };

        // Check both conditions: score above threshold AND stability met
        if top_score >= self.threshold && round_data.stable_rounds >= self.stability_rounds {
            ClosingDecision::Converged {
                winner: top_model.clone(),
                explanation: format!(
                    "Model {} achieved mean score {top_score:.1} (>= {:.1} threshold) \
                     and has been top-ranked for {} consecutive rounds (>= {} required)",
                    top_model, self.threshold, round_data.stable_rounds, self.stability_rounds
                ),
            }
        } else {
            ClosingDecision::Continue
        }
    }

    fn name(&self) -> &'static str {
        "vote-threshold"
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rstest::rstest;

    use super::*;

    fn round_data(mean_score: f64, stable_rounds: u32) -> RoundData {
        let mut mean_scores = HashMap::new();
        mean_scores.insert(ModelId::new("test/model_a"), mean_score);
        mean_scores.insert(ModelId::new("test/model_b"), mean_score - 1.0);
        RoundData {
            round: 3,
            mean_scores,
            previous_winner: Some(ModelId::new("test/model_a")),
            stable_rounds,
        }
    }

    #[rstest]
    #[case(7.9, 2, false)] // score below threshold
    #[case(8.0, 1, false)] // stable only 1 round (need 2)
    #[case(8.0, 2, true)] // at threshold, stable -> converge
    #[case(9.5, 3, true)] // above threshold, stable -> converge
    #[case(10.0, 2, true)] // max score -> converge
    #[tokio::test]
    async fn vote_threshold_convergence(
        #[case] mean_score: f64,
        #[case] stable_rounds: u32,
        #[case] should_converge: bool,
    ) {
        let strategy = VoteThreshold::new(8.0, 2);
        let data = round_data(mean_score, stable_rounds);
        let decision = strategy.check(&data).await;

        match decision {
            ClosingDecision::Converged { .. } => assert!(should_converge, "Expected Continue"),
            ClosingDecision::Continue => assert!(!should_converge, "Expected Converged"),
        }
    }

    #[tokio::test]
    async fn all_scores_identical_converges() {
        let mut mean_scores = HashMap::new();
        mean_scores.insert(ModelId::new("test/a"), 9.0);
        mean_scores.insert(ModelId::new("test/b"), 9.0);
        let data = RoundData {
            round: 3,
            mean_scores,
            previous_winner: Some(ModelId::new("test/a")),
            stable_rounds: 2,
        };
        let strategy = VoteThreshold::new(8.0, 2);
        let decision = strategy.check(&data).await;
        assert!(matches!(decision, ClosingDecision::Converged { .. }));
    }

    #[tokio::test]
    async fn empty_scores_continues() {
        let data = RoundData {
            round: 1,
            mean_scores: HashMap::new(),
            previous_winner: None,
            stable_rounds: 0,
        };
        let strategy = VoteThreshold::new(8.0, 2);
        let decision = strategy.check(&data).await;
        assert!(matches!(decision, ClosingDecision::Continue));
    }

    #[test]
    fn strategy_name() {
        let strategy = VoteThreshold::new(8.0, 2);
        assert_eq!(strategy.name(), "vote-threshold");
    }
}
