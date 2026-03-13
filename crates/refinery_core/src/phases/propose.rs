use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::ModelProvider;
use crate::error::ProviderError;
use crate::progress::{self, ProgressEvent, ProgressFn};
use crate::prompts;
use crate::types::{Message, ModelId, ProposalSet};

/// Execute the PROPOSE phase: each model independently produces an answer.
///
/// In round N>1, if `model_histories` is provided, each model's prompt is
/// enriched with its full trajectory (prior proposals + reviews per round).
#[allow(clippy::too_many_arguments)]
pub async fn run(
    providers: &[Arc<dyn ModelProvider>],
    prompt: &str,
    round: u32,
    round_ctx: &str,
    semaphore: &Arc<Semaphore>,
    timeout: std::time::Duration,
    additional_context: Option<&str>,
    model_histories: Option<&HashMap<ModelId, Vec<(String, Vec<(String, String)>)>>>,
    progress: Option<ProgressFn>,
) -> ProposalSet {
    let mut proposals = std::collections::HashMap::new();
    let mut dropped = Vec::new();

    let mut handles = tokio::task::JoinSet::new();

    for provider in providers {
        let model_id = provider.model_id().clone();
        let sem = semaphore.clone();
        let mut user_content = if let Some(histories) = model_histories {
            if let Some(history) = histories.get(&model_id) {
                prompts::propose_with_history_prompt(prompt, round_ctx, history)
            } else {
                prompts::propose_prompt(prompt, round_ctx)
            }
        } else {
            prompts::propose_prompt(prompt, round_ctx)
        };
        if let Some(ctx) = additional_context {
            let _ = write!(user_content, "\n\nAdditional context: {ctx}");
        }
        let messages = vec![
            Message::system(prompts::system_prompt()),
            Message::user(user_content),
        ];
        let provider = provider.clone();

        handles.spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            let result = tokio::time::timeout(timeout, provider.send_message(&messages)).await;
            match result {
                Ok(Ok(response)) => {
                    info!(model = %model_id, phase = "propose", "received response");
                    Ok((model_id, response))
                }
                Ok(Err(e)) => {
                    warn!(model = %model_id, phase = "propose", error = %e, "provider error");
                    Err((model_id.clone(), e))
                }
                Err(_) => {
                    warn!(model = %model_id, phase = "propose", "timeout");
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
                    cb(ProgressEvent::ModelProposed {
                        model: model_id.clone(),
                        word_count: response.split_whitespace().count(),
                        preview: progress::preview(&response, 60),
                    });
                }
                proposals.insert(model_id, response);
            }
            Ok(Err((model_id, err))) => {
                if let Some(ref cb) = progress {
                    cb(ProgressEvent::ModelProposeFailed {
                        model: model_id.clone(),
                        error: err.to_string(),
                    });
                }
                dropped.push((model_id, err));
            }
            Err(join_err) => {
                warn!(error = %join_err, "task panicked in propose phase");
            }
        }
    }

    info!(
        phase = "propose",
        round,
        succeeded = proposals.len(),
        failed = dropped.len(),
        "propose phase complete"
    );

    ProposalSet { proposals, dropped }
}
