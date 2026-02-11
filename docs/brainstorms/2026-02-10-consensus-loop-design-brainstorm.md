---
date: 2026-02-10
topic: consensus-loop-design
---

# Iterative Multi-Model Consensus Loop

## What We're Building

A Rust library and CLI that orchestrates iterative consensus across multiple AI models. Given a prompt, N models independently produce answers, cross-review each other's work, vote/score, then refine — repeating until a configurable convergence criterion is met. The library owns the conversation state internally and delegates execution to provider backends (initially via CLI subprocesses).

## Why This Approach

Classical distributed systems consensus algorithms (Paxos, Raft, PBFT) solve a fundamentally different problem: getting deterministic nodes to agree on a single exact value despite crash/Byzantine failures. AI model consensus is about **semantic convergence** among non-deterministic participants whose outputs vary even on identical inputs.

The relevant analogies are the **Delphi method** (iterative expert polling), **multi-agent debate** (AI alignment literature), and **jury deliberation**. From distributed systems, we borrow:
- **Quorum concepts** — K-of-N agreement thresholds
- **Round-based structure** — iterative refinement with defined phases
- **Byzantine intuition** — treating consistently hallucinating models as unreliable actors
- **Convergence criteria** — knowing when to stop

We rejected direct application of Paxos/Raft because there is no need for leader election, log replication, or agreement on a single deterministic value.

## Round Structure

Each round has four phases:

```
Round R:
  Phase 1: PROPOSE (N parallel calls)
    Each model produces an answer independently.

  Phase 2: CROSS-REVIEW + VOTE (N² + N parallel forks)
    Each model's session is forked into:
      - (N-1) review forks: review each OTHER model's answer (qualitative)
      - 1 score fork: rate all answers numerically (1-10)
      - 1 rank fork: rank all answers ordinally
    Reviews feed Phase 3. Scores and rankings feed the closing strategy.

  Phase 3: REFINE (N parallel calls)
    Roll back each model's session to pre-fork state.
    Inject all reviews from Phase 2.
    Each model produces a refined answer.

  Phase 4: CLOSE CHECK
    Apply closing strategy to scores/rankings.
    If converged → output final result.
    If not → next round (refined answers become Phase 1 of Round R+1).
```

### Parallelism Profile

| Phase | Parallel calls | Purpose |
|-------|---------------|---------|
| PROPOSE | N | Initial/refined answers |
| CROSS-REVIEW | N × (N-1) | Qualitative feedback |
| VOTE (score) | N | Numeric scores (1-10) |
| VOTE (rank) | N | Ordinal rankings |
| REFINE | N | Improved answers |

Total per round: N² + 3N calls (Phase 2 dominates).

## Architecture

### Crate Structure

- **Library crate** — core orchestration, session state model, provider trait, closing strategies
- **CLI crate** — thin wrapper, config parsing, output formatting

### Conversation State

The library maintains an internal conversation model (messages, roles, content including images). This is the source of truth for all session state.

- **Fork** = clone the conversation history at a point, append review/vote instructions
- **Rollback** = discard fork, return to the pre-fork conversation state
- Provider backends serialize this internal model to/from provider-specific formats

### Provider Backends (v0: CLI-based)

Shell out to provider CLIs in structured/non-interactive mode:

| Provider | CLI | Mode |
|----------|-----|------|
| Anthropic | `claude` | `--output-format json`, `--resume` for multi-turn |
| OpenAI | `codex exec` | Structured output mode |
| Google | `gemini` | `-p` / `--prompt` for non-interactive |

This avoids implementing three HTTP API clients upfront. The orchestration logic is the hard part; the backends are swappable.

**Future:** native API clients as drop-in replacements for lower latency and richer control.

### Closing Strategies (configurable)

- **Vote threshold** — converge when scores exceed a threshold or rankings stabilize
- **Synthesis** — a judge model synthesizes the best parts of all answers
- **Delphi** — iterative narrowing until variance drops below a threshold
- User selects strategy at invocation time

### Inputs / Outputs

- **Inputs:** text, images (multimodal — all three target providers support vision)
- **Outputs:** text (v0), structured outputs and tool calls (future, lowest-common-denominator across providers)

## Key Decisions

- **CLI-first backends for v0:** Focus engineering effort on the consensus loop, not API client plumbing. Swap to native APIs later.
- **Internal session state model:** The library owns conversation state. Provider quirks are an adapter concern, not a core concern.
- **Dual quantitative signals:** Both numeric scores AND ordinal rankings run in parallel. Two independent lenses on convergence are cheap and more robust.
- **Reviews are separate from votes:** Reviews carry qualitative feedback for refinement. Votes carry quantitative signals for convergence. Different purposes, different forks.
- **Closing strategy is pluggable:** No single "right" way to close — the user/developer chooses.

## Open Questions

- Exact structured output schemas for reviews, scores, and rankings
- Session state serialization format for fork/rollback (in-memory clone vs. persistent snapshots)
- CLI invocation details per provider (exact flags, streaming support, error recovery)
- How tool calls work cross-provider (lowest common denominator definition)
- Whether to support weighting models differently (e.g., trust one model's votes more)
- Timeout and error handling when a provider CLI hangs or fails mid-round

## Next Steps

-> `/workflows:plan` for implementation details
