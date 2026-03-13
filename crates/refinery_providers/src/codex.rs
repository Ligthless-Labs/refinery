use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use refinery_core::ModelProvider;
use refinery_core::error::ProviderError;
use refinery_core::types::{Message, ModelId};

use crate::credential::{self, Credential};
use crate::process;

/// Codex CLI provider adapter.
///
/// Invokes: `codex exec --json --sandbox read-only -m gpt-5.4 -c model_reasoning_effort="xhigh" -- "PROMPT"`
/// System prompt via: `--config developer_instructions="..."`
///
/// Supports: `OPENAI_API_KEY` (pay-per-use) or `CODEX_API_KEY` (for `codex exec`).
#[derive(Debug)]
pub struct CodexProvider {
    model_id: ModelId,
    binary_path: PathBuf,
    credential: Credential,
    model_name: String,
    reasoning_effort: String,
    timeout: Duration,
}

impl CodexProvider {
    /// Create a new Codex provider, validating credentials and binary.
    pub async fn new(model_name: &str, reasoning_effort: &str, timeout: Duration) -> Result<Self, ProviderError> {
        let credential =
            credential::resolve_credential("codex", &["OPENAI_API_KEY", "CODEX_API_KEY"])?;

        let binary_path = process::resolve_binary("codex").await?;

        Ok(Self {
            model_id: ModelId::new(format!("codex-{model_name}")),
            binary_path,
            credential,
            model_name: model_name.to_string(),
            reasoning_effort: reasoning_effort.to_string(),
            timeout,
        })
    }

    fn build_args(&self, system_prompt: &str, user_prompt: &str) -> Vec<String> {
        vec![
            "exec".to_string(),
            "--json".to_string(),
            "--sandbox".to_string(),
            "read-only".to_string(),
            "--model".to_string(),
            self.model_name.clone(),
            "--config".to_string(),
            format!("model_reasoning_effort={}", self.reasoning_effort),
            "--config".to_string(),
            format!("developer_instructions={system_prompt}"),
            "--".to_string(),
            user_prompt.to_string(),
        ]
    }
}

#[async_trait]
impl ModelProvider for CodexProvider {
    async fn send_message(&self, messages: &[Message]) -> Result<String, ProviderError> {
        let (system_prompt, user_prompt) = process::extract_prompts(messages);

        let args = self.build_args(&system_prompt, &user_prompt);
        let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let env_vars = [self.credential.as_env_pair()];

        let output = process::spawn_cli(
            &self.binary_path,
            &args_refs,
            &env_vars,
            self.timeout,
            &self.model_id,
        )
        .await?;

        // Codex outputs JSONL, extract from turn.completed
        process::extract_codex_response(&output)
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::credential::resolve_credential_with;

    fn test_credential() -> Credential {
        resolve_credential_with("codex", &["OPENAI_API_KEY"], |_| Ok("test-key".to_string()))
            .unwrap()
    }

    #[test]
    fn build_args_contains_required_flags() {
        let provider = CodexProvider {
            model_id: ModelId::new("codex-gpt-5.4"),
            binary_path: PathBuf::from("/usr/local/bin/codex"),
            credential: test_credential(),
            model_name: "gpt-5.4".to_string(),
            reasoning_effort: "xhigh".to_string(),
            timeout: Duration::from_secs(120),
        };

        let args = provider.build_args("system prompt", "user prompt");

        assert!(args.contains(&"exec".to_string()));
        assert!(args.contains(&"--json".to_string()));
        assert!(args.contains(&"--sandbox".to_string()));
        assert!(args.contains(&"read-only".to_string()));
        assert!(args.contains(&"--".to_string())); // sentinel
        assert!(args.contains(&"user prompt".to_string()));
    }
}
