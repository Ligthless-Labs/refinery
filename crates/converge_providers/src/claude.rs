use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use converge_core::ModelProvider;
use converge_core::error::ProviderError;
use converge_core::types::{Message, ModelId};

use crate::process;

/// Claude CLI provider adapter.
///
/// Invokes: `claude -p --output-format json --tools "" --max-turns 1 --append-system-prompt "SYSTEM" -- "PROMPT"`
#[derive(Debug)]
pub struct ClaudeProvider {
    model_id: ModelId,
    binary_path: PathBuf,
    api_key: String,
    model_name: String,
    timeout: Duration,
}

impl ClaudeProvider {
    /// Create a new Claude provider, validating credentials and binary.
    pub async fn new(model_name: &str, timeout: Duration) -> Result<Self, ProviderError> {
        let api_key =
            std::env::var("ANTHROPIC_API_KEY").map_err(|_| ProviderError::MissingCredential {
                provider: "claude".to_string(),
                var_name: "ANTHROPIC_API_KEY".to_string(),
            })?;

        let binary_path = process::resolve_binary("claude").await?;

        Ok(Self {
            model_id: ModelId::new(format!("claude-{model_name}")),
            binary_path,
            api_key,
            model_name: model_name.to_string(),
            timeout,
        })
    }

    fn build_args(&self, system_prompt: &str, user_prompt: &str) -> Vec<String> {
        vec![
            "-p".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "--tools".to_string(),
            String::new(), // empty string disables all tools
            "--max-turns".to_string(),
            "1".to_string(),
            "--model".to_string(),
            self.model_name.clone(),
            "--append-system-prompt".to_string(),
            system_prompt.to_string(),
            "--".to_string(),
            user_prompt.to_string(),
        ]
    }
}

#[async_trait]
impl ModelProvider for ClaudeProvider {
    async fn send_message(&self, messages: &[Message]) -> Result<String, ProviderError> {
        let (system_prompt, user_prompt) = process::extract_prompts(messages);

        let args = self.build_args(&system_prompt, &user_prompt);
        let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let env_vars = [("ANTHROPIC_API_KEY", self.api_key.as_str())];

        let output = process::spawn_cli(
            &self.binary_path,
            &args_refs,
            &env_vars,
            self.timeout,
            &self.model_id,
        )
        .await?;

        process::extract_claude_response(&output)
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_contains_required_flags() {
        let provider = ClaudeProvider {
            model_id: ModelId::new("claude-sonnet"),
            binary_path: PathBuf::from("/usr/local/bin/claude"),
            api_key: "test-key".to_string(),
            model_name: "sonnet".to_string(),
            timeout: Duration::from_secs(120),
        };

        let args = provider.build_args("system prompt", "user prompt");

        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"json".to_string()));
        assert!(args.contains(&"--tools".to_string()));
        assert!(args.contains(&String::new())); // empty string for --tools
        assert!(args.contains(&"--max-turns".to_string()));
        assert!(args.contains(&"1".to_string()));
        assert!(args.contains(&"--".to_string())); // sentinel
        assert!(args.contains(&"user prompt".to_string()));
    }
}
