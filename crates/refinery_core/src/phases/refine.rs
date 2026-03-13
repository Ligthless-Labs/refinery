use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::ModelProvider;
use crate::error::ProviderError;
use crate::progress::{ProgressEvent, ProgressFn};
use crate::prompts;
use crate::types::{EvaluationSet, Message, ModelId, RefinementSet};

/// Execute the REFINE phase: each model improves its answer based on reviews.
#[allow(clippy::too_many_arguments, clippy::implicit_hasher)]
pub async fn run(
    providers: &[Arc<dyn ModelProvider>],
    proposals: &HashMap<ModelId, String>,
    evaluations: &EvaluationSet,
    prompt: &str,
    round_ctx: &str,
    semaphore: &Arc<Semaphore>,
    timeout: std::time::Duration,
    additional_context: Option<&str>,
    progress: Option<ProgressFn>,
) -> RefinementSet {
    let mut refinements = HashMap::new();
    let mut unrefined = Vec::new();

    let mut handles = tokio::task::JoinSet::new();

    for provider in providers {
        let model_id = provider.model_id().clone();

        // Skip models that didn't produce a proposal
        let Some(own_answer) = proposals.get(&model_id) else {
            continue;
        };

        // Gather reviews for this model from other evaluators
        let reviews: Vec<(String, &str)> = evaluations
            .evaluations
            .iter()
            .filter(|((_, evaluatee), _)| *evaluatee == model_id)
            .enumerate()
            .map(|(i, ((_, _), eval))| {
                let label = format!(
                    "Reviewer {}",
                    char::from(b'A' + u8::try_from(i).unwrap_or(0))
                );
                (label, eval.review.overall_assessment.as_str())
            })
            .collect();

        // Collect reviews into owned data for the spawned task
        let reviews_owned: Vec<(String, String)> = reviews
            .into_iter()
            .map(|(label, assessment)| (label, assessment.to_string()))
            .collect();

        let sem = semaphore.clone();
        let provider = provider.clone();
        let own_answer = own_answer.clone();
        let prompt = prompt.to_string();
        let round_ctx = round_ctx.to_string();
        let additional_context = additional_context.map(String::from);

        handles.spawn(async move {
            let reviews_refs: Vec<(String, &str)> = reviews_owned
                .iter()
                .map(|(l, a)| (l.clone(), a.as_str()))
                .collect();

            let mut refine_content =
                prompts::refine_prompt(&prompt, &own_answer, &reviews_refs, &round_ctx);
            if let Some(ctx) = &additional_context {
                let _ = write!(refine_content, "\n\nAdditional context: {ctx}");
            }

            let messages = vec![
                Message::system(prompts::system_prompt()),
                Message::user(refine_content),
            ];

            let _permit = sem.acquire().await.expect("semaphore closed");
            let result = tokio::time::timeout(timeout, provider.send_message(&messages)).await;

            match result {
                Ok(Ok(response)) => {
                    info!(model = %model_id, phase = "refine", "refined answer received");
                    Ok((model_id, response))
                }
                Ok(Err(e)) => {
                    warn!(model = %model_id, phase = "refine", error = %e, "provider error");
                    Err((model_id, e))
                }
                Err(_) => {
                    warn!(model = %model_id, phase = "refine", "timeout");
                    Err((
                        model_id.clone(),
                        ProviderError::Timeout {
                            model: model_id,
                            elapsed: timeout,
                        },
                    ))
                }
            }
        });
    }

    while let Some(result) = handles.join_next().await {
        match result {
            Ok(Ok((model_id, response))) => {
                if let Some(ref cb) = progress {
                    cb(ProgressEvent::ModelRefined {
                        model: model_id.clone(),
                        word_count: response.split_whitespace().count(),
                    });
                }
                refinements.insert(model_id, response);
            }
            Ok(Err((model_id, err))) => {
                if let Some(ref cb) = progress {
                    cb(ProgressEvent::ModelRefineFailed {
                        model: model_id.clone(),
                        error: err.to_string(),
                    });
                }
                // Keep previous answer on refine failure (D4)
                warn!(model = %model_id, error = %err, "refine failed, keeping previous answer");
                if let Some(prev) = proposals.get(&model_id) {
                    refinements.insert(model_id.clone(), prev.clone());
                }
                unrefined.push(model_id);
            }
            Err(join_err) => {
                warn!(error = %join_err, "task panicked in refine phase");
            }
        }
    }

    info!(
        phase = "refine",
        refined = refinements.len(),
        unrefined = unrefined.len(),
        "refine phase complete"
    );

    RefinementSet {
        refinements,
        unrefined,
    }
}
