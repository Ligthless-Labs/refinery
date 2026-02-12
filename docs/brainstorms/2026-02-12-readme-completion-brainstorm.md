---
title: Complete the README.md
date: 2026-02-12
status: decided
---

# Complete the README.md

## What We're Building

A complete README in Ankane style: concise, imperative, code-heavy. Covers both the library API and the CLI tool. Minimal prose — let code examples do the talking.

## Why This Approach

Ankane-style READMEs (searchkick, pgvector, neighbor) are the gold standard for developer tools: scannable, copy-paste friendly, zero fluff. Fits a Rust CLI/library perfectly.

## Key Decisions

1. **Style**: Ankane — imperative voice, short sentences, single-purpose code fences
2. **Scope**: Both library and CLI, separate sections
3. **Credentials section**: Keep existing (already detailed and correct)
4. **AWS Bedrock / Vertex AI**: Keep as "Coming Soon" stubs
5. **Section order**: Installation → Quick Start → CLI Usage → Library Usage → Credentials → How It Works → Contributing

## Structure

```
# ConVerge Refinery
[One-line description]

## Installation
  ### CLI (cargo install)
  ### Library (Cargo.toml dep)

## Quick Start
  [Minimal CLI example — 3 lines]

## CLI Usage
  ### Models
  ### Options
  ### Output Formats (text, JSON)
  ### Dry Run
  ### Reading from stdin

## Library Usage
  ### Basic example (Engine::run)
  ### Round-by-round (Session API)
  ### Custom providers

## Credentials
  [Keep existing section as-is]

## How It Works
  [4-phase loop diagram: PROPOSE → EVALUATE → REFINE → CLOSE]
  [Cost table: N models → calls per round]

## Contributing
  [cargo test, cargo fmt, cargo clippy]

## License
  MIT
```

## Open Questions

None — ready to plan.
