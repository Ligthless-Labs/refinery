---
title: "Ankane-style README conventions for Rust CLI and library projects"
category: documentation
tags: [readme, ankane-style, documentation, rust, cli, library]
module: documentation
symptom: "README is verbose, prose-heavy, or uses inconsistent formatting"
root_cause: "No established style guide for project documentation"
date: 2026-02-12
---

# Ankane-style README conventions for Rust CLI and library projects

## Context

Andrew Kane's open-source projects (pgvector, neighbor, tokenizers, etc.) share a
distinctive documentation style: minimal prose, imperative voice, single-purpose
code blocks, and a predictable rhythm. This document captures the conventions
distilled while rewriting the ConVerge Refinery README and the lessons learned
during review.

## 1. Voice: imperative, short, no passives

Every label and transition sentence uses imperative mood. Sentences stay at or
under 15 words. Never use passive voice ("is installed", "can be configured").

**Before (passive, wordy):**

```markdown
The application can be installed using cargo's install subcommand.
It is recommended that you have Rust 1.85 or later installed.
```

**After (imperative, short):**

````markdown
Install the CLI

```sh
cargo install refinery_cli
```

Requires [Rust](https://www.rust-lang.org/tools/install) 1.85+.
````

The label ("Install the CLI") has no period. The requirement note is a single
sentence fragment.

## 2. Rhythm: label-then-codeblock, alternating

The fundamental unit of Ankane-style docs is:

````
Imperative label (no period)

```lang
code
```
````

Labels and code blocks alternate. Explanatory prose is rare; when needed it sits
between the label and the code block as a single short sentence. Never use
numbered steps.

**Before (numbered steps):**

```markdown
## Installation

1. Make sure you have Rust installed
2. Run `cargo install refinery_cli`
3. Verify the installation with `converge --version`
```

**After (label-then-codeblock):**

````markdown
## Getting Started

### As a CLI

```sh
cargo install refinery_cli
```

Requires [Rust](https://www.rust-lang.org/tools/install) 1.85+.
````

## 3. Section naming

Use "Getting Started" for the combined install-and-first-use section, not
"Installation". Use "Quick Start" for the first runnable example. These are the
two most important sections and they appear in this order.

| Standard Ankane section | What goes in it |
|-------------------------|-----------------|
| Getting Started | Install instructions (CLI + library) |
| Quick Start | One runnable example with real output |
| CLI Usage / Library Usage | Reference docs, grouped by feature |
| Contributing | Verbatim Ankane template |

Do not create an "Installation" section. Do not create a "License" section.

## 4. Code fences: single purpose, language-tagged

Each code fence demonstrates exactly one concept. Always set the language tag
(`sh`, `rust`, `toml`, `json`, `bash`). Never combine multiple unrelated
operations in a single block.

**Before (multi-purpose block):**

````markdown
```sh
cargo install refinery_cli
converge "prompt" --models claude,codex
converge "prompt" --models claude,codex --output-format json
```
````

**After (one concept per block):**

````markdown
Install the CLI

```sh
cargo install refinery_cli
```

Run a consensus session

```sh
converge "prompt" --models claude,codex
```

Get JSON output

```sh
converge "prompt" --models claude,codex --output-format json
```
````

## 5. Tables: only for parallel structure

Use tables only when data has genuine parallel structure (model aliases, exit
codes, environment variables). Three columns maximum. Keep cells short.

Good table use:

```markdown
| Code | Meaning |
|------|---------|
| 0 | Converged or single model |
| 1 | Error or cancellation |
| 2 | Max rounds exceeded |
```

Bad table use: prose descriptions, long sentences in cells, more than three
columns, or repeating the same formula across rows (see lesson 6).

## 6. Cost table lesson: deduplicate formulas

When a formula applies uniformly, state it once with an example. Do not repeat it
for every row.

**Before (redundant):**

```markdown
| Models | Propose | Evaluate | Refine | Total |
|--------|---------|----------|--------|-------|
| 2 | 2 | 2*(2-1)=2 | 2 | 6 |
| 3 | 3 | 3*(3-1)=6 | 3 | 12 |
| 4 | 4 | 4*(4-1)=12 | 4 | 20 |
| 5 | 5 | 5*(5-1)=20 | 5 | 30 |
```

**After (formula + example):**

```markdown
Each round makes N² + N API calls (e.g., 3 models = 12 calls). Use `--dry-run`
to estimate.
```

One line replaces an entire table. Readers who care about exact counts will use
`--dry-run`.

## 7. JSON examples: show the essential shape

Show the structural skeleton of JSON output. Do not enumerate every field or
include realistic-length string values. Users will see real output when they run
the tool.

**Before (exhaustive):**

```json
{
  "status": "converged",
  "winner": {
    "model_id": "claude-sonnet",
    "answer": "Physics has seen many breakthroughs...",
    "mean_score": 9.5,
    "scores_received": [9.0, 10.0],
    "round_submitted": 2
  },
  "all_answers": [
    {
      "model_id": "claude-sonnet",
      "answer": "...",
      "mean_score": 9.5,
      "scores_received": [9.0, 10.0]
    },
    {
      "model_id": "codex",
      "answer": "...",
      "mean_score": 8.2,
      "scores_received": [8.0, 8.5]
    }
  ],
  "final_round": 2,
  "strategy": "vote-threshold",
  "metadata": {
    "total_rounds": 2,
    "total_calls": 12,
    "elapsed_ms": 45000,
    "threshold": 8.0,
    "stability_rounds": 2
  }
}
```

**After (essential shape):**

```json
{
  "status": "converged",
  "winner": { "model_id": "claude-sonnet", "answer": "..." },
  "final_round": 2,
  "strategy": "vote-threshold",
  "all_answers": [{ "model_id": "...", "answer": "...", "mean_score": 9.5 }],
  "metadata": { "total_rounds": 2, "total_calls": 12, "elapsed_ms": 45000 }
}
```

The second version shows every top-level key and the shape of nested objects
without overwhelming the reader.

## 8. Contributing: Ankane template verbatim

Use this exact structure. Four bullets describing ways to contribute, then a
clone/cd/test block.

````markdown
## Contributing

Everyone is encouraged to help improve this project. Here are a few ways you can help:

- [Report bugs](https://github.com/ORG/REPO/issues)
- Fix bugs and [submit pull requests](https://github.com/ORG/REPO/pulls)
- Write, clarify, or fix documentation
- Suggest or add new features

To get started with development:

```sh
git clone https://github.com/ORG/REPO.git
cd REPO
cargo test --workspace
```
````

Do not add extra prose, badges, or a code of conduct link. The simplicity is the
point.

## 9. No License section

Put a LICENSE file in the repo root. Do not add a "License" section to the
README. Ankane projects consistently omit it; the file speaks for itself.

## 10. Badge: single CI badge only

One badge, top of file, after the tagline. Use the GitHub Actions workflow badge.
Do not add coverage badges, crates.io version badges, or download counts.

```markdown
# Project Name

Short tagline

[![Build Status](https://github.com/ORG/REPO/actions/workflows/ci.yml/badge.svg)](https://github.com/ORG/REPO/actions/workflows/ci.yml)
```

## 11. Model table simplification: deduplicate aliases

When models have short aliases, do not give each alias its own row. List the
canonical models in the table and add a one-line note for aliases.

**Before (6 rows, redundant):**

```markdown
| Alias | Provider | Model |
|-------|----------|-------|
| `claude-sonnet` | Anthropic | claude-sonnet |
| `claude` | Anthropic | claude-sonnet |
| `claude-opus` | Anthropic | claude-opus |
| `codex` | OpenAI | codex |
| `gemini-2.5-pro` | Google | gemini-2.5-pro |
| `gemini` | Google | gemini-2.5-pro |
```

**After (4 rows + note):**

```markdown
| Alias | Provider | Model |
|-------|----------|-------|
| `claude-sonnet` | Anthropic | claude-sonnet |
| `claude-opus` | Anthropic | claude-opus |
| `codex` | OpenAI | codex |
| `gemini-2.5-pro` | Google | gemini-2.5-pro |

Short aliases: `claude` = `claude-sonnet`, `gemini` = `gemini-2.5-pro`.
```

The note line replaces two redundant rows and makes the alias mapping explicit.

## 12. Review lesson: different reviewers catch different things

Architecture reviewers and simplicity reviewers find complementary issues.

**Architecture reviewers** catch:
- Missing imports in code examples
- Wrong package names (`converge-cli` vs `refinery_cli`)
- Incorrect API surfaces (methods that do not exist yet)
- Broken cross-references between sections

**Simplicity reviewers** catch:
- Redundant table rows (alias duplication)
- Repeated formulas (cost table)
- Over-specified JSON examples
- YAGNI sections that can be removed entirely

Both perspectives are valuable. Run at least two review passes with different
instructions when polishing a README.

## 13. Cargo package name gotcha

`cargo install` uses the **package name from Cargo.toml**, which uses
underscores. It does not accept hyphens as a substitute.

**Wrong:**

```sh
cargo install converge-cli
```

**Right:**

```sh
cargo install refinery_cli
```

This is a common source of "package not found" errors. The crate name in
`Cargo.toml` is the source of truth:

```toml
[package]
name = "refinery_cli"
```

Always verify the package name before writing install instructions. A reviewer
caught this exact mistake in the ConVerge Refinery README.

## Checklist

Use this checklist when writing or reviewing a README in Ankane style:

- [ ] Single CI badge after tagline
- [ ] "Getting Started" section (not "Installation")
- [ ] "Quick Start" with one runnable example
- [ ] Every label is imperative, no period
- [ ] Every code fence has a language tag
- [ ] Every code fence demonstrates one concept
- [ ] Tables have 3 columns max, short cells
- [ ] No redundant table rows (aliases as a note)
- [ ] No repeated formulas (one line + example)
- [ ] JSON shows essential shape only
- [ ] Contributing uses verbatim Ankane template
- [ ] No License section (LICENSE file in repo root)
- [ ] `cargo install` uses underscore package name
- [ ] No numbered steps anywhere
- [ ] No passive voice
- [ ] Every sentence is 15 words or fewer
