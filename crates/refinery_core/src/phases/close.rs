use std::collections::HashMap;

use tracing::info;

use crate::strategy::{ClosingDecision, ClosingStrategy};
use crate::types::{EvaluationSet, ModelId, RoundData};

/// Execute the CLOSE CHECK phase: apply the closing strategy to round data.
pub async fn run(
    strategy: &dyn ClosingStrategy,
    evaluations: &EvaluationSet,
    round: u32,
    previous_winner: &Option<ModelId>,
    previous_stable_rounds: u32,
) -> (ClosingDecision, Option<ModelId>, u32) {
    let mean_scores = compute_mean_scores(evaluations);

    // Determine current winner
    let current_winner = mean_scores
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(id, _)| id.clone());

    // Compute stability
    let stable_rounds = if let (Some(current), Some(previous)) = (&current_winner, previous_winner)
    {
        if current == previous {
            previous_stable_rounds + 1
        } else {
            1
        }
    } else {
        u32::from(current_winner.is_some())
    };

    let round_data = RoundData {
        round,
        mean_scores,
        previous_winner: previous_winner.clone(),
        stable_rounds,
    };

    let decision = strategy.check(&round_data).await;

    info!(
        phase = "close",
        round,
        strategy = strategy.name(),
        winner = ?current_winner,
        stable_rounds,
        converged = matches!(decision, ClosingDecision::Converged { .. }),
        "close check complete"
    );

    (decision, current_winner, stable_rounds)
}

/// Compute mean scores per model from evaluations (self-scores already excluded).
#[must_use]
pub fn compute_mean_scores(evaluations: &EvaluationSet) -> HashMap<ModelId, f64> {
    let mut score_sums: HashMap<ModelId, (f64, u32)> = HashMap::new();

    for ((_, evaluatee), evaluation) in &evaluations.evaluations {
        let entry = score_sums.entry(evaluatee.clone()).or_insert((0.0, 0));
        entry.0 += f64::from(evaluation.score.value());
        entry.1 += 1;
    }

    score_sums
        .into_iter()
        .map(|(model, (sum, count))| (model, sum / f64::from(count)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Evaluation, Review, Score};

    #[test]
    fn compute_mean_scores_excludes_nothing_extra() {
        let mut evals = HashMap::new();
        // model_b evaluates model_a: score 8
        evals.insert(
            (ModelId::new("b"), ModelId::new("a")),
            Evaluation {
                review: Review {
                    strengths: vec![],
                    weaknesses: vec![],
                    suggestions: vec![],
                    overall_assessment: String::new(),
                },
                score: Score::new(8).unwrap(),
                rationale: String::new(),
            },
        );
        // model_c evaluates model_a: score 6
        evals.insert(
            (ModelId::new("c"), ModelId::new("a")),
            Evaluation {
                review: Review {
                    strengths: vec![],
                    weaknesses: vec![],
                    suggestions: vec![],
                    overall_assessment: String::new(),
                },
                score: Score::new(6).unwrap(),
                rationale: String::new(),
            },
        );

        let eval_set = EvaluationSet {
            evaluations: evals,
            dropped: vec![],
        };

        let means = compute_mean_scores(&eval_set);
        // model_a: (8 + 6) / 2 = 7.0
        assert!((means[&ModelId::new("a")] - 7.0).abs() < f64::EPSILON);
    }
}
