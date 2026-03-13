use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use refinery_core::error::ProviderError;
use refinery_core::progress::{ProgressEvent, ProgressFn};
use refinery_core::types::{Message, ModelId, Role};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, warn};

/// Maximum response size in bytes (1MB).
const MAX_RESPONSE_SIZE: usize = 1_000_000;

/// Return a sanitized PATH for child processes.
///
/// - Uses `var_os` to preserve non-UTF-8 entries.
/// - Falls back to a minimal default when the parent PATH is missing or empty.
/// - Strips empty and `"."` segments to reduce PATH-hijack risk.
fn sanitized_path() -> OsString {
    let default = std::env::join_paths(["/usr/bin", "/usr/local/bin", "/bin"]).unwrap_or_default();
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
/// - Reads stdout line-by-line, resetting an **idle timeout** on each line
/// - A hard **max timeout** caps total wall-clock time
#[allow(clippy::too_many_lines)]
pub async fn spawn_cli(
    binary_path: &PathBuf,
    args: &[&str],
    env_vars: &[(&str, &str)],
    max_timeout: Duration,
    idle_timeout: Duration,
    model: &ModelId,
    progress: Option<ProgressFn>,
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

    // Capture stdout as a pipe so we can read it incrementally
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // Security: kill child on drop
    cmd.kill_on_drop(true);

    debug!(
        model = %model,
        binary = ?binary_path,
        args = ?args,
        max_timeout = ?max_timeout,
        idle_timeout = ?idle_timeout,
        "spawning CLI subprocess"
    );

    let mut child = cmd.spawn().map_err(|e| ProviderError::ProcessFailed {
        model: model.clone(),
        message: e.to_string(),
        exit_code: None,
    })?;

    // Take stdout/stderr handles — these are Option, unwrap is safe because we set Stdio::piped()
    let stdout_handle = child.stdout.take().expect("stdout piped");
    let stderr_handle = child.stderr.take().expect("stderr piped");

    // Drain stderr concurrently to prevent pipe-buffer deadlock.
    // If a subprocess writes >64KB to stderr while we block on stdout,
    // the OS pipe buffer fills and the subprocess blocks, stalling stdout too.
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr_handle);
        let mut buf = String::new();
        while let Ok(n) = reader.read_line(&mut buf).await {
            if n == 0 {
                break;
            }
        }
        buf
    });

    let model_clone = model.clone();

    // Read stdout line-by-line with idle timeout, under a hard wall-clock cap
    let streaming_read = async {
        let mut reader = BufReader::new(stdout_handle);
        let mut collected = String::new();
        let mut line_buf = String::new();
        let mut line_count: usize = 0;
        let start = Instant::now();

        loop {
            line_buf.clear();
            let read_result =
                tokio::time::timeout(idle_timeout, reader.read_line(&mut line_buf)).await;

            match read_result {
                Ok(Ok(0)) => break, // EOF
                Ok(Ok(_)) => {
                    line_count += 1;
                    let preview: String = line_buf.trim_end().chars().take(200).collect();
                    debug!(model = %model_clone, line = %preview, "stream event");
                    if let Some(ref cb) = progress {
                        cb(ProgressEvent::SubprocessOutput {
                            model: model_clone.clone(),
                            lines: line_count,
                            elapsed: start.elapsed(),
                        });
                    }
                    collected.push_str(&line_buf);
                    if collected.len() > MAX_RESPONSE_SIZE {
                        return Err(ProviderError::ResponseTooLarge {
                            model: model_clone.clone(),
                            size: collected.len(),
                            max: MAX_RESPONSE_SIZE,
                        });
                    }
                }
                Ok(Err(e)) => {
                    return Err(ProviderError::ProcessFailed {
                        model: model_clone.clone(),
                        message: format!("stdout read error: {e}"),
                        exit_code: None,
                    });
                }
                Err(_) => {
                    // Idle timeout — no output for idle_timeout duration
                    return Err(ProviderError::IdleTimeout {
                        model: model_clone.clone(),
                        idle: idle_timeout,
                    });
                }
            }
        }

        Ok(collected)
    };

    // Apply hard wall-clock timeout over the entire streaming read
    let result = tokio::time::timeout(max_timeout, streaming_read).await;

    match result {
        Ok(Ok(stdout)) => {
            // Wait for process exit and collect stderr from the drain task
            let status = child
                .wait()
                .await
                .map_err(|e| ProviderError::ProcessFailed {
                    model: model.clone(),
                    message: e.to_string(),
                    exit_code: None,
                })?;

            let stderr = stderr_task.await.unwrap_or_default();

            if !status.success() {
                let message = if stderr.is_empty() {
                    stdout.as_str()
                } else {
                    stderr.as_str()
                };
                warn!(
                    model = %model,
                    exit_code = ?status.code(),
                    stderr = %stderr,
                    stdout_len = stdout.len(),
                    "CLI process failed"
                );
                return Err(ProviderError::ProcessFailed {
                    model: model.clone(),
                    message: message.to_string(),
                    exit_code: status.code(),
                });
            }

            Ok(stdout)
        }
        Ok(Err(e)) => Err(e), // IdleTimeout or other streaming error
        Err(_) => Err(ProviderError::Timeout {
            model: model.clone(),
            elapsed: max_timeout,
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
/// Parses all JSONL lines in reverse and finds the last completion event.
/// Supports two event formats:
/// - `item.completed`: `{"type":"item.completed","item":{"text":"{\"answer\":\"...\"}"}}`
/// - `turn.completed`: `{"type":"turn.completed","text":"{\"answer\":\"...\"}"}`
///
/// With `--output-schema`, the `text` field contains JSON (`{"answer":"..."}`),
/// which is parsed to extract the `answer` value. Without a schema, `text` is
/// returned as-is.
pub fn extract_codex_response(jsonl: &str) -> Result<String, ProviderError> {
    let model = ModelId::from_parts("codex-cli", "unknown");
    let preview: String = jsonl.chars().take(200).collect();

    for line in jsonl.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let event_type = parsed.get("type").and_then(|t| t.as_str()).unwrap_or("");

        // Extract text from turn.completed (top-level) or item.completed (nested in item)
        let text = match event_type {
            "turn.completed" => parsed.get("text").and_then(|t| t.as_str()),
            "item.completed" => parsed
                .get("item")
                .and_then(|i| i.get("text"))
                .and_then(|t| t.as_str()),
            _ => continue,
        };

        if let Some(text) = text {
            // text is `{"answer":"..."}` when --output-schema is used
            if let Ok(inner) = serde_json::from_str::<serde_json::Value>(text) {
                if let Some(answer) = inner.get("answer").and_then(|a| a.as_str()) {
                    return Ok(answer.to_string());
                }
            }
            // fallback: plain text (no schema)
            return Ok(text.to_string());
        }
    }

    Err(ProviderError::InvalidJson {
        model,
        message: format!("no turn.completed event found in JSONL stream (raw: {preview})"),
    })
}

/// Extract the response text from a Gemini JSON envelope.
///
/// Handles Issue #11184: response field may contain markdown-wrapped JSON.
pub fn extract_gemini_response(json_text: &str) -> Result<String, ProviderError> {
    let model = ModelId::from_parts("gemini-cli", "unknown");

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

/// Try to extract the answer from a single Claude result event.
///
/// Checks `structured_output.answer` first, then falls back to `result`.
fn extract_from_result_event(event: &serde_json::Value) -> Option<String> {
    if event.get("type").and_then(|t| t.as_str()) != Some("result") {
        return None;
    }
    if let Some(answer) = event
        .get("structured_output")
        .and_then(|so| so.get("answer"))
        .and_then(|a| a.as_str())
    {
        return Some(answer.to_string());
    }
    event
        .get("result")
        .and_then(|r| r.as_str())
        .filter(|r| !r.is_empty())
        .map(String::from)
}

/// Extract the response from Claude's `--output-format stream-json` + `--json-schema` output.
///
/// The CLI emits JSONL (one JSON object per line). The final event has
/// `"type":"result"` and carries `structured_output.answer` (the `result`
/// field itself is empty when `--json-schema` is used).
///
/// Also accepts `--output-format json` (a JSON array) for backwards compatibility.
pub fn extract_claude_response(output: &str) -> Result<String, ProviderError> {
    let model = ModelId::from_parts("claude-code", "unknown");
    let preview: String = output.chars().take(200).collect();

    // Try JSONL first (stream-json format): parse each line independently
    for line in output.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(answer) = extract_from_result_event(&parsed) {
            return Ok(answer);
        }
    }

    // Fallback: try parsing as a JSON array (--output-format json)
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(output) {
        let events: Vec<&serde_json::Value> = if let Some(arr) = parsed.as_array() {
            arr.iter().collect()
        } else {
            vec![&parsed]
        };
        for event in events.iter().rev() {
            if let Some(answer) = extract_from_result_event(event) {
                return Ok(answer);
            }
        }
    }

    Err(ProviderError::InvalidJson {
        model,
        message: format!("no result event with structured_output found in stream (raw: {preview})"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_claude_structured_output() {
        let json = r#"[{"type":"system","subtype":"init"},{"type":"assistant","message":{"content":[{"type":"text","text":"Hello!"}]}},{"type":"result","subtype":"success","is_error":false,"result":"","structured_output":{"answer":"Hello! How can I help you today?"}}]"#;
        let result = extract_claude_response(json).unwrap();
        assert_eq!(result, "Hello! How can I help you today?");
    }

    #[test]
    fn extract_claude_stream_json_jsonl() {
        // stream-json format: one JSON object per line (JSONL)
        let jsonl = r#"{"type":"system","subtype":"init","session_id":"abc123"}
{"type":"assistant","message":{"content":[{"type":"text","text":"Thinking..."}]}}
{"type":"result","subtype":"success","is_error":false,"result":"","structured_output":{"answer":"Stream JSON answer"}}"#;
        let result = extract_claude_response(jsonl).unwrap();
        assert_eq!(result, "Stream JSON answer");
    }

    #[test]
    fn extract_claude_fallback_to_result() {
        let json = r#"[{"type":"result","subtype":"success","result":"Plain text fallback","structured_output":null}]"#;
        let result = extract_claude_response(json).unwrap();
        assert_eq!(result, "Plain text fallback");
    }

    #[test]
    fn extract_claude_no_result_event() {
        let json = r#"[{"type":"system","subtype":"init"},{"type":"assistant","message":{}}]"#;
        assert!(extract_claude_response(json).is_err());
    }

    #[test]
    fn extract_codex_with_schema() {
        // Real Codex --json + --output-schema format: answer in item.completed, turn.completed has only usage
        let jsonl = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"{\"answer\":\"Full response text\"}"}}
{"type":"turn.completed","usage":{"input_tokens":100,"output_tokens":200}}"#;
        let result = extract_codex_response(jsonl).unwrap();
        assert_eq!(result, "Full response text");
    }

    #[test]
    fn extract_codex_plain_text() {
        // Real Codex --json format (no --output-schema): plain text in item.completed
        let jsonl = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"Plain text response"}}
{"type":"turn.completed","usage":{"input_tokens":100,"output_tokens":200}}"#;
        let result = extract_codex_response(jsonl).unwrap();
        assert_eq!(result, "Plain text response");
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
