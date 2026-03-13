use std::fmt::Write as _;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::ModelProvider;
use crate::error::ProviderError;
use crate::prompts;
use crate::types::{Message, ProposalSet};

/// Execute the PROPOSE phase: each model independently produces an answer.
pub async fn run(
    providers: &[Arc<dyn ModelProvider>],
    prompt: &str,
    round: u32,
    round_ctx: &str,
    semaphore: &Arc<Semaphore>,
    timeout: std::time::Duration,
    additional_context: Option<&str>,
) -> ProposalSet {
    let mut proposals = std::collections::HashMap::new();
    let mut dropped = Vec::new();

    let mut handles = tokio::task::JoinSet::new();

    for provider in providers {
        let model_id = provider.model_id().clone();
        let sem = semaphore.clone();
        let mut user_content = prompts::propose_prompt(prompt, round_ctx);
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
                proposals.insert(model_id, response);
            }
            Ok(Err((model_id, err))) => {
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
