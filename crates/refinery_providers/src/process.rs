use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

use refinery_core::error::ProviderError;
use refinery_core::types::{Message, ModelId, Role};
use tokio::process::Command;
use tracing::{debug, warn};

/// Maximum response size in bytes (100KB).
const MAX_RESPONSE_SIZE: usize = 100_000;

/// Return a sanitized PATH for child processes.
///
/// - Uses `var_os` to preserve non-UTF-8 entries.
/// - Falls back to a minimal default when the parent PATH is missing or empty.
/// - Strips empty and `"."` segments to reduce PATH-hijack risk.
fn sanitized_path() -> OsString {
    let default = std::env::join_paths(["/usr/bin", "/usr/local/bin", "/bin"])
        .unwrap_or_default();
    let base = std::env::var_os("PATH")
        .filter(|v| !v.is_empty())
        .unwrap_or(default.clone());
    std::env::join_paths(
        std::env::split_paths(&base)
            .filter(|p| !p.as_os_str().is_empty() && p.as_os_str() != OsStr::new(".")),
    )
    .unwrap_or(base)
}

/// Resolve a CLI binary to its absolute path via `which`.
///
/// Must be called BEFORE `env_clear()` so PATH is still available.
pub async fn resolve_binary(name: &str) -> Result<PathBuf, ProviderError> {
    let output = Command::new("which")
        .arg(name)
        .output()
        .await
        .map_err(|_| ProviderError::BinaryNotFound {
            binary_name: name.to_string(),
        })?;

    if !output.status.success() {
        return Err(ProviderError::BinaryNotFound {
            binary_name: name.to_string(),
        });
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err(ProviderError::BinaryNotFound {
            binary_name: name.to_string(),
        });
    }

    Ok(PathBuf::from(path))
}

/// Spawn a CLI subprocess with proper isolation and capture its output.
///
/// - Uses absolute binary path (resolved via `resolve_binary`)
/// - Clears environment, injects only specified vars + minimal PATH
/// - Sets `kill_on_drop(true)` for cleanup
/// - Applies timeout
pub async fn spawn_cli(
    binary_path: &PathBuf,
    args: &[&str],
    env_vars: &[(&str, &str)],
    timeout: Duration,
    model: &ModelId,
) -> Result<String, ProviderError> {
    let mut cmd = Command::new(binary_path);

    // Security: clear all inherited environment
    cmd.env_clear();

    // Inherit a sanitized PATH so CLI tools can find their own dependencies (e.g. node for gemini)
    cmd.env("PATH", sanitized_path());

    // Inherit TMPDIR — required by many CLIs on macOS for temp file creation
    if let Ok(tmpdir) = std::env::var("TMPDIR") {
        cmd.env("TMPDIR", tmpdir);
    }

    // Inject provider-specific env vars
    for (key, value) in env_vars {
        cmd.env(key, value);
    }

    // Add arguments
    for arg in args {
        cmd.arg(arg);
    }

    // Security: kill child on drop
    cmd.kill_on_drop(true);

    debug!(
        model = %model,
        binary = ?binary_path,
        args = ?args,
        "spawning CLI subprocess"
    );

    let result = tokio::time::timeout(timeout, cmd.output()).await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Some CLIs (e.g. claude) print errors to stdout rather than stderr.
                // Prefer stderr; fall back to stdout so the error is never silently swallowed.
                let message = if stderr.is_empty() { stdout.as_str() } else { stderr.as_ref() };
                warn!(
                    model = %model,
                    exit_code = ?output.status.code(),
                    stderr = %stderr,
                    stdout = %stdout,
                    "CLI process failed"
                );
                return Err(ProviderError::ProcessFailed {
                    model: model.clone(),
                    message: message.to_string(),
                    exit_code: output.status.code(),
                });
            }

            // Check response size
            if stdout.len() > MAX_RESPONSE_SIZE {
                return Err(ProviderError::ResponseTooLarge {
                    model: model.clone(),
                    size: stdout.len(),
                    max: MAX_RESPONSE_SIZE,
                });
            }

            Ok(stdout)
        }
        Ok(Err(e)) => Err(ProviderError::ProcessFailed {
            model: model.clone(),
            message: e.to_string(),
            exit_code: None,
        }),
        Err(_) => Err(ProviderError::Timeout {
            model: model.clone(),
            elapsed: timeout,
        }),
    }
}

/// Extract system and user prompts from a message slice.
#[must_use]
pub fn extract_prompts(messages: &[Message]) -> (String, String) {
    let system = messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let user = messages
        .iter()
        .filter(|m| m.role == Role::User)
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    (system, user)
}

/// Extract the response text from a Codex JSONL event stream.
///
/// Parses all JSONL lines and finds the last `turn.completed` event.
pub fn extract_codex_response(jsonl: &str) -> Result<String, ProviderError> {
    let model = ModelId::new("codex");

    for line in jsonl.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) {
            if parsed.get("type").and_then(|t| t.as_str()) == Some("turn.completed") {
                if let Some(text) = parsed.get("text").and_then(|t| t.as_str()) {
                    return Ok(text.to_string());
                }
            }
        }
    }

    Err(ProviderError::InvalidJson {
        model,
        message: "no turn.completed event found in JSONL stream".to_string(),
    })
}

/// Extract the response text from a Gemini JSON envelope.
///
/// Handles Issue #11184: response field may contain markdown-wrapped JSON.
pub fn extract_gemini_response(json_text: &str) -> Result<String, ProviderError> {
    let model = ModelId::new("gemini");

    let parsed: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| ProviderError::InvalidJson {
            model: model.clone(),
            message: e.to_string(),
        })?;

    // Check for error field
    if let Some(error) = parsed.get("error") {
        if !error.is_null() {
            return Err(ProviderError::ProcessFailed {
                model,
                message: error.to_string(),
                exit_code: None,
            });
        }
    }

    let response = parsed
        .get("response")
        .and_then(|r| r.as_str())
        .ok_or_else(|| ProviderError::InvalidJson {
            model: model.clone(),
            message: "missing 'response' field".to_string(),
        })?;

    // Strip markdown fences if present (Issue #11184)
    if let Some(inner) = refinery_core::prompts::extract_json(response) {
        Ok(inner.to_string())
    } else {
        Ok(response.to_string())
    }
}

/// Extract the response text from a Claude JSON envelope.
pub fn extract_claude_response(json_text: &str) -> Result<String, ProviderError> {
    let model = ModelId::new("claude");

    let parsed: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| ProviderError::InvalidJson {
            model: model.clone(),
            message: e.to_string(),
        })?;

    if parsed.get("is_error").and_then(serde_json::Value::as_bool) == Some(true) {
        let message = parsed
            .get("result")
            .and_then(|r| r.as_str())
            .unwrap_or("unknown error");
        return Err(ProviderError::ProcessFailed {
            model,
            message: message.to_string(),
            exit_code: None,
        });
    }

    parsed
        .get("result")
        .and_then(|r| r.as_str())
        .map(String::from)
        .ok_or_else(|| ProviderError::InvalidJson {
            model,
            message: "missing 'result' field".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_claude_valid() {
        let json =
            r#"{"type":"result","result":"Hello world","session_id":"abc","is_error":false}"#;
        let result = extract_claude_response(json).unwrap();
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn extract_claude_error() {
        let json = r#"{"type":"result","result":"Something went wrong","is_error":true}"#;
        assert!(extract_claude_response(json).is_err());
    }

    #[test]
    fn extract_codex_valid() {
        let jsonl = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"turn.started","turn_id":"u1"}
{"type":"item.text_delta","content":"partial"}
{"type":"turn.completed","turn_id":"u1","text":"Full response text","usage":{"input_tokens":100,"output_tokens":200}}"#;
        let result = extract_codex_response(jsonl).unwrap();
        assert_eq!(result, "Full response text");
    }

    #[test]
    fn extract_codex_no_turn_completed() {
        let jsonl = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"turn.started","turn_id":"u1"}"#;
        assert!(extract_codex_response(jsonl).is_err());
    }

    #[test]
    fn extract_gemini_valid() {
        let json = r#"{"response":"Hello from Gemini","stats":{"models":["gemini-2.5-pro"]},"error":null}"#;
        let result = extract_gemini_response(json).unwrap();
        assert_eq!(result, "Hello from Gemini");
    }

    #[test]
    fn extract_gemini_markdown_wrapped() {
        let json = r#"{"response":"```json\n{\"answer\": \"test\"}\n```","stats":{},"error":null}"#;
        let result = extract_gemini_response(json).unwrap();
        assert!(result.contains("answer"));
    }

    #[test]
    fn extract_gemini_error() {
        let json = r#"{"response":null,"error":"auth failed"}"#;
        assert!(extract_gemini_response(json).is_err());
    }

    #[tokio::test]
    async fn resolve_binary_echo() {
        let result = resolve_binary("echo").await;
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.exists());
    }

    #[tokio::test]
    async fn resolve_binary_not_found() {
        let result = resolve_binary("definitely_not_a_real_binary_xyz123").await;
        assert!(result.is_err());
    }
}
