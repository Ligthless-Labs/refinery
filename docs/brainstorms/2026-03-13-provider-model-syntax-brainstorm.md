---
title: "Provider/model syntax for CLI model selection"
date: 2026-03-13
status: decided
---

# Provider/Model Syntax for CLI Model Selection

## What We're Building

Change the CLI `-m` flag from simple names (`claude`, `codex`, `gemini`) to a `provider/model` format (`claude-code/claude-opus-4-6`, `codex-cli/codex-gpt-5.4`, `gemini-cli/gemini-3.1-pro-preview`). This decouples the provider (CLI tool) from the model it serves, enabling support for multiple providers that can serve the same or different models (Anthropic Agents SDK, API calls, OpenCode, Amazon Bedrock, etc).

## Why This Approach

The current system hard-wires provider identity to model name prefixes (`claude-*` → ClaudeProvider). This breaks down when:

- Multiple providers can serve the same model (e.g. Claude via `claude` CLI vs Anthropic API vs Bedrock)
- A provider supports models that don't match its prefix (e.g. Codex serving non-GPT models)
- New providers are added that don't fit the prefix-matching pattern

The `provider/model` format makes both dimensions explicit.

## Key Decisions

1. **Format:** `provider/model` with `/` as separator. Examples:
   - `claude-code/claude-opus-4-6`
   - `codex-cli/codex-gpt-5.4`
   - `gemini-cli/gemini-3.1-pro-preview`

2. **Clean break:** No backward compatibility aliases. Old format (`claude`, `codex`, `gemini`) produces a helpful error suggesting the new syntax.

3. **Provider-level defaults:** Specifying just the provider name (e.g. `claude-code`) uses a default model. Defaults:
   - `claude-code` → `claude-code/claude-opus-4-6`
   - `codex-cli` → `codex-cli/codex-gpt-5.4`
   - `gemini-cli` → `gemini-cli/gemini-3.1-pro-preview`

4. **ModelId becomes two fields:** `ModelId { provider: String, model: String }`. Display as `provider/model`. Parse via `ModelId::parse(s)` which splits on `/`.

5. **Full provider/model in output:** Logs, progress events, and final output all show the full `provider/model` identifier.

6. **Hardcoded factory match:** `build_provider()` matches on provider name. Adding a new provider means adding a match arm + module. No plugin registry — YAGNI.

7. **Provider names:** `claude-code`, `codex-cli`, `gemini-cli` — named after the CLI tool they wrap.

## Open Questions

- When we add API-based providers (no CLI subprocess), will they follow the same `ModelProvider` trait or need a different interface? (Likely same trait — `send_message` abstracts over transport.)
- Future provider names: `anthropic-api`, `openai-api`, `bedrock`, `opencode`? (Decide when we add them.)
