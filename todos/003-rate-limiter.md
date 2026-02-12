---
title: "feat: rate limiter for API call pacing"
priority: low
milestone: v0.1
deferred_from: PR #1 design decision
---

# Rate limiter for API call pacing

## Problem

With N models and N*(N-1) evaluation calls per round, burst traffic can hit provider rate limits. Currently there is no pacing — only a concurrency semaphore (`max_concurrent`).

## Challenge

CLI backends (claude, codex, gemini) do not expose rate limit headers (`X-RateLimit-Remaining`, `Retry-After`), so adaptive rate limiting is not possible without parsing stderr or guessing.

## Possible Approaches

1. **Fixed token-bucket** — configurable requests/sec per provider, simple but requires user tuning
2. **Exponential backoff on failure** — retry with increasing delay when a provider returns an error that looks like rate limiting
3. **Per-provider delay** — add a configurable minimum delay between calls to the same provider

## Notes

- The concurrency semaphore already provides some natural pacing
- This becomes more important at N=5+ models (30+ calls per round)
- Consider making this opt-in via CLI flag (`--rate-limit 5/s`)

## References

- `crates/converge_core/src/engine.rs` — semaphore-based concurrency control
- `crates/converge_core/src/types.rs` — `EngineConfig.max_concurrent`
