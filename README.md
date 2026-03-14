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
  --models claude-code,codex-cli,gemini-cli
```

Models propose, evaluate each other, and repeat until consensus.

## CLI Usage

### Models

Pass models as a comma-separated list using `provider/model` format

```sh
refinery "your prompt" --models claude-code/claude-opus-4-6,gemini-cli/gemini-3.1-pro-preview
```

Short aliases use each provider's default model

```sh
refinery "your prompt" --models claude-code,codex-cli,gemini-cli
```

Available providers and defaults:

| Provider | Default model | Binary |
|----------|---------------|--------|
| `claude-code` | claude-opus-4-6 | `claude` |
| `codex-cli` | gpt-5.4 | `codex` |
| `gemini-cli` | gemini-3.1-pro-preview | `gemini` |

Override the model with `provider/model` syntax (e.g., `claude-code/claude-sonnet-4-6`, `codex-cli/o3-pro`).

### Options

Set the convergence threshold

```sh
refinery "prompt" --models claude-code,codex-cli --threshold 9.0
```

Limit the number of rounds

```sh
refinery "prompt" --models claude-code,codex-cli --max-rounds 3
```

Set per-call timeout (seconds)

```sh
refinery "prompt" --models claude-code,codex-cli --timeout 180
```

Limit concurrent API calls

```sh
refinery "prompt" --models claude-code,codex-cli --max-concurrent 4
```

### Output Formats

Output is plain text by default. Get JSON for programmatic use

```sh
refinery "prompt" --models claude-code,codex-cli --output-format json
```

```json
{
  "status": "converged",
  "winner": { "model_id": "claude-code/claude-opus-4-6", "answer": "..." },
  "final_round": 2,
  "strategy": "vote-threshold",
  "all_answers": [{ "model_id": "...", "answer": "...", "mean_score": 9.5 }],
  "metadata": { "total_rounds": 2, "total_calls": 12, "elapsed_ms": 45000 }
}
```

### Dry Run

Estimate API call count without running

```sh
refinery "prompt" --models claude-code,codex-cli,gemini-cli --dry-run
```

### File Input

Pass one or more files with `-f`/`--file` (repeatable, 1 MB total)

```sh
# Files as the subject — no text prompt needed
refinery --file src/auth.rs --file src/crypto.rs --models claude-code,codex-cli,gemini-cli

# Files with an instruction prompt
refinery "review these for security issues" --file src/auth.rs --file src/lib.rs --models claude-code,codex-cli

# Combine stdin instruction with a file
echo "what does this do?" | refinery - --file src/main.rs --models claude-code,gemini-cli
```

File contents are wrapped in nonce-tagged blocks (`<file-{nonce} path="...">`) so models know which content came from where. Non-UTF-8 files and files exceeding the 1 MB budget are rejected with a clear error before any API calls are made.

### Reading from Stdin

Pipe a prompt from another command (max 1 MB)

```sh
cat question.txt | refinery - --models claude-code,codex-cli
```

### Verbose and Debug

```sh
refinery "prompt" --models claude-code,codex-cli --verbose  # per-round progress
refinery "prompt" --models claude-code,codex-cli --debug    # raw CLI invocations
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Converged or single model |
| 1 | Error or cancellation |
| 2 | Max rounds exceeded |
| 3 | Insufficient models |
| 4 | Config or input error |


## CLI Examples

<details>
  <summary>### The Hitchhiker's Guide to the Galaxy</summary>
  ```
  $ refinery --max-rounds 5 --output-format json --models claude-code,codex-cli,gemini-cli --timeout 1800 --idle-timeout 480 --output-dir out "What's the answer to life, the Universe, and everything?"
  
    Round 1/5
    ── propose ──
      ✓ codex-cli/gpt-5.4 proposed (1 words) — "42."
      ✓ gemini-cli/gemini-3.1-pro-preview proposed (56 words) — "The answer to life, the Universe, and everything is **42**, ..."
      ✓ claude-code/claude-opus-4-6 proposed (72 words) — "42 — the answer computed by the supercomputer Deep Thought i..."
    ── evaluate ──
      ✓ gemini-cli/gemini-3.1-pro-preview → codex-cli/gpt-5.4: 8.0 — "The answer correctly identifies the iconic response to the p..."
      ✓ codex-cli/gpt-5.4 → gemini-cli/gemini-3.1-pro-preview: 9.0 — "This is a strong answer: accurate, clear, and appropriately ..."
      ✓ claude-code/claude-opus-4-6 → codex-cli/gpt-5.4: 8.0 — "The answer is correct and appropriately succinct. '42' is th..."
      ✓ gemini-cli/gemini-3.1-pro-preview → claude-code/claude-opus-4-6: 10.0 — "An excellent, comprehensive answer that not only provides th..."
      ✓ codex-cli/gpt-5.4 → claude-code/claude-opus-4-6: 9.0 — "This is a strong answer: accurate, well-written, and appropr..."
      ✓ claude-code/claude-opus-4-6 → gemini-cli/gemini-3.1-pro-preview: 8.0 — "This is a solid, accurate, and well-written answer. It corre..."
    → Not converged (9.5/8.0, stable 1/2)
                                         R1  
      claude-code/claude-opus-4-6         9.5 ★
      gemini-cli/gemini-3.1-pro-preview   8.5
      codex-cli/gpt-5.4                   8.0
  
    Round 2/5
    ── propose ──
      ✓ codex-cli/gpt-5.4 proposed (11 words) — "42, according to Douglas Adams' *The Hitchhiker's Guide to t..."
      ✓ claude-code/claude-opus-4-6 proposed (100 words) — "**42** — the answer computed by the supercomputer Deep Thoug..."
      ✓ gemini-cli/gemini-3.1-pro-preview proposed (188 words) — "The answer to life, the Universe, and everything is **42**."
    ── evaluate ──
      ✓ claude-code/claude-opus-4-6 → codex-cli/gpt-5.4: 8.0 — "This is a correct, well-attributed, and concise answer. It i..."
      ✓ codex-cli/gpt-5.4 → claude-code/claude-opus-4-6: 9.0 — "This is a strong answer: accurate, well-written, and appropr..."
      ✓ codex-cli/gpt-5.4 → gemini-cli/gemini-3.1-pro-preview: 9.0 — "This is a strong answer: correct, clear, and engaging. It an..."
      ✓ gemini-cli/gemini-3.1-pro-preview → codex-cli/gpt-5.4: 10.0 — "The answer perfectly addresses the classic pop-culture trivi..."
      ✓ claude-code/claude-opus-4-6 → gemini-cli/gemini-3.1-pro-preview: 9.0 — "This is an excellent, accurate, and well-structured answer t..."
      ✓ gemini-cli/gemini-3.1-pro-preview → claude-code/claude-opus-4-6: 10.0 — "An exceptionally well-crafted response that not only provide..."
    → Converged! Winner: claude-code/claude-opus-4-6 (9.5 ≥ 8.0, stable 2/2)
                                         R1    R2  
      claude-code/claude-opus-4-6         9.5   9.5 ★
      codex-cli/gpt-5.4                   8.0   9.0
      gemini-cli/gemini-3.1-pro-preview   8.5   9.0
  {
    "status": "converged",
    "winner": {
      "model_id": "claude-code/claude-opus-4-6",
      "answer": "**42** — the answer computed by the supercomputer Deep Thought in Douglas Adams' *The Hitchhiker's Guide to the Galaxy* (1979). Deep Thought took 7.5 million years to arrive at this answer, only to reveal that the real problem was that nobody knew what the actual Question was. A larger computer — the Earth — was then built to find the Question, but was unfortunately demolished by the Vogons five minutes before completing its 10-million-year program.\n\nThe joke, of course, is that a precise answer without a meaningful question is useless — a wry commentary on humanity's search for cosmic meaning."
    },
    "final_round": 2,
    "strategy": "vote-threshold",
    "all_answers": [
      {
        "model_id": "codex-cli/gpt-5.4",
        "answer": "42, according to Douglas Adams' *The Hitchhiker's Guide to the Galaxy*.",
        "mean_score": 9.0
      },
      {
        "model_id": "claude-code/claude-opus-4-6",
        "answer": "**42** — the answer computed by the supercomputer Deep Thought in Douglas Adams' *The Hitchhiker's Guide to the Galaxy* (1979). Deep Thought took 7.5 million years to arrive at this answer, only to reveal that the real problem was that nobody knew what the actual Question was. A larger computer — the Earth — was then built to find the Question, but was unfortunately demolished by the Vogons five minutes before completing its 10-million-year program.\n\nThe joke, of course, is that a precise answer without a meaningful question is useless — a wry commentary on humanity's search for cosmic meaning.",
        "mean_score": 9.5
      },
      {
        "model_id": "gemini-cli/gemini-3.1-pro-preview",
        "answer": "The answer to life, the Universe, and everything is **42**.\n\nThis famous answer originates from Douglas Adams' classic science fiction comedy series, *The Hitchhiker's Guide to the Galaxy*. \n\nIn the story, a group of hyper-intelligent, pan-dimensional beings build a massive supercomputer named Deep Thought to calculate the answer to the Ultimate Question of Life, the Universe, and Everything. After pondering the problem for 7.5 million years, Deep Thought finally reveals the answer to be 42.\n\nHowever, the computer points out that the answer is ultimately meaningless to them because the creators never actually knew what the \"Ultimate Question\" was in the first place. To figure out the specific question that yields the answer 42, an even larger and more complex computer is built—a planet called Earth. Tragically, Earth is destroyed by the Vogons to make way for a hyperspace bypass just five minutes before its ten-million-year calculation is set to complete.\n\nSince the series' publication, the number \"42\" has transcended its origins to become a beloved staple of geek culture and science fiction trivia, frequently appearing as an Easter egg in software, video games, and across the internet.",
        "mean_score": 9.0
      }
    ],
    "metadata": {
      "total_rounds": 2,
      "total_calls": 18,
      "elapsed_ms": 80667,
      "models_dropped": []
    }
  }
  ```
</details>

<details>
  <summary>### The Car Wash Test</summary>
  
  ```
  $ refinery --max-rounds 5 --output-format json --models claude-code,codex-cli,gemini-cli --timeout 1800 --idle-timeout 480 --output-dir out "The car wash is only 100m away from my house, should I walk or drive?" --dry-run
 Dry run estimate:
   Models: 3
   Calls per round: 9
   Max rounds: 5
   Total calls (max): 45
  ```
  
  ```
  $ refinery --max-rounds 5 --output-format json --models claude-code,codex-cli,gemini-cli --timeout 1800 --idle-timeout 480 --output-dir out "The car wash is only 100m away from my house, should I walk or drive?"
  
    Round 1/5
    ── propose ──
      ✓ codex-cli/gpt-5.4 proposed (42 words) — "Walk."
      ✓ gemini-cli/gemini-3.1-pro-preview proposed (32 words) — "You should definitely drive. "
      ✓ claude-code/claude-opus-4-6 proposed (59 words) — "Walk. 100 meters is roughly a one-minute walk. Driving such ..."
    ── evaluate ──
      ✓ codex-cli/gpt-5.4 → claude-code/claude-opus-4-6: 5.0 — "The answer is clear and reasonable on the narrow assumption ..."
      ✓ claude-code/claude-opus-4-6 → codex-cli/gpt-5.4: 4.0 — "The answer is well-written and concise but misses the most o..."
      ✓ codex-cli/gpt-5.4 → gemini-cli/gemini-3.1-pro-preview: 8.0 — "The answer is logically correct and directly addresses the p..."
      ✓ gemini-cli/gemini-3.1-pro-preview → codex-cli/gpt-5.4: 2.0 — "The answer provides excellent generic advice for traveling a..."
      ✓ claude-code/claude-opus-4-6 → gemini-cli/gemini-3.1-pro-preview: 9.0 — "This is a strong, clever answer that correctly identifies th..."
      ✓ gemini-cli/gemini-3.1-pro-preview → claude-code/claude-opus-4-6: 2.0 — "The model provides excellent advice for a generic 100-meter ..."
    → Not converged (8.5/8.0, stable 1/2)
                                         R1  
      gemini-cli/gemini-3.1-pro-preview   8.5 ★
      claude-code/claude-opus-4-6         3.5
      codex-cli/gpt-5.4                   3.0
  
    Round 2/5
    ── propose ──
      ✓ claude-code/claude-opus-4-6 proposed (52 words) — "Drive. You're going to a car wash, so you need the car there..."
      ✓ codex-cli/gpt-5.4 proposed (39 words) — "If you’re going there to wash your car, drive. The distance ..."
      ✓ gemini-cli/gemini-3.1-pro-preview proposed (125 words) — "You should definitely **drive**—at least initially! "
    ── evaluate ──
      ✓ codex-cli/gpt-5.4 → claude-code/claude-opus-4-6: 8.0 — "This is a strong, practical answer that matches the most lik..."
      ✓ gemini-cli/gemini-3.1-pro-preview → codex-cli/gpt-5.4: 10.0 — "The answer perfectly addresses the core trick of the questio..."
      ✓ gemini-cli/gemini-3.1-pro-preview → claude-code/claude-opus-4-6: 9.0 — "The answer perfectly nails the trick of the question by poin..."
      ✓ codex-cli/gpt-5.4 → gemini-cli/gemini-3.1-pro-preview: 8.0 — "This is a strong, practical answer that addresses the obviou..."
      ✓ claude-code/claude-opus-4-6 → gemini-cli/gemini-3.1-pro-preview: 9.0 — "This is a strong answer that correctly identifies the implic..."
      ✓ claude-code/claude-opus-4-6 → codex-cli/gpt-5.4: 8.0 — "This is a strong, practical answer that correctly identifies..."
    → Not converged (9.0/8.0, stable 1/2)
                                         R1    R2  
      codex-cli/gpt-5.4                   3.0   9.0 ★
      claude-code/claude-opus-4-6         3.5   8.5
      gemini-cli/gemini-3.1-pro-preview   8.5   8.5
  
    Round 3/5
    ── propose ──
      ✓ codex-cli/gpt-5.4 proposed (53 words) — "If you’re using the car wash, drive. The car has to be there..."
      ✓ claude-code/claude-opus-4-6 proposed (97 words) — "Drive. You're going to a car wash — you need the car there t..."
      ✓ gemini-cli/gemini-3.1-pro-preview proposed (176 words) — "You should definitely **drive**—at least initially! "
    ── evaluate ──
      ✓ gemini-cli/gemini-3.1-pro-preview → claude-code/claude-opus-4-6: 10.0 — "This is an excellent response that correctly identifies the ..."
      ✓ codex-cli/gpt-5.4 → claude-code/claude-opus-4-6: 8.0 — "This is a strong answer for a simple question. It is logical..."
      ✓ codex-cli/gpt-5.4 → gemini-cli/gemini-3.1-pro-preview: 8.0 — "This is a strong, practical answer that correctly resolves t..."
      ✓ gemini-cli/gemini-3.1-pro-preview → codex-cli/gpt-5.4: 10.0 — "The answer is excellent. It immediately catches the logical ..."
      ✓ claude-code/claude-opus-4-6 → gemini-cli/gemini-3.1-pro-preview: 9.0 — "This is an excellent answer that nails the key insight of th..."
      ✓ claude-code/claude-opus-4-6 → codex-cli/gpt-5.4: 8.0 — "This is a strong, practical answer that nails the central in..."
    → Converged! Winner: codex-cli/gpt-5.4 (9.0 ≥ 8.0, stable 2/2)
                                         R1    R2    R3  
      claude-code/claude-opus-4-6         3.5   8.5   9.0
      codex-cli/gpt-5.4                   3.0   9.0   9.0 ★
      gemini-cli/gemini-3.1-pro-preview   8.5   8.5   8.5
  {
    "status": "converged",
    "winner": {
      "model_id": "codex-cli/gpt-5.4",
      "answer": "If you’re using the car wash, drive. The car has to be there, and at 100m the trip is trivial.\n\nA sensible option is: drive it there, then walk home or back while it’s being cleaned if that’s allowed.\n\nIf you only mean going over to ask a question or check prices, walk."
    },
    "final_round": 3,
    "strategy": "vote-threshold",
    "all_answers": [
      {
        "model_id": "codex-cli/gpt-5.4",
        "answer": "If you’re using the car wash, drive. The car has to be there, and at 100m the trip is trivial.\n\nA sensible option is: drive it there, then walk home or back while it’s being cleaned if that’s allowed.\n\nIf you only mean going over to ask a question or check prices, walk.",
        "mean_score": 9.0
      },
      {
        "model_id": "claude-code/claude-opus-4-6",
        "answer": "Drive. You're going to a car wash — you need the car there to get it washed. Walking 100 meters to a car wash without your car defeats the entire purpose of the trip.\n\nThe short distance is actually a bonus: minimal fuel use, and you're there in seconds.\n\nThe only scenario where walking would make sense is if you're not actually getting the car washed — e.g., you're going to ask about pricing, buy a gift card, or the wash offers a pickup/drop-off service. But if the goal is to wash your car, drive it there.",
        "mean_score": 9.0
      },
      {
        "model_id": "gemini-cli/gemini-3.1-pro-preview",
        "answer": "You should definitely **drive**—at least initially! \n\nWhile 100 meters is a very short distance, it is going to be incredibly difficult to get your car washed if you leave it at home. \n\nHow you handle the rest of the trip, however, depends on what kind of car wash it is:\n\n*   **Full-service or hand-wash (drop-off):** You have the perfect setup for a hybrid approach. **Drive** the car to the wash and drop it off, **walk** the 100m back home to relax or get some chores done while they work, then **walk** back to pick it up and drive your clean car home.\n*   **Automatic or drive-through:** You'll just need to **drive**. Pull through the wash, dry it off, and make the very short drive back to your driveway.\n*   **Self-serve bay:** **Drive** over, wash it yourself, and drive back. \n\n*(Of course, if you're just walking over to buy a snack at the attached convenience store, or you're planning to wash your bicycle or the dog, then leave the car in the driveway and enjoy the walk!)*",
        "mean_score": 8.5
      }
    ],
    "metadata": {
      "total_rounds": 3,
      "total_calls": 27,
      "elapsed_ms": 128612,
      "models_dropped": []
    }
  }
  ```
  
  ```
  $ refinery --max-rounds 5 --output-format json --models claude-code,codex-cli,gemini-cli --timeout 1800 --idle-timeout 480 --output-dir out "The car wash is only 100m away from my house, should I walk or drive?"
  
    Round 1/5
    ── propose ──
      ✓ claude-code/claude-opus-4-6 proposed (70 words) — "Walk. 100 meters is roughly a one-minute walk. Driving such ..."
      ✓ gemini-cli/gemini-3.1-pro-preview proposed (57 words) — "You should definitely **drive**—assuming you are going there..."
      ✓ codex-cli/gpt-5.4 proposed (9 words) — "Drive. The car wash needs the car there too."
    ── evaluate ──
      ✓ codex-cli/gpt-5.4 → gemini-cli/gemini-3.1-pro-preview: 8.0 — "This is a solid answer that correctly interprets the most li..."
      ✓ codex-cli/gpt-5.4 → claude-code/claude-opus-4-6: 6.0 — "The answer is clear and practical, but its main recommendati..."
      ✓ gemini-cli/gemini-3.1-pro-preview → codex-cli/gpt-5.4: 9.0 — "The model successfully navigates the trick question by point..."
      ✓ claude-code/claude-opus-4-6 → codex-cli/gpt-5.4: 9.0 — "This is an excellent, pragmatic answer that identifies the k..."
      ✓ claude-code/claude-opus-4-6 → gemini-cli/gemini-3.1-pro-preview: 8.0 — "This is a clever, practical answer that nails the most impor..."
      ✓ gemini-cli/gemini-3.1-pro-preview → claude-code/claude-opus-4-6: 4.0 — "The answer provides standard, logical advice for a generic 1..."
    → Not converged (9.0/8.0, stable 1/2)
                                         R1  
      codex-cli/gpt-5.4                   9.0 ★
      gemini-cli/gemini-3.1-pro-preview   8.0
      claude-code/claude-opus-4-6         5.0
  
    Round 2/5
    ── propose ──
      ✓ claude-code/claude-opus-4-6 proposed (26 words) — "Drive. You're going to a car wash — you need your car there...."
      ✓ codex-cli/gpt-5.4 proposed (28 words) — "Drive. It’s only 100m, but the car wash still needs the car...."
      ✓ gemini-cli/gemini-3.1-pro-preview proposed (182 words) — "Whether you should walk or drive depends entirely on **why**..."
    ── evaluate ──
      ✓ codex-cli/gpt-5.4 → claude-code/claude-opus-4-6: 8.0 — "This is a mostly correct answer that addresses the main logi..."
      ✓ codex-cli/gpt-5.4 → gemini-cli/gemini-3.1-pro-preview: 8.0 — "This is a solid, sensible answer that handles the ambiguity ..."
      ✓ gemini-cli/gemini-3.1-pro-preview → codex-cli/gpt-5.4: 10.0 — "The answer perfectly handles the trick question by pointing ..."
      ✓ claude-code/claude-opus-4-6 → codex-cli/gpt-5.4: 8.0 — "This is a strong, practical answer that nails the key insigh..."
      ✓ claude-code/claude-opus-4-6 → gemini-cli/gemini-3.1-pro-preview: 9.0 — "This is a strong, well-reasoned answer that nails the centra..."
      ✓ gemini-cli/gemini-3.1-pro-preview → claude-code/claude-opus-4-6: 9.0 — "The answer perfectly captures the logical catch of the user'..."
    → Converged! Winner: codex-cli/gpt-5.4 (9.0 ≥ 8.0, stable 2/2)
                                         R1    R2  
      codex-cli/gpt-5.4                   9.0   9.0 ★
      gemini-cli/gemini-3.1-pro-preview   8.0   8.5
      claude-code/claude-opus-4-6         5.0   8.5
  {
    "status": "converged",
    "winner": {
      "model_id": "codex-cli/gpt-5.4",
      "answer": "Drive. It’s only 100m, but the car wash still needs the car. If you already left it there and you’re just going to pick it up, then walk."
    },
    "final_round": 2,
    "strategy": "vote-threshold",
    "all_answers": [
      {
        "model_id": "gemini-cli/gemini-3.1-pro-preview",
        "answer": "Whether you should walk or drive depends entirely on **why** you are going to the car wash. \n\nHere is a breakdown of your best options:\n\n**1. If you are going to get your car washed:**\nYou should **drive**. Even though 100m is only about a 1 to 2-minute walk, you will need to bring the car with you to actually get it washed!\n\n**2. If you are dropping your car off to be detailed:**\nYou have the perfect setup. You should **drive** your car there to drop it off, and then enjoy the quick 1-minute **walk** back home while you wait for the service to be completed. \n\n**3. If you are going for any other reason:**\nYou should **walk**. If you are just heading there to buy a snack from their convenience store, use an ATM, ask a question, or apply for a job, a 100m distance (about 330 feet) is incredibly short. Walking will only take a minute or two, whereas driving would probably take longer just to start the engine, pull out of your driveway, and find a parking spot.",
        "mean_score": 8.5
      },
      {
        "model_id": "codex-cli/gpt-5.4",
        "answer": "Drive. It’s only 100m, but the car wash still needs the car. If you already left it there and you’re just going to pick it up, then walk.",
        "mean_score": 9.0
      },
      {
        "model_id": "claude-code/claude-opus-4-6",
        "answer": "Drive. You're going to a car wash — you need your car there. Walking 100 meters to a car wash without your car defeats the purpose.",
        "mean_score": 8.5
      }
    ],
    "metadata": {
      "total_rounds": 2,
      "total_calls": 18,
      "elapsed_ms": 84181,
      "models_dropped": []
    }
  }
  ```
</details>


<details>
  <summary>### The Car Wash Test</summary>
  ```
  $refinery --max-rounds 5 --output-format json --models claude-code,codex-cli,gemini-cli --timeout 1800 --idle-timeout 480 --output-dir out "What's the answer to life, the Universe, and everything?"
  
    Round 1/5
    ── propose ──
      ✓ codex-cli/gpt-5.4 proposed (1 words) — "42."
      ✓ gemini-cli/gemini-3.1-pro-preview proposed (56 words) — "The answer to life, the Universe, and everything is **42**, ..."
      ✓ claude-code/claude-opus-4-6 proposed (72 words) — "42 — the answer computed by the supercomputer Deep Thought i..."
    ── evaluate ──
      ✓ gemini-cli/gemini-3.1-pro-preview → codex-cli/gpt-5.4: 8.0 — "The answer correctly identifies the iconic response to the p..."
      ✓ codex-cli/gpt-5.4 → gemini-cli/gemini-3.1-pro-preview: 9.0 — "This is a strong answer: accurate, clear, and appropriately ..."
      ✓ claude-code/claude-opus-4-6 → codex-cli/gpt-5.4: 8.0 — "The answer is correct and appropriately succinct. '42' is th..."
      ✓ gemini-cli/gemini-3.1-pro-preview → claude-code/claude-opus-4-6: 10.0 — "An excellent, comprehensive answer that not only provides th..."
      ✓ codex-cli/gpt-5.4 → claude-code/claude-opus-4-6: 9.0 — "This is a strong answer: accurate, well-written, and appropr..."
      ✓ claude-code/claude-opus-4-6 → gemini-cli/gemini-3.1-pro-preview: 8.0 — "This is a solid, accurate, and well-written answer. It corre..."
    → Not converged (9.5/8.0, stable 1/2)
                                         R1  
      claude-code/claude-opus-4-6         9.5 ★
      gemini-cli/gemini-3.1-pro-preview   8.5
      codex-cli/gpt-5.4                   8.0
  
    Round 2/5
    ── propose ──
      ✓ codex-cli/gpt-5.4 proposed (11 words) — "42, according to Douglas Adams' *The Hitchhiker's Guide to t..."
      ✓ claude-code/claude-opus-4-6 proposed (100 words) — "**42** — the answer computed by the supercomputer Deep Thoug..."
      ✓ gemini-cli/gemini-3.1-pro-preview proposed (188 words) — "The answer to life, the Universe, and everything is **42**."
    ── evaluate ──
      ✓ claude-code/claude-opus-4-6 → codex-cli/gpt-5.4: 8.0 — "This is a correct, well-attributed, and concise answer. It i..."
      ✓ codex-cli/gpt-5.4 → claude-code/claude-opus-4-6: 9.0 — "This is a strong answer: accurate, well-written, and appropr..."
      ✓ codex-cli/gpt-5.4 → gemini-cli/gemini-3.1-pro-preview: 9.0 — "This is a strong answer: correct, clear, and engaging. It an..."
      ✓ gemini-cli/gemini-3.1-pro-preview → codex-cli/gpt-5.4: 10.0 — "The answer perfectly addresses the classic pop-culture trivi..."
      ✓ claude-code/claude-opus-4-6 → gemini-cli/gemini-3.1-pro-preview: 9.0 — "This is an excellent, accurate, and well-structured answer t..."
      ✓ gemini-cli/gemini-3.1-pro-preview → claude-code/claude-opus-4-6: 10.0 — "An exceptionally well-crafted response that not only provide..."
    → Converged! Winner: claude-code/claude-opus-4-6 (9.5 ≥ 8.0, stable 2/2)
                                         R1    R2  
      claude-code/claude-opus-4-6         9.5   9.5 ★
      codex-cli/gpt-5.4                   8.0   9.0
      gemini-cli/gemini-3.1-pro-preview   8.5   9.0
  {
    "status": "converged",
    "winner": {
      "model_id": "claude-code/claude-opus-4-6",
      "answer": "**42** — the answer computed by the supercomputer Deep Thought in Douglas Adams' *The Hitchhiker's Guide to the Galaxy* (1979). Deep Thought took 7.5 million years to arrive at this answer, only to reveal that the real problem was that nobody knew what the actual Question was. A larger computer — the Earth — was then built to find the Question, but was unfortunately demolished by the Vogons five minutes before completing its 10-million-year program.\n\nThe joke, of course, is that a precise answer without a meaningful question is useless — a wry commentary on humanity's search for cosmic meaning."
    },
    "final_round": 2,
    "strategy": "vote-threshold",
    "all_answers": [
      {
        "model_id": "codex-cli/gpt-5.4",
        "answer": "42, according to Douglas Adams' *The Hitchhiker's Guide to the Galaxy*.",
        "mean_score": 9.0
      },
      {
        "model_id": "claude-code/claude-opus-4-6",
        "answer": "**42** — the answer computed by the supercomputer Deep Thought in Douglas Adams' *The Hitchhiker's Guide to the Galaxy* (1979). Deep Thought took 7.5 million years to arrive at this answer, only to reveal that the real problem was that nobody knew what the actual Question was. A larger computer — the Earth — was then built to find the Question, but was unfortunately demolished by the Vogons five minutes before completing its 10-million-year program.\n\nThe joke, of course, is that a precise answer without a meaningful question is useless — a wry commentary on humanity's search for cosmic meaning.",
        "mean_score": 9.5
      },
      {
        "model_id": "gemini-cli/gemini-3.1-pro-preview",
        "answer": "The answer to life, the Universe, and everything is **42**.\n\nThis famous answer originates from Douglas Adams' classic science fiction comedy series, *The Hitchhiker's Guide to the Galaxy*. \n\nIn the story, a group of hyper-intelligent, pan-dimensional beings build a massive supercomputer named Deep Thought to calculate the answer to the Ultimate Question of Life, the Universe, and Everything. After pondering the problem for 7.5 million years, Deep Thought finally reveals the answer to be 42.\n\nHowever, the computer points out that the answer is ultimately meaningless to them because the creators never actually knew what the \"Ultimate Question\" was in the first place. To figure out the specific question that yields the answer 42, an even larger and more complex computer is built—a planet called Earth. Tragically, Earth is destroyed by the Vogons to make way for a hyperspace bypass just five minutes before its ten-million-year calculation is set to complete.\n\nSince the series' publication, the number \"42\" has transcended its origins to become a beloved staple of geek culture and science fiction trivia, frequently appearing as an Easter egg in software, video games, and across the internet.",
        "mean_score": 9.0
      }
    ],
    "metadata": {
      "total_rounds": 2,
      "total_calls": 18,
      "elapsed_ms": 80667,
      "models_dropped": []
    }
  }
  ```
</details>

<details>
  <summary>### The Car Wash Test</summary>
</details>

## Library Usage

### Basic

Run the full consensus loop

```rust
use std::time::Duration;
use refinery_core::{Engine, EngineConfig, ModelId, VoteThreshold};

let config = EngineConfig::new(
    vec![ModelId::new("provider-a/model-a"), ModelId::new("provider-b/model-b")],
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

ConVerge runs a 3-phase loop until convergence or max rounds:

1. **Propose** — each model independently answers the prompt (round 2+ includes prior scores and peer answers as context)
2. **Evaluate** — each model reviews and scores every other model's answer (1–10)
3. **Close** — check if the top-scoring model meets the threshold and has been stable

Models are anonymized during evaluation (shuffled labels A, B, C…) to reduce bias. Self-scores are excluded.

### Convergence Criterion

The default `VoteThreshold` strategy converges when:
- The top model's mean score ≥ threshold (default 8.0), **and**
- The same model has led for `stability_rounds` consecutive rounds (default 2)

### Cost per Round

Each round makes N(N-1) + N = N² API calls (e.g., 3 models = 9 calls). Use `--dry-run` to estimate.

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
