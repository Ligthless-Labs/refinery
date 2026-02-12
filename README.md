# ConVerge Refinery

Iteratively reach consensus across multiple AI models

[![Build Status](https://github.com/Bande-a-Bonnot/converge-refinery/actions/workflows/ci.yml/badge.svg)](https://github.com/Bande-a-Bonnot/converge-refinery/actions/workflows/ci.yml)

## Getting Started

### As a CLI

```sh
cargo install converge-cli
```

Requires [Rust](https://www.rust-lang.org/tools/install) 1.85+.

### As a Library

Add to your `Cargo.toml`

```toml
[dependencies]
converge_core = "0.1"
```

## Quick Start

Set up credentials for at least one provider (see [Credentials](#credentials))

```sh
converge "What are the three most impactful breakthroughs in physics?" \
  --models claude,codex,gemini
```

Each model proposes an answer, reviews the others, refines based on feedback, and repeats until consensus.

## CLI Usage

### Models

Pass models as a comma-separated list

```sh
converge "your prompt" --models claude,gemini
```

Available models:

| Alias | Provider | Default model |
|-------|----------|---------------|
| `claude` | Anthropic | claude-sonnet |
| `claude-opus` | Anthropic | claude-opus |
| `claude-sonnet` | Anthropic | claude-sonnet |
| `codex` | OpenAI | codex |
| `gemini` | Google | gemini-2.5-pro |
| `gemini-2.5-pro` | Google | gemini-2.5-pro |

### Options

Set the convergence threshold

```sh
converge "prompt" --models claude,codex --threshold 9.0
```

Limit the number of rounds

```sh
converge "prompt" --models claude,codex --max-rounds 3
```

Set per-call timeout (seconds)

```sh
converge "prompt" --models claude,codex --timeout 180
```

Limit concurrent API calls

```sh
converge "prompt" --models claude,codex --max-concurrent 4
```

### Output Formats

Plain text (default)

```sh
converge "prompt" --models claude,codex
```

JSON for programmatic use

```sh
converge "prompt" --models claude,codex --output-format json
```

```json
{
  "status": "converged",
  "winner": { "model_id": "claude-sonnet", "answer": "..." },
  "final_round": 2,
  "strategy": "vote-threshold",
  "all_answers": [
    { "model_id": "claude-sonnet", "answer": "...", "mean_score": 9.5 },
    { "model_id": "codex", "answer": "...", "mean_score": 8.0 }
  ],
  "metadata": {
    "total_rounds": 2,
    "total_calls": 12,
    "elapsed_ms": 45000,
    "models_dropped": []
  }
}
```

### Dry Run

Estimate API call count without running

```sh
converge "prompt" --models claude,codex,gemini --dry-run
```

### Reading from Stdin

Pipe a prompt from another command (max 1 MB)

```sh
cat question.txt | converge - --models claude,codex
```

### Verbose and Debug

Show per-round progress

```sh
converge "prompt" --models claude,codex --verbose
```

Show raw CLI invocations and responses

```sh
converge "prompt" --models claude,codex --debug
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Converged or single model |
| 1 | Error or cancellation |
| 2 | Max rounds exceeded |
| 3 | Insufficient models |
| 4 | Config or input error |

## Library Usage

### Basic

Run the full consensus loop

```rust
use std::sync::Arc;
use std::time::Duration;
use converge_core::{Engine, EngineConfig, ModelId, VoteThreshold};

let config = EngineConfig::new(
    vec![ModelId::new("model-a"), ModelId::new("model-b")],
    5,    // max_rounds
    8.0,  // threshold
    2,    // stability_rounds
    Duration::from_secs(120),
    0,    // max_concurrent (0 = unlimited)
)?;

let strategy = Box::new(VoteThreshold::new(8.0, 2));
let engine = Engine::new(providers, strategy, config);

let outcome = engine.run("What is the capital of France?").await?;
println!("{}: {}", outcome.winner, outcome.answer);
```

### Round-by-Round

Step through rounds for fine-grained control

```rust
let mut session = engine.start("prompt").await?;

loop {
    let round = session.next_round().await?;
    println!("Round {}: {:?}", round.round, round.closing_decision);

    if matches!(round.closing_decision, ClosingDecision::Converged { .. }) {
        break;
    }
}

let outcome = session.finalize();
```

Inject overrides between rounds

```rust
use converge_core::RoundOverrides;

let overrides = RoundOverrides {
    additional_context: Some("Focus on recent developments".into()),
    drop_models: vec![],
};
let round = session.next_round_with(overrides).await?;
```

### Custom Providers

Implement the `ModelProvider` trait

```rust
use async_trait::async_trait;
use converge_core::{ModelProvider, ModelId};
use converge_core::types::Message;
use converge_core::error::ProviderError;

#[derive(Debug)]
struct MyProvider { model_id: ModelId }

#[async_trait]
impl ModelProvider for MyProvider {
    async fn send_message(&self, messages: &[Message]) -> Result<String, ProviderError> {
        // Call your model here
        Ok("response".to_string())
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }
}
```

### Cost Estimation

Estimate API calls without running

```rust
let estimate = Engine::estimate(&config);
println!("{} calls/round, {} total", estimate.calls_per_round, estimate.total_calls);
```

## Credentials

Credentials are read from environment variables. You need at least one provider.

Copy `.env.example` to `.env` and fill in your credentials:

```bash
cp .env.example .env
```

### Anthropic (Claude)

**API Key** (pay-per-use) — set `ANTHROPIC_API_KEY`:

1. Create an account at [console.anthropic.com](https://console.anthropic.com/)
2. Go to **Settings → API Keys**
3. Click **Create Key**, give it a name, and copy the value

```bash
ANTHROPIC_API_KEY=sk-ant-api03-...
```

**Subscription** (Claude Pro/Max) — set `CLAUDE_CODE_OAUTH_TOKEN`:

1. Install the Claude CLI: `npm install -g @anthropic-ai/claude-code`
2. Run `claude setup-token` and follow the prompts — this generates a long-lived (~1 year) token
3. Copy the token it outputs

```bash
CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-...
```

### OpenAI (Codex)

**API Key** (pay-per-use) — set `OPENAI_API_KEY`:

1. Create an account at [platform.openai.com](https://platform.openai.com/)
2. Go to **Dashboard → API Keys** ([direct link](https://platform.openai.com/api-keys))
3. Click **Create new secret key**, give it a name, and copy the value

```bash
OPENAI_API_KEY=sk-...
```

**Alternative** — set `CODEX_API_KEY`:

The Codex CLI also accepts `CODEX_API_KEY` for non-interactive (`codex exec`) mode. Same key format as `OPENAI_API_KEY`.

```bash
CODEX_API_KEY=sk-...
```

### Google (Gemini)

**API Key** (Google AI Studio) — set `GEMINI_API_KEY`:

1. Go to [Google AI Studio](https://aistudio.google.com/apikey)
2. Sign in with your Google account
3. Click **Create API Key**, select a Google Cloud project (one will be created if needed), and copy the value

```bash
GEMINI_API_KEY=AI...
```

**Alternative** (Google Cloud) — set `GOOGLE_API_KEY`:

If you already have a Google Cloud API key with the Generative Language API enabled, you can use it directly.

```bash
GOOGLE_API_KEY=AI...
```

### AWS Bedrock

Coming soon — for accessing Claude and other models via AWS Bedrock.

### Google Cloud (Vertex AI)

Coming soon — for accessing Gemini via Vertex AI.

## How It Works

ConVerge runs a 4-phase loop until convergence or max rounds:

1. **Propose** — each model independently answers the prompt
2. **Evaluate** — each model reviews and scores every other model's answer (1–10)
3. **Refine** — each model improves its answer based on peer feedback
4. **Close** — check if the top-scoring model meets the threshold and has been stable

Models are anonymized during evaluation (shuffled labels A, B, C…) to reduce bias. Self-scores are excluded.

### Convergence Criterion

The default `VoteThreshold` strategy converges when:
- The top model's mean score ≥ threshold (default 8.0), **and**
- The same model has led for `stability_rounds` consecutive rounds (default 2)

### Cost per Round

| Models (N) | Calls per round | Formula |
|------------|----------------|---------|
| 2 | 6 | N² + N |
| 3 | 12 | N² + N |
| 5 | 30 | N² + N |
| 7 | 56 | N² + N |

Use `--dry-run` to estimate before running.

## Contributing

Everyone is encouraged to help improve this project. Here are a few ways you can help:

- [Report bugs](https://github.com/Bande-a-Bonnot/converge-refinery/issues)
- Fix bugs and [submit pull requests](https://github.com/Bande-a-Bonnot/converge-refinery/pulls)
- Write, clarify, or fix documentation
- Suggest or add new features

To get started with development:

```sh
git clone https://github.com/Bande-a-Bonnot/converge-refinery.git
cd converge-refinery
cargo test --workspace
```
