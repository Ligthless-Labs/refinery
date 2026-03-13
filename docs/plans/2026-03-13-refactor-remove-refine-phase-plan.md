---
title: "refactor: Remove REFINE phase from consensus loop"
type: refactor
date: 2026-03-13
brainstorm: docs/brainstorms/2026-03-13-remove-refine-phase-brainstorm.md
---

# Remove REFINE Phase from Consensus Loop

## Overview

Remove the REFINE phase from the consensus loop entirely. The loop becomes **Propose → Evaluate → Close** (was Propose → Evaluate → Refine → Close). Round N>1 propose prompts are enriched with the model's full history: all its own prior proposals and all reviews it received, organized as per-round pairs.

## Problem Statement / Motivation

Two problems with the current refine phase:

1. **Redundancy.** Refinement does the same thing as a feedback-aware proposal: take your previous answer + reviews, produce an improved answer. A refine step followed by a propose step is doing the same work twice, burning N extra API calls per round.

2. **Score/answer mismatch.** Scores are computed on proposals, but the "winning answer" returned to the user is a refinement that was never scored by anyone. The convergence check says "this proposal scored 9.2" but the output is a different text entirely (`engine.rs:345` — `self.last_answers.clone_from(&refinement_set.refinements)`).

The fix: fold refinement into proposal. Round N>1 proposals include the model's full trajectory (all prior proposals + reviews per round), so models naturally improve. The winning answer is the actual scored proposal.

## Proposed Solution

### Key Decisions (from brainstorm)

- **Feedback format:** Each model sees all of its own prior proposals and all reviews it received, organized as per-round pairs (round 1: proposal + reviews, round 2: proposal + reviews, ...). Full trajectory, not just the latest.
- **No cross-model proposal sharing:** Models don't see other models' proposals. They only see reviews of their own work.
- **Scope:** Only remove refine. Don't change evaluate or close logic.

### What does NOT change

- **No cross-model proposal sharing** — models don't see other models' proposals
- **Evaluate phase** — unchanged
- **Close phase** — unchanged
- **Prompt injection defenses** — history content sanitized with nonce-delimited tags + DATA instruction

### Cost savings

Per-round calls drop from N²+N to N² (remove N refine calls).

| Models | Before (N²+N) | After (N²) | Saved |
|--------|---------------|------------|-------|
| 2      | 6             | 4          | 2     |
| 3      | 12            | 9          | 3     |
| 5      | 30            | 25         | 5     |
| 7      | 56            | 49         | 7     |

## Technical Approach

### History Data Flow

The engine stores per-model history: `HashMap<ModelId, Vec<(String, Vec<(String, String)>)>>` — each model's trajectory as (proposal, reviews) pairs per completed round. In round N>1, the propose phase receives this history and builds per-model context.

### History-aware propose prompt

New function that builds a propose prompt enriched with the model's trajectory. For round 1, falls through to existing `propose_prompt()`. For round N>1, injects per-round pairs:

```
<your_history>
<round number="1">
<your_proposal>
[model's round-1 proposal]
</your_proposal>
<reviews_received>
<review reviewer="Reviewer A">
[sanitized review text]
</review>
<review reviewer="Reviewer B">
[sanitized review text]
</review>
</reviews_received>
</round>
<round number="2">
...
</round>
</your_history>
```

```rust
// prompts.rs
pub fn propose_with_history_prompt(
    user_prompt: &str,
    round_ctx: &str,
    history: &[(String, Vec<(String, String)>)], // Vec of (proposal, reviews) per round
) -> String
```

Uses `sanitize_for_delimiter()` + nonce-delimited tags for proposals, and `<review>` tags with sanitization for reviews. Includes the `DATA` instruction to prevent prompt injection from historical content.

### Modified: propose phase

```rust
// propose.rs — run() signature gains optional history parameter
pub async fn run(
    providers: &[Arc<dyn ModelProvider>],
    prompt: &str,
    round: u32,
    round_ctx: &str,
    semaphore: &Arc<Semaphore>,
    timeout: Duration,
    additional_context: Option<&str>,
    model_histories: Option<&HashMap<ModelId, Vec<(String, Vec<(String, String)>)>>>,
    progress: Option<ProgressFn>,
) -> ProposalSet
```

Round 1: calls `propose_prompt()` as before (no history).
Round N>1: calls `propose_with_history_prompt()` with model-specific history.

### Dependency chain

```
engine.rs
  ├─→ phases::refine::run()         [DELETE call]
  ├─→ RefinementSet                 [DELETE type]
  └─→ RoundOutcome.refinements      [DELETE field]

prompts.rs
  ├─→ refine_prompt()               [DELETE]
  └─→ sanitize_for_review_tag()     [DELETE — only used by refine_prompt]

progress.rs
  ├─→ ModelRefined                  [DELETE event]
  └─→ ModelRefineFailed             [DELETE event]

cli/main.rs
  ├─→ ModelRefined render arm       [DELETE]
  ├─→ ModelRefineFailed render arm   [DELETE]
  └─→ Refinement artifact export    [DELETE]

types.rs
  ├─→ RefinementSet definition      [DELETE]
  ├─→ RoundOutcome.refinements      [DELETE field]
  ├─→ Phase::Refine variant         [DELETE]
  └─→ Cost formula                  [UPDATE N²+N → N²]
```

### Blast Radius

#### Delete

| Target | Location |
|--------|----------|
| `phases/refine.rs` | Entire file — `crates/refinery_core/src/phases/refine.rs` |
| `pub mod refine` | `crates/refinery_core/src/phases/mod.rs:4` |
| `RefinementSet` struct | `crates/refinery_core/src/types.rs:70-76` |
| `Phase::Refine` variant | `crates/refinery_core/src/types.rs:108` + Display impl line 117 |
| `ModelRefined` event | `crates/refinery_core/src/progress.rs:40` |
| `ModelRefineFailed` event | `crates/refinery_core/src/progress.rs:43` |
| `refine_prompt()` | `crates/refinery_core/src/prompts.rs:164-197` |
| `sanitize_for_review_tag()` | `crates/refinery_core/src/prompts.rs:20-26` |
| `refine_prompt` test | `crates/refinery_core/src/prompts.rs:305-316` |
| `sanitize_for_review_tag` test | `crates/refinery_core/src/prompts.rs:287-294` |
| Refine artifact export | `crates/refinery_cli/src/main.rs:504-508` |
| `ModelRefined` render arm | `crates/refinery_cli/src/main.rs:589-591` |
| `ModelRefineFailed` render arm | `crates/refinery_cli/src/main.rs:593-596` |

#### Modify

| Target | Location | Change |
|--------|----------|--------|
| `engine.rs` Session struct | `:142-158` | Add `model_histories: HashMap<ModelId, Vec<(String, Vec<(String, String)>)>>` |
| `engine.rs` next_round_with | `:314-332` | Remove refine phase call, remove refine call_count |
| `engine.rs` next_round_with | `:345` | `last_answers.clone_from(&proposal_set.proposals)` instead of refinements |
| `engine.rs` next_round_with | `:369-377` | Remove `refinements` from `RoundOutcome` construction |
| `engine.rs` next_round_with | After evaluate | Collect each model's proposal + reviews into `model_histories` |
| `engine.rs` single-model | `:179-200` | Remove `refinements` field from single-model `RoundOutcome` |
| `propose.rs` run() | `:15-24` | Accept `model_histories: Option<&HashMap<ModelId, Vec<...>>>` parameter |
| `propose.rs` run() | `:33` | Call `propose_with_history_prompt()` for round > 1 when history exists |
| `prompts.rs` | New function | `propose_with_history_prompt()` — enriched prompt with per-round proposal+review pairs |
| `prompts.rs` system_prompt | `:50-56` | "iteratively refining" → "iteratively improving" |
| `RoundOutcome` struct | `types.rs:79-88` | Remove `refinements: RefinementSet` field |
| `types.rs` cost formula | `:213-220` | Change from `n*n + n` to `n*n` |
| `types.rs` cost comment | `:218` | Update comment to `N (propose) + N*(N-1) (evaluate) = N²` |
| `engine.rs` estimate test | `:648-654` | Update expected: 3 models = 9 (was 12) |
| `types.rs` estimate test | `:318-337` | Update all expected values |
| `cli/main.rs` doc comment | `:17-19` | Remove "refine" from description |
| `cli/main.rs` help text | `:72` | Remove "refinements" from `--output-dir` description |

#### Preserve (unchanged)

- `sanitize_for_delimiter()`, `wrap_answer()` — still needed for evaluate
- Evaluate phase — unchanged
- Close phase — unchanged
- `testing.rs` — no refine references

## Acceptance Criteria

- [x] `Phase::Refine` variant removed; `Phase` enum has only `Propose`, `Evaluate`, `Close`
- [x] `RefinementSet` type deleted
- [x] `refine.rs` file deleted
- [x] `RoundOutcome` has no `refinements` field
- [x] `last_answers` set from `proposal_set.proposals` in engine
- [x] Round N>1 propose prompts include full model history (all prior proposals + reviews per round)
- [x] No cross-model proposal sharing in history
- [x] Winning answer is always a scored proposal
- [x] Cost formula is N² per round (N propose + N(N-1) evaluate)
- [x] Prompt injection defenses preserved on injected history (nonces, sanitization, DATA instruction)
- [x] All existing tests updated and passing
- [x] New tests for history-aware propose prompt
- [x] `cargo test --workspace` green
- [x] `cargo clippy --workspace -- -D warnings` clean
- [x] CLI `--output-dir` artifacts no longer emit `refine-*.md` files
- [x] CLI progress rendering has no refine-related arms

## Implementation (5 atomic commits)

### Commit 1: Add `propose_with_history_prompt()` to prompts.rs

New function that builds a propose prompt enriched with the model's trajectory. For round 1, falls through to existing `propose_prompt()`. For round N>1, injects per-round pairs using the XML schema above.

Also add tests for the new prompt function: verifies history rendering, sanitization, empty-history fallback.

**Gate:** `cargo test -p refinery_core` (new tests pass, existing tests still pass)

### Commit 2: Wire history through propose phase

- `propose.rs`: Add `model_histories: Option<&HashMap<ModelId, Vec<...>>>` parameter to `run()`
- For each model, extract its own proposals and reviews from history
- Call `propose_with_history_prompt()` instead of `propose_prompt()` when history is non-empty
- `engine.rs`: Add `model_histories` field to `Session`, pass to `propose::run()`
- `engine.rs`: After evaluate phase, collect each model's proposal + reviews received into history

**Gate:** `cargo test -p refinery_core` (engine tests still pass — refine still runs but history is now also tracked)

### Commit 3: Remove refine phase from engine

- `engine.rs`: Remove the refine phase call (lines 314-332) and refine call_count
- `engine.rs`: Set `last_answers.clone_from(&proposal_set.proposals)` instead of `refinement_set.refinements`
- `engine.rs`: Remove `refinements` from `RoundOutcome` construction
- `engine.rs`: Remove `RefinementSet` from single-model `RoundOutcome`
- `types.rs`: Remove `refinements` field from `RoundOutcome`
- `types.rs`: Delete `RefinementSet` struct
- `types.rs`: Update cost formula from `n*n + n` to `n*n`
- `progress.rs`: Remove `ModelRefined` and `ModelRefineFailed` variants
- `types.rs`: Remove `Phase::Refine` variant and its Display arm
- `phases/mod.rs`: Remove `pub mod refine`
- Delete `phases/refine.rs`
- `prompts.rs`: Delete `refine_prompt()`, `sanitize_for_review_tag()`, and their tests
- `prompts.rs`: Update `system_prompt()` — "iteratively refining" → "iteratively improving"
- Update all engine tests and cost estimate tests with new expected values

**Gate:** `cargo test -p refinery_core`

### Commit 4: Update CLI

- `main.rs`: Remove refine artifact export from `save_round_artifacts()` (lines 504-508)
- `main.rs`: Remove `ModelRefined` and `ModelRefineFailed` arms from `render_progress()`
- `main.rs`: Remove "refine" from CLI doc comment (line 19)
- `main.rs`: Remove "refinements" from `--output-dir` help text (line 72)

**Gate:** `cargo test --workspace`

### Commit 5: Final cleanup + clippy

- Remove any dead imports (`RefinementSet`, `Phase::Refine` references)
- Remove unused `use` statements
- Run full verification

**Gate:** `cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Key Files

| File | Role |
|------|------|
| `crates/refinery_core/src/prompts.rs` | New `propose_with_history_prompt()`, delete `refine_prompt()` |
| `crates/refinery_core/src/phases/propose.rs` | Accept + render history in round N>1 |
| `crates/refinery_core/src/engine.rs` | Track model_histories, remove refine call, set last_answers from proposals |
| `crates/refinery_core/src/types.rs` | Delete RefinementSet, remove from RoundOutcome, update cost formula |
| `crates/refinery_core/src/progress.rs` | Remove ModelRefined/ModelRefineFailed |
| `crates/refinery_core/src/phases/refine.rs` | **Delete entirely** |
| `crates/refinery_cli/src/main.rs` | Remove refine artifact export + progress arms |

## Success Metrics

- All 89+ workspace tests pass
- Zero clippy warnings
- No remaining references to "refine" in code paths (grep verification)
- Cost estimate for N=3 returns 9 (was 12)

## Dependencies & Risks

**Risk: History prompt length.** Multi-round history could produce long prompts. Mitigation: this is bounded by `max_rounds` (max 20) and CLI models handle long contexts well. Monitor in practice.

**Risk: Prompt injection in historical content.** Mitigation: use existing `sanitize_for_delimiter()` + `wrap_answer()` patterns, plus `DATA` instruction. Consistent with documented security learnings (`docs/solutions/security-issues/prompt-injection-prevention-multi-model.md`).

**No external dependencies.** Pure internal refactoring.

## Verification

- `cargo test --workspace` — all tests pass
- `cargo clippy --workspace -- -D warnings` — clean
- Cost estimate for N=3 returns 9 (was 12)
- `--output-dir` produces only `propose-*.md` and `evaluate-*.json` per round (no `refine-*.md`)
- Single-model short-circuit still works without `RefinementSet`

## References & Research

### Internal References
- Brainstorm: `docs/brainstorms/2026-03-13-remove-refine-phase-brainstorm.md`
- Prompt injection learnings: `docs/solutions/security-issues/prompt-injection-prevention-multi-model.md`
- Graceful degradation patterns: `docs/solutions/runtime-errors/graceful-degradation-and-review-patterns.md`
- Engine architecture: `docs/plans/2026-02-10-feat-consensus-loop-engine-plan.md`
