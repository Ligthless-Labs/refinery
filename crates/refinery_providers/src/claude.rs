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
/// Invokes: `claude -p --output-format json --json-schema {...} --max-turns 10 --effort high --model claude-opus-4-6 --append-system-prompt "SYSTEM" -- "PROMPT"`
///
/// With `--json-schema`, Claude uses a `StructuredOutput` tool call internally.
/// The final `type: "result"` event carries the answer in `structured_output.answer`
/// (the `result` field is empty).
///
/// Supports: `ANTHROPIC_API_KEY` (pay-per-use) or `CLAUDE_CODE_OAUTH_TOKEN` (Pro/Max subscription).
/// When neither is set, falls back to the Claude CLI's own stored credentials (`~/.claude.json`).
#[derive(Debug)]
pub struct ClaudeProvider {
    model_id: ModelId,
    binary_path: PathBuf,
    credential: Option<Credential>,
    model_name: String,
    max_timeout: Duration,
    idle_timeout: Duration,
}

impl ClaudeProvider {
    /// Create a new Claude provider, resolving credentials and binary.
    ///
    /// Credentials are optional: if no env var is set the Claude CLI will use its own
    /// stored authentication (e.g. `~/.claude.json`).
    pub async fn new(
        model_name: &str,
        max_timeout: Duration,
        idle_timeout: Duration,
    ) -> Result<Self, ProviderError> {
        let credential = credential::try_resolve_credential(
            "claude",
            &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
        );

        let binary_path = process::resolve_binary("claude").await?;

        Ok(Self {
            model_id: ModelId::new(format!("claude-{model_name}")),
            binary_path,
            credential,
            model_name: model_name.to_string(),
            max_timeout,
            idle_timeout,
        })
    }

    fn build_args(&self, system_prompt: &str, user_prompt: &str) -> Vec<String> {
        vec![
            "-p".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--json-schema".to_string(),
            r#"{"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false}"#
                .to_string(),
            "--max-turns".to_string(),
            "10".to_string(), // structured output requires multiple turns (hook → StructuredOutput tool)
            "--effort".to_string(),
            "high".to_string(),
            "--model".to_string(),
            format!("claude-{}", self.model_name),
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

        // Always pass HOME so the CLI can find ~/.claude.json for stored credentials.
        let home = std::env::var("HOME").ok();
        let mut env_vars: Vec<(&str, &str)> = Vec::new();
        if let Some(ref cred) = self.credential {
            env_vars.push(cred.as_env_pair());
        }
        if let Some(ref h) = home {
            env_vars.push(("HOME", h.as_str()));
        }

        let output = process::spawn_cli(
            &self.binary_path,
            &args_refs,
            &env_vars,
            self.max_timeout,
            self.idle_timeout,
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
            credential: Some(test_credential()),
            model_name: "opus-4-6".to_string(),
            max_timeout: Duration::from_secs(1800),
            idle_timeout: Duration::from_secs(120),
        };

        let args = provider.build_args("system prompt", "user prompt");

        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--json-schema".to_string()));
        assert!(args.contains(&"--effort".to_string()));
        assert!(args.contains(&"high".to_string()));
        assert!(args.contains(&"--max-turns".to_string()));
        assert!(args.contains(&"10".to_string()));
        assert!(args.contains(&"--".to_string())); // sentinel
        assert!(args.contains(&"user prompt".to_string()));
    }
}
