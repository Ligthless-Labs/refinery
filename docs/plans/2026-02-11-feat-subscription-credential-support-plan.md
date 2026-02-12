---
title: "feat: Add subscription-based credential support"
type: feat
date: 2026-02-11
enhanced: 2026-02-11 (via `/deepen-plan` — 7 parallel agents: 3 CLI auth research, security, architecture, simplicity, pattern recognition)
completed: 2026-02-11
reviewed: 2026-02-11 (via `/workflows:review` — 4 agents: security-sentinel, code-simplicity-reviewer, architecture-strategist, pattern-recognition-specialist)
---

# feat: Add subscription-based credential support

## Enhancement Summary

**Sections enhanced:** All
**Research agents used:** Claude CLI auth, Codex CLI auth, Gemini CLI auth, security-sentinel, architecture-strategist, code-simplicity-reviewer, pattern-recognition-specialist

### Critical Corrections from Research

The original README and plan used **wrong env var names**. Verified names per CLI documentation:

| Provider | API Key (pay-per-use) | Subscription (plan-based) | Needs HOME? |
|----------|----------------------|--------------------------|-------------|
| Claude   | `ANTHROPIC_API_KEY`  | `CLAUDE_CODE_OAUTH_TOKEN` | Yes — needs `~/.claude.json` with `{"hasCompletedOnboarding": true}` |
| Codex    | `OPENAI_API_KEY`     | `CODEX_API_KEY` (for `codex exec`) | No |
| Gemini   | `GEMINI_API_KEY`     | _(none — no OAuth env var)_ | No |

Key findings:
- `CLAUDE_SESSION_TOKEN` does not exist → correct var is `CLAUDE_CODE_OAUTH_TOKEN`
- `OPENAI_OAUTH_TOKEN` does not exist → Codex uses `CODEX_API_KEY` for non-interactive mode
- `GEMINI_OAUTH_TOKEN` does not exist → Gemini only supports API key or browser-based OAuth
- Claude CLI requires `HOME` even when using env var auth (for onboarding config)
- Gemini CLI package is `@google/gemini-cli` (not `@anthropic-ai/gemini-cli`)

### New Considerations Discovered

1. Claude's `env_clear()` breaks subscription auth without `HOME` injection
2. `Credential` struct must redact values in `Debug` impl (security)
3. Whitespace-only credential values must be treated as "not set" (trim before check)
4. Use `resolve_credential_with` closure pattern for testable env var reading (Rust 2024 `set_var` is unsafe)
5. Gemini has no subscription env var — only API key and browser OAuth are supported

---

## Overview

Each provider (Claude, Codex, Gemini) currently only accepts a single API key env var.
Users with Pro/Plus/Advanced subscriptions should be able to authenticate via alternative
credential env vars instead of (or in addition to) API keys.

## Problem Statement

The README documents subscription-based auth methods but none are implemented. Users who
only have subscriptions (no API keys) cannot use the tool at all. Additionally, the
documented env var names are incorrect and must be fixed.

## Proposed Solution

Add a credential resolution chain to each provider: try env vars in priority order, use
the first non-empty match. Inject whichever credential was found into the child process
via the existing `env_clear()` + selective injection pattern.

### Credential Resolution (Corrected)

For each provider, resolve credentials in priority order:

| Provider | Priority 1 (API Key) | Priority 2 (Alternative) | Notes |
|----------|---------------------|--------------------------|-------|
| Claude   | `ANTHROPIC_API_KEY`  | `CLAUDE_CODE_OAUTH_TOKEN` | OAuth token from `claude setup-token`. Format: `sk-ant-oat01-...` |
| Codex    | `OPENAI_API_KEY`     | `CODEX_API_KEY`            | Inline API key for `codex exec` (non-interactive). Same key format. |
| Gemini   | `GEMINI_API_KEY`     | `GOOGLE_API_KEY`           | GCP API key (Vertex AI express mode). No OAuth env var exists. |

### Data Model

New file: `crates/converge_providers/src/credential.rs`

```rust
use std::fmt;

/// A resolved credential: the env var name and its value.
///
/// Fields are private to ensure construction only through `resolve_credential`.
/// Debug impl redacts the value to prevent credential leakage in logs.
pub struct Credential {
    env_var: &'static str,
    value: String,
}

impl Credential {
    pub fn env_var(&self) -> &'static str {
        self.env_var
    }

    pub fn value(&self) -> &str {
        &self.value
    }

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
```

#### Research Insights (Architecture + Simplicity)

- **Private fields with accessors** — matches `ModelId(String)` pattern in `converge_core::types`
- **Redacted Debug** — prevents credential leakage through `debug!()` or `{:?}` formatting
- **`as_env_pair()`** — formalizes the contract with `spawn_cli()`'s `env_vars` parameter
- **Separate `credential.rs` module** — `process.rs` already has 4 responsibilities; credential resolution is a 5th distinct concern

### Credential Resolution Helper

```rust
/// Try env vars in order. Return the first non-empty match, or MissingCredential error.
pub fn resolve_credential(
    provider: &str,
    candidates: &[&'static str],
) -> Result<Credential, ProviderError> {
    resolve_credential_with(provider, candidates, std::env::var)
}

/// Testable variant: accepts a custom env var reader (avoids unsafe std::env::set_var).
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
                return Ok(Credential { env_var: var, value: trimmed });
            }
        }
    }
    Err(ProviderError::MissingCredential {
        provider: provider.to_string(),
        var_name: candidates.join(" or "),
    })
}
```

#### Research Insights (Architecture + Testability)

- **`resolve_credential_with` pattern** — Rust 2024 edition marks `std::env::set_var` as unsafe.
  Tests inject a closure instead of mutating global state. Thread-safe, no `#[allow(unsafe_code)]`.
- **Trim whitespace** — catches `ANTHROPIC_API_KEY=" "` (common CI misconfiguration)
- **Keep `var_name` field** — no need to rename the error field. Just format "X or Y" into it.
  Avoids breaking change churn for cosmetic purposes. (Simplicity review finding)
- **No `warn!()` on dual credentials** — the `info!()` showing which one was picked is sufficient.
  The `warn!()` adds complexity for no user-facing value. (Simplicity review finding)

### Provider Changes

Each provider struct replaces `api_key: String` with `credential: Credential`:

```rust
// In each provider's new()
let credential = credential::resolve_credential(
    "claude",
    &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
)?;

// In send_message()
let env_vars = [self.credential.as_env_pair()];
```

**Gemini special case**: Gemini injects two env vars (credential + `GEMINI_SYSTEM_MD`):

```rust
let env_vars = [
    self.credential.as_env_pair(),
    ("GEMINI_SYSTEM_MD", system_prompt.as_str()),
];
```

### Claude HOME Requirement

The Claude CLI requires `HOME` to locate `~/.claude.json` (onboarding bypass config)
even when authenticating via env var. For `CLAUDE_CODE_OAUTH_TOKEN`, `HOME` must be injected.

**Security mitigation**: Create a synthetic HOME with only the required file, not the real HOME:

```rust
// In ClaudeProvider::send_message(), when using OAuth token:
if self.credential.env_var() == "CLAUDE_CODE_OAUTH_TOKEN" {
    // Inject HOME so claude can find ~/.claude.json
    // The real HOME is not leaked — only the specific config path is needed
    if let Ok(home) = std::env::var("HOME") {
        env_vars.push(("HOME", home.as_str()));
    }
}
```

**Alternative (simpler, v0)**: Always inject HOME for the Claude provider since even API key
mode may need it for `~/.claude.json`. This is a pragmatic choice — the child `claude` binary
already has access to the filesystem (it can read any file), so `HOME` does not meaningfully
expand its capabilities.

#### Research Insights (Security)

- `env_clear()` without HOME causes Claude CLI to fail even with valid API key in some scenarios
- On macOS, credentials may also be stored in Keychain (service: `Claude Code-credentials`)
- `CLAUDE_CONFIG_DIR` can override where `~/.claude/` contents go, but `~/.claude.json` is always at `$HOME/.claude.json`
- For maximum isolation: create a tmpdir with only `$HOME/.claude.json` copied, set HOME to that tmpdir

### Error Type (Unchanged)

Keep the existing `ProviderError::MissingCredential` field name. Just format the candidates
list into `var_name`:

```rust
// No structural change needed
#[error("missing credential: {var_name} not set for {provider}")]
MissingCredential { provider: String, var_name: String },

// Construction in resolve_credential:
var_name: candidates.join(" or "),
// Produces: "missing credential: ANTHROPIC_API_KEY or CLAUDE_CODE_OAUTH_TOKEN not set for claude"
```

### README Corrections

Fix the incorrect env var names and setup instructions:

| Section | Old (Wrong) | New (Correct) |
|---------|------------|---------------|
| Claude subscription | `CLAUDE_SESSION_TOKEN` | `CLAUDE_CODE_OAUTH_TOKEN` |
| Claude setup command | `claude setup-token` | `claude setup-token` (correct) |
| Codex subscription | `OPENAI_OAUTH_TOKEN` | `CODEX_API_KEY` |
| Codex setup command | `codex --full-setup` (doesn't exist) | `codex login --api-key` |
| Gemini subscription | `GEMINI_OAUTH_TOKEN` | Remove — no subscription env var exists |
| Gemini alt key | _(not documented)_ | Add `GOOGLE_API_KEY` as alternative |
| Gemini package | `@anthropic-ai/gemini-cli` | `@google/gemini-cli` |

### .env.example Update

```
# Provider API Keys (pay-per-use)
ANTHROPIC_API_KEY=your-api-key-here
OPENAI_API_KEY=your-api-key-here
GEMINI_API_KEY=your-api-key-here

# Alternative credentials
# CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-...   # Claude Pro/Max (from `claude setup-token`)
# CODEX_API_KEY=sk-proj-...                   # OpenAI API key for codex exec
# GOOGLE_API_KEY=AI...                        # Google Cloud API key (Vertex AI express)
```

## Technical Considerations

### Security: HOME and env_clear()

**Verified per-CLI behavior:**

| CLI | Needs HOME with API key? | Needs HOME with subscription token? |
|-----|-------------------------|-------------------------------------|
| claude | Maybe (for `~/.claude.json`) | Yes (for `~/.claude.json` + onboarding bypass) |
| codex exec | No | N/A (uses inline `CODEX_API_KEY`) |
| gemini | No | N/A (no subscription env var) |

**Decision for v0**: Inject `HOME` for the Claude provider only. Codex and Gemini work
without it. Document that the Claude provider has weaker env isolation than Codex/Gemini.

#### Research Insights (Security)

- Subscription/OAuth tokens grant broader access than API keys (full account, not just API)
- `Credential` struct must NOT auto-derive `Debug` — manual impl with `[REDACTED]` value
- stderr from child processes may contain credential values — consider sanitization in future
- The `secrecy` crate (`SecretString`) provides zeroize-on-drop; nice-to-have for v0.1

### Empty String Handling

`std::env::var()` returns `Ok("")` for empty values. The resolution chain trims whitespace
and rejects empty-after-trim values. This catches:
- `ANTHROPIC_API_KEY=""` (empty)
- `ANTHROPIC_API_KEY=" "` (whitespace only)
- `ANTHROPIC_API_KEY=your-api-key-here` (placeholder — NOT caught; deferred to v0.1)

### Token Expiry

`CLAUDE_CODE_OAUTH_TOKEN` from `setup-token` is long-lived (~1 year). Standard OAuth tokens
from `/login` expire in 8-12 hours. If a token expires mid-run, the provider fails with a
process error. Future work can detect auth-specific exit codes.

### Testability

Use `resolve_credential_with` closure injection for all credential resolution tests.
This avoids `std::env::set_var` (unsafe in Rust 2024) and is thread-safe for parallel tests.

Test matrix (5 cases on `resolve_credential_with`, NOT per-provider):
1. First candidate present → resolves to first
2. Only second candidate present → falls back
3. Both present → resolves to first (higher priority)
4. Neither present → `MissingCredential` error with both var names
5. First is empty string → falls back to second

Plus 1 wiring test per provider: `new()` calls `resolve_credential` with correct candidates.

## Acceptance Criteria

- [x] Each provider tries env vars in documented priority order
- [x] Empty/whitespace-only credentials are treated as "not set"
- [x] `MissingCredential` error lists all accepted env var names
- [x] `info!()` log shows which credential env var was resolved (never the value)
- [x] `Credential` Debug impl redacts the value
- [x] Claude provider injects `HOME` when using `CLAUDE_CODE_OAUTH_TOKEN`
- [x] `.env.example` updated with correct alternative credential vars
- [x] README corrected: wrong env var names fixed, Gemini package name fixed
- [x] Unit tests: 5 cases on `resolve_credential_with` + 3 wiring tests
- [x] All existing tests pass

## Out of Scope

- AWS Bedrock (`AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`) — separate provider
- Google Vertex AI full setup (`GOOGLE_CLOUD_PROJECT` + `GOOGLE_CLOUD_LOCATION`) — separate follow-up
- `GOOGLE_APPLICATION_CREDENTIALS` (service account file-based auth) — needs different Credential model
- Token refresh / re-authentication
- `--auth-method` CLI flag
- Credential value validation (prefix checking)
- `secrecy::SecretString` / zeroize-on-drop (v0.1)
- stderr credential sanitization (v0.1)

## Dependencies & Risks

- **Risk (resolved)**: CLI env var names verified via documentation research
- **Risk**: Claude CLI behavior may change across versions (HOME requirement)
- **Mitigation**: Integration test that spawns each CLI with env_clear + credential to verify
- **Risk**: `CODEX_API_KEY` only works with `codex exec`, not interactive mode — converge already uses non-interactive mode, so this is fine

## References

- Provider implementations: `crates/converge_providers/src/{claude,codex,gemini}.rs`
- Process spawning: `crates/converge_providers/src/process.rs:43-64`
- Error type: `crates/converge_core/src/error.rs:63-64`
- README credential docs: `README.md:20-104`
- Security design (env_clear): plan D7

### CLI Documentation Sources

- Claude Code: https://code.claude.com/docs/en/authentication, https://code.claude.com/docs/en/settings
- Codex: https://developers.openai.com/codex/auth/, https://developers.openai.com/codex/noninteractive/
- Gemini: https://github.com/google-gemini/gemini-cli/blob/main/docs/get-started/authentication.md
- Claude setup-token: https://github.com/anthropics/claude-code-action/blob/main/docs/setup.md
