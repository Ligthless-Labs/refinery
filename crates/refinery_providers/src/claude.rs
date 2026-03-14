use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use refinery_core::ModelProvider;
use refinery_core::error::ProviderError;
use refinery_core::progress::ProgressFn;
use refinery_core::types::{Message, ModelId};

use crate::credential::{self, Credential};
use crate::{process, tools};

/// Claude CLI provider adapter.
///
/// Invokes: `claude -p --output-format json --json-schema {...} --max-turns 10 --effort high --model claude-opus-4-6 --append-system-prompt "SYSTEM" -- "PROMPT"`
///
/// When a JSON schema is provided, Claude uses a `StructuredOutput` tool call internally.
/// The final `type: "result"` event carries the structured data in `structured_output`
/// (the `result` field is empty). The schema varies by phase (answer vs evaluation).
///
/// Supports: `ANTHROPIC_API_KEY` (pay-per-use) or `CLAUDE_CODE_OAUTH_TOKEN` (Pro/Max subscription).
/// When neither is set, falls back to the Claude CLI's own stored credentials (`~/.claude.json`).
pub struct ClaudeProvider {
    model_id: ModelId,
    binary_path: PathBuf,
    credential: Option<Credential>,
    model_name: String,
    allowed_tools: Vec<String>,
    max_timeout: Duration,
    idle_timeout: Duration,
    progress: Option<ProgressFn>,
}

impl std::fmt::Debug for ClaudeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeProvider")
            .field("model_id", &self.model_id)
            .field("model_name", &self.model_name)
            .finish_non_exhaustive()
    }
}

impl ClaudeProvider {
    /// Create a new Claude provider, resolving credentials and binary.
    ///
    /// Credentials are optional: if no env var is set the Claude CLI will use its own
    /// stored authentication (e.g. `~/.claude.json`).
    pub async fn new(
        model_id: ModelId,
        canonical_tools: &[String],
        max_timeout: Duration,
        idle_timeout: Duration,
        progress: Option<ProgressFn>,
    ) -> Result<Self, ProviderError> {
        let credential = credential::try_resolve_credential(
            "claude",
            &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
        );

        let binary_path = process::resolve_binary("claude").await?;
        let model_name = model_id.model().to_string();

        let (allowed_tools, unknown) = tools::resolve(canonical_tools, tools::claude_tool);
        for name in &unknown {
            tracing::warn!(provider = "claude", tool = %name, "unknown tool, skipping");
        }

        Ok(Self {
            model_id,
            binary_path,
            credential,
            model_name,
            allowed_tools,
            max_timeout,
            idle_timeout,
            progress,
        })
    }

    fn build_args(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        schema: Option<&str>,
    ) -> Vec<String> {
        let mut args = vec![
            "-p".to_string(),
            "--verbose".to_string(), // required for stream-json in print mode
            "--output-format".to_string(),
            "stream-json".to_string(),
        ];

        if let Some(s) = schema {
            args.push("--json-schema".to_string());
            args.push(s.to_string());
        }

        args.extend([
            "--max-turns".to_string(),
            "50".to_string(), // structured output requires multiple turns (hook → StructuredOutput tool)
            "--effort".to_string(),
            "high".to_string(),
            "--model".to_string(),
            self.model_name.clone(),
            "--append-system-prompt".to_string(),
            system_prompt.to_string(),
        ]);

        if self.allowed_tools.is_empty() {
            // No tools requested — disable all tools
            args.push("--tools".to_string());
            args.push(String::new());
        } else {
            args.push("--allowedTools".to_string());
            args.push(self.allowed_tools.join(","));
        }

        args.push("--".to_string());
        args.push(user_prompt.to_string());
        args
    }
}

#[async_trait]
impl ModelProvider for ClaudeProvider {
    async fn send_message(
        &self,
        messages: &[Message],
        schema: Option<&str>,
    ) -> Result<String, ProviderError> {
        let (system_prompt, user_prompt) = process::extract_prompts(messages);

        let args = self.build_args(&system_prompt, &user_prompt, schema);
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
            self.progress.clone(),
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
            model_id: ModelId::from_parts("claude-code", "claude-opus-4-6"),
            binary_path: PathBuf::from("/usr/local/bin/claude"),
            credential: Some(test_credential()),
            model_name: "opus-4-6".to_string(),
            allowed_tools: vec![],
            max_timeout: Duration::from_secs(1800),
            idle_timeout: Duration::from_secs(120),
            progress: None,
        };

        let args =
            provider.build_args("system prompt", "user prompt", Some(r#"{"type":"object"}"#));

        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"--verbose".to_string()));
        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--json-schema".to_string()));
        assert!(args.contains(&"--effort".to_string()));
        assert!(args.contains(&"high".to_string()));
        assert!(args.contains(&"--max-turns".to_string()));
        assert!(args.contains(&"50".to_string()));
        assert!(args.contains(&"--".to_string())); // sentinel
        assert!(args.contains(&"user prompt".to_string()));
    }
}
