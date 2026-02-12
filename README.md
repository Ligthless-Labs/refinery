# ConVerge Refinery

ConVerge is a domain-agnostic Rust Library and CLI tool designed for iteratively reaching consensus across multiple AI models.

## Library

### Usage

// TK

### Dependencies


## CLI

### Usage

### Dependencies

### Environment variables

The CLI requires credentials for the providers you want to use. You need at least one.

Copy `.env.example` to `.env` and fill in your credentials:

```bash
cp .env.example .env
```

#### Anthropic (Claude)

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

#### OpenAI (Codex)

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

#### Google (Gemini)

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

#### AWS Bedrock

[TK] — For accessing Claude and other models via AWS Bedrock. Requires `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`.

#### Google Cloud (Vertex AI)

[TK] — For accessing Gemini via Vertex AI with full project configuration.
