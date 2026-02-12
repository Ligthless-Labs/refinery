---
title: "CLI provider flags for tool restriction and sandboxing"
category: integration-issues
tags: [claude-cli, codex-cli, gemini-cli, subprocess, sandboxing, tool-restriction]
module: converge_providers
symptom: "CLI tools execute arbitrary tools/code when invoked as subprocess"
root_cause: "Each CLI has different flags for restricting tool use and sandboxing"
date: 2026-02-12
---

# CLI Provider Flags for Tool Restriction and Sandboxing

When invoking AI CLI tools (Claude CLI, Codex CLI, Gemini CLI) as subprocesses from a Rust program, each tool has a different interface for disabling tools, restricting filesystem access, passing system prompts, and producing machine-readable output. This document captures the working configurations discovered during development of the `converge_providers` crate.

## 1. Claude CLI

**Package:** `@anthropic-ai/claude-code` (npm)
**Binary:** `claude`

### Invocation pattern

```
claude -p \
  --output-format json \
  --tools "" \
  --max-turns 1 \
  --model <model-name> \
  --append-system-prompt "SYSTEM PROMPT" \
  -- "USER PROMPT"
```

### Key flags

| Flag | Purpose |
|---|---|
| `-p` | Print mode (non-interactive, reads prompt from positional arg) |
| `--output-format json` | Machine-readable JSON output |
| `--tools ""` | Disables ALL tools. The empty string is required. |
| `--max-turns 1` | Single turn only (prevents agentic loops) |
| `--model <name>` | Model selection (e.g. `sonnet`, `opus`) |
| `--append-system-prompt "..."` | Injects a system prompt |
| `--` | Sentinel separating flags from the user prompt |

### Gotcha: `--disallowedTools` vs `--tools ""`

Do NOT use `--disallowedTools` to restrict tools. It is a fragile blocklist approach -- new tools added to the CLI will not be blocked unless you update the list. `--tools ""` is a whitelist set to empty, which disables everything unconditionally.

### Credentials

Claude CLI accepts two credential env vars, checked in priority order:

1. `ANTHROPIC_API_KEY` -- pay-per-use API key
2. `CLAUDE_CODE_OAUTH_TOKEN` -- Pro/Max subscription OAuth token

When using `CLAUDE_CODE_OAUTH_TOKEN`, the `HOME` env var must also be injected so the CLI can find `~/.claude.json` (its onboarding config).

### JSON output schema

```json
{
  "type": "result",
  "result": "The model's response text goes here",
  "session_id": "abc-123",
  "is_error": false
}
```

Extraction logic: check `is_error` first; if `false`, read the `result` field.

```rust
// From crates/converge_providers/src/process.rs
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
```

### Rust build_args implementation

```rust
// From crates/converge_providers/src/claude.rs
fn build_args(&self, system_prompt: &str, user_prompt: &str) -> Vec<String> {
    vec![
        "-p".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "--tools".to_string(),
        String::new(), // empty string disables all tools
        "--max-turns".to_string(),
        "1".to_string(),
        "--model".to_string(),
        self.model_name.clone(),
        "--append-system-prompt".to_string(),
        system_prompt.to_string(),
        "--".to_string(),
        user_prompt.to_string(),
    ]
}
```

---

## 2. Codex CLI

**Package:** `@openai/codex` (npm)
**Binary:** `codex`

### Invocation pattern

```
codex exec \
  --json \
  --sandbox read-only \
  --config developer_instructions="SYSTEM PROMPT" \
  -- "USER PROMPT"
```

### Key flags

| Flag | Purpose |
|---|---|
| `exec` | Non-interactive execution mode (required for subprocess use) |
| `--json` | Machine-readable JSONL output |
| `--sandbox read-only` | Restricts filesystem access to read-only |
| `--config developer_instructions="..."` | Injects a system prompt via config |
| `--` | Sentinel separating flags from the user prompt |

### Credentials

Codex CLI accepts two credential env vars, checked in priority order:

1. `OPENAI_API_KEY` -- standard OpenAI API key
2. `CODEX_API_KEY` -- dedicated Codex key

### JSON output schema (JSONL event stream)

Codex does not output a single JSON object. It emits a JSONL stream of events, one per line:

```jsonl
{"type":"thread.started","thread_id":"t1"}
{"type":"turn.started","turn_id":"u1"}
{"type":"item.text_delta","content":"partial"}
{"type":"turn.completed","turn_id":"u1","text":"Full response text","usage":{"input_tokens":100,"output_tokens":200}}
```

Extraction logic: scan lines in reverse for the last `turn.completed` event and read its `text` field.

```rust
// From crates/converge_providers/src/process.rs
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
```

### Rust build_args implementation

```rust
// From crates/converge_providers/src/codex.rs
fn build_args(&self, system_prompt: &str, user_prompt: &str) -> Vec<String> {
    vec![
        "exec".to_string(),
        "--json".to_string(),
        "--sandbox".to_string(),
        "read-only".to_string(),
        "--config".to_string(),
        format!("developer_instructions={system_prompt}"),
        "--".to_string(),
        user_prompt.to_string(),
    ]
}
```

---

## 3. Gemini CLI

**Package:** `@google/gemini-cli` (npm)
**Binary:** `gemini`

### Invocation pattern

```
GEMINI_SYSTEM_MD="SYSTEM PROMPT" \
gemini \
  --output-format json \
  --model gemini-2.5-pro \
  --sandbox \
  --approval-mode plan \
  --allowed-tools "" \
  -- "USER PROMPT"
```

### Key flags

| Flag | Purpose |
|---|---|
| `--output-format json` | Machine-readable JSON output |
| `--model <name>` | Model selection |
| `--sandbox` | Enables sandboxed execution |
| `--approval-mode plan` | Prevents tool use without approval (belt-and-suspenders with `--allowed-tools ""`) |
| `--allowed-tools ""` | Whitelist set to empty, disabling all tools |
| `--` | Sentinel separating flags from the user prompt |

### System prompt: `GEMINI_SYSTEM_MD` env var

Gemini CLI has no `--system-prompt` flag. Instead, the system prompt must be passed via the `GEMINI_SYSTEM_MD` environment variable. This is a notable asymmetry with the other CLIs.

```rust
// From crates/converge_providers/src/gemini.rs
let env_vars = [
    self.credential.as_env_pair(),
    ("GEMINI_SYSTEM_MD", system_prompt.as_str()),
];
```

### Credentials

Gemini CLI accepts two credential env vars, checked in priority order:

1. `GEMINI_API_KEY` -- Google AI Studio key
2. `GOOGLE_API_KEY` -- Vertex AI express mode key

### JSON output schema

```json
{
  "response": "The model's response text goes here",
  "stats": {"models": ["gemini-2.5-pro"]},
  "error": null
}
```

Extraction logic: check `error` is null, then read `response`. There is a known issue (Gemini CLI Issue #11184) where the `response` field may contain markdown-wrapped JSON (triple-backtick fences around JSON content). The extraction logic strips these fences when present.

```rust
// From crates/converge_providers/src/process.rs
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
    if let Some(inner) = converge_core::prompts::extract_json(response) {
        Ok(inner.to_string())
    } else {
        Ok(response.to_string())
    }
}
```

---

## 4. Process Management

### Environment isolation

All subprocesses have their environment completely cleared with `env_clear()` before injecting only the required variables. This prevents credential leakage and ensures reproducible behavior:

```rust
// From crates/converge_providers/src/process.rs
let mut cmd = Command::new(binary_path);

// Security: clear all inherited environment
cmd.env_clear();

// Inject minimal PATH for child process needs
cmd.env("PATH", "/usr/bin:/usr/local/bin:/bin");

// Inject provider-specific env vars
for (key, value) in env_vars {
    cmd.env(key, value);
}
```

### Binary resolution

CLI binaries are resolved to absolute paths via `which` BEFORE `env_clear()` removes `PATH` from the parent's view. This is necessary because `Command::new("claude")` would fail in a cleared environment:

```rust
// From crates/converge_providers/src/process.rs
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
    Ok(PathBuf::from(path))
}
```

### Child cleanup with `kill_on_drop(true)`

`tokio::process::Command` supports `kill_on_drop(true)`, which sends SIGKILL to the child if the `Child` handle is dropped (e.g., on timeout or parent panic). This is the v0 strategy for preventing orphaned CLI processes.

```rust
cmd.kill_on_drop(true);
```

**Limitation:** `kill_on_drop` only kills the direct child process, not its process group. If the CLI itself spawns children (e.g., a language server), those grandchildren may survive. The `setsid` approach (creating a new process group and killing the whole group) is deferred to v0.1.

### Timeout

Timeouts use `tokio::time::timeout` wrapping the `cmd.output()` future. When the timeout fires, the `Child` is dropped, triggering `kill_on_drop`:

```rust
let result = tokio::time::timeout(timeout, cmd.output()).await;

match result {
    Ok(Ok(output)) => { /* success path */ }
    Ok(Err(e)) => Err(ProviderError::ProcessFailed { .. }),
    Err(_) => Err(ProviderError::Timeout { .. }),
}
```

### Response size guard

A 100KB limit prevents runaway responses from consuming memory:

```rust
const MAX_RESPONSE_SIZE: usize = 100_000;

if stdout.len() > MAX_RESPONSE_SIZE {
    return Err(ProviderError::ResponseTooLarge {
        model: model.clone(),
        size: stdout.len(),
        max: MAX_RESPONSE_SIZE,
    });
}
```

---

## 5. Credential Resolution

### Priority chain pattern

Each provider accepts multiple env vars for authentication. The `resolve_credential` function checks them in declared order and returns the first non-empty match:

```rust
// From crates/converge_providers/src/credential.rs
pub fn resolve_credential(
    provider: &str,
    candidates: &[&'static str],
) -> Result<Credential, ProviderError> {
    resolve_credential_with(provider, candidates, |key| std::env::var(key))
}
```

Provider-specific priority orders:

| Provider | First choice | Fallback |
|---|---|---|
| Claude | `ANTHROPIC_API_KEY` | `CLAUDE_CODE_OAUTH_TOKEN` |
| Codex | `OPENAI_API_KEY` | `CODEX_API_KEY` |
| Gemini | `GEMINI_API_KEY` | `GOOGLE_API_KEY` |

### Testable resolver with closure injection

`std::env::set_var` is unsafe in Rust 2024 edition, making it unsuitable for tests. The solution is a `resolve_credential_with` function that accepts a reader closure instead of calling `std::env::var` directly:

```rust
// From crates/converge_providers/src/credential.rs
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
                return Ok(Credential { env_var: var, value: trimmed });
            }
        }
    }
    Err(ProviderError::MissingCredential { .. })
}
```

Tests use a mock reader built from a hashmap:

```rust
fn mock_reader(vars: &[(&str, &str)]) -> impl Fn(&str) -> Result<String, std::env::VarError> {
    let map: std::collections::HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    move |key: &str| map.get(key).cloned().ok_or(std::env::VarError::NotPresent)
}
```

### Credential redaction in Debug

The `Credential` struct uses a custom `Debug` impl that replaces the value with `[REDACTED]` to prevent credential leakage in logs:

```rust
impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credential")
            .field("env_var", &self.env_var)
            .field("value", &"[REDACTED]")
            .finish()
    }
}
```

---

## 6. Common Pitfalls

### Each CLI has a different JSON schema

This is the most error-prone aspect. A summary of the differences:

| CLI | Output format | Response field | Error detection |
|---|---|---|---|
| Claude | Single JSON object | `.result` | `.is_error == true` |
| Codex | JSONL event stream | `.text` on `turn.completed` event | No `turn.completed` event |
| Gemini | Single JSON object | `.response` | `.error` is non-null |

### Gemini markdown-wrapping

Gemini CLI sometimes wraps JSON responses in markdown code fences (triple backticks), particularly when the model is asked to produce structured output. The extraction logic must detect and strip these fences.

### stdin handling

When using `Command::new().stdin(Stdio::piped())` to write prompts via stdin (rather than positional args), the stdin pipe must be closed immediately after writing. If you hold the `ChildStdin` handle open, the CLI process will hang indefinitely waiting for more input. In the current implementation, this is avoided by passing prompts as command-line arguments instead.

### `--` sentinel

All three CLIs use `--` as a sentinel to separate flags from the user prompt. This is critical when the user prompt might start with `-` and would otherwise be parsed as a flag.

### Empty string as "disable all"

Both Claude (`--tools ""`) and Gemini (`--allowed-tools ""`) use an empty string to mean "no tools allowed." This is a whitelist approach -- the whitelist is set to empty. This is more robust than a blocklist because new tools added to the CLI are automatically excluded.

---

## 7. Quick Reference

### Minimal subprocess invocations (shell)

**Claude:**
```bash
claude -p --output-format json --tools "" --max-turns 1 \
  --append-system-prompt "You are a helpful assistant." \
  -- "What is 2+2?"
```

**Codex:**
```bash
codex exec --json --sandbox read-only \
  --config 'developer_instructions=You are a helpful assistant.' \
  -- "What is 2+2?"
```

**Gemini:**
```bash
GEMINI_SYSTEM_MD="You are a helpful assistant." \
gemini --output-format json --sandbox --approval-mode plan \
  --allowed-tools "" -- "What is 2+2?"
```

### Prompt routing summary

| Concern | Claude | Codex | Gemini |
|---|---|---|---|
| System prompt | `--append-system-prompt` flag | `--config developer_instructions=` | `GEMINI_SYSTEM_MD` env var |
| User prompt | Positional arg after `--` | Positional arg after `--` | Positional arg after `--` |
| Tool disable | `--tools ""` | N/A (use `--sandbox`) | `--allowed-tools ""` |
| Sandbox | N/A | `--sandbox read-only` | `--sandbox` + `--approval-mode plan` |
| JSON output | `--output-format json` | `--json` | `--output-format json` |
| Non-interactive | `-p` | `exec` subcommand | Automatic with `--` |
