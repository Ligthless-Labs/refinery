use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use refinery_core::ModelProvider;
use refinery_core::error::ProviderError;
use refinery_core::types::{Message, ModelId};

use crate::credential::{self, Credential};
use crate::process;

/// Gemini CLI provider adapter.
///
/// Invokes: `gemini --output-format json --model gemini-3.1-pro-preview --sandbox --approval-mode plan --allowed-tools "" -- "PROMPT"`
/// System prompt via: `GEMINI_SYSTEM_MD` env var
///
/// Supports: `GEMINI_API_KEY` (Google AI Studio) or `GOOGLE_API_KEY` (Vertex AI express mode).
/// When neither is set, falls back to the Gemini CLI's own stored credentials (gcloud auth).
#[derive(Debug)]
pub struct GeminiProvider {
    model_id: ModelId,
    binary_path: PathBuf,
    credential: Option<Credential>,
    model_name: String,
    timeout: Duration,
}

impl GeminiProvider {
    /// Create a new Gemini provider, resolving credentials and binary.
    ///
    /// Credentials are optional: if no env var is set the Gemini CLI will use its own
    /// stored authentication (e.g. gcloud credentials).
    pub async fn new(model_name: &str, timeout: Duration) -> Result<Self, ProviderError> {
        let credential =
            credential::try_resolve_credential("gemini", &["GEMINI_API_KEY", "GOOGLE_API_KEY"]);

        let binary_path = process::resolve_binary("gemini").await?;

        Ok(Self {
            model_id: ModelId::new(model_name),
            binary_path,
            credential,
            model_name: model_name.to_string(),
            timeout,
        })
    }

    fn build_args(&self, user_prompt: &str) -> Vec<String> {
        vec![
            "--output-format".to_string(),
            "json".to_string(),
            "--model".to_string(),
            self.model_name.clone(),
            "--sandbox".to_string(),
            "--approval-mode".to_string(),
            "plan".to_string(),
            "--allowed-tools".to_string(),
            String::new(), // empty string for no tools
            "--".to_string(),
            user_prompt.to_string(),
        ]
    }
}

#[async_trait]
impl ModelProvider for GeminiProvider {
    async fn send_message(&self, messages: &[Message]) -> Result<String, ProviderError> {
        let (system_prompt, user_prompt) = process::extract_prompts(messages);

        let args = self.build_args(&user_prompt);
        let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        // Gemini uses env var for system prompt (no --system-prompt flag).
        // Always pass HOME so the CLI can find gcloud/stored credentials.
        let home = std::env::var("HOME").ok();
        let mut env_vars: Vec<(&str, &str)> = Vec::new();
        if let Some(ref cred) = self.credential {
            env_vars.push(cred.as_env_pair());
        }
        env_vars.push(("GEMINI_SYSTEM_MD", system_prompt.as_str()));
        if let Some(ref h) = home {
            env_vars.push(("HOME", h.as_str()));
        }

        let output = process::spawn_cli(
            &self.binary_path,
            &args_refs,
            &env_vars,
            self.timeout,
            &self.model_id,
        )
        .await?;

        process::extract_gemini_response(&output)
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
        resolve_credential_with(
            "gemini",
            &["GEMINI_API_KEY"],
            |_| Ok("test-key".to_string()),
        )
        .unwrap()
    }

    #[test]
    fn build_args_contains_required_flags() {
        let provider = GeminiProvider {
            model_id: ModelId::new("gemini-3.1-pro-preview"),
            binary_path: PathBuf::from("/usr/local/bin/gemini"),
            credential: Some(test_credential()),
            model_name: "gemini-3.1-pro-preview".to_string(),
            timeout: Duration::from_secs(120),
        };

        let args = provider.build_args("user prompt");

        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"json".to_string()));
        assert!(args.contains(&"--sandbox".to_string()));
        assert!(args.contains(&"--approval-mode".to_string()));
        assert!(args.contains(&"plan".to_string()));
        assert!(args.contains(&"--allowed-tools".to_string()));
        assert!(args.contains(&String::new())); // empty for no tools
        assert!(args.contains(&"--".to_string())); // sentinel
        assert!(args.contains(&"user prompt".to_string()));
    }
}
