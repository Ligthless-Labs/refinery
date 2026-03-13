use std::fmt;

use refinery_core::error::ProviderError;
use tracing::info;

/// A resolved credential: the env var name and its value.
///
/// Fields are private to ensure construction only through [`resolve_credential`].
/// The [`Debug`] impl redacts the value to prevent credential leakage in logs.
pub struct Credential {
    env_var: &'static str,
    value: String,
}

impl Credential {
    /// The environment variable name to inject into the subprocess.
    #[must_use]
    pub fn env_var(&self) -> &'static str {
        self.env_var
    }

    /// The credential value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Returns the `(env_var, value)` pair for subprocess env injection.
    #[must_use]
    pub fn as_env_pair(&self) -> (&str, &str) {
        (self.env_var, &self.value)
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credential")
            .field("env_var", &self.env_var)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

/// Try env vars in order. Return the first non-empty match, or `MissingCredential` error.
pub fn resolve_credential(
    provider: &str,
    candidates: &[&'static str],
) -> Result<Credential, ProviderError> {
    resolve_credential_with(provider, candidates, |key| std::env::var(key))
}

/// Try env vars in order. Return `Some` on match, `None` when none are set.
///
/// Use this when a provider can fall back to the CLI's own stored credentials.
#[must_use] 
pub fn try_resolve_credential(provider: &str, candidates: &[&'static str]) -> Option<Credential> {
    resolve_credential(provider, candidates).ok()
}

/// Testable variant: accepts a custom env var reader.
///
/// Avoids `std::env::set_var` (unsafe in Rust 2024 edition) in tests.
pub(crate) fn resolve_credential_with<F>(
    provider: &str,
    candidates: &[&'static str],
    reader: F,
) -> Result<Credential, ProviderError>
where
    F: Fn(&str) -> Result<String, std::env::VarError>,
{
    for &var in candidates {
        if let Ok(value) = reader(var) {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                info!(provider, env_var = var, "credential resolved");
                return Ok(Credential {
                    env_var: var,
                    value: trimmed,
                });
            }
        }
    }
    Err(ProviderError::MissingCredential {
        provider: provider.to_string(),
        var_name: candidates.join(" or "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_reader(vars: &[(&str, &str)]) -> impl Fn(&str) -> Result<String, std::env::VarError> {
        let map: std::collections::HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |key: &str| map.get(key).cloned().ok_or(std::env::VarError::NotPresent)
    }

    #[test]
    fn resolves_first_candidate() {
        let reader = mock_reader(&[("ANTHROPIC_API_KEY", "sk-ant-123")]);
        let cred = resolve_credential_with(
            "claude",
            &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
            reader,
        )
        .unwrap();
        assert_eq!(cred.env_var(), "ANTHROPIC_API_KEY");
        assert_eq!(cred.value(), "sk-ant-123");
    }

    #[test]
    fn falls_back_to_second_candidate() {
        let reader = mock_reader(&[("CLAUDE_CODE_OAUTH_TOKEN", "sk-ant-oat01-xyz")]);
        let cred = resolve_credential_with(
            "claude",
            &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
            reader,
        )
        .unwrap();
        assert_eq!(cred.env_var(), "CLAUDE_CODE_OAUTH_TOKEN");
        assert_eq!(cred.value(), "sk-ant-oat01-xyz");
    }

    #[test]
    fn both_present_uses_first() {
        let reader = mock_reader(&[
            ("ANTHROPIC_API_KEY", "sk-ant-123"),
            ("CLAUDE_CODE_OAUTH_TOKEN", "sk-ant-oat01-xyz"),
        ]);
        let cred = resolve_credential_with(
            "claude",
            &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
            reader,
        )
        .unwrap();
        assert_eq!(cred.env_var(), "ANTHROPIC_API_KEY");
    }

    #[test]
    fn neither_present_returns_error() {
        let reader = mock_reader(&[]);
        let err = resolve_credential_with(
            "claude",
            &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
            reader,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ANTHROPIC_API_KEY"));
        assert!(msg.contains("CLAUDE_CODE_OAUTH_TOKEN"));
        assert!(msg.contains("claude"));
    }

    #[test]
    fn empty_string_treated_as_not_set() {
        let reader = mock_reader(&[
            ("ANTHROPIC_API_KEY", ""),
            ("CLAUDE_CODE_OAUTH_TOKEN", "sk-ant-oat01-xyz"),
        ]);
        let cred = resolve_credential_with(
            "claude",
            &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
            reader,
        )
        .unwrap();
        assert_eq!(cred.env_var(), "CLAUDE_CODE_OAUTH_TOKEN");
    }

    #[test]
    fn whitespace_only_treated_as_not_set() {
        let reader = mock_reader(&[
            ("ANTHROPIC_API_KEY", "   "),
            ("CLAUDE_CODE_OAUTH_TOKEN", "sk-ant-oat01-xyz"),
        ]);
        let cred = resolve_credential_with(
            "claude",
            &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
            reader,
        )
        .unwrap();
        assert_eq!(cred.env_var(), "CLAUDE_CODE_OAUTH_TOKEN");
    }

    #[test]
    fn debug_redacts_value() {
        let reader = mock_reader(&[("KEY", "super-secret")]);
        let cred = resolve_credential_with("test", &["KEY"], reader).unwrap();
        let debug_str = format!("{cred:?}");
        assert!(debug_str.contains("REDACTED"));
        assert!(!debug_str.contains("super-secret"));
    }

    #[test]
    fn as_env_pair_returns_correct_tuple() {
        let reader = mock_reader(&[("MY_KEY", "my-value")]);
        let cred = resolve_credential_with("test", &["MY_KEY"], reader).unwrap();
        let (k, v) = cred.as_env_pair();
        assert_eq!(k, "MY_KEY");
        assert_eq!(v, "my-value");
    }

    #[test]
    fn surrounding_whitespace_trimmed() {
        let reader = mock_reader(&[("KEY", "  sk-ant-123  ")]);
        let cred = resolve_credential_with("test", &["KEY"], reader).unwrap();
        assert_eq!(cred.value(), "sk-ant-123");
    }
}
