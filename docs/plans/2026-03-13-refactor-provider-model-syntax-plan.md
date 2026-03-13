---
title: "refactor: Provider/model syntax for CLI model selection"
type: refactor
date: 2026-03-13
brainstorm: docs/brainstorms/2026-03-13-provider-model-syntax-brainstorm.md
---

# Provider/Model Syntax for CLI Model Selection

## Overview

Change the CLI `-m` flag from simple names (`claude`, `codex`, `gemini`) to a `provider/model` format (`claude-code/claude-opus-4-6`, `codex-cli/codex-gpt-5.4`, `gemini-cli/gemini-3.1-pro-preview`). `ModelId` becomes a two-field struct with custom serde to preserve the flat `"provider/model"` JSON format.

## Problem Statement / Motivation

The current system hard-wires provider identity to model name prefixes (`claude-*` → ClaudeProvider, `starts_with("codex")` → CodexProvider). This breaks when:

1. **Multiple providers serve the same model** — Claude via `claude` CLI vs Anthropic API vs Bedrock
2. **Provider doesn't match prefix** — Codex serving non-GPT models
3. **Inconsistent naming** — Claude/Codex prefix their `ModelId` (`claude-opus-4-6`, `codex-gpt-5.4`) but Gemini doesn't (`gemini-3.1-pro-preview`)
4. **New providers** — Anthropic Agents SDK, OpenCode, Bedrock require new prefix-matching hacks

The `provider/model` format makes both dimensions explicit and extensible.

## Proposed Solution

### Key Decisions (from brainstorm)

- **Format:** `provider/model` with `/` separator
- **Clean break:** No backward compat. Old format → helpful error
- **Provider defaults:** `claude-code` alone → `claude-code/claude-opus-4-6`
- **ModelId:** Two fields `{ provider, model }`. Display as `provider/model`
- **Serde:** Custom impl — serialize as flat `"provider/model"` string (preserves JSON output schema)
- **Factory:** Hardcoded match on `provider` field in `build_provider()`
- **Provider names:** `claude-code`, `codex-cli`, `gemini-cli`

### What does NOT change

- `ModelProvider` trait — unchanged
- Evaluate, Propose, Close phases — unchanged
- Provider implementations (claude.rs, codex.rs, gemini.rs internals) — only constructor changes
- Binary resolution (`which claude`) — unchanged
- Credential resolution — unchanged
- Progress event structure — unchanged (ModelId Display changes but types don't)

### Provider defaults

| Input | Resolved |
|-------|----------|
| `claude-code` | `claude-code/claude-opus-4-6` |
| `claude-code/claude-sonnet-4-6` | `claude-code/claude-sonnet-4-6` |
| `codex-cli` | `codex-cli/codex-gpt-5.4` |
| `codex-cli/o3-pro` | `codex-cli/o3-pro` |
| `gemini-cli` | `gemini-cli/gemini-3.1-pro-preview` |
| `gemini-cli/gemini-2.5-flash` | `gemini-cli/gemini-2.5-flash` |
| `claude` | Error: "Unknown provider 'claude'. Did you mean 'claude-code'?" |

## Technical Approach

### ModelId Refactoring

```rust
// types.rs — new definition
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelId {
    provider: String,
    model: String,
}

impl ModelId {
    /// Parse "provider/model" string. Panics on invalid format (use in tests).
    pub fn new(s: impl Into<String>) -> Self { ... }

    /// Parse "provider/model" string, returning Err on invalid format.
    pub fn parse(s: &str) -> Result<Self, ConvergeError> { ... }

    /// Construct from explicit parts.
    pub fn from_parts(provider: impl Into<String>, model: impl Into<String>) -> Self { ... }

    pub fn provider(&self) -> &str { &self.provider }
    pub fn model(&self) -> &str { &self.model }
}

impl Display for ModelId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.provider, self.model)
    }
}

// Custom serde: serialize as flat "provider/model" string
impl Serialize for ModelId { ... }
impl<'de> Deserialize<'de> for ModelId { ... }
```

### Parsing Rules

1. If input contains `/`, split on first `/` → `(provider, model)`
2. If input has no `/`, treat as provider-only → apply default model
3. Empty provider or model → error
4. Unknown provider → error with "Did you mean...?" suggestion

### CLI Parsing

```rust
// main.rs — parse_model_spec()
fn parse_model_spec(input: &str) -> Result<ModelId, String> {
    if input.contains('/') {
        let (provider, model) = input.split_once('/').unwrap();
        if model.contains('/') {
            return Err("Model spec must be 'provider/model', got extra '/'");
        }
        Ok(ModelId::from_parts(provider, model))
    } else {
        // Provider-only: apply default
        match input {
            "claude-code" => Ok(ModelId::from_parts("claude-code", "claude-opus-4-6")),
            "codex-cli" => Ok(ModelId::from_parts("codex-cli", "codex-gpt-5.4")),
            "gemini-cli" => Ok(ModelId::from_parts("gemini-cli", "gemini-3.1-pro-preview")),
            // Helpful error for old format
            "claude" | "codex" | "gemini" => Err(format!(
                "Unknown provider '{input}'. The format is now 'provider/model'. \
                 Did you mean '{input}-code' or '{input}-cli'? \
                 Supported providers: claude-code, codex-cli, gemini-cli"
            )),
            _ => Err(format!("Unknown provider '{input}'. Supported: claude-code, codex-cli, gemini-cli")),
        }
    }
}
```

### Updated build_provider

```rust
// main.rs — match on provider field
async fn build_provider(
    model_id: &ModelId,
    max_timeout: Duration,
    idle_timeout: Duration,
    progress: Option<refinery_core::ProgressFn>,
) -> Result<Arc<dyn ModelProvider>, Box<dyn std::error::Error>> {
    match model_id.provider() {
        "claude-code" => {
            let provider = ClaudeProvider::new(model_id.clone(), ...);
            Ok(Arc::new(provider))
        }
        "codex-cli" => { ... }
        "gemini-cli" => { ... }
        other => Err(format!("Unknown provider: {other}").into()),
    }
}
```

### Provider Constructor Changes

Each provider receives the full `ModelId` and extracts `.model()` for CLI args:

```rust
// claude.rs — constructor takes ModelId, passes model() to --model flag
pub fn new(model_id: ModelId, ...) -> Self {
    Self {
        model_id,
        model_name: model_id.model().to_string(), // for --model flag
        ...
    }
}
```

Wait — the provider needs to own `model_id` for the `ModelProvider::model_id()` trait method, but also needs just the model name for CLI args. Currently each provider stores both `model_id: ModelId` and `model_name: String`. With the new format, `model_name` is just `model_id.model().to_string()`.

### Dependency Chain

```
types.rs
  ├─→ ModelId(String)                    [REPLACE with struct { provider, model }]
  ├─→ ModelId::new()                     [CHANGE to parse "provider/model"]
  ├─→ ModelId::as_str()                  [DELETE — replace with provider() + model()]
  ├─→ Display impl                       [CHANGE to "provider/model"]
  └─→ Serialize/Deserialize derives      [REPLACE with custom impls]

engine.rs
  ├─→ ModelId::new("unknown")            [CHANGE to from_parts("unknown", "unknown")]
  ├─→ id.as_str()                        [CHANGE to id.to_string() or id.model()]
  └─→ evaluator.to_string()             [OK — Display updated automatically]

phases/*.rs
  └─→ tracing %model_id                 [OK — Display updated automatically]

strategy.rs
  └─→ format!("Model {}", top_model)    [OK — Display updated automatically]

providers/claude.rs
  ├─→ ModelId::new(format!("claude-{}")) [CHANGE to ModelId::from_parts()]
  └─→ self.model_name in build_args()   [OK — still passes model name to CLI]

providers/codex.rs
  └─→ ModelId::new(format!("codex-{}")) [CHANGE to ModelId::from_parts()]

providers/gemini.rs
  └─→ ModelId::new(model_name)          [CHANGE to ModelId::from_parts()]

providers/process.rs
  ├─→ ModelId::new("codex")             [CHANGE to from_parts()]
  ├─→ ModelId::new("gemini")            [CHANGE to from_parts()]
  └─→ ModelId::new("claude")            [CHANGE to from_parts()]

cli/main.rs
  ├─→ ModelId::new from CLI input       [CHANGE to parse_model_spec()]
  ├─→ build_provider() prefix matching  [CHANGE to match on provider()]
  ├─→ as_str() calls in output          [CHANGE to to_string()]
  ├─→ Help text for -m flag             [UPDATE examples]
  └─→ Error message for unknown model   [UPDATE suggestions]
```

### Blast Radius

#### Modify

| Target | Location | Change |
|--------|----------|--------|
| `ModelId` struct | `types.rs:13-26` | Single field → two fields |
| `ModelId::new()` | `types.rs:17` | Accept `"provider/model"`, parse on `/` |
| `ModelId::as_str()` | `types.rs:22` | Delete, replace with `provider()` + `model()` |
| `ModelId` Display | `types.rs:28-32` | `"{provider}/{model}"` |
| `ModelId` Serialize | `types.rs:13` | Custom impl, flat `"provider/model"` string |
| `ModelId` Deserialize | `types.rs:13` | Custom impl, parse `"provider/model"` string |
| `ClaudeProvider::new()` | `claude.rs:47-69` | Accept `ModelId`, extract `.model()` for CLI args |
| `CodexProvider::new()` | `codex.rs:45-67` | Accept `ModelId`, extract `.model()` for CLI args |
| `GeminiProvider::new()` | `gemini.rs:44-64` | Accept `ModelId`, extract `.model()` for CLI args |
| `extract_codex_response` | `process.rs:253` | `ModelId::from_parts("codex-cli", "unknown")` |
| `extract_gemini_response` | `process.rs:298` | `ModelId::from_parts("gemini-cli", "unknown")` |
| `extract_claude_response` | `process.rs:341` | `ModelId::from_parts("claude-code", "unknown")` |
| `Session::finalize_with_status` | `engine.rs:409` | `ModelId::from_parts("unknown", "unknown")` |
| `engine.rs` round_context | `engine.rs:234` | `id.as_str()` → `id.to_string()` |
| `build_provider()` | `main.rs:482-532` | Match on `model_id.provider()` instead of prefix |
| CLI `-m` parsing | `main.rs:198` | Use `parse_model_spec()` |
| CLI help text | `main.rs:29` | Update examples |
| CLI error message | `main.rs:527-529` | Update supported list |
| CLI JSON output | `main.rs:353,362` | `as_str()` → `to_string()` |
| CLI error detail | `main.rs:543` | `as_str()` → `to_string()` |
| ~40 test sites | across 8 files | `ModelId::new("x")` → `ModelId::new("test/x")` or `from_parts()` |

#### Preserve (unchanged)

- `ModelProvider` trait
- Phase implementations (propose, evaluate, close)
- Binary resolution (`which` command)
- Credential resolution
- Progress event types (Display changes propagate automatically)
- Strategy implementations

## Acceptance Criteria

- [ ] `ModelId` has `provider` and `model` fields
- [ ] `ModelId::parse("provider/model")` splits on first `/`
- [ ] `ModelId::from_parts("p", "m")` constructs from explicit parts
- [ ] `ModelId` Display shows `provider/model`
- [ ] `ModelId` serializes to/from flat `"provider/model"` string (not JSON object)
- [ ] `-m claude-code/claude-opus-4-6` works end-to-end
- [ ] `-m claude-code` defaults to `claude-code/claude-opus-4-6`
- [ ] `-m claude` produces helpful error suggesting `claude-code`
- [ ] `-m codex-cli,gemini-cli` works with defaults
- [ ] `build_provider()` matches on `provider()` field
- [ ] Provider constructors receive full `ModelId`, use `.model()` for CLI args
- [ ] JSON output (`--output-format json`) uses flat `"provider/model"` strings
- [ ] Progress output shows `provider/model` format
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Car wash test passes: `refinery "The car wash is only 100m..." -m gemini-cli,codex-cli`

## Implementation (4 atomic commits)

### Commit 1: Refactor `ModelId` to two-field struct

- `types.rs`: Change `ModelId(String)` to `ModelId { provider: String, model: String }`
- Add `ModelId::parse()`, `ModelId::from_parts()`, `ModelId::provider()`, `ModelId::model()`
- Keep `ModelId::new()` as convenience that calls `parse()` (panics on invalid)
- Remove `ModelId::as_str()`
- Custom `Serialize`/`Deserialize` — flat `"provider/model"` string
- Update `Display` to `"{provider}/{model}"`
- Update all `ModelId::new()` calls in `refinery_core` tests to use `"test/name"` format
- Update `engine.rs:409` fallback to `from_parts("unknown", "unknown")`
- Update `engine.rs:234` `id.as_str()` → `id.to_string()`

**Gate:** `cargo test -p refinery_core`

### Commit 2: Update provider constructors

- `claude.rs`: Constructor takes `ModelId`, uses `.model()` for CLI `--model` arg
- `codex.rs`: Same pattern
- `gemini.rs`: Same pattern
- `process.rs`: Update hardcoded `ModelId::new()` in extract functions to `from_parts()`
- Update all provider tests

**Gate:** `cargo test -p refinery_providers`

### Commit 3: Update CLI parsing and factory

- Add `parse_model_spec()` function with defaults table and old-format detection
- Update `-m` flag help text with new examples
- Update `build_provider()` to accept `&ModelId`, match on `.provider()`
- Update all `as_str()` calls in output formatting to `to_string()`
- Update error messages

**Gate:** `cargo test --workspace`

### Commit 4: Final cleanup + clippy

- Remove dead imports
- Verify no remaining `as_str()` calls
- Run full verification

**Gate:** `cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Key Files

| File | Role |
|------|------|
| `crates/refinery_core/src/types.rs` | `ModelId` struct, parse, serde |
| `crates/refinery_core/src/engine.rs` | Update fallback ModelId, round_context |
| `crates/refinery_providers/src/claude.rs` | Constructor takes ModelId |
| `crates/refinery_providers/src/codex.rs` | Constructor takes ModelId |
| `crates/refinery_providers/src/gemini.rs` | Constructor takes ModelId |
| `crates/refinery_providers/src/process.rs` | Update extract function ModelIds |
| `crates/refinery_cli/src/main.rs` | Parsing, factory, output formatting |

## Success Metrics

- All workspace tests pass
- Zero clippy warnings
- Car wash test converges with `gemini-cli,codex-cli`
- JSON output uses flat `"provider/model"` string format
- Old format (`-m claude`) produces actionable error

## Dependencies & Risks

**Risk: Test blast radius.** ~40 test sites construct `ModelId::new("simple_name")`. Mitigation: `ModelId::new("test/name")` convention keeps changes mechanical.

**Risk: Serde backward compat.** Any stored/cached JSON with old `ModelId` format won't parse. Mitigation: no persistent storage of `ModelId` currently — only in CLI stdout output.

**Risk: Provider binary naming.** Provider name `claude-code` implies binary `claude`, not `claude-code`. The mapping is in the provider constructor, not derived from the provider name. This is fine — provider names are logical, not binary paths.

**No external dependencies.** Pure internal refactoring.

## Verification

- `cargo test --workspace` — all tests pass
- `cargo clippy --workspace -- -D warnings` — clean
- `refinery "The car wash is only 100m..." -m gemini-cli,codex-cli` — converges
- `refinery "test" -m claude` — produces helpful error
- `refinery "test" -m claude-code` — uses default model
- `--output-format json` — ModelId appears as `"claude-code/claude-opus-4-6"` flat string

## References & Research

### Internal References
- Brainstorm: `docs/brainstorms/2026-03-13-provider-model-syntax-brainstorm.md`
- CLI provider flags learnings: `docs/solutions/integration-issues/cli-provider-flags-and-sandboxing.md`
- Current factory: `crates/refinery_cli/src/main.rs:482-532`
