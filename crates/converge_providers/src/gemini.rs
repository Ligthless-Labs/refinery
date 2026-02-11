use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use converge_core::ModelProvider;
use converge_core::error::ProviderError;
use converge_core::types::{Message, ModelId};

use crate::process;

/// Gemini CLI provider adapter.
///
/// Invokes: `gemini --output-format json --model gemini-2.5-pro --sandbox --approval-mode plan --allowed-tools "" -- "PROMPT"`
/// System prompt via: `GEMINI_SYSTEM_MD` env var
#[derive(Debug)]
pub struct GeminiProvider {
    model_id: ModelId,
    binary_path: PathBuf,
    api_key: String,
    model_name: String,
    timeout: Duration,
}

impl GeminiProvider {
    /// Create a new Gemini provider, validating credentials and binary.
    pub async fn new(model_name: &str, timeout: Duration) -> Result<Self, ProviderError> {
        let api_key =
            std::env::var("GEMINI_API_KEY").map_err(|_| ProviderError::MissingCredential {
                provider: "gemini".to_string(),
                var_name: "GEMINI_API_KEY".to_string(),
            })?;

        let binary_path = process::resolve_binary("gemini").await?;

        Ok(Self {
            model_id: ModelId::new(format!("gemini-{model_name}")),
            binary_path,
            api_key,
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

        // Gemini uses env var for system prompt (no --system-prompt flag)
        let env_vars = [
            ("GEMINI_API_KEY", self.api_key.as_str()),
            ("GEMINI_SYSTEM_MD", system_prompt.as_str()),
        ];

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

    #[test]
    fn build_args_contains_required_flags() {
        let provider = GeminiProvider {
            model_id: ModelId::new("gemini-2.5-pro"),
            binary_path: PathBuf::from("/usr/local/bin/gemini"),
            api_key: "test-key".to_string(),
            model_name: "gemini-2.5-pro".to_string(),
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
