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
/// Invokes: `codex exec --json --sandbox read-only --output-schema <file> -m gpt-5.4 -c model_reasoning_effort="xhigh" -- "PROMPT"`
/// System prompt via: `--config developer_instructions="..."`
///
/// Supports: `OPENAI_API_KEY` (pay-per-use) or `CODEX_API_KEY` (for `codex exec`).
/// When neither is set, falls back to the Codex CLI's own stored credentials.
#[derive(Debug)]
pub struct CodexProvider {
    model_id: ModelId,
    binary_path: PathBuf,
    credential: Option<Credential>,
    model_name: String,
    reasoning_effort: String,
    timeout: Duration,
}

impl CodexProvider {
    /// Create a new Codex provider, resolving credentials and binary.
    ///
    /// Credentials are optional: if no env var is set the Codex CLI will use its own
    /// stored authentication.
    pub async fn new(
        model_name: &str,
        reasoning_effort: &str,
        timeout: Duration,
    ) -> Result<Self, ProviderError> {
        let credential =
            credential::try_resolve_credential("codex", &["OPENAI_API_KEY", "CODEX_API_KEY"]);

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

    fn build_args(&self, system_prompt: &str, user_prompt: &str, schema_path: &str) -> Vec<String> {
        vec![
            "exec".to_string(),
            "--json".to_string(),
            "--sandbox".to_string(),
            "read-only".to_string(),
            "--output-schema".to_string(),
            schema_path.to_string(),
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

        // --output-schema expects a file path — write schema to temp file.
        let schema_path =
            std::env::temp_dir().join(format!("refinery-codex-schema-{}.json", std::process::id()));
        let schema = r#"{"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false}"#;
        std::fs::write(&schema_path, schema).map_err(|e| ProviderError::ProcessFailed {
            model: self.model_id.clone(),
            message: format!("failed to write schema temp file: {e}"),
            exit_code: None,
        })?;
        let schema_path_str = schema_path.to_string_lossy().into_owned();

        let args = self.build_args(&system_prompt, &user_prompt, &schema_path_str);
        let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let home = std::env::var("HOME").ok();
        let mut env_vars: Vec<(&str, &str)> = Vec::new();
        if let Some(ref cred) = self.credential {
            env_vars.push(cred.as_env_pair());
        }
        if let Some(ref h) = home {
            env_vars.push(("HOME", h.as_str()));
        }

        let result = process::spawn_cli(
            &self.binary_path,
            &args_refs,
            &env_vars,
            self.timeout,
            &self.model_id,
        )
        .await;

        let _ = std::fs::remove_file(&schema_path);

        // Codex outputs JSONL; extract answer from turn.completed
        process::extract_codex_response(&result?)
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
            credential: Some(test_credential()),
            model_name: "gpt-5.4".to_string(),
            reasoning_effort: "xhigh".to_string(),
            timeout: Duration::from_secs(120),
        };

        let args = provider.build_args("system prompt", "user prompt", "/tmp/schema.json");

        assert!(args.contains(&"exec".to_string()));
        assert!(args.contains(&"--json".to_string()));
        assert!(args.contains(&"--sandbox".to_string()));
        assert!(args.contains(&"read-only".to_string()));
        assert!(args.contains(&"--output-schema".to_string()));
        assert!(args.contains(&"/tmp/schema.json".to_string()));
        assert!(args.contains(&"--".to_string())); // sentinel
        assert!(args.contains(&"user prompt".to_string()));
    }
}
