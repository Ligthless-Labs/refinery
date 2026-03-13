use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use refinery_core::ModelProvider;
use refinery_core::error::ProviderError;
use refinery_core::types::{Message, ModelId};

use crate::credential::{self, Credential};
use crate::process;

/// Claude CLI provider adapter.
///
/// Invokes: `claude -p --output-format json --tools "" --max-turns 1 --effort high --model claude-opus-4-6 --append-system-prompt "SYSTEM" -- "PROMPT"`
///
/// Supports: `ANTHROPIC_API_KEY` (pay-per-use) or `CLAUDE_CODE_OAUTH_TOKEN` (Pro/Max subscription).
#[derive(Debug)]
pub struct ClaudeProvider {
    model_id: ModelId,
    binary_path: PathBuf,
    credential: Credential,
    model_name: String,
    timeout: Duration,
}

impl ClaudeProvider {
    /// Create a new Claude provider, validating credentials and binary.
    pub async fn new(model_name: &str, timeout: Duration) -> Result<Self, ProviderError> {
        let credential = credential::resolve_credential(
            "claude",
            &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
        )?;

        let binary_path = process::resolve_binary("claude").await?;

        Ok(Self {
            model_id: ModelId::new(format!("claude-{model_name}")),
            binary_path,
            credential,
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
            "--effort".to_string(),
            "high".to_string(),
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

        // Claude CLI needs HOME to find ~/.claude.json (onboarding config)
        let home = if self.credential.env_var() == "CLAUDE_CODE_OAUTH_TOKEN" {
            std::env::var("HOME").ok()
        } else {
            None
        };
        let mut env_vars = vec![self.credential.as_env_pair()];
        if let Some(ref home) = home {
            env_vars.push(("HOME", home.as_str()));
        }

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

    use crate::credential::resolve_credential_with;

    fn test_credential() -> Credential {
        resolve_credential_with("claude", &["ANTHROPIC_API_KEY"], |_| {
            Ok("test-key".to_string())
        })
        .unwrap()
    }

    #[test]
    fn build_args_contains_required_flags() {
        let provider = ClaudeProvider {
            model_id: ModelId::new("claude-opus-4-6"),
            binary_path: PathBuf::from("/usr/local/bin/claude"),
            credential: test_credential(),
            model_name: "opus-4-6".to_string(),
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
