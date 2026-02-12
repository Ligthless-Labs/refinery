---
title: "docs: Complete the README.md"
type: docs
date: 2026-02-12
enhanced: 2026-02-12 (via `/deepen-plan` — Ankane style research + skill conventions)
reviewed: 2026-02-12 (via `/workflows:review` — simplicity + architecture reviewers)
completed: 2026-02-12
brainstorm: docs/brainstorms/2026-02-12-readme-completion-brainstorm.md
---

# docs: Complete the README.md

Rewrite `README.md` in Ankane style (concise, imperative, code-heavy). Cover both CLI and library. Keep existing credentials section intact.

## Acceptance Criteria

- [x] One-line project description at top
- [x] Installation section (CLI via `cargo install`, library via `Cargo.toml`)
- [x] Quick Start section (3-line CLI example)
- [x] CLI Usage section (models, options table, output formats, dry-run, stdin)
- [x] Library Usage section (`Engine::run` example, `Session` stepping API, custom `ModelProvider` trait)
- [x] Credentials section preserved as-is (already correct and detailed)
- [x] Bedrock and Vertex AI kept as "Coming Soon"
- [x] How It Works section (4-phase loop explanation, cost-per-round table)
- [x] Contributing section (Ankane template: 4-bullet list + clone/cd/test)
- [x] No separate License section (MIT license file in repo root only)
- [x] No prose paragraphs longer than 2 sentences
- [x] Every sentence ≤ 15 words, imperative voice
- [x] Every code fence has a single purpose
- [x] Label-then-codeblock rhythm (imperative label, no period, then code)
- [x] Single CI badge only
- [x] Create MIT LICENSE file in repo root

## Context

### Section outline

```
# ConVerge Refinery
[One-liner]

## Installation
  ### CLI
  ### Library

## Quick Start

## CLI Usage
  ### Models
  ### Options
  ### Output Formats
  ### Dry Run
  ### Reading from Stdin

## Library Usage
  ### Basic (Engine::run)
  ### Round-by-Round (Session)
  ### Custom Providers

## Credentials
  [existing section — keep as-is]

## How It Works
  [PROPOSE → EVALUATE → REFINE → CLOSE]
  [cost table: N=2..7]

## Contributing

## License
```

### Key data for content

**CLI binary:** `converge`

**Model aliases:**
- `claude` (default: sonnet), `claude-opus`, `claude-sonnet`
- `codex`
- `gemini` (default: 2.5-pro), `gemini-2.5-pro`

**Options:**

| Flag | Default | Range | Description |
|------|---------|-------|-------------|
| `--models`, `-m` | required | — | Comma-separated model list |
| `--threshold`, `-t` | 8.0 | 1.0–10.0 | Score threshold for convergence |
| `--max-rounds`, `-r` | 5 | 1–20 | Maximum consensus rounds |
| `--timeout` | 120 | 1–600 | Per-call timeout (seconds) |
| `--max-concurrent` | 0 | 0–50 | Max concurrent calls (0 = unlimited) |
| `--output-format`, `-o` | text | text, json | Output format |
| `--verbose`, `-v` | false | — | Show per-round progress |
| `--debug` | false | — | Show raw CLI invocations |
| `--dry-run` | false | — | Estimate cost, then exit |

**Exit codes:** 0 (converged/single), 1 (error/cancel), 2 (max rounds), 3 (insufficient models), 4 (config invalid)

**Cost per round:** N² + N calls (N propose + N(N-1) evaluate + N refine)

| Models | Calls/round |
|--------|-------------|
| 2 | 6 |
| 3 | 12 |
| 5 | 30 |
| 7 | 56 |

**Public library API:**
- `Engine::new(providers, strategy, config)` → `Engine`
- `Engine::run(&self, prompt)` → `Result<ConsensusOutcome>`
- `Engine::start(&self, prompt)` → `Result<Session>`
- `Session::next_round(&mut self)` → `Result<RoundOutcome>`
- `Session::cancel(self)` → `ConsensusOutcome`
- `ModelProvider` trait: `send_message`, `model_id`
- `EngineConfig::new(models, max_rounds, threshold, stability_rounds, timeout, max_concurrent)`
- `VoteThreshold::new(threshold, stability_rounds)` — closing strategy

**Version:** 0.1.0, MIT license, Rust edition 2024

**Missing file:** No `LICENSE` file exists yet — create one (MIT).

## References

- Brainstorm: `docs/brainstorms/2026-02-12-readme-completion-brainstorm.md`
- Current README: `README.md`
- CLI args: `crates/converge_cli/src/main.rs:17-58`
- Public API: `crates/converge_core/src/lib.rs`
- Engine: `crates/converge_core/src/engine.rs:1-128`
- Types: `crates/converge_core/src/types.rs`
- Providers: `crates/converge_providers/src/lib.rs`
