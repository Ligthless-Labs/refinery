use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::ModelProvider;
use crate::error::ProviderError;
use crate::prompts;
use crate::types::{Evaluation, EvaluationSet, Message, ModelId, Review, Score};

/// Execute the EVALUATE phase: each model reviews and scores every other model's answer.
///
/// Produces N*(N-1) evaluations (self-evaluation excluded per D10).
#[allow(clippy::too_many_lines, clippy::implicit_hasher)]
pub async fn run(
    providers: &[Arc<dyn ModelProvider>],
    proposals: &HashMap<ModelId, String>,
    prompt: &str,
    round_ctx: &str,
    semaphore: &Arc<Semaphore>,
    timeout: std::time::Duration,
) -> EvaluationSet {
    let mut evaluations = HashMap::new();
    let mut dropped = Vec::new();

    let nonce = prompts::generate_nonce();

    // Create anonymous labels for each model
    let model_ids: Vec<&ModelId> = proposals.keys().collect();
    let labels: HashMap<&ModelId, String> = {
        let shuffled = prompts::shuffled_labels(model_ids.len());
        model_ids
            .iter()
            .zip(shuffled)
            .map(|(id, label)| (*id, label))
            .collect()
    };

    let mut handles = tokio::task::JoinSet::new();

    for evaluator_provider in providers {
        let evaluator_id = evaluator_provider.model_id().clone();

        // Skip models that didn't produce proposals
        if !proposals.contains_key(&evaluator_id) {
            continue;
        }

        for (evaluatee_id, answer) in proposals {
            // D10: Self-evaluation exclusion
            if *evaluatee_id == evaluator_id {
                continue;
            }

            let sem = semaphore.clone();
            let provider = evaluator_provider.clone();
            let evaluator = evaluator_id.clone();
            let evaluatee = evaluatee_id.clone();
            let answer_label = labels.get(evaluatee_id).cloned().unwrap_or_default();
            let eval_prompt =
                prompts::evaluate_prompt(prompt, answer, &answer_label, &nonce, round_ctx);
            let messages = vec![
                Message::system(prompts::system_prompt()),
                Message::user(eval_prompt),
            ];

            handles.spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore closed");
                let result = tokio::time::timeout(timeout, provider.send_message(&messages)).await;

                match result {
                    Ok(Ok(response)) => match parse_evaluation(&response, &evaluator) {
                        Ok(evaluation) => {
                            info!(
                                evaluator = %evaluator,
                                evaluatee = %evaluatee,
                                score = evaluation.score.value(),
                                "evaluation complete"
                            );
                            Ok((evaluator, evaluatee, evaluation))
                        }
                        Err(e) => {
                            warn!(
                                evaluator = %evaluator,
                                evaluatee = %evaluatee,
                                error = %e,
                                "failed to parse evaluation"
                            );
                            Err((evaluator, evaluatee, e))
                        }
                    },
                    Ok(Err(e)) => {
                        warn!(
                            evaluator = %evaluator,
                            evaluatee = %evaluatee,
                            error = %e,
                            "provider error during evaluation"
                        );
                        Err((evaluator, evaluatee, e))
                    }
                    Err(_) => {
                        let err = ProviderError::Timeout {
                            model: evaluator.clone(),
                            elapsed: timeout,
                        };
                        Err((evaluator, evaluatee, err))
                    }
                }
            });
        }
    }

    while let Some(result) = handles.join_next().await {
        match result {
            Ok(Ok((evaluator, evaluatee, evaluation))) => {
                evaluations.insert((evaluator, evaluatee), evaluation);
            }
            Ok(Err((evaluator, evaluatee, err))) => {
                dropped.push((evaluator, evaluatee, err));
            }
            Err(join_err) => {
                warn!(error = %join_err, "task panicked in evaluate phase");
            }
        }
    }

    info!(
        phase = "evaluate",
        succeeded = evaluations.len(),
        failed = dropped.len(),
        "evaluate phase complete"
    );

    EvaluationSet {
        evaluations,
        dropped,
    }
}

/// Parse an evaluation response (JSON) into an `Evaluation`.
fn parse_evaluation(response: &str, model: &ModelId) -> Result<Evaluation, ProviderError> {
    // Check response size
    const MAX_RESPONSE_SIZE: usize = 100_000;
    if response.len() > MAX_RESPONSE_SIZE {
        return Err(ProviderError::ResponseTooLarge {
            model: model.clone(),
            size: response.len(),
            max: MAX_RESPONSE_SIZE,
        });
    }

    let json_text = prompts::extract_json(response).ok_or_else(|| ProviderError::InvalidJson {
        model: model.clone(),
        message: "no JSON found in response".to_string(),
    })?;

    // Check depth before parsing
    if let Err(depth) = prompts::check_json_depth(json_text, 10) {
        return Err(ProviderError::JsonTooDeep {
            model: model.clone(),
            depth,
            max: 10,
        });
    }

    let parsed: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| ProviderError::InvalidJson {
            model: model.clone(),
            message: e.to_string(),
        })?;

    let strengths = parsed["strengths"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let weaknesses = parsed["weaknesses"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let suggestions = parsed["suggestions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let overall_assessment = parsed["overall_assessment"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let rationale = parsed["rationale"].as_str().unwrap_or("").to_string();

    let score_value = parsed["score"]
        .as_u64()
        .ok_or_else(|| ProviderError::InvalidJson {
            model: model.clone(),
            message: "missing or invalid 'score' field".to_string(),
        })?;

    let score_u8 = u8::try_from(score_value).map_err(|_| ProviderError::InvalidJson {
        model: model.clone(),
        message: format!("score {score_value} out of u8 range"),
    })?;

    let score = Score::new(score_u8).map_err(|_| ProviderError::InvalidJson {
        model: model.clone(),
        message: format!("score {score_value} out of range 1-10"),
    })?;

    Ok(Evaluation {
        review: Review {
            strengths,
            weaknesses,
            suggestions,
            overall_assessment,
        },
        score,
        rationale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_evaluation() {
        let json = r#"```json
{
  "strengths": ["clear", "concise"],
  "weaknesses": ["missing examples"],
  "suggestions": ["add code samples"],
  "overall_assessment": "Good but could be better.",
  "rationale": "Strong clarity but lacks practical examples.",
  "score": 7
}
```"#;
        let model = ModelId::new("test");
        let eval = parse_evaluation(json, &model).unwrap();
        assert_eq!(eval.score.value(), 7);
        assert_eq!(eval.review.strengths.len(), 2);
        assert_eq!(eval.review.weaknesses.len(), 1);
    }

    #[test]
    fn parse_evaluation_invalid_score() {
        let json = r#"{"strengths": [], "weaknesses": [], "suggestions": [], "overall_assessment": "", "rationale": "", "score": 15}"#;
        let model = ModelId::new("test");
        let result = parse_evaluation(json, &model);
        assert!(result.is_err());
    }

    #[test]
    fn parse_evaluation_no_json() {
        let model = ModelId::new("test");
        let result = parse_evaluation("No JSON here", &model);
        assert!(result.is_err());
    }

    #[test]
    fn parse_evaluation_too_large() {
        let model = ModelId::new("test");
        let large = "x".repeat(200_000);
        let result = parse_evaluation(&large, &model);
        assert!(matches!(
            result,
            Err(ProviderError::ResponseTooLarge { .. })
        ));
    }
}
