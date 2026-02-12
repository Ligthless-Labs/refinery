---
title: "Graceful degradation and multi-reviewer triage patterns"
category: runtime-errors
tags: [graceful-degradation, code-review, triage, error-handling, serde, rustfmt]
module: converge_core
symptom: "Models dropping mid-run causes hard failure; multiple reviewers produce duplicate findings"
root_cause: "No fallback for partial failure; no systematic triage process for review comments"
date: 2026-02-12
---

# Graceful degradation and multi-reviewer triage patterns

## Context

ConVerge Refinery runs N models through iterative consensus rounds. Each round has four phases: PROPOSE, EVALUATE, REFINE, CLOSE. Models can fail at any phase (timeout, process crash, malformed output), and the engine must decide whether to abort or continue with fewer models.

Separately, when multiple automated reviewers (Gemini, Copilot, Codex) review the same PR, they produce overlapping findings that need systematic triage.

This document captures patterns learned while building v0.

---

## 1. Graceful degradation when models drop mid-run

### Problem

In a multi-round consensus run, a model that succeeded in round 1 may fail in round 2 (e.g., rate limit, transient network error). If the remaining model count drops below N=2, the engine cannot run cross-evaluation. Before graceful degradation, this was a hard error, discarding all prior round data.

### Solution: return best-so-far with `InsufficientModels` status

When `InsufficientModels` fires after round 1, the engine has at least one completed round of proposals, evaluations, and refinements. Instead of returning `Err`, it returns `Ok` with the best answer from the last successful round.

From `crates/converge_core/src/engine.rs` (the `Engine::run` loop):

```rust
loop {
    let outcome = match session.next_round().await {
        Ok(o) => o,
        Err(ConvergeError::InsufficientModels { round, .. }) if round > 1 => {
            // Graceful degradation: return best-so-far from prior rounds
            return Ok(session.finalize_with_status(ConvergenceStatus::InsufficientModels));
        }
        Err(e) => return Err(e),
    };
    if matches!(outcome.closing_decision, ClosingDecision::Converged { .. }) {
        return Ok(session.finalize());
    }
    if session.current_round >= self.config.max_rounds {
        return Ok(session.finalize_with_status(ConvergenceStatus::MaxRoundsExceeded));
    }
}
```

The key mechanism is `Session::finalize_with_status()`, which packages whatever state the session has accumulated into a `ConsensusOutcome` with an overridden status:

```rust
fn finalize_with_status(self, status: ConvergenceStatus) -> ConsensusOutcome {
    let winner = self
        .current_winner
        .unwrap_or_else(|| ModelId::new("unknown"));
    let answer = self.last_answers.get(&winner).cloned().unwrap_or_default();

    let all_answers: Vec<ModelAnswer> = self
        .last_answers
        .iter()
        .map(|(model_id, ans)| {
            let mean_score = self.last_mean_scores.get(model_id).copied().unwrap_or(0.0);
            ModelAnswer {
                model_id: model_id.clone(),
                answer: ans.clone(),
                mean_score,
            }
        })
        .collect();

    ConsensusOutcome {
        status,
        winner,
        answer,
        final_round: self.current_round,
        all_answers,
        total_calls: self.total_calls,
        elapsed: self.start_time.elapsed(),
    }
}
```

This same `finalize_with_status` is reused for `MaxRoundsExceeded`, `Cancelled`, and `SingleModel` short-circuits, making it the single point of outcome construction regardless of how the run ends.

### Critical: the `round > 1` guard

The pattern-match guard `if round > 1` is essential. Without it, the engine would silently swallow failures in round 1, where no prior data exists. This was a real bug during development: removing the guard caused the `all_models_fail_returns_error` test to fail.

Round 1 failures must propagate as errors because:
- There is no "best-so-far" to return (no prior round data).
- The caller needs to know that zero useful work was done.
- Returning an empty `ConsensusOutcome` would be misleading.

The test that guards this invariant:

```rust
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn all_models_fail_returns_error() {
    let providers: Vec<Arc<dyn ModelProvider>> = vec![
        Arc::new(FailingProvider::new("model_a")),
        Arc::new(FailingProvider::new("model_b")),
    ];
    let config = default_config(2);
    let strategy = Box::new(crate::strategy::VoteThreshold::new(8.0, 2));
    let engine = Engine::new(providers, strategy, config);

    let result = engine.run("test prompt").await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ConvergeError::InsufficientModels { .. }
    ));
}
```

And the complementary test that verifies graceful degradation works when round 1 succeeds:

```rust
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn insufficient_models_returns_best_so_far() {
    // 3 providers: model_a and model_b succeed round 1, model_c fails.
    // In round 2, model_b also fails -> only 1 proposal -> InsufficientModels.
    // Engine::run should return best-so-far, not an error.
    let providers: Vec<Arc<dyn ModelProvider>> = vec![
        Arc::new(EchoProvider::with_json_eval("model_a", 9)),
        Arc::new(FailAfterNProvider::new("model_b", 1)),
        Arc::new(FailingProvider::new("model_c")),
    ];
    // ...
    let result = engine.run("test prompt").await;
    assert!(result.is_ok());
    let outcome = result.unwrap();
    assert_eq!(outcome.status, ConvergenceStatus::InsufficientModels);
}
```

The `FailAfterNProvider` is critical for testing this: it succeeds for exactly N calls, then fails on call N+1, simulating a model that drops out mid-run.

### Pattern summary

| Condition | Round 1 | Round 2+ |
|---|---|---|
| N >= 2 models alive | Continue normally | Continue normally |
| N < 2, some prior data | N/A (round 1 always has N >= 2) | Return `Ok(InsufficientModels)` with best-so-far |
| N < 2, no prior data | Return `Err(InsufficientModels)` | N/A |

---

## 2. Serde serialization: never use `Debug` formatting for JSON

### Problem

When serializing an enum variant to a JSON string for CLI output, it is tempting to write:

```rust
// DO NOT DO THIS
let status_str = format!("{:?}", outcome.status).to_lowercase();
```

For `ConvergenceStatus::MaxRoundsExceeded`, this produces `"maxroundsexceeded"` -- not `"max_rounds_exceeded"`. The `Debug` trait concatenates the variant name without separators.

### Solution: use `serde_json::to_value` with `#[serde(rename_all = "snake_case")]`

The `ConvergenceStatus` enum uses serde's `rename_all` attribute:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConvergenceStatus {
    Converged,
    MaxRoundsExceeded,
    SingleModel,
    InsufficientModels,
    Cancelled,
}
```

The CLI serializes the status through serde, falling back to `Debug` only if serde fails (which it never should for a simple enum):

```rust
let status_str = match serde_json::to_value(&outcome.status) {
    Ok(serde_json::Value::String(s)) => s,
    _ => format!("{:?}", outcome.status).to_lowercase(),
};
```

This produces the expected `"max_rounds_exceeded"`, `"insufficient_models"`, etc.

A unit test in `types.rs` locks this behavior:

```rust
#[test]
fn convergence_status_serialization() {
    assert_eq!(
        serde_json::to_string(&ConvergenceStatus::Converged).unwrap(),
        "\"converged\""
    );
    assert_eq!(
        serde_json::to_string(&ConvergenceStatus::MaxRoundsExceeded).unwrap(),
        "\"max_rounds_exceeded\""
    );
    // ...
}
```

### Lesson

Any time an enum is used in structured output (JSON, YAML, TOML), derive `Serialize` with an explicit `rename_all` and test the serialized values. Never rely on `Debug` formatting for machine-readable output.

---

## 3. Multi-reviewer triage pattern

### Problem

When 3+ automated reviewers (e.g., Gemini, Copilot, Codex) all review the same PR, expect 30-50% duplicate findings. Without a systematic process, you either waste time fixing the same thing three ways, or miss legitimate distinct findings buried among duplicates.

### Process: fetch, group, classify, batch-fix, reply

1. **Fetch all unreplied comments** -- gather every review comment that has not been replied to yet. This gives you the full picture before acting.

2. **Group by file and line** -- if three reviewers all flag `engine.rs:57`, that is one logical finding with three instances. Group them.

3. **Classify each group** into one of four categories:
   - **Fix**: the finding is correct, action needed.
   - **Defer**: the finding is valid but out of scope (create a tracking issue or add to a plan).
   - **Duplicate**: already addressed by fixing another instance in the same group.
   - **Disagree**: the finding is incorrect or conflicts with a project convention.

4. **Batch-fix** all items classified as Fix in a single pass. This avoids the pattern of fix-push-review-fix-push-review for each individual comment.

5. **Reply to ALL comments with rationale** -- this is the most commonly skipped step. Every comment deserves a reply, even if the reply is "duplicate of finding X, fixed in commit abc123."

### Reply patterns

For duplicates, acknowledge explicitly:

> This is the same issue flagged by [other reviewer] at [file:line]. Fixed in commit abc123.

For disagreements, provide specific rationale rather than a bare "disagree":

> This follows the project's convention for [X]. See [CLAUDE.md / docs / prior decision]. Keeping as-is.

For deferred items:

> Valid finding. Out of scope for this PR. Tracked in [issue/plan reference].

### Why reply to everything

- Unreplied review comments signal to future reviewers (human or automated) that findings were ignored.
- Automated reviewers may re-flag the same issues on subsequent PRs if they see no resolution.
- It creates an audit trail for decisions, which is valuable when the same question comes up later.

---

## 4. `rustfmt` CI gotcha

### Problem

`cargo fmt` may reformat multi-line expressions that look correct to the human eye. A common case is a multi-line `return` or `match` arm that compiles and passes `clippy` but fails `cargo fmt --check` in CI.

This is especially common with chained method calls, long match patterns, and expressions that are just over the line length limit.

### Solution

Always run `cargo fmt --check` before pushing, not just after writing code. The workflow should be:

```bash
# After making changes
cargo fmt
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo fmt --check   # Final gate before push
```

The last `cargo fmt --check` catches cases where test or clippy fixes introduced new formatting drift. Running it as the final step before `git push` prevents a CI round-trip for a trivial formatting fix.

### Why it matters for automated workflows

When an AI agent writes code, it often produces syntactically valid Rust that nonetheless violates `rustfmt` conventions. If the agent writes code, runs tests, and pushes without a format check, the CI pipeline fails on formatting -- wasting a full CI cycle on something that takes 2 seconds to fix locally.

---

## Summary of patterns

| Pattern | When to use | Key detail |
|---|---|---|
| Best-so-far with status override | Runtime degradation in iterative systems | Guard on `round > 1` to avoid swallowing initial failures |
| `finalize_with_status()` | Any early-exit from an iterative loop | Single point of outcome construction prevents inconsistencies |
| `serde(rename_all)` over `Debug` | Enum serialization for structured output | `Debug` concatenates variant names without separators |
| Multi-reviewer triage | 3+ automated reviewers on one PR | Group by file+line, classify, batch-fix, reply to all |
| `cargo fmt --check` as final gate | Before every push | Catches format drift introduced by test/clippy fixes |
