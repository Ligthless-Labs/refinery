use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::ModelProvider;
use crate::error::ConvergeError;
use crate::phases;
use crate::progress::{ProgressEvent, ProgressFn};
use crate::prompts;
use crate::strategy::{ClosingDecision, ClosingStrategy};
use crate::types::{
    ConsensusOutcome, ConvergenceStatus, CostEstimate, EngineConfig, ModelAnswer, ModelId, Phase,
    RoundHistory, RoundOutcome, RoundOverrides,
};
use tokio::sync::Semaphore;

/// The consensus engine orchestrates iterative multi-model consensus.
pub struct Engine {
    providers: Vec<Arc<dyn ModelProvider>>,
    strategy: Box<dyn ClosingStrategy>,
    config: EngineConfig,
    progress: Option<ProgressFn>,
}

impl Engine {
    /// Create a new engine with the given providers, strategy, and configuration.
    #[must_use]
    pub fn new(
        providers: Vec<Arc<dyn ModelProvider>>,
        strategy: Box<dyn ClosingStrategy>,
        config: EngineConfig,
        progress: Option<ProgressFn>,
    ) -> Self {
        Self {
            providers,
            strategy,
            config,
            progress,
        }
    }

    /// Estimate cost without executing.
    #[must_use]
    pub fn estimate(config: &EngineConfig) -> CostEstimate {
        let calls_per_round = config.estimate_calls_per_round();
        CostEstimate {
            calls_per_round,
            total_calls: calls_per_round * config.max_rounds,
            model_count: config.models.len(),
            max_rounds: config.max_rounds,
        }
    }

    /// Run the consensus loop to completion.
    ///
    /// Returns both the final outcome and per-round data for artifact export.
    pub async fn run(
        &self,
        prompt: &str,
    ) -> Result<(ConsensusOutcome, Vec<RoundOutcome>), ConvergeError> {
        let mut session = self.start(prompt).await?;

        loop {
            let outcome = match session.next_round().await {
                Ok(o) => o,
                Err(ConvergeError::InsufficientModels { round, .. }) if round > 1 => {
                    // Graceful degradation: return best-so-far from prior rounds
                    return Ok(session.finalize_with_status(ConvergenceStatus::InsufficientModels));
                }
                Err(e) => return Err(e),
            };
            if matches!(outcome.closing_decision, ClosingDecision::Converged { .. }) {
                return Ok(session.finalize());
            }
            if session.current_round >= self.config.max_rounds {
                return Ok(session.finalize_with_status(ConvergenceStatus::MaxRoundsExceeded));
            }
        }
    }

    /// Start a stepping session for round-by-round control.
    pub async fn start(&self, prompt: &str) -> Result<Session<'_>, ConvergeError> {
        let n = self.providers.len();

        // N=1 short-circuit
        if n == 1 {
            let provider = &self.providers[0];
            let messages = vec![
                crate::types::Message::system(prompts::system_prompt()),
                crate::types::Message::user(prompt.to_string()),
            ];
            let start = Instant::now();
            let response = provider
                .send_message(&messages, Some(crate::prompts::ANSWER_SCHEMA))
                .await
                .map_err(|e| ConvergeError::PhaseFailure {
                    phase: crate::types::Phase::Propose,
                    model: provider.model_id().clone(),
                    source: e,
                })?;
            let elapsed = start.elapsed();
            // Extract answer from structured output, fall back to raw response
            let answer = serde_json::from_str::<serde_json::Value>(&response)
                .ok()
                .and_then(|v| v.get("answer").and_then(|a| a.as_str()).map(String::from))
                .unwrap_or(response);

            return Ok(Session {
                prompt: prompt.to_string(),
                providers: self.providers.clone(),
                strategy: &*self.strategy,
                config: &self.config,
                current_round: 1,
                total_calls: 1,
                start_time: start,
                last_answers: HashMap::from([(provider.model_id().clone(), answer)]),
                last_mean_scores: HashMap::new(),
                current_winner: Some(provider.model_id().clone()),
                stable_rounds: 1,
                outcomes: vec![],
                single_model: true,
                single_model_elapsed: Some(elapsed),
                progress: self.progress.clone(),
                model_histories: HashMap::new(),
            });
        }

        Ok(Session {
            prompt: prompt.to_string(),
            providers: self.providers.clone(),
            strategy: &*self.strategy,
            config: &self.config,
            current_round: 0,
            total_calls: 0,
            start_time: Instant::now(),
            last_answers: HashMap::new(),
            last_mean_scores: HashMap::new(),
            current_winner: None,
            stable_rounds: 0,
            outcomes: vec![],
            single_model: false,
            single_model_elapsed: None,
            progress: self.progress.clone(),
            model_histories: HashMap::new(),
        })
    }
}

/// A stepping session for round-by-round control of the consensus loop.
pub struct Session<'a> {
    prompt: String,
    providers: Vec<Arc<dyn ModelProvider>>,
    strategy: &'a dyn ClosingStrategy,
    config: &'a EngineConfig,
    current_round: u32,
    total_calls: u32,
    start_time: Instant,
    last_answers: HashMap<ModelId, String>,
    last_mean_scores: HashMap<ModelId, f64>,
    current_winner: Option<ModelId>,
    stable_rounds: u32,
    outcomes: Vec<RoundOutcome>,
    single_model: bool,
    single_model_elapsed: Option<std::time::Duration>,
    progress: Option<ProgressFn>,
    /// Per-model trajectory for history-aware proposals in round N>1.
    model_histories: HashMap<ModelId, RoundHistory>,
}

impl Session<'_> {
    /// Advance one round with default behavior.
    pub async fn next_round(&mut self) -> Result<RoundOutcome, ConvergeError> {
        self.next_round_with(RoundOverrides::default()).await
    }

    /// Advance one round with overrides for agent intervention.
    #[allow(clippy::too_many_lines)]
    pub async fn next_round_with(
        &mut self,
        overrides: RoundOverrides,
    ) -> Result<RoundOutcome, ConvergeError> {
        // Handle single-model case
        if self.single_model {
            let winner = self
                .current_winner
                .clone()
                .expect("single model has winner");
            return Ok(RoundOutcome {
                round: 1,
                proposals: crate::types::ProposalSet {
                    proposals: self.last_answers.clone(),
                    dropped: vec![],
                },
                evaluations: crate::types::EvaluationSet {
                    evaluations: HashMap::new(),
                    dropped: vec![],
                },
                closing_decision: ClosingDecision::Converged {
                    winner,
                    explanation: "Single model — no consensus loop needed.".to_string(),
                },
                elapsed: self.single_model_elapsed.unwrap_or_default(),
                call_count: 1,
            });
        }

        self.current_round += 1;
        let round = self.current_round;
        let round_start = Instant::now();

        self.emit(ProgressEvent::RoundStarted {
            round,
            total: self.config.max_rounds,
        });

        // Apply model drops from overrides
        let mut active_providers = self.providers.clone();
        for drop_id in &overrides.drop_models {
            active_providers.retain(|p| p.model_id() != drop_id);
        }

        // Check minimum model count
        if active_providers.len() < 2 {
            return Err(ConvergeError::InsufficientModels {
                round,
                remaining: active_providers.len(),
                minimum: 2,
            });
        }

        let permits = if self.config.max_concurrent == 0 {
            // 0 = unlimited: allow all tasks to run concurrently
            active_providers.len().pow(2).max(1)
        } else {
            self.config.max_concurrent
        };
        let semaphore = Arc::new(Semaphore::new(permits));

        let additional_context = overrides.additional_context.as_deref();

        // Build round context
        let previous_scores: Vec<(String, f64)> = self
            .last_mean_scores
            .iter()
            .map(|(id, score)| (id.to_string(), *score))
            .collect();

        let round_ctx = prompts::round_context(
            round,
            self.config.max_rounds,
            active_providers.len(),
            0,
            self.strategy.name(),
            self.config.threshold,
            self.config.stability_rounds,
            &previous_scores,
            self.current_winner
                .as_ref()
                .map(std::string::ToString::to_string)
                .as_deref(),
            self.stable_rounds,
            false,
        );

        // Phase 1: PROPOSE
        self.emit(ProgressEvent::PhaseStarted {
            round,
            phase: Phase::Propose,
        });
        let histories_ref = if self.model_histories.is_empty() {
            None
        } else {
            Some(&self.model_histories)
        };
        let proposal_set = phases::propose::run(
            &active_providers,
            &self.prompt,
            round,
            &round_ctx,
            &semaphore,
            self.config.timeout,
            additional_context,
            histories_ref,
            self.progress.clone(),
        )
        .await;

        let mut call_count =
            u32::try_from(proposal_set.proposals.len() + proposal_set.dropped.len()).unwrap_or(0);

        // Check if enough models produced proposals
        if proposal_set.proposals.len() < 2 {
            return Err(ConvergeError::InsufficientModels {
                round,
                remaining: proposal_set.proposals.len(),
                minimum: 2,
            });
        }

        // Update active providers to only those that proposed successfully
        let proposed_ids: Vec<ModelId> = proposal_set.proposals.keys().cloned().collect();
        let eval_providers: Vec<_> = active_providers
            .iter()
            .filter(|p| proposed_ids.contains(p.model_id()))
            .cloned()
            .collect();

        // Phase 2: EVALUATE
        self.emit(ProgressEvent::PhaseStarted {
            round,
            phase: Phase::Evaluate,
        });
        let evaluation_set = phases::evaluate::run(
            &eval_providers,
            &proposal_set.proposals,
            &self.prompt,
            &round_ctx,
            &semaphore,
            self.config.timeout,
            self.progress.clone(),
        )
        .await;

        call_count +=
            u32::try_from(evaluation_set.evaluations.len() + evaluation_set.dropped.len())
                .unwrap_or(0);

        // Collect per-model history: each model's proposal + reviews received this round
        for (model_id, proposal) in &proposal_set.proposals {
            let reviews: Vec<(String, String)> = evaluation_set
                .evaluations
                .iter()
                .filter(|((_, evaluatee), _)| evaluatee == model_id)
                .map(|((evaluator, _), eval)| {
                    (
                        evaluator.to_string(),
                        eval.review.overall_assessment.clone(),
                    )
                })
                .collect();
            self.model_histories
                .entry(model_id.clone())
                .or_default()
                .push((proposal.clone(), reviews));
        }

        // Phase 3: CLOSE CHECK
        let (closing_decision, new_winner, new_stable) = phases::close::run(
            self.strategy,
            &evaluation_set,
            round,
            &self.current_winner,
            self.stable_rounds,
        )
        .await;

        // Update state — winning answer is the scored proposal
        self.last_answers.clone_from(&proposal_set.proposals);
        self.last_mean_scores = phases::close::compute_mean_scores(&evaluation_set);
        self.current_winner.clone_from(&new_winner);
        self.stable_rounds = new_stable;

        // Emit convergence check
        let best_score = self
            .last_mean_scores
            .values()
            .copied()
            .fold(0.0_f64, f64::max);
        self.emit(ProgressEvent::ConvergenceCheck {
            round,
            converged: matches!(closing_decision, ClosingDecision::Converged { .. }),
            winner: new_winner,
            best_score,
            threshold: self.config.threshold,
            stable_rounds: new_stable,
            required_stable: self.config.stability_rounds,
        });
        self.total_calls += call_count;

        let elapsed = round_start.elapsed();

        let outcome = RoundOutcome {
            round,
            proposals: proposal_set,
            evaluations: evaluation_set,
            closing_decision: closing_decision.clone(),
            elapsed,
            call_count,
        };

        self.outcomes.push(outcome.clone());

        Ok(outcome)
    }

    /// Clean cancellation: return best-so-far with Cancelled status.
    #[must_use]
    pub fn cancel(self) -> (ConsensusOutcome, Vec<RoundOutcome>) {
        self.finalize_with_status(ConvergenceStatus::Cancelled)
    }

    /// Finalize after convergence or max rounds.
    #[must_use]
    pub fn finalize(self) -> (ConsensusOutcome, Vec<RoundOutcome>) {
        if self.single_model {
            return self.finalize_with_status(ConvergenceStatus::SingleModel);
        }
        // Check the last outcome's closing decision
        if let Some(last) = self.outcomes.last() {
            if matches!(last.closing_decision, ClosingDecision::Converged { .. }) {
                return self.finalize_with_status(ConvergenceStatus::Converged);
            }
        }
        self.finalize_with_status(ConvergenceStatus::MaxRoundsExceeded)
    }

    fn emit(&self, event: ProgressEvent) {
        if let Some(ref cb) = self.progress {
            cb(event);
        }
    }

    fn finalize_with_status(
        self,
        status: ConvergenceStatus,
    ) -> (ConsensusOutcome, Vec<RoundOutcome>) {
        let winner = self
            .current_winner
            .unwrap_or_else(|| ModelId::from_parts("unknown", "unknown"));
        let answer = self.last_answers.get(&winner).cloned().unwrap_or_default();

        let all_answers: Vec<ModelAnswer> = self
            .last_answers
            .iter()
            .map(|(model_id, ans)| {
                let mean_score = self.last_mean_scores.get(model_id).copied().unwrap_or(0.0);
                ModelAnswer {
                    model_id: model_id.clone(),
                    answer: ans.clone(),
                    mean_score,
                }
            })
            .collect();

        let outcome = ConsensusOutcome {
            status,
            winner,
            answer,
            final_round: self.current_round,
            all_answers,
            total_calls: self.total_calls,
            elapsed: self.start_time.elapsed(),
        };

        (outcome, self.outcomes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{AlwaysConvergeAfterN, EchoProvider, FailAfterNProvider, FailingProvider};

    fn make_providers(names: &[&str]) -> Vec<Arc<dyn ModelProvider>> {
        names
            .iter()
            .map(|name| Arc::new(EchoProvider::new(*name)) as Arc<dyn ModelProvider>)
            .collect()
    }

    fn default_config(n: usize) -> EngineConfig {
        let models: Vec<ModelId> = (0..n)
            .map(|i| ModelId::new(format!("test/model_{i}")))
            .collect();
        EngineConfig::new(models, 5, 8.0, 2, std::time::Duration::from_secs(120), 10).unwrap()
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn single_model_short_circuits() {
        let providers = make_providers(&["test/solo"]);
        let config = default_config(1);
        let strategy = Box::new(crate::strategy::VoteThreshold::new(8.0, 2));
        let engine = Engine::new(providers, strategy, config, None);

        let (result, _rounds) = engine.run("test prompt").await.unwrap();
        assert_eq!(result.status, ConvergenceStatus::SingleModel);
        assert_eq!(result.winner, ModelId::new("test/solo"));
        assert_eq!(result.total_calls, 1);
        assert_eq!(result.final_round, 1);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn converges_with_mock_providers() {
        let providers: Vec<Arc<dyn ModelProvider>> = vec![
            Arc::new(EchoProvider::with_json_eval("test/model_a", 9)),
            Arc::new(EchoProvider::with_json_eval("test/model_b", 9)),
            Arc::new(EchoProvider::with_json_eval("test/model_c", 9)),
        ];
        let config = default_config(3);
        let strategy = Box::new(AlwaysConvergeAfterN::new(2));
        let engine = Engine::new(providers, strategy, config, None);

        let (result, _rounds) = engine.run("test prompt").await.unwrap();
        assert_eq!(result.status, ConvergenceStatus::Converged);
        assert_eq!(result.final_round, 2);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn max_rounds_exceeded() {
        let providers: Vec<Arc<dyn ModelProvider>> = vec![
            Arc::new(EchoProvider::with_json_eval("test/model_a", 5)),
            Arc::new(EchoProvider::with_json_eval("test/model_b", 5)),
        ];
        let models = vec![ModelId::new("test/model_a"), ModelId::new("test/model_b")];
        let config =
            EngineConfig::new(models, 3, 8.0, 2, std::time::Duration::from_secs(120), 10).unwrap();
        let strategy = Box::new(crate::strategy::VoteThreshold::new(8.0, 2));
        let engine = Engine::new(providers, strategy, config, None);

        let (result, _rounds) = engine.run("test prompt").await.unwrap();
        assert_eq!(result.status, ConvergenceStatus::MaxRoundsExceeded);
        assert_eq!(result.final_round, 3);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn partial_failure_drops_model() {
        let providers: Vec<Arc<dyn ModelProvider>> = vec![
            Arc::new(EchoProvider::with_json_eval("test/model_a", 9)),
            Arc::new(EchoProvider::with_json_eval("test/model_b", 9)),
            Arc::new(FailingProvider::new("test/model_c")),
        ];
        let config = default_config(3);
        let strategy = Box::new(AlwaysConvergeAfterN::new(2));
        let engine = Engine::new(providers, strategy, config, None);

        let (result, _rounds) = engine.run("test prompt").await.unwrap();
        // Should still succeed with 2 models
        assert_eq!(result.status, ConvergenceStatus::Converged);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn all_models_fail_returns_error() {
        let providers: Vec<Arc<dyn ModelProvider>> = vec![
            Arc::new(FailingProvider::new("test/model_a")),
            Arc::new(FailingProvider::new("test/model_b")),
        ];
        let config = default_config(2);
        let strategy = Box::new(crate::strategy::VoteThreshold::new(8.0, 2));
        let engine = Engine::new(providers, strategy, config, None);

        let result = engine.run("test prompt").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ConvergeError::InsufficientModels { .. }
        ));
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn insufficient_models_returns_best_so_far() {
        // 3 providers: model_a and model_b succeed round 1, model_c fails.
        // In round 2, model_b also fails → only 1 proposal → InsufficientModels.
        // Engine::run should return best-so-far, not an error.
        let providers: Vec<Arc<dyn ModelProvider>> = vec![
            Arc::new(EchoProvider::with_json_eval("test/model_a", 9)),
            Arc::new(FailAfterNProvider::new("test/model_b", 1)),
            Arc::new(FailingProvider::new("test/model_c")),
        ];
        let models = vec![
            ModelId::new("test/model_a"),
            ModelId::new("test/model_b"),
            ModelId::new("test/model_c"),
        ];
        let config =
            EngineConfig::new(models, 5, 8.0, 2, std::time::Duration::from_secs(120), 10).unwrap();
        let strategy = Box::new(crate::strategy::VoteThreshold::new(8.0, 2));
        let engine = Engine::new(providers, strategy, config, None);

        let result = engine.run("test prompt").await;
        // Should succeed with best-so-far, not error
        assert!(result.is_ok());
        let (outcome, _rounds) = result.unwrap();
        assert_eq!(outcome.status, ConvergenceStatus::InsufficientModels);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn stepping_api_works() {
        let providers: Vec<Arc<dyn ModelProvider>> = vec![
            Arc::new(EchoProvider::with_json_eval("test/model_a", 9)),
            Arc::new(EchoProvider::with_json_eval("test/model_b", 9)),
        ];
        let config = default_config(2);
        let strategy = Box::new(AlwaysConvergeAfterN::new(2));
        let engine = Engine::new(providers, strategy, config, None);

        let mut session = engine.start("test prompt").await.unwrap();

        let outcome1 = session.next_round().await.unwrap();
        assert_eq!(outcome1.round, 1);
        assert!(matches!(
            outcome1.closing_decision,
            ClosingDecision::Continue
        ));

        let outcome2 = session.next_round().await.unwrap();
        assert_eq!(outcome2.round, 2);
        assert!(matches!(
            outcome2.closing_decision,
            ClosingDecision::Converged { .. }
        ));

        let (result, _rounds) = session.finalize();
        assert_eq!(result.status, ConvergenceStatus::Converged);
        assert_eq!(result.final_round, 2);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn stepping_api_cancel() {
        let providers: Vec<Arc<dyn ModelProvider>> = vec![
            Arc::new(EchoProvider::with_json_eval("test/model_a", 5)),
            Arc::new(EchoProvider::with_json_eval("test/model_b", 5)),
        ];
        let config = default_config(2);
        let strategy = Box::new(crate::strategy::VoteThreshold::new(8.0, 2));
        let engine = Engine::new(providers, strategy, config, None);

        let mut session = engine.start("test prompt").await.unwrap();
        let _outcome = session.next_round().await.unwrap();

        let (result, _rounds) = session.cancel();
        assert_eq!(result.status, ConvergenceStatus::Cancelled);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn stepping_api_with_overrides() {
        let providers: Vec<Arc<dyn ModelProvider>> = vec![
            Arc::new(EchoProvider::with_json_eval("test/model_a", 9)),
            Arc::new(EchoProvider::with_json_eval("test/model_b", 9)),
            Arc::new(EchoProvider::with_json_eval("test/model_c", 9)),
        ];
        let config = default_config(3);
        let strategy = Box::new(AlwaysConvergeAfterN::new(1));
        let engine = Engine::new(providers, strategy, config, None);

        let mut session = engine.start("test prompt").await.unwrap();

        let overrides = RoundOverrides {
            additional_context: Some("Focus on security.".to_string()),
            drop_models: vec![ModelId::new("test/model_c")],
        };
        let outcome = session.next_round_with(overrides).await.unwrap();
        // model_c should not appear in proposals
        assert!(
            !outcome
                .proposals
                .proposals
                .contains_key(&ModelId::new("test/model_c"))
        );
    }

    #[test]
    fn estimate_returns_correct_counts() {
        let config = default_config(3);
        let estimate = Engine::estimate(&config);
        assert_eq!(estimate.calls_per_round, 9); // 3²
        assert_eq!(estimate.total_calls, 45); // 9 * 5
        assert_eq!(estimate.model_count, 3);
    }
}
