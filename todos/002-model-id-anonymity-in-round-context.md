---
title: "fix: ModelId leakage in round_context breaks anonymity"
priority: medium
milestone: v0.1
deferred_from: PR #1 Copilot review
---

# ModelId leakage in round_context breaks anonymity

## Problem

The `round_context()` prompt helper exposes real model IDs in two places:

1. `previous_scores` — displays scores as `"claude-sonnet-4-5 = 8.2"` instead of anonymous labels
2. `top_model` — displays the leading model's real ID like `"Top-ranked: gemini-2.5-pro"`

This breaks the anonymity guarantee: evaluating models see shuffled anonymous labels ("Answer A", "Answer B") during evaluation, but `round_context` leaks the real identities in subsequent rounds.

## Solution

Map `ModelId` → anonymous label in `round_context()` before embedding in the prompt. Reuse the same shuffled label mapping already generated for evaluate prompts.

## References

- `crates/refinery_core/src/prompts.rs:61` — `round_context()` function
- `crates/refinery_core/src/prompts.rs:37` — `shuffled_labels()` function
- `crates/refinery_core/src/engine.rs` — where `round_context` is called
- Copilot review comment on PR #1
