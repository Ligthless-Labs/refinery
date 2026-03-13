# Refinery

Iteratively reach consensus across multiple AI models

[![Build Status](https://github.com/Ligthless-Labs/refinery/actions/workflows/ci.yml/badge.svg)](https://github.com/Ligthless-Labs/refinery/actions/workflows/ci.yml)

## Getting Started

### As a CLI

```sh
cargo install --path crates/refinery_cli
```

Requires [Rust](https://www.rust-lang.org/tools/install) 1.85+.

### As a Library

Add to your `Cargo.toml`

```toml
[dependencies]
refinery_core = "0.1"
```

## Quick Start

Set up credentials for at least one provider (see [Credentials](#credentials))

```sh
refinery "What are the three most impactful breakthroughs in physics?" \
  --models claude,codex,gemini
```

Models propose, review, refine, and repeat until consensus.

## CLI Usage

### Models

Pass models as a comma-separated list

```sh
refinery "your prompt" --models claude,gemini
```

Available models:

| Alias | Resolves to |
|-------|-------------|
| `claude` | claude-opus-4-6, effort: high |
| `codex` | gpt-5.4, reasoning: xhigh |
| `gemini` | gemini-3.1-pro-preview |

Any `claude-*` value (e.g., `claude-sonnet`) is passed through to Anthropic. Any `codex-*` value (e.g., `codex-o3`) overrides the underlying model.

### Options

Set the convergence threshold

```sh
refinery "prompt" --models claude,codex --threshold 9.0
```

Limit the number of rounds

```sh
refinery "prompt" --models claude,codex --max-rounds 3
```

Set per-call timeout (seconds)

```sh
refinery "prompt" --models claude,codex --timeout 180
```

Limit concurrent API calls

```sh
refinery "prompt" --models claude,codex --max-concurrent 4
```

### Output Formats

Output is plain text by default. Get JSON for programmatic use

```sh
refinery "prompt" --models claude,codex --output-format json
```

```json
{
  "status": "converged",
  "winner": { "model_id": "claude-opus-4-6", "answer": "..." },
  "final_round": 2,
  "strategy": "vote-threshold",
  "all_answers": [{ "model_id": "...", "answer": "...", "mean_score": 9.5 }],
  "metadata": { "total_rounds": 2, "total_calls": 12, "elapsed_ms": 45000 }
}
```

### Dry Run

Estimate API call count without running

```sh
refinery "prompt" --models claude,codex,gemini --dry-run
```

### File Input

Pass one or more files with `-f`/`--file` (repeatable, 1 MB total)

```sh
# Files as the subject — no text prompt needed
refinery --file src/auth.rs --file src/crypto.rs --models claude,codex,gemini

# Files with an instruction prompt
refinery "review these for security issues" --file src/auth.rs --file src/lib.rs --models claude,codex

# Combine stdin instruction with a file
echo "what does this do?" | refinery - --file src/main.rs --models claude,gemini
```

File contents are wrapped in nonce-tagged blocks (`<file-{nonce} path="...">`) so models know which content came from where. Non-UTF-8 files and files exceeding the 1 MB budget are rejected with a clear error before any API calls are made.

### Reading from Stdin

Pipe a prompt from another command (max 1 MB)

```sh
cat question.txt | refinery - --models claude,codex
```

### Verbose and Debug

```sh
refinery "prompt" --models claude,codex --verbose  # per-round progress
refinery "prompt" --models claude,codex --debug    # raw CLI invocations
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
use std::time::Duration;
use refinery_core::{Engine, EngineConfig, ModelId, VoteThreshold};

let config = EngineConfig::new(
    vec![ModelId::new("model-a"), ModelId::new("model-b")],
    5,    // max_rounds
    8.0,  // threshold
    2,    // stability_rounds
    Duration::from_secs(120),
    0,    // max_concurrent (0 = unlimited)
)?;

let providers = vec![/* your Arc<dyn ModelProvider> instances */];
let strategy = Box::new(VoteThreshold::new(8.0, 2));
let engine = Engine::new(providers, strategy, config);

let outcome = engine.run("What is the capital of France?").await?;
println!("{}: {}", outcome.winner, outcome.answer);
```

### Round-by-Round

Step through rounds for fine-grained control

```rust
use refinery_core::ClosingDecision;

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
use refinery_core::RoundOverrides;

let overrides = RoundOverrides {
    additional_context: Some("Focus on recent developments".into()),
    ..Default::default()
};
let round = session.next_round_with(overrides).await?;
```

### Custom Providers

Implement the `ModelProvider` trait

```rust
use async_trait::async_trait;
use refinery_core::{ModelId, ModelProvider, Message, ProviderError};

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

Set credentials via environment variables. You need at least one provider.

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

Each round makes N² + N API calls (e.g., 3 models = 12 calls). Use `--dry-run` to estimate.

## Contributing

Everyone is encouraged to help improve this project. Here are a few ways you can help:

- [Report bugs](https://github.com/Ligthless-Labs/refinery/issues)
- Fix bugs and [submit pull requests](https://github.com/Ligthless-Labs/refinery/pulls)
- Write, clarify, or fix documentation
- Suggest or add new features

To get started with development:

```sh
git clone https://github.com/Ligthless-Labs/refinery.git
cd refinery
cargo test --workspace
```
