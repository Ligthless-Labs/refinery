---
title: "feat: Iterative Multi-Model Consensus Engine"
type: feat
date: 2026-02-10
enhanced: 2026-02-10 (via /deepen-plan), 2026-02-11 (via /deepen-plan round 2)
reviewed: 2026-02-11 (via /plan_review — 6 agents: DHH, simplicity, architecture, performance, security, agent-native)
amended: 2026-02-11 (review findings incorporated — 15 inline fixes, 2 factual corrections, 3 scope reductions)
completed: 2026-02-11 (PR #1 — all 61 tests passing, CI green, Gemini review triaged)
post-review: 2026-02-11 (4-agent review sweep — security, architecture, performance, simplicity — 30+ findings triaged, 15 fixes applied)
---

# Iterative Multi-Model Consensus Engine

## Enhancement Summary

**Deepened on:** 2026-02-10, 2026-02-11 (round 2)
**Research agents used:**
- Round 1: architecture-strategist, performance-oracle, security-sentinel, code-simplicity-reviewer, agent-native-reviewer, pattern-recognition-specialist, agent-native-architecture skill, orchestrating-swarms skill, web search (multi-agent debate + Delphi method)
- Round 2: Claude/Codex/Gemini CLI research agents, convergence algorithms researcher, multi-agent debate prompt researcher, error handling patterns researcher, Rust testing patterns researcher, agent-native reviewer (round 2), Rust workspace patterns researcher, tokio subprocess patterns researcher

### Key Improvements (Round 1)

1. **Significant v0 simplification** — flattened module structure (~14 files vs ~35), removed 4 dependencies (governor, tokio-util, nix, uuid), deferred images/TOML config/Synthesis+Delphi strategies to v0.1+
2. **5 new security design decisions (D9-D13)** — subprocess tool-use disabled, credential scoping via env_clear, output sanitization with randomized delimiter nonces, self-scoring exclusion, command injection prevention
3. **Critical concurrency fix** — rate limiter acquired BEFORE semaphore to prevent permit starvation (flagged independently by architecture + performance reviewers)
4. **Phase parallelism discovery** — REVIEW and VOTE are independent and can run concurrently, saving ~30% per-round wall time
5. **Agent-native CLI contract** — defined JSON output schema, structured errors, NDJSON progress format, stepping library API

### Key Improvements (Round 2)

6. **Exact CLI invocation specs per provider** — researched actual headless mode flags, JSON output schemas, known bugs, and workarounds for Claude CLI, Codex CLI, and Gemini CLI
7. **Security fix: D7 env_clear() breaks PATH** — resolve CLI binaries to absolute paths at startup via `which` before calling `env_clear()`
8. **Security fix: D9 Claude `--allowedTools none` invalid** — ~~use `--disallowedTools` with comprehensive blocklist~~ `[REVIEW FIX]` use `--tools ""` instead; ~~Gemini has NO tool restriction flags~~ `[REVIEW FIX]` Gemini has `--sandbox`, `--approval-mode plan`, `--allowed-tools ""`
9. **Agent-native stepping API contract defined** — `RoundOutcome` struct, `RoundOverrides` for inter-round intervention, `Session::cancel()`, `Engine::estimate()` as library API
10. **Error handling architecture** — `thiserror` 2.0 for library crates, `anyhow` for CLI; no `#[from]` on context-requiring variants; `ErrorResponse` at CLI boundary
11. **Testing infrastructure** — `tokio::test(start_paused = true)` for deterministic time, `insta` for prompt snapshots, `rstest` for parameterized tests, `proptest` for boundary types
12. **Workspace configuration** — resolver 3 (Rust 2024), `[workspace.dependencies]` for centralized versions, feature flags for optional providers, Bazel `MODULE.bazel` with `crate_universe`
13. **Prompt engineering best practices** — chain-of-thought scoring, rubric anchoring, position bias mitigation via randomization, temperature ≤0.3 for scoring consistency
14. **Convergence math grounded** — Borda count for v0 ranking aggregation, Kendall tau distance for stability detection, IQR-based convergence for Delphi (v0.1), Kemeny-Young feasible for N<8 (v1)

### New Considerations Discovered

- Self-voting bias: models consistently favor their own output; excluding self-scoring is mandatory
- Gemini CLI has no structured output schema enforcement and broken headless multi-turn (Issue #14180)
- At N=5, 5 rounds ≈ 200 API calls, $5-15 estimated cost — budget cap needed
- Process group management required: `kill_on_drop` only kills direct child, not grandchildren spawned by CLI tools
- Multi-agent debate literature (S²-MAD) shows redundancy filtering can cut tokens by 94.5% — future optimization
- Adaptive stability detection via Beta-Binomial mixture models — more sophisticated than threshold-based convergence
- `env_clear()` removes `PATH`, breaking binary resolution — must resolve to absolute paths before clearing
- `serde_json` has no native max depth enforcement — need pre-parse size check or `json_threat_protection` crate
- `[REVIEW FIX]` Claude `--tools ""` disables all tools (replaces incorrect `--allowedTools none` and fragile `--disallowedTools` blocklist); `--max-turns 1` does NOT prevent tool use within that turn
- Gemini `--resume` was broken (Issue #14180, fixed in nightly 0.20.0+); `response` field sometimes contains markdown-wrapped JSON (Issue #11184)
- Codex CLI uses JSONL event streaming, not single JSON object — must parse `turn.completed` event for final response
- `RoundOutcome` (stepping API return type) was never defined — critical gap for agent integration
- Gemini CLI has no `--system-prompt` flag — must use `GEMINI_SYSTEM_MD` env var or `GEMINI.md` file

---

**Addendum: 2026-02-11 — Critical review findings incorporated (/plan_review)**

The `/plan_review` ran 6 parallel review agents (DHH, simplicity, architecture, performance, security, agent-native). This addendum incorporates critical and high-priority findings. Changes are marked with `[REVIEW FIX]` inline.

### Verified Corrections

**[REVIEW FIX] D9: Gemini CLI DOES have sandbox and tool restriction flags.**
The plan incorrectly stated "NO tool restriction flags exist" and "Gemini CLI has no sandbox mode." Verified against [official Gemini CLI sandbox docs](https://geminicli.com/docs/cli/sandbox/) and [configuration docs](https://geminicli.com/docs/get-started/configuration/):
- `--sandbox` / `-s` — enables macOS Seatbelt or container-based sandboxing
- `--approval-mode plan` — read-only mode for tool calls
- `--allowed-tools` — comma-separated allowlist of permitted tools
- `tools.exclude` in settings — exclude tools from discovery

Updated invocation: `gemini --sandbox --approval-mode plan --allowed-tools "" ...`

**[REVIEW FIX] D9: Claude `--tools ""` disables all tools (simpler than blocklist).**
The plan used a verbose `--disallowedTools` blocklist. The [`--tools ""`](https://code.claude.com/docs/en/cli-reference) flag disables ALL tools in one shot. The blocklist approach was fragile (new tools added to Claude would not be blocked). Updated invocation.

**[REVIEW FIX] `unsafe_code = "forbid"` vs `setsid` via `pre_exec`.**
The plan sets `unsafe_code = "forbid"` in workspace lints but requires `pre_exec` for `setsid` process group management, which is an unsafe function. **Decision: defer `setsid` to v0.1. `kill_on_drop(true)` is sufficient for v0 subprocess cleanup. Change lint to `deny` for future flexibility.**

**[REVIEW FIX] ProviderError must live in converge_core.**
If `ProviderError` lives in `converge_providers`, then `converge_core`'s `ConvergeError::PhaseFailure` can't reference it without a circular dependency. `ProviderError` is defined in `converge_core` (as part of the provider trait contract) and implemented by providers.

### Design Simplifications (v0 scope reduction)

**[REVIEW FIX] Drop rankings from v0. Score-only convergence.**
Multiple reviewers (simplicity, performance, DHH) recommended dropping ordinal rankings. Rankings add N extra API calls/round, Borda count/Kendall tau add complexity. Score-based convergence (mean score ≥ threshold + same winner for K rounds) is sufficient. Rankings return in v0.1 alongside Kemeny-Young.

**[REVIEW FIX] Merge REVIEW + SCORE into single EVALUATE call.**
Reviews and scores share the same context (reading another model's answer) and can be produced in a single LLM call. This reduces per-round calls from N²+2N to N(N-1). At N=5: 40→30, at N=3: 18→12 calls/round.

**[REVIEW FIX] ClosingStrategy::check is async from v0.**
The Synthesis strategy (v0.1) requires an LLM call. Making the trait async from v0 avoids a breaking API change later.

**[REVIEW FIX] Rate limiter deferred to v1.**
The concurrency diagram referenced a rate limiter but no implementation was specified (`governor` was removed). v0 relies on the configurable semaphore only. Per-provider rate limiting requires rate limit headers that CLI backends don't expose. Add with native API backends in v1.

**[REVIEW FIX] Per-provider semaphores (v0.1).**
A single global semaphore doesn't account for different per-provider rate limits. v0 ships with a global semaphore (simple, sufficient for CLI backends). v0.1 adds per-provider semaphores with native API backends.

### Updated Cost Projections (post-review)

| N | Calls/round (before) | Calls/round (after) | Calls/5 rounds | Est. cost |
|---|---------------------|--------------------|--------------------|-----------|
| 2 | 12 | 6 | 30 | $0.50-2 |
| 3 | 18 | 12 | 60 | $1-4 |
| 5 | 40 | 30 | 150 | $3-10 |
| 7 | 70 | 56 | 280 | $7-22 |

---

## Overview

Build a Rust library and CLI that orchestrates iterative consensus across multiple AI models. Given a prompt, N models independently produce answers, cross-review each other's work, score and rank all answers, then refine — repeating until a configurable convergence criterion is met. The library owns conversation state internally and delegates execution to provider CLI backends (`claude`, `codex exec`, `gemini -p`).

v0 scope: text-only input, Vote Threshold closing strategy, three provider backends. Synthesis, Delphi, image support, and config files are v0.1+.

## Problem Statement

There is no good tool for systematically getting multiple AI models to deliberate on a question and converge toward a high-quality answer. Users currently run the same prompt against multiple models manually, compare outputs by eye, and pick a winner. This misses the iterative refinement signal: models reviewing and improving each other's work across rounds.

### Research Context

Multi-agent debate (MAD) is an active area of AI research. Key findings that inform this design:

- Multiple LLM instances debating over multiple rounds "significantly enhances mathematical and strategic reasoning and improves the factual validity of generated content, reducing fallacious answers and hallucinations" ([Improving Factuality and Reasoning via Multiagent Debate](https://composable-models.github.io/llm_debate/))
- Heterogeneous multi-agent debate (different models, not just same-model sampling) yields "4-6% higher accuracy and over 30% fewer factual errors" ([ICLR 2025 MAD analysis](https://d2jud02ci9yv69.cloudfront.net/2025-04-28-mad-159/blog/mad/))
- S²-MAD demonstrates redundancy filtering can "cut token costs by up to 94.5% while keeping accuracy loss below 2%" — a future optimization for this engine
- Adaptive stability detection using Beta-Binomial mixture models provides more mathematically grounded convergence than simple thresholds ([Multi-Agent Debate for LLM Judges](https://arxiv.org/html/2510.12697v1))
- The Delphi method's four-tier consensus classification (Strong, Conditional, Operational, Divergent) maps naturally to our `ConvergenceStatus` enum ([Human-AI Hybrid Delphi](https://arxiv.org/html/2508.09349v1))

## Proposed Solution

A round-based consensus engine with four phases per round:

```
PROPOSE → EVALUATE (review+score merged) → REFINE → CLOSE CHECK
```
`[REVIEW FIX]` Merged CROSS-REVIEW and VOTE into a single EVALUATE phase. Each model reviews and scores each other model's answer in one LLM call, producing both qualitative feedback (for REFINE) and a numeric score (for CLOSE CHECK). Rankings dropped from v0 (score-only convergence).

The engine is model-agnostic (provider backends are pluggable) and closing strategy-agnostic (pluggable trait). v0 ships with Vote Threshold; Synthesis and Delphi strategies follow in v0.1.

v0 uses CLI-based backends (shelling out to `claude`, `codex`, `gemini`) to avoid building three HTTP API clients. Native API backends can be swapped in later without changing the core engine.

## Technical Approach

### Architecture (v0: simplified)

Three-crate workspace with flattened internal structure. The full hexagonal layering (domain/ports/driven/driving) from the infinidash sibling project is deferred until the project grows enough to warrant the ceremony — extracting flat modules into nested layers is a ~30-minute refactor.

```
converge-refinery/
├── Cargo.toml                      # Workspace root
├── MODULE.bazel                    # Bazel config (CLI build)
├── crates/
│   ├── converge_core/              # Library crate
│   │   └── src/
│   │       ├── lib.rs              # Re-exports, ModelProvider trait, ClosingStrategy trait
│   │       ├── types.rs            # All domain types: Message, Role, Content, Score, Ranking, etc.
│   │       ├── engine.rs           # Orchestrator: propose/review+vote/refine/close loop
│   │       ├── phases/             # Phase implementations as composable functions
│   │       │   ├── mod.rs
│   │       │   ├── propose.rs
│   │       │   ├── evaluate.rs    # [REVIEW FIX] merged review+score into single EVALUATE phase
│   │       │   ├── refine.rs
│   │       │   └── close.rs
│   │       ├── strategy.rs         # ClosingStrategy trait + VoteThreshold impl
│   │       ├── prompts.rs          # Prompt templates (review, score, rank, refine)
│   │       └── error.rs            # ConvergeError + ProviderError [REVIEW FIX: both in core]
│   ├── converge_providers/         # Provider backend crate
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── claude.rs           # Claude CLI adapter
│   │       ├── codex.rs            # Codex CLI adapter
│   │       ├── gemini.rs           # Gemini CLI adapter
│   │       └── process.rs          # Subprocess utilities (spawn, timeout, kill, JSON extraction)
│   └── converge_cli/               # CLI crate (thin wrapper)
│       └── src/
│           └── main.rs             # clap args + engine wiring + tracing output
├── docs/
│   ├── brainstorms/
│   └── plans/
└── tests/
    └── integration/
```

~12 files `[REVIEW FIX: reduced from ~14 — merged review.rs+vote.rs into evaluate.rs]`. The 3-crate split is justified: providers depend on `tokio::process` and external CLIs while the core is testable with mocks only; the CLI is a thin wrapper.

### Research Insight: Prompt Templates Belong in Their Own Module

Prompt templates for reviews, scores, rankings, and refinement instructions are consumed by engine phases but are a separate concern from orchestration. Extracting them to `prompts.rs` makes templates testable, customizable, and editable without touching engine logic. This is the first step toward making templates user-configurable (v1).

### Key Design Decisions

**D1: Conversation state is owned by the library, not the CLI backends.**
Even though Claude CLI has native `--fork-session`, we maintain our own conversation state for all providers. This gives us uniform semantics and makes the system testable without real CLI backends. Claude's `--fork-session` is an optimization we can layer on later.

**D2: Structured output schemas use JSON with prompt engineering for all providers.**
Rather than relying on provider-specific schema enforcement (`--json-schema` for Claude, `--output-schema` for Codex, nothing for Gemini), we use prompt engineering with clear JSON schema instructions for all providers uniformly. We parse and validate the JSON from the model's text response. On parse failure: retry once with the malformed output + correction prompt. On second failure: log a warning, exclude that model's vote from the round, continue.

### Research Insight: JSON Parsing Must Be Strict

Parse responses with: (a) max response size before extraction (100KB), (b) strict schema validation after parsing (not just "does it parse?"), (c) deterministic extraction — require JSON inside ` ```json ` fences, not heuristic scanning for `{`. The `serde_json` crate's error messages include byte offsets for targeted recovery before full retry.

**CORRECTION (Round 2):** `serde_json` has NO native max depth enforcement. The original "max JSON depth (10)" claim was incorrect. Options: (a) pre-parse byte scan counting nesting depth (simple, ~20 LOC), (b) `json_threat_protection` crate, (c) accept the risk for v0 since we already cap response size at 100KB. **Recommendation: option (a) — simple depth counter before `serde_json::from_str`.**

**D3: Async-first API with tokio.**
The N^2 parallelism makes async mandatory. The public API is async. Both a run-to-completion API (`Engine::run()`) and a stepping API (`Engine::start()` / `session.next_round()` / `session.finalize()`) are provided.

### Research Insight: Stepping API Enables Agent Integration

An embedding agent cannot inspect state between rounds or alter strategy mid-run with only `Engine::run()`. The stepping API gives library consumers parity with what the CLI can observe — the "primitives over workflows" principle.

**D4: Failure policy — graceful degradation, minimum N=2.**
- Individual call failure: retry up to 2 times with exponential backoff (1s, 4s). Retries happen **inside** the semaphore permit to avoid re-acquisition overhead and starvation of other tasks.
- If a model fails all retries in PROPOSE: drop it from the round (proceed with N-1). Minimum N=2 to continue; below that, abort.
- If a model fails in REVIEW/VOTE: drop that specific review/vote. The model stays in the round if it succeeded in PROPOSE.
- If a model fails in REFINE: keep its previous (unrefined) answer.
- All failures are logged and reported in the final outcome.

**D5: Termination guarantees — max_rounds defaults to 5, hard cap 20.**
Every run has a `max_rounds` parameter (default 5, max 20). When exceeded without convergence, the engine returns the best answer so far (highest mean score from the last round) with a `ConvergenceStatus::MaxRoundsExceeded` flag.

**D6: N=1 short-circuits. N capped at 7 with warning at N>5.**
If N=1, skip the consensus loop entirely. Run the single model once, return its answer with `ConvergenceStatus::SingleModel`. Hard cap N=7; warn at N>5 about quadratic cost scaling.

**D7: Credentials via scoped environment variables, never CLI arguments.**
Each subprocess environment is **cleared** with `Command::env_clear()` then selectively injected with only the provider-specific credentials. The parent process's full environment is never inherited. Each adapter declares its required env vars.

### Research Insight: Credential Scoping

Blanket environment inheritance means the Claude subprocess receives the Google API key and vice versa. With up to 10+ concurrent subprocesses, the attack surface is multiplied. `env_clear()` + selective `env()` per provider implements the principle of least privilege.

**CRITICAL FIX (Round 2):** `env_clear()` also removes `PATH`, which means the OS cannot resolve CLI binary names. Each adapter MUST resolve its binary to an absolute path at startup (via `which claude`, `which codex`, `which gemini`) and store the path. Then `Command::new(absolute_path)` works after `env_clear()`. Additionally, inject `PATH=/usr/bin:/bin` as a minimal PATH for any child process needs.

**D8: Prompt injection mitigation — sanitization + randomized delimiters.**
When injecting Model A's answer into Model B's review prompt:
1. Generate a per-round random nonce (e.g., `a7f3b2`).
2. Wrap answers in `<answer-a7f3b2 model="model-a">...</answer-a7f3b2>` tags.
3. Before injection, scan model output for the delimiter tags and escape any occurrences (`<answer-` → `&lt;answer-`).
4. Cap injected answer length (configurable, default 50KB).
5. Instruct reviewing model to treat content within tags as data, not instructions.

### Research Insight: XML Delimiters Alone Are Insufficient

A model can output `</answer>Ignore previous instructions...` to break out of the delimited block. LLMs process XML as token sequences, not as parsed structures. Sanitizing model outputs by escaping occurrences of the delimiter tags is necessary. Randomized nonces per round raise the bar further.

**D9: Subprocess isolation — tool use disabled in all CLI invocations.** `[REVIEW FIX]`
All provider CLIs are invoked with tool execution, file access, and shell commands **disabled**:
- **Claude:** `--tools ""` (disables ALL tools in one shot; simpler and more robust than `--disallowedTools` blocklist which breaks when new tools are added)
- **Codex:** `--sandbox read-only` (prevents filesystem writes and network access)
- **Gemini:** `--sandbox` (enables macOS Seatbelt or container-based sandboxing) + `--approval-mode plan` (read-only mode for tool calls) + `--allowed-tools ""` (empty allowlist)

This prevents a prompt-injected response from triggering file reads, writes, or network calls within any provider CLI. All three providers now have tool restriction mechanisms.

### Research Insight: Most Dangerous Attack Chain

Without disabling tool-use, the prompt injection risk (D8) escalates from "model produces wrong answer" to "model reads your SSH keys." The combination of prompt injection and subprocess isolation represents the system's most dangerous attack chain. `[REVIEW FIX]` All three providers now have tool restriction mechanisms, closing this attack chain for all backends.

**D10: Self-scoring exclusion + answer anonymization.**
During the VOTE phase, each model scores and ranks only the OTHER models' answers, never its own. This follows the same N*(N-1) pattern used in reviews. Answers are presented with randomized labels ("Answer A", "Answer B") that change each round to prevent style-based identification.

### Research Insight: Self-Preference Bias

Self-preference bias is well-documented in LLM evaluation literature. Models consistently rate their own outputs higher. Combined with prompt injection, this enables a model to guarantee convergence on its own answer. Excluding self-scoring eliminates both the bias and the attack vector.

**D11: Command injection prevention.**
All subprocess invocations use `tokio::process::Command::arg()` with direct argument passing, NEVER shell string interpolation. A `--` sentinel is inserted before user-supplied positional arguments to prevent flag injection.

### CLI Backend Implementation Details (Round 2 Research)

Exact invocation patterns per provider, validated against official docs and known issues.

#### Claude CLI (`claude`)

```bash
claude -p \
  --output-format json \
  --tools "" \
  --max-turns 1 \
  --model sonnet \
  --append-system-prompt "You are participating in a multi-model consensus process. [SYSTEM INSTRUCTIONS]" \
  -- "PROMPT_TEXT"
```
`[REVIEW FIX]` Changed from `--disallowedTools` blocklist to `--tools ""` which disables ALL tools.

**JSON output envelope:**
```json
{
  "type": "result",
  "result": "The model's text response",
  "session_id": "abc123",
  "is_error": false,
  "cost_usd": 0.042,
  "duration_ms": 3200
}
```

**Key flags:**
- `-p` — headless/non-interactive (pipe) mode. REQUIRED.
- `--output-format json` — returns structured JSON envelope above
- `--append-system-prompt` — injects system instructions without replacing Claude's default system prompt. Preferred over `--system-prompt` which replaces entirely.
- `--model` — accepts aliases: `opus`, `sonnet`, `haiku` (or full model IDs)
- `--resume SESSION_ID` / `--continue` — multi-turn (not used in v0, each call is independent)
- `--json-schema SCHEMA` — enforced structured output in `structured_output` field (not used in v0 per D2)

**Env vars:** `ANTHROPIC_API_KEY` (or `CLAUDE_SESSION_TOKEN` for OAuth)

#### Codex CLI (`codex`)

```bash
codex exec \
  --json \
  --sandbox read-only \
  -- "PROMPT_TEXT"
```

**JSON output:** JSONL event stream on stdout. Parse for the `turn.completed` event:
```jsonl
{"type":"thread.started","thread_id":"..."}
{"type":"turn.started","turn_id":"..."}
{"type":"item.text_delta","content":"partial..."}
{"type":"turn.completed","turn_id":"...","text":"FULL_RESPONSE_TEXT","usage":{"input_tokens":150,"output_tokens":300}}
```

**Key details:**
- `codex exec` — non-interactive execution mode. REQUIRED.
- `--json` — emits JSONL event stream (not a single JSON object!)
- `--sandbox read-only` — prevents filesystem writes (D9)
- `--output-schema JSON_SCHEMA` — enforced schema on `text` field of `turn.completed` (not used in v0 per D2)
- `--config developer_instructions="..."` — system prompt injection (alternative: `CODEX_DEVELOPER_INSTRUCTIONS` env var). No native `--system-prompt` flag.
- `codex exec resume --last` / `codex exec resume SESSION_ID` — multi-turn (not used in v0)
- `codex fork` — session branching (not used in v0)

**Env vars:** `OPENAI_API_KEY`

**Parsing strategy:** Read all JSONL lines, find the last `turn.completed` event, extract `text` field. If no `turn.completed` found, treat as error.

#### Gemini CLI (`gemini`)

```bash
gemini \
  --output-format json \
  --model gemini-2.5-pro \
  --sandbox \
  --approval-mode plan \
  --allowed-tools "" \
  -- "PROMPT_TEXT"
```
`[REVIEW FIX]` Added `--sandbox`, `--approval-mode plan`, and `--allowed-tools ""` for tool restriction.

**JSON output envelope:**
```json
{
  "response": "The model's text response",
  "stats": {
    "models": ["gemini-2.5-pro"],
    "tools": [],
    "files": []
  },
  "error": null
}
```

**Key details and KNOWN ISSUES:**
- Positional argument preferred over deprecated `--prompt`/`-p` flags
- `--output-format json` — returns structured JSON envelope above
- **NO structured output schema enforcement** — Issue #13388 OPEN, PR #18032 pending. Must use prompt engineering (D2).
- **NO `--system-prompt` flag** — system prompt must be injected via `GEMINI_SYSTEM_MD` env var (set to the system prompt text) or by creating a `GEMINI.md` file in CWD. We use the env var approach.
- `--sandbox` — enables macOS Seatbelt or container-based sandboxing `[REVIEW FIX]`
- `--approval-mode plan` — read-only mode for tool calls `[REVIEW FIX]`
- `--allowed-tools ""` — empty allowlist disables all tools `[REVIEW FIX]`
- `--resume SESSION_ID` — multi-turn (was broken in Issue #14180, fixed in nightly 0.20.0+; not used in v0)
- `@path` syntax for multimodal image input (not used in v0)
- **Issue #11184:** `response` field sometimes contains markdown-wrapped JSON (` ```json ... ``` `). JSON extraction must strip markdown fences from the response field itself, not just from the outer envelope.
- Exit codes: 0 success, 41 auth error, 42 input error, 52 config error, 130 cancellation

**Env vars:** `GEMINI_API_KEY` or `GOOGLE_API_KEY` or `GOOGLE_APPLICATION_CREDENTIALS`

**Parsing strategy:** Parse JSON envelope, extract `response` field. If `response` contains markdown fences, strip them before parsing inner JSON. If `error` field is non-null, treat as provider error.

#### Provider Env Var Summary

| Provider | Required Env Var | System Prompt Mechanism | Tool Restriction |
|----------|-----------------|------------------------|-----------------|
| Claude | `ANTHROPIC_API_KEY` | `--append-system-prompt` | `--tools ""` `[REVIEW FIX]` |
| Codex | `OPENAI_API_KEY` | `--config developer_instructions="..."` | `--sandbox read-only` |
| Gemini | `GEMINI_API_KEY` | `GEMINI_SYSTEM_MD` env var | `--sandbox` + `--approval-mode plan` + `--allowed-tools ""` `[REVIEW FIX]` |

### Prompt Engineering Best Practices (Round 2 Research)

Based on LLM-as-judge literature (MT-Bench, DebateLLM, M-MAD) and AWS multi-agent debate research:

**Chain-of-thought scoring:** Require models to explain their reasoning before outputting a numeric score. This reduces position bias (models favoring the first or last answer) and improves scoring consistency. The score prompt should require `rationale` before `score` in the JSON:
```json
{ "answer_id": "A", "rationale": "Brief reasoning...", "score": 8 }
```

**Rubric anchoring:** Define what each score means to reduce inter-model scoring variance:
- 9-10: Comprehensive, accurate, well-structured, no significant gaps
- 7-8: Mostly correct with minor issues or missing details
- 5-6: Partially correct, significant gaps or inaccuracies
- 3-4: Mostly incorrect or superficial
- 1-2: Fundamentally wrong or irrelevant

**Position bias mitigation:** Randomize the order in which answers are presented to each scoring model. The plan already randomizes labels (D10), but the presentation ORDER should also be shuffled per model per round.

**Temperature:** Use temperature ≤0.3 for scoring/ranking calls (not for propose/refine, which benefit from higher temperature). Lower temperature reduces scoring variance.

**Debate prompt structure (from AWS DebateLLM):**
1. Opening: present question + all answers with context
2. Thinking: `<thinking>` tags for hidden chain-of-thought reasoning
3. Argument/Score: structured output in `<argument>` or JSON tags
4. The judge pattern separates evaluation from debate — the judge never sees the question's ground truth, only the debaters' arguments

### Structured Output Schemas

#### Evaluate Schema (merged review+score) `[REVIEW FIX]`

```
You are evaluating another model's answer. Review it qualitatively AND score it on a 1-10 scale.
Think step by step about the answer's quality before scoring.
Respond with ONLY a JSON block.

```json
{
  "strengths": ["strength 1", "strength 2"],
  "weaknesses": ["weakness 1", "weakness 2"],
  "suggestions": ["suggestion 1", "suggestion 2"],
  "overall_assessment": "A brief paragraph summarizing your assessment.",
  "rationale": "Brief reasoning for the score, referencing specific strengths/weaknesses.",
  "score": 8
}
```
```

Note: `rationale` comes BEFORE `score` to enforce chain-of-thought scoring (per prompt engineering best practices). Answer labels use randomized anonymous labels (A/B/C), not model IDs, per D10. Presentation order is shuffled per evaluator per round to mitigate position bias.

#### v0.1: Ranking Schema (deferred) `[REVIEW FIX]`

Rankings are deferred to v0.1. When re-introduced, they will use Borda count for N ≤ 7 and Kemeny-Young for optimal aggregation.

### Closing Strategy Definitions

#### v0: Vote Threshold `[REVIEW FIX: score-only, rankings deferred]`

Converges when:
- The top-scoring answer (by mean score across all evaluating models, self-excluded) has a mean score >= `threshold` (default 8.0), AND
- The top-scoring answer has been the same model for 2 consecutive rounds (stability check).
- Either condition alone is insufficient.

#### v0.1: Synthesis (deferred)

A designated judge model synthesizes all refined answers into a final output. Triggered after a fixed number of rounds or when score delta between rounds < 0.5.

#### v0.1: Delphi (deferred)

Converges when standard deviation of all scores drops below `threshold` (default 1.0) AND mean score of top-ranked answer >= `min_score` (default 6.0).

### Research Insight: Convergence Mathematics (Round 2 Research)

**v0: Borda Count + Threshold (simple, correct for small N)**

The v0 Vote Threshold strategy already uses mean scores and rankings. The ranking aggregation should use **Borda count** — each ranker assigns N-1 points to their top choice, N-2 to second, etc. Sum across rankers. This is the simplest positionally-sensitive aggregation and is appropriate for N ≤ 7.

**v0: Kendall Tau Distance for Stability Detection**

The stability check ("top-ranked answer same for 2 rounds") can be made more precise. Instead of checking only the winner, compute the Kendall tau distance between consecutive rounds' aggregate rankings. If τ distance < threshold (e.g., 0.1 × max possible distance), the ranking is "stable" regardless of whether the winner changed by a small amount. The **Diaconis-Graham inequality** guarantees: K(σ) ≤ F(σ) ≤ 2K(σ) (Kendall tau ≤ Spearman footrule ≤ 2 × Kendall tau), so either metric can be used interchangeably with known bounds.

**v0.1: IQR-Based Delphi Convergence**

The Delphi literature defines consensus as IQR ≤ 2 on a 10-point scale (or IQR ≤ 1 on a 5-point scale). This is more robust than standard deviation because IQR is resistant to outliers (a single model scoring anomalously high/low). For the Delphi closing strategy:
- Compute IQR of all scores for the top-ranked answer
- If IQR ≤ 2.0 AND mean score ≥ 6.0, converge

**v1: Kemeny-Young Optimal Ranking**

For N < 8, the Kemeny-Young optimal ranking (minimizes total Kendall tau distance from all input rankings) is exactly computable in O(N × 2^N) via the Held-Karp algorithm. This replaces Borda count with a theoretically optimal aggregation. At N=7 (our hard cap), this is 7 × 128 = 896 operations per round — trivial.

**v1: Adaptive Stability Detection (Beta-Binomial)**

From the literature:
- **Adaptive stability detection** using Beta-Binomial mixture models to track consensus dynamics ([arxiv 2510.12697](https://arxiv.org/html/2510.12697v1))
- **Four-tier classification**: Strong, Conditional, Operational, Divergent consensus rather than binary converged/not ([Human-AI Hybrid Delphi](https://arxiv.org/html/2508.09349v1))
- **Model-judged convergence**: ask a judge model "has this converged?" rather than relying on arithmetic — the agent-native approach
- **KS test**: D_t < 0.05 for 2 consecutive rounds indicates score distribution stability (low power at small N, but usable as a secondary signal)

### Concurrency Architecture

```
                  JoinSet<Result<PhaseOutcome, ProviderError>>
                  ╱    │    ╲     ...    ╲
               Task1  Task2  Task3  ...  TaskN(N-1)
                 │      │      │           │
          [semaphore] [semaphore] ...      │    ← acquire semaphore (long-held)
                 │      │      │           │
               Child  Child  Child       Child  ← kill_on_drop(true)
```
`[REVIEW FIX]` Simplified: rate limiter removed from v0 (CLI backends don't expose rate limit headers). Global semaphore only.

- **Per-call timeout**: default 120s (user-configurable), via `tokio::time::timeout`.
- **Global semaphore**: default `min(N*(N-1), 30)` (user-configurable). Sized to allow full evaluate-phase parallelism at typical N values.
- **Retry inside permit**: on failure, backoff and retry while holding the semaphore permit. Release only on final success or failure. This avoids re-acquisition overhead and prevents a failing provider from starving others.
- **Process cleanup**: `kill_on_drop(true)` on all child processes. `[REVIEW FIX]` `setsid` process group management deferred to v0.1 — it requires unsafe code, conflicting with `unsafe_code = "deny"`. `kill_on_drop(true)` is sufficient for v0; add SIGTERM grace periods via `nix` crate in v0.1.

### v0.1: Per-Provider Semaphores + Rate Limiting `[REVIEW FIX]`

The global semaphore treats all providers equally. In v0.1, add per-provider `Semaphore` instances sized to each provider's known rate limits, plus `governor`-based token bucket rate limiting when native API backends expose `Retry-After` headers. The starvation bug (semaphore acquired before rate limiter) applies only when rate limiting is present — not a concern for v0.

### Inter-Phase Data Types

Each phase returns an explicit typed result. The orchestrator checks these, it does not interpret raw data:

```rust
// propose phase output
struct ProposalSet {
    proposals: HashMap<ModelId, String>,
    dropped: Vec<(ModelId, ProviderError)>,
}

// evaluate phase output (review + score merged) [REVIEW FIX]
struct EvaluationSet {
    evaluations: HashMap<(ModelId, ModelId), Evaluation>,  // (evaluator, evaluatee) -> evaluation
    dropped: Vec<(ModelId, ModelId, ProviderError)>,
}

// single evaluation: review + score in one call [REVIEW FIX]
struct Evaluation {
    review: Review,       // qualitative feedback (feeds REFINE)
    score: Score,         // numeric 1-10 (feeds CLOSE CHECK)
    rationale: String,    // chain-of-thought scoring rationale
}

// refine phase output
struct RefinementSet {
    refinements: HashMap<ModelId, String>,
    unrefined: Vec<ModelId>,  // kept previous answer
}
```

Every set carries its own error/dropped information, making it self-describing.

### RoundOutcome (Stepping API Contract — Round 2)

**CRITICAL (from agent-native review round 2):** The stepping API returns `RoundOutcome` but this type was never defined. Without it, `Session::next_round()` has no contract. Define explicitly:

```rust
/// The complete output of one round, returned by Session::next_round()
pub struct RoundOutcome {
    pub round: u32,
    pub proposals: ProposalSet,
    pub evaluations: EvaluationSet,  // [REVIEW FIX] merged review+score
    pub refinements: RefinementSet,
    pub closing_decision: ClosingDecision,
    pub elapsed: Duration,
    pub call_count: u32,
}
```

This surfaces all inter-phase data to library consumers, enabling agents to inspect what happened in each round.

### ConvergenceStatus Enum (Round 2)

Define the complete enum (referenced but never specified):

```rust
pub enum ConvergenceStatus {
    Converged,           // Closing strategy reported convergence
    MaxRoundsExceeded,   // Hit max_rounds without convergence; best answer returned
    SingleModel,         // N=1 short-circuit; no consensus loop ran
    InsufficientModels,  // Fell below N=2 mid-run; best available returned
    Cancelled,           // Run was cancelled via Session::cancel()
}
```

The JSON `status` field maps to this enum as snake_case strings: `"converged"`, `"max_rounds_exceeded"`, `"single_model"`, `"insufficient_models"`, `"cancelled"`.

### Session API Extensions (Round 2 Agent-Native Review)

```rust
impl Session {
    /// Advance one round with default behavior
    fn next_round(&mut self) -> Result<RoundOutcome, ConvergeError>;

    /// Advance one round with overrides for agent intervention
    fn next_round_with(&mut self, overrides: RoundOverrides) -> Result<RoundOutcome, ConvergeError>;

    /// Clean cancellation: kill in-flight subprocesses, return best-so-far
    fn cancel(self) -> ConsensusOutcome;

    /// Finalize after convergence or max rounds
    fn finalize(self) -> ConsensusOutcome;
}

/// Optional overrides for inter-round agent intervention
pub struct RoundOverrides {
    pub additional_context: Option<String>,  // Injected into all prompts this round
    pub drop_models: Vec<ModelId>,           // Remove models for remaining rounds
}

impl Engine {
    /// Estimate cost without executing
    fn estimate(config: &EngineConfig) -> CostEstimate;
}
```

`RoundOverrides` is intentionally minimal for v0 — `additional_context` and `drop_models` cover the most important intervention patterns. Strategy parameter overrides and model addition are v0.1.

### Round Context Injection

Each model receives context about the run's state in every prompt:

```
<run_context>
Round: 3 of 5
Models: 3 participating (1 dropped in round 2)
Strategy: vote-threshold (threshold: 8.0, stability: 2 rounds)
Previous round top scores: Answer A = 7.2, Answer B = 6.8, Answer C = 7.5
Top-ranked: Answer C (1 round stable, need 2)
Status: Not yet converged. Top score 0.5 below threshold.
</run_context>
```

This transforms models from blind executors into informed participants that can target specific gaps.

### Progress Reporting (v0: tracing)

v0 uses `tracing` for progress instead of a custom broadcast event system. The engine emits `tracing::info!` and `tracing::debug!` events at phase boundaries. The CLI subscribes with a `tracing-subscriber` layer that formats progress to stderr. This gives full observability with zero custom event types.

The CLI additionally supports `--progress-format json` which emits NDJSON on stderr for machine consumers. **Complete event vocabulary (Round 2):**

```jsonl
{"event":"run_started","run_id":"a7f3b2","models":["claude","codex","gemini"],"max_rounds":5,"strategy":"vote-threshold"}
{"event":"round_started","run_id":"a7f3b2","round":1,"models":["claude","codex","gemini"]}
{"event":"phase_started","run_id":"a7f3b2","round":1,"phase":"propose"}
{"event":"model_responded","run_id":"a7f3b2","round":1,"phase":"propose","model":"claude","duration_ms":3200,"tokens":null}
{"event":"model_failed","run_id":"a7f3b2","round":1,"phase":"propose","model":"codex","error":"timeout after 120s","retryable":true}
{"event":"phase_completed","run_id":"a7f3b2","round":1,"phase":"propose","duration_ms":5100,"succeeded":2,"failed":1}
{"event":"round_completed","run_id":"a7f3b2","round":1,"duration_ms":18500,"closing_decision":"continue","top_model":"claude","top_score":7.2,"models_remaining":2}
{"event":"run_completed","run_id":"a7f3b2","status":"converged","final_round":3,"total_calls":42,"elapsed_ms":95000}
```

The `run_id` is a random 6-character hex nonce generated at run start. This enables an agent composing multiple concurrent runs to attribute events. `tokens` and `cost_usd` fields are nullable — reserved for when providers report them (v0.1+).

v1 adds a typed `ConsensusEvent` enum via `tokio::sync::broadcast` for library consumers needing structured event subscriptions. For v0, library consumers use the stepping API for inter-round visibility.

### CLI JSON Output Schema

When `--output-format json`, stdout emits:

```json
{
  "status": "converged",
  "winner": {
    "model_id": "claude",
    "answer": "The full winning answer text..."
  },
  "final_round": 3,
  "strategy": "vote-threshold",
  "all_answers": [
    { "model_id": "claude", "answer": "...", "mean_score": 8.5, "mean_rank": 1.2 },
    { "model_id": "codex", "answer": "...", "mean_score": 7.8, "mean_rank": 1.8 },
    { "model_id": "gemini", "answer": "...", "mean_score": 7.2, "mean_rank": 2.0 }
  ],
  "metadata": {
    "total_rounds": 3,
    "total_calls": 54,
    "elapsed_ms": 95000,
    "models_dropped": []
  }
}
```

On error:
```json
{
  "status": "error",
  "error": {
    "code": "provider_timeout",
    "message": "Claude CLI timed out after 120s in round 2, phase propose",
    "provider": "claude",
    "round": 2,
    "phase": "propose",
    "retryable": true
  }
}
```

### Context Window Management (v0: simple, v1: summarization)

v0 approach — **sliding window with full latest round:**
- Always include: original prompt + system instructions + round context.
- Always include: the latest round's full data (proposals, reviews, refinements).
- For previous rounds: include only the refined answers (not reviews/votes). Reviews are the largest per-round artifact — dropping them from history saves the most tokens.

v1 (future): summarize previous rounds via a separate LLM call.

### Cost Projections `[REVIEW FIX: updated for merged EVALUATE phase + dropped rankings]`

| N | Calls/round | Calls/5 rounds | Est. tokens | Est. cost |
|---|-------------|----------------|-------------|-----------|
| 2 | 6 | 30 | ~120K | $0.50-2 |
| 3 | 12 | 60 | ~250K | $1-4 |
| 5 | 30 | 150 | ~700K | $3-10 |
| 7 | 56 | 280 | ~1.3M | $7-22 |

Formula: N (propose) + N(N-1) (evaluate) + N (refine) = N² + N calls/round.

N=7 is the hard cap. N>5 displays a cost warning before execution.

## Implementation Phases

### Phase 1: Project Scaffolding

**Tasks:**
- [ ] Initialize git repository
- [ ] Create Cargo workspace with virtual workspace root, `resolver = "3"` (Rust 2024), `[workspace.package]` for shared metadata, `[workspace.dependencies]` for centralized version management
- [ ] Create member crates: `crates/converge_core`, `crates/converge_providers` (with feature flags per provider), `crates/converge_cli`
- [ ] Set up `MODULE.bazel` with `rules_rust` and `crate_universe` (reads from `Cargo.toml`/`Cargo.lock`), `BUILD.bazel` per crate
- [ ] Add `.gitignore` (include `.env`, `*.key`, `*.pem`, `target/`, `.bazel*`)
- [ ] Update `.env.example` with all three providers' env vars
- [ ] Add workspace dependencies: `tokio` (rt-multi-thread, macros, process, sync, time), `serde` + `serde_json`, `thiserror` 2.0, `async-trait`, `clap` 4 (derive), `tracing`, `tracing-subscriber` (env-filter)
- [ ] Add workspace dev-dependencies: `tokio` (test-util), `insta` (json, redactions), `rstest`, `proptest`, `serial_test`, `assert_matches`
- [ ] Add workspace lints: `unsafe_code = "deny"` `[REVIEW FIX]`, `clippy::all = "warn"`, `clippy::pedantic = "warn"`
- [ ] Verify `cargo build --workspace` and `cargo test --workspace` pass (trivially)
- [ ] Create stub `lib.rs` / `main.rs` files with module structure

**Workspace Cargo.toml structure (Round 2):**
```toml
[workspace]
resolver = "3"
members = ["crates/converge_core", "crates/converge_providers", "crates/converge_cli"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[workspace.dependencies]
converge_core = { path = "crates/converge_core", version = "0.1.0" }
converge_providers = { path = "crates/converge_providers", version = "0.1.0" }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "process", "sync", "time"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
async-trait = "0.1"
clap = { version = "4", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1"

[workspace.lints.rust]
unsafe_code = "deny"  # [REVIEW FIX] changed from "forbid" to "deny" — allows targeted #[allow(unsafe_code)] for setsid in v0.1

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"

[profile.release]
lto = "thin"
strip = "symbols"
```

**converge_providers feature flags:**
```toml
[features]
default = ["claude", "codex", "gemini"]
claude = []
codex = []
gemini = []
```

**Success criteria:** `cargo build --workspace` succeeds. `cargo test --workspace` runs (0 tests, 0 failures). `bazel build //...` succeeds.

### Phase 2: Core Domain Model

**Tasks:**
- [ ] `types.rs` — all domain types in one file:
  - `ModelId` newtype (`String`, derive `Eq`, `Hash`, `Clone`, `Debug`, `Serialize`, `Deserialize`)
  - `Message`, `Role` enum (`System`, `User`, `Assistant`), `Content` (text-only for v0)
  - `Score` as constrained newtype (fallible constructor, enforces 1-10 range)
  - `Ranking`, `RankedAnswer`, `Review`
  - `ConsensusOutcome`, `ConvergenceStatus` enum
  - `EngineConfig` (models, strategy params, max_rounds, timeout, concurrency)
  - Phase enum, `RoundData` (read-only view for closing strategy)
  - Inter-phase types: `ProposalSet`, `EvaluationSet`, `Evaluation`, `RefinementSet` `[REVIEW FIX]`
- [ ] `error.rs` — `ConvergeError` enum + `ProviderError` using `thiserror` 2.0:
  ```rust
  #[derive(Error, Debug)]
  pub enum ConvergeError {
      #[error("phase {phase:?} failed for model {model}: {source}")]
      PhaseFailure { phase: Phase, model: ModelId, source: ProviderError },

      #[error("insufficient models in round {round}: {remaining} remaining, {minimum} required")]
      InsufficientModels { round: u32, remaining: usize, minimum: usize },

      #[error("invalid config: {field} = {value} ({constraint})")]
      ConfigInvalid { field: &'static str, value: String, constraint: String },

      #[error("consensus run cancelled")]
      Cancelled,
  }
  ```
  - **Do NOT use `#[from]`** on `PhaseFailure` — the `phase` and `model` context fields cannot be populated by a blanket `From` impl. Use explicit `map_err` at call sites.
  - **Do NOT derive `Serialize`** on `ConvergeError` — error chains are not cleanly serializable. Create a separate `ErrorResponse` struct at the CLI boundary (see Phase 6).
  - `[REVIEW FIX]` `ProviderError` is defined in `converge_core` (alongside `ConvergeError`) because `ConvergeError::PhaseFailure` references it. Providers implement the trait using `converge_core::ProviderError` variants.

**Tests (TDD — Round 2 enhanced):**
- [ ] `Score::new(0)` fails, `Score::new(1)` succeeds, `Score::new(10)` succeeds, `Score::new(11)` fails
- [ ] `Score` property test: `proptest!(|value in 1u8..=10| { Score::new(value).unwrap().value() == value })`
- [ ] `Score` property test: `proptest!(|value in prop::num::u8::ANY.prop_filter(|v| !(1..=10).contains(v))| { Score::new(value).is_err() })`
- [ ] `ModelId` usable as `HashMap` key
- [ ] `EngineConfig` validation: N in 1-7, max_rounds in 1-20, threshold in 1.0-10.0
- [ ] `EngineConfig` validation: `ConfigInvalid` carries structured field/value/constraint (not freeform string)
- [ ] `EngineConfig` property test: `proptest!(|n in 1usize..=7| { EngineConfig::new(models(n), ...).is_ok() })`
- [ ] Inter-phase types: `ProposalSet` carries both successes and failures
- [ ] `ConvergenceStatus` serializes to expected snake_case strings

**Files:**
- `crates/converge_core/src/types.rs`
- `crates/converge_core/src/error.rs`

### Phase 3: Traits + Mocks

**Tasks:**
- [ ] `ModelProvider` trait in `lib.rs`:
  ```rust
  #[async_trait]  // needed for Box<dyn ModelProvider> object safety
  pub trait ModelProvider: Send + Sync + std::fmt::Debug {
      async fn send_message(
          &self,
          messages: &[Message],
      ) -> Result<String, ProviderError>;

      fn model_id(&self) -> &ModelId;
  }
  ```
  Note: `#[async_trait]` is required for dyn dispatch (`Box<dyn ModelProvider>`). Native async-in-trait (stable since Rust 1.75) does not support trait objects. Remove `async-trait` dependency when `dyn async trait` stabilizes.
- [ ] `ClosingStrategy` trait in `strategy.rs`:
  ```rust
  #[async_trait]  // [REVIEW FIX] async from v0 to support Synthesis strategy (v0.1) without breaking change
  pub trait ClosingStrategy: Send + Sync {
      async fn check(&self, round_data: &RoundData) -> ClosingDecision;
      fn name(&self) -> &str;
  }

  pub enum ClosingDecision {
      Converged { winner: ModelId, explanation: String },
      Continue,
  }
  ```
  Note: `Synthesize` variant removed from v0. The judge model is a strategy configuration concern, not a per-decision concern. Synthesis strategy (v0.1) will return `Converged` after triggering its own LLM call internally.
- [ ] `VoteThreshold` implementation
- [ ] Mock providers (in `converge_core::testing` module, `#[cfg(any(test, feature = "testing"))]`):
  - `EchoProvider` — returns fixed text (configurable per call via `Arc<Mutex<VecDeque<String>>>`)
  - `FailingProvider` — fails after N calls (uses `AtomicUsize` counter for thread safety)
  - `DelayProvider` — wraps another provider with artificial delay (works with `tokio::time::pause`)
  - `FailAfterNProvider` — succeeds N times, then fails (for testing mid-run model death)
- [ ] Mock strategies: `AlwaysConverge`, `NeverConverge`, `ConvergeAfterN(u32)`

**Tests (TDD — Round 2 enhanced with `rstest` parameterized tests):**
- [ ] Trait object safety: `Box<dyn ModelProvider>` compiles and dispatches
- [ ] VoteThreshold parameterized with `rstest`:
  ```rust
  #[rstest]
  #[case(7.9, 2, false)]  // score below threshold
  #[case(8.0, 1, false)]  // stable only 1 round (need 2)
  #[case(8.0, 2, true)]   // at threshold, stable → converge
  #[case(9.5, 3, true)]   // above threshold, stable → converge
  #[case(10.0, 2, true)]  // max score → converge
  fn vote_threshold_convergence(#[case] mean_score: f64, #[case] stable_rounds: u32, #[case] should_converge: bool)
  ```
- [ ] Edge: all scores identical → converges
- [ ] Edge: all scores maximum (10) → converges immediately

**Files:**
- `crates/converge_core/src/lib.rs`
- `crates/converge_core/src/strategy.rs`

### Phase 4: Engine Core (Orchestrator)

**Tasks:**
- [ ] `engine.rs` — `Engine` struct with constructor injection:
  ```rust
  pub struct Engine {
      providers: Vec<Box<dyn ModelProvider>>,
      strategy: Box<dyn ClosingStrategy>,
      config: EngineConfig,
  }
  ```
  Two APIs:
  - `Engine::run(prompt) -> Result<ConsensusOutcome, ConvergeError>` (run to completion)
  - `Engine::start(prompt) -> Result<Session, ConvergeError>` (stepping API)
  - `Session::next_round() -> Result<RoundOutcome, ConvergeError>`
  - `Session::finalize() -> ConsensusOutcome`
- [ ] `phases/propose.rs` — spawn N parallel calls, return `ProposalSet`
- [ ] `phases/evaluate.rs` — `[REVIEW FIX]` spawn N*(N-1) parallel evaluate calls (merged review+score), each producing `Evaluation` (review + score + rationale) with sanitized XML-delimited answer injection + round context, return `EvaluationSet`
- [ ] `phases/refine.rs` — inject reviews (from `EvaluationSet`) + round context, spawn N parallel refinement calls, return `RefinementSet`
- [ ] `phases/close.rs` — dispatch to closing strategy
- [ ] N=1 short-circuit path
- [ ] Graceful degradation per D4
- [ ] `prompts.rs` — all prompt templates extracted, tested independently (propose, evaluate, refine) `[REVIEW FIX]`

**Tests (TDD — Round 2 enhanced):**
- [ ] Full round loop with mock providers: propose → evaluate → refine → close → converged (use `#[tokio::test(start_paused = true)]` for deterministic time) `[REVIEW FIX]`
- [ ] Multi-round loop: converge on round 2 with mock providers
- [ ] Max rounds exceeded: returns best answer with `ConvergenceStatus::MaxRoundsExceeded`
- [ ] N=1 short-circuit: single model, returns `ConvergenceStatus::SingleModel`
- [ ] Partial failure: one model fails PROPOSE, round continues with N-1
- [ ] Partial failure: one model fails EVALUATE, round continues without that evaluation `[REVIEW FIX]`
- [ ] All models fail: engine returns `ConvergeError::InsufficientModels`
- [ ] Stepping API: `start()` → `next_round()` × 2 → `finalize()` produces same result as `run()`
- [ ] Stepping API: `next_round()` returns `RoundOutcome` with all four phase outputs populated
- [ ] Stepping API: `next_round_with(RoundOverrides { additional_context: Some("focus on security") })` injects context
- [ ] Stepping API: `Session::cancel()` returns best-so-far with `ConvergenceStatus::Cancelled`
- [ ] Concurrency: 3 providers with 5s `DelayProvider` complete in ~5s not 15s (`start_paused` elapsed assertion)
- [ ] Prompt templates: `insta::assert_snapshot!` for evaluate and refine templates (regression detection) `[REVIEW FIX]`
- [ ] Self-evaluation exclusion: evaluate phase never sends a model its own answer `[REVIEW FIX]`
- [ ] JoinSet error handling: panicking provider propagates panic, failing provider goes to `dropped`
- [ ] `Engine::estimate()` returns correct call counts for N=2,3,5,7 (formula: N²+N per round) `[REVIEW FIX]`

**Files:**
- `crates/converge_core/src/engine.rs`
- `crates/converge_core/src/phases/propose.rs`
- `crates/converge_core/src/phases/evaluate.rs` `[REVIEW FIX]`
- `crates/converge_core/src/phases/refine.rs`
- `crates/converge_core/src/phases/close.rs`
- `crates/converge_core/src/prompts.rs`

### Phase 5: Provider Backends (CLI-based)

**Tasks:**
- [ ] `process.rs` — async subprocess utilities:
  - `resolve_binary(name: &str) -> Result<PathBuf, ProviderError>`: resolve CLI binary to absolute path via `which` at startup (BEFORE `env_clear` — see D7 fix)
  - Spawn with `Command::new(absolute_path).arg(...)` (never shell interpolation, per D11)
  - `env_clear()` + selective `env()` per provider (per D7) + `env("PATH", "/usr/bin:/bin")` (minimal PATH for child process needs)
  - `--` sentinel before user-supplied positional arguments
  - `kill_on_drop(true)` + `setsid` for process group management
  - `tokio::time::timeout` per call
  - JSON extraction: strip markdown fences (` ```json ... ``` `), `serde_json::from_str`, size limit (100KB), pre-parse depth scan
  - Retry: up to 2 retries with exponential backoff (1s, 4s), inside the calling scope
- [ ] `claude.rs` — exact invocation per "CLI Backend Implementation Details" section:
  - `claude -p --output-format json --tools "" --max-turns 1 --append-system-prompt "SYSTEM" -- "PROMPT"` `[REVIEW FIX]`
  - Parse JSON envelope: extract `result` field
  - Env: `ANTHROPIC_API_KEY`
- [ ] `codex.rs` — exact invocation per "CLI Backend Implementation Details" section:
  - `codex exec --json --sandbox read-only -- "PROMPT"` (system prompt via `--config developer_instructions="..."`)
  - Parse JSONL event stream: find last `turn.completed` event, extract `text` field
  - Env: `OPENAI_API_KEY`
- [ ] `gemini.rs` — exact invocation per "CLI Backend Implementation Details" section:
  - `gemini --output-format json --model gemini-2.5-pro --sandbox --approval-mode plan --allowed-tools "" -- "PROMPT"` (system prompt via `GEMINI_SYSTEM_MD` env var) `[REVIEW FIX]`
  - Parse JSON envelope: extract `response` field, strip markdown fences if present (Issue #11184)
  - Env: `GEMINI_API_KEY`
- [ ] Each adapter implements `ModelProvider` trait
- [ ] Credential validation: check required env vars at construction time, return `ProviderError::MissingCredential { var_name }` if absent
- [ ] Binary validation: check CLI binary exists at construction time via `resolve_binary()`, return `ProviderError::BinaryNotFound { binary_name }` if absent

**Tests (TDD — Round 2 enhanced):**
- [ ] `process.rs` — timeout triggers kill, output captured correctly (use `echo`/`sleep` as test CLIs, `start_paused` for deterministic timing)
- [ ] `process.rs` — JSON extraction from markdown-fenced responses, handles malformed JSON, handles missing fields, rejects oversized responses (>100KB)
- [ ] `process.rs` — JSON depth scanner rejects deeply nested payloads (>10 levels)
- [ ] `process.rs` — Gemini response field with markdown-wrapped JSON (Issue #11184 regression test)
- [ ] `process.rs` — Codex JSONL stream parsing: extracts `turn.completed` event correctly, handles missing event gracefully
- [ ] Each adapter: correct CLI invocation constructed (unit test command building — verify exact flags match spec)
- [ ] Each adapter: `env_clear` applied, only required vars injected (use `env` command as test binary, check output contains ONLY expected vars — `#[serial]` for env var tests)
- [ ] Each adapter: `--` sentinel present before user content
- [ ] Each adapter: binary resolved to absolute path at construction time
- [ ] Credential validation: missing env var returns `ProviderError::MissingCredential`
- [ ] Binary validation: missing CLI returns `ProviderError::BinaryNotFound`

**Integration tests (require actual CLIs):**
- [ ] `claude` adapter: simple prompt → structured JSON response
- [ ] `codex` adapter: same
- [ ] `gemini` adapter: same
- [ ] Mark with `#[ignore]` for CI

**Files:**
- `crates/converge_providers/src/process.rs`
- `crates/converge_providers/src/claude.rs`
- `crates/converge_providers/src/codex.rs`
- `crates/converge_providers/src/gemini.rs`
- `tests/integration/`

### Phase 6: CLI

**Tasks:**
- [ ] `clap`-based argument parsing
- [ ] Progress display via `tracing-subscriber` to stderr
- [ ] `--progress-format text|json` (NDJSON on stderr when json)
- [ ] Output formatting: `--output-format text|json` (final result to stdout, schema defined above)
- [ ] Structured errors in JSON mode via `ErrorResponse` struct (separate from `ConvergeError` — not serialized directly):
  ```rust
  #[derive(Serialize)]
  struct ErrorResponse {
      code: &'static str,       // machine-readable: "provider_timeout", "insufficient_models", etc.
      message: String,          // human-readable from Display
      provider: Option<String>,
      round: Option<u32>,
      phase: Option<String>,
      retryable: bool,
  }
  ```
- [ ] Exit codes: 0 = converged, 1 = error, 2 = max rounds exceeded, 3 = partial failure (models dropped), 4 = config/credential error
- [ ] CLI uses `anyhow` for its own setup errors (env loading, arg validation). Matches on `ConvergeError` variants for exit codes; delegates to `ErrorResponse` for JSON formatting.
- [ ] Composition root: parse args → resolve binaries → validate credentials → instantiate providers → instantiate strategy → wire into Engine → run

**CLI interface:**

```
converge [OPTIONS] <PROMPT>

Arguments:
  <PROMPT>                The prompt to reach consensus on (or - for stdin, max 1MB)

Options:
  -m, --models <MODELS>         Comma-separated model list [e.g., claude,codex,gemini]
  -t, --threshold <FLOAT>       Score threshold for convergence [default: 8.0] (range: 1.0-10.0)
  -r, --max-rounds <N>          Maximum rounds [default: 5] (range: 1-20)
      --timeout <SECS>          Per-call timeout [default: 120] (range: 1-600)
      --max-concurrent <N>      Max concurrent subprocess calls [default: auto] (range: 1-50)
  -o, --output-format <FMT>     Output format [text|json] [default: text]
      --progress-format <FMT>   Progress format [text|json] [default: text]
  -v, --verbose                 Show per-round progress
      --debug                   Show raw CLI invocations and responses
      --dry-run                 Show estimated call count and cost, then exit
  -h, --help                    Print help
  -V, --version                 Print version
```

**Example invocations:**

```bash
# Basic 3-model consensus
converge -m claude,codex,gemini "What is the best approach to error handling in Rust?"

# Verbose with JSON output for piping
converge -m claude,codex,gemini -v -o json "Design a database schema for a blog"

# Cost estimate before running
converge -m claude,codex,gemini --dry-run "What are the security implications of X?"

# From stdin
cat question.md | converge -m claude,codex -o json -
```

**Tests:**
- [ ] Argument parsing: all flag combinations
- [ ] Invalid args: helpful errors (out of range, missing models)
- [ ] `-` reads from stdin (capped at 1MB)
- [ ] `--dry-run` outputs call estimate without executing

**Files:**
- `crates/converge_cli/src/main.rs`

### Phase 7: Hardening (v0.1+)

Deferred from v0 core. Pick up after v0 ships:
- [ ] `tracing` spans for every phase/call (beyond the basic info! logging in v0)
- [ ] Token count tracking from provider responses
- [ ] Structured JSON log output
- [ ] `--transcript path` for full round history persistence
- [ ] Synthesis and Delphi closing strategies
- [ ] Image support (`--image <PATH>`)
- [ ] TOML config file (`--config path.toml`)

## Alternative Approaches Considered

**Native API clients instead of CLI backends:** More control, lower latency. Rejected for v0 because it triples the implementation effort for the backend layer without improving the core engine. CLI backends validate the architecture; native backends can be swapped in as a v1 optimization.

**Using Claude's `--fork-session` natively instead of library-managed state:** Tempting because it's efficient, but it creates an asymmetry between providers (only Claude supports it). Library-managed state keeps the engine provider-agnostic and fully testable with mocks.

**Full hexagonal architecture from day 1:** The infinidash sibling project uses `domain/ports/driven/driving` layering. For a v0 with one orchestrator, one trait, and three adapters, this adds ~20 files of ceremony. The flat structure can be refactored into layers when the project grows. The 3-crate boundary (core/providers/cli) enforces the important architectural constraint at the compilation level.

**broadcast channel for progress events:** A `tokio::sync::broadcast` with typed `ConsensusEvent` enum provides rich library-consumer events but adds API surface. v0 uses `tracing` (zero custom types, works immediately); the broadcast channel is a v1 addition for consumers needing structured event subscriptions.

**Three closing strategies in v0:** Synthesis requires a judge model concept and an async code path in what should be a sync trait. Delphi is a ~50 LOC variant of Vote Threshold. Shipping with Vote Threshold only proves the core loop; the others follow in the first week after v0.

## Acceptance Criteria

### Functional Requirements

- [ ] Library: `Engine::run()` executes the full consensus loop with N models
- [ ] Library: `Engine::start()` + `Session::next_round()` stepping API works
- [ ] Library: Vote Threshold closing strategy converges correctly
- [ ] Library: Configurable max_rounds, timeout, concurrency
- [ ] Library: Graceful degradation on partial model failure (minimum N=2)
- [ ] Library: N=1 short-circuit
- [ ] CLI: `converge` command works with all flags documented above
- [ ] CLI: JSON output conforms to defined schema
- [ ] CLI: NDJSON progress on stderr with `--progress-format json`
- [ ] CLI: `--dry-run` shows cost estimate
- [ ] Providers: Claude, Codex, Gemini backends work via CLI
- [ ] Providers: Structured output parsing with retry-on-failure
- [ ] Security: Tool use disabled in all subprocess invocations (D9)
- [ ] Security: Credentials scoped per provider (D7)
- [ ] Security: Self-scoring excluded (D10)
- [ ] Security: Prompt injection sanitization (D8)
- [ ] Security: No shell interpolation in subprocess calls (D11)

### Non-Functional Requirements

- [ ] All domain types have unit tests
- [ ] Engine orchestration tested with mock providers (no real CLI calls)
- [ ] Integration tests for each provider (marked `#[ignore]`)
- [ ] `cargo clippy` clean, `cargo fmt` applied
- [ ] `cargo test --workspace` passes

### Quality Gates

- [ ] TDD: tests written before implementation for each module
- [ ] Each phase builds and passes tests before proceeding to the next
- [ ] Integration test with real providers passes before v0 is declared complete

## Dependencies & Prerequisites

**External:**
- `claude` CLI installed and authenticated (`ANTHROPIC_API_KEY`)
- `codex` CLI installed and authenticated (`OPENAI_API_KEY`)
- `gemini` CLI installed and authenticated (`GEMINI_API_KEY`)
- Rust 2024 edition (1.85.0+ with resolver 3)

**Crate dependencies (v0):**
- `tokio` (rt-multi-thread, macros, process, sync, time)
- `serde` + `serde_json` (serialization)
- `thiserror` 2.0 (error types — uses `#[error(fmt)]` and auto-source)
- `anyhow` 1.0 (CLI crate only — for setup/teardown errors)
- `async-trait` (required for `Box<dyn ModelProvider>` — remove when dyn async trait stabilizes)
- `clap` 4 (CLI parsing, derive feature)
- `tracing` + `tracing-subscriber` (observability + progress, env-filter feature)

**Dev dependencies (v0 — Round 2):**
- `tokio` (test-util feature — `start_paused`, `time::advance`)
- `insta` (json, redactions features — snapshot testing for prompts and JSON output)
- `rstest` (fixture injection and `#[case]` parameterized tests)
- `proptest` (property-based testing for `Score`, `EngineConfig` boundaries)
- `serial_test` (`#[serial]` for tests that modify env vars)
- `assert_matches` (stable `assert_matches!` macro for error variant checking)

**Removed from v0 (vs. original plan):**
- `governor` — CLI backends don't expose rate limit headers; add with native API backends
- `tokio-util` — `JoinSet` + `tokio::time::timeout` suffice; v0 cancellation uses `tokio::sync::watch`; add `CancellationToken` in v1
- `nix` — `kill_on_drop(true)` is sufficient for v0 subprocess cleanup; add for SIGTERM grace periods in v1
- `uuid` — rounds are `u32`, `run_id` is a random hex nonce; no persistent sessions in v0

## Risk Analysis & Mitigation

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Gemini structured output parsing fails frequently | High | Medium | Retry with correction prompt. Strip markdown fences from response field (Issue #11184). Fallback: exclude Gemini evaluations, keep its answers. |
| ~~Gemini tool-use cannot be restricted~~ | ~~High~~ | ~~Medium~~ | `[REVIEW FIX]` **RESOLVED.** Gemini CLI has `--sandbox`, `--approval-mode plan`, and `--allowed-tools ""`. All three providers now have tool restriction. |
| N^2 scaling hits rate limits | Medium | High | Configurable concurrency. Default N=3. Hard cap N=7. |
| Prompt injection (model manipulates other models) | Medium | High | D8 (sanitized nonce delimiters) + D9 (tool-use disabled, except Gemini) + D10 (self-scoring excluded). |
| Command injection via user prompt | Low | Critical | D11 (Command::arg only, -- sentinel, never shell). |
| Credential leakage to unintended subprocesses | Medium | High | D7 (env_clear + selective injection + absolute path resolution). |
| env_clear breaks CLI binary resolution | High | High | Resolve to absolute paths at startup via `which` before env_clear (D7 fix). |
| Context window overflow on multi-round runs | Medium | Medium | Sliding window truncation. Warn user. |
| CLI subprocess hangs indefinitely | Low | High | Per-call timeout (120s). kill_on_drop. |
| Cost spirals on long runs | Medium | Medium | max_rounds cap 20, N cap 7, --dry-run, cost projections in docs. |
| Provider CLI interface changes | Low | Medium | Adapter pattern isolates changes. Each adapter is a single file. |
| Codex JSONL format changes | Low | Medium | Parse only well-known event types, ignore unknown events gracefully. |

## Future Considerations

**v0.1 (immediate follow-up):**
- Synthesis and Delphi closing strategies (Delphi uses IQR ≤ 2.0 convergence)
- Ordinal rankings: Borda count aggregation, Kendall tau stability detection `[REVIEW FIX: deferred from v0]`
- Image input support (`@path` syntax for Gemini, base64 for Claude/Codex)
- TOML config file
- `--budget` flag for cost ceiling
- Token count tracking from provider responses (Claude: `cost_usd` in envelope, Codex: `usage` in `turn.completed`)
- Prompt template overrides in `RoundOverrides`
- `strategy_params` override in `RoundOverrides`
- `setsid` process group management for clean subprocess tree cleanup `[REVIEW FIX: deferred from v0]`
- Per-provider semaphores `[REVIEW FIX: deferred from v0]`

**v1:**
- Native API backends (HTTP clients) for lower latency
- Session state persistence for crash recovery
- Model weighting (trust some models' votes more)
- Typed `ConsensusEvent` broadcast channel for library consumers
- Per-provider rate limiting via `governor`
- Graceful SIGTERM via `nix` before SIGKILL
- Redundancy filtering (S²-MAD-inspired token savings)
- Adaptive stability detection (Beta-Binomial)
- Kemeny-Young optimal ranking aggregation (feasible for N ≤ 7, O(N × 2^N))
- Prompt-defined custom closing strategies
- Config file with model profiles
- Streaming output from providers
- `CancellationToken` from `tokio-util` replacing `watch` channel
- Composability: `prior_results` field in `EngineConfig` for multi-run orchestration
- Intra-round progress callback: `Session::on_progress(impl Fn(ProgressEvent))` for real-time agent monitoring

**v2:**
- Web UI for observing consensus in real time
- Structured inputs/outputs and tool calls
- Replay mechanism from persisted transcripts
- Run-over-run learning (which models perform best on which topics)

## References & Research

### Internal References

- Brainstorm: `docs/brainstorms/2026-02-10-consensus-loop-design-brainstorm.md`
- Sibling project patterns: `/Users/thomas/Projects/Banade-a-Bonnot/infinidash/` (hexagonal architecture, Cargo workspace, Bazel, Rust 2024)

### External References — Multi-Agent Debate

- [Improving Factuality and Reasoning via Multiagent Debate](https://composable-models.github.io/llm_debate/) — foundational work on LLM debate
- [Multi-LLM-Agents Debate: ICLR 2025](https://d2jud02ci9yv69.cloudfront.net/2025-04-28-mad-159/blog/mad/) — performance, efficiency, scaling challenges
- [Multi-Agent Debate for LLM Judges with Adaptive Stability Detection](https://arxiv.org/html/2510.12697v1) — Beta-Binomial convergence
- [Voting or Consensus? Decision-Making in Multi-Agent Systems](https://aclanthology.org/2025.findings-acl.606.pdf) — ACL 2025
- [Human-AI Hybrid Delphi Model](https://arxiv.org/html/2508.09349v1) — four-tier consensus classification
- [Real-Time AI Delphi](https://www.sciencedirect.com/science/article/pii/S0016328725001661) — AI-accelerated convergence

### External References — Implementation

- [Awesome Consensus](https://github.com/dgryski/awesome-consensus) — classical consensus algorithm survey
- [Claude CLI headless mode](https://code.claude.com/docs/en/headless) — `-p`, `--output-format json`, `--json-schema`, `--tools ""`, `--append-system-prompt` `[REVIEW FIX]`
- [Codex CLI non-interactive](https://developers.openai.com/codex/noninteractive/) — `exec --json` JSONL events, `--output-schema`, `--sandbox`
- [Codex CLI reference](https://developers.openai.com/codex/cli/reference/) — full flag documentation
- [Gemini CLI headless](https://geminicli.com/docs/cli/headless/) — `--output-format json`, JSON envelope schema, exit codes
- [Gemini CLI sandbox](https://geminicli.com/docs/cli/sandbox/) — `--sandbox`, macOS Seatbelt, container-based sandboxing `[REVIEW FIX]`
- [Gemini CLI configuration](https://geminicli.com/docs/get-started/configuration/) — `--approval-mode`, `--allowed-tools`, `tools.exclude` `[REVIEW FIX]`
- Gemini CLI Issue #13388 — no structured output schema enforcement (OPEN, PR #18032 pending)
- Gemini CLI Issue #11184 — response field sometimes contains markdown-wrapped JSON
- Gemini CLI Issue #14180 — `--resume` was broken, fixed in nightly 0.20.0+
- [nextest and tokio](https://sunshowers.io/posts/nextest-and-tokio/) — subprocess management patterns in Rust
- [Tokio process module docs](https://docs.rs/tokio/latest/tokio/process/index.html)
- [Tokio Semaphore API](https://docs.rs/tokio/latest/tokio/sync/struct.Semaphore.html)

### External References — Error Handling & Testing (Round 2)

- [Error Handling in Rust - A Deep Dive (Palmieri)](https://lpalmieri.com/posts/error-handling-rust/) — `thiserror` vs manual, `#[from]` caveats
- [thiserror 2.0 release](https://github.com/dtolnay/thiserror/releases/tag/2.0.0) — new features: `#[error(fmt)]`, `no_std`
- [Cancelling Async Rust (RustConf 2025)](https://sunshowers.io/posts/cancelling-async-rust/) — JoinSet cancel safety, `watch` channel pattern
- [Tokio Unit Testing Guide](https://tokio.rs/tokio/topics/testing) — `start_paused`, deterministic time
- [insta snapshot testing](https://insta.rs/) — `assert_snapshot!`, `assert_json_snapshot!`, redactions
- [rstest documentation](https://docs.rs/rstest/latest/rstest/) — fixtures, `#[case]` parameterized tests
- [proptest book](https://altsysrq.github.io/proptest-book/) — property-based testing for constrained types

### External References — Convergence Mathematics (Round 2)

- [Kemeny-Young method (Wikipedia)](https://en.wikipedia.org/wiki/Kemeny%E2%80%93Young_method) — O(N × 2^N) via Held-Karp, feasible for N < 8
- [Diaconis & Graham 1977](https://rss.onlinelibrary.wiley.com/doi/10.1111/j.2517-6161.1977.tb01624.x) — K(σ) ≤ F(σ) ≤ 2K(σ) inequality
- [KS test critical values](https://people.cs.pitt.edu/~lipschultz/cs1538/prob-table_KS.pdf) — exact values for small N, low power warning

### External References — Prompt Engineering (Round 2)

- [AWS LLM Debate technique](https://aws.amazon.com/blogs/machine-learning/improve-factual-consistency-with-llm-debates/) — debate prompt templates, judge pattern
- [AWS debate prompts.py](https://github.com/aws-samples/improve-factual-consistency-with-llm-debate-technique/blob/main/improve-factual-consistency-with-llm-debate-technique/prompts.py) — complete debate/judge templates
- [DebateLLM (InstaDeep)](https://github.com/instadeepai/DebateLLM) — multi-agent debate benchmarking
- [M-MAD](https://github.com/SU-JIAYUAN/M-MAD) — multidimensional multi-agent debate for MT evaluation

### External References — Rust Workspace (Round 2)

- [Cargo Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html) — `[workspace.dependencies]`, `[workspace.package]`
- [Cargo Resolver 3 (Rust 2024)](https://doc.rust-lang.org/edition-guide/rust-2024/cargo-resolver.html) — MSRV-aware resolution
- [crate_universe bzlmod](https://bazelbuild.github.io/rules_rust/crate_universe_bzlmod.html) — Bazel + Cargo integration
- [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html) — additive features, `dep:` prefix, unification
