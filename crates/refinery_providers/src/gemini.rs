use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use refinery_core::ModelProvider;
use refinery_core::error::ProviderError;
use refinery_core::progress::ProgressFn;
use refinery_core::types::{Message, ModelId};

use crate::credential::{self, Credential};
use crate::{process, tools};

/// Gemini CLI provider adapter.
///
/// Invokes: `gemini --output-format json --model gemini-3.1-pro-preview --sandbox --approval-mode plan --prompt "PROMPT"`
/// System prompt via: `GEMINI_SYSTEM_MD` env var
///
/// Supports: `GEMINI_API_KEY` (Google AI Studio) or `GOOGLE_API_KEY` (Vertex AI express mode).
/// When neither is set, falls back to the Gemini CLI's own stored credentials (gcloud auth).
pub struct GeminiProvider {
    model_id: ModelId,
    binary_path: PathBuf,
    credential: Option<Credential>,
    model_name: String,
    allowed_tools: Vec<String>,
    max_timeout: Duration,
    idle_timeout: Duration,
    progress: Option<ProgressFn>,
}

impl std::fmt::Debug for GeminiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiProvider")
            .field("model_id", &self.model_id)
            .field("model_name", &self.model_name)
            .finish_non_exhaustive()
    }
}

impl GeminiProvider {
    /// Create a new Gemini provider, resolving credentials and binary.
    ///
    /// Credentials are optional: if no env var is set the Gemini CLI will use its own
    /// stored authentication (e.g. gcloud credentials).
    pub async fn new(
        model_id: ModelId,
        canonical_tools: &[String],
        max_timeout: Duration,
        idle_timeout: Duration,
        progress: Option<ProgressFn>,
    ) -> Result<Self, ProviderError> {
        let credential =
            credential::try_resolve_credential("gemini", &["GEMINI_API_KEY", "GOOGLE_API_KEY"]);

        let binary_path = process::resolve_binary("gemini").await?;
        let model_name = model_id.model().to_string();

        let (allowed_tools, unknown) = tools::resolve(canonical_tools, tools::gemini_tool);
        for name in &unknown {
            tracing::warn!(provider = "gemini", tool = %name, "unknown tool, skipping");
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

    fn build_args(&self, user_prompt: &str) -> Vec<String> {
        let mut args = vec![
            "--output-format".to_string(),
            "json".to_string(),
            "--model".to_string(),
            self.model_name.clone(),
            "--sandbox".to_string(),
            "--approval-mode".to_string(),
            "plan".to_string(),
        ];

        if self.allowed_tools.is_empty() {
            args.push("--allowed-tools".to_string());
            args.push(String::new());
        } else {
            args.push("--allowed-tools".to_string());
            args.push(self.allowed_tools.join(","));
        }

        args.push("--prompt".to_string());
        args.push(user_prompt.to_string());
        args
    }
}

#[async_trait]
impl ModelProvider for GeminiProvider {
    async fn send_message(&self, messages: &[Message]) -> Result<String, ProviderError> {
        let (system_prompt, user_prompt) = process::extract_prompts(messages);

        let args = self.build_args(&user_prompt);
        let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        // GEMINI_SYSTEM_MD expects a file path, not inline content — write to temp file.
        let tmp_path =
            std::env::temp_dir().join(format!("refinery-gemini-{}.md", std::process::id()));
        std::fs::write(&tmp_path, system_prompt.as_bytes()).map_err(|e| {
            ProviderError::ProcessFailed {
                model: self.model_id.clone(),
                message: format!("failed to write system prompt temp file: {e}"),
                exit_code: None,
            }
        })?;
        let tmp_path_str = tmp_path.to_string_lossy().into_owned();

        // Always pass HOME so the CLI can find gcloud/stored credentials.
        let home = std::env::var("HOME").ok();
        let mut env_vars: Vec<(&str, &str)> = Vec::new();
        if let Some(ref cred) = self.credential {
            env_vars.push(cred.as_env_pair());
        }
        env_vars.push(("GEMINI_SYSTEM_MD", &tmp_path_str));
        if let Some(ref h) = home {
            env_vars.push(("HOME", h.as_str()));
        }

        let result = process::spawn_cli(
            &self.binary_path,
            &args_refs,
            &env_vars,
            self.max_timeout,
            self.idle_timeout,
            &self.model_id,
            self.progress.clone(),
        )
        .await;

        let _ = std::fs::remove_file(&tmp_path);

        process::extract_gemini_response(&result?)
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
            model_id: ModelId::from_parts("gemini-cli", "gemini-3.1-pro-preview"),
            binary_path: PathBuf::from("/usr/local/bin/gemini"),
            credential: Some(test_credential()),
            model_name: "gemini-3.1-pro-preview".to_string(),
            allowed_tools: vec![],
            max_timeout: Duration::from_secs(1800),
            idle_timeout: Duration::from_secs(120),
            progress: None,
        };

        let args = provider.build_args("user prompt");

        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"json".to_string()));
        assert!(args.contains(&"--sandbox".to_string()));
        assert!(args.contains(&"--approval-mode".to_string()));
        assert!(args.contains(&"plan".to_string()));
        assert!(args.contains(&"--prompt".to_string()));
        assert!(args.contains(&"user prompt".to_string()));
    }
}
