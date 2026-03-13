use std::collections::HashMap;
use std::io::{IsTerminal as _, Read as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use serde::Serialize;
use tracing::info;

use refinery_core::types::{ConvergenceStatus, ModelId, RoundOutcome};
use refinery_core::{EngineConfig, ModelProvider};

/// Iterative multi-model consensus engine.
///
/// Given a prompt, N models independently produce answers, cross-review each other's work,
/// score all answers — repeating until a configurable convergence criterion is met.
#[derive(Parser, Debug)]
#[command(name = "refinery", version, about)]
struct Cli {
    /// The prompt to reach consensus on (or - for stdin, max 1MB). Optional when --file is used.
    #[arg(value_name = "PROMPT")]
    prompt: Option<String>,

    /// File(s) to include in the prompt, tagged by filename (repeatable, 1MB total)
    #[arg(long = "file", short = 'f', value_name = "PATH")]
    files: Vec<PathBuf>,

    /// Comma-separated model list [e.g., claude-code,codex-cli/o3-pro,gemini-cli]
    #[arg(short, long, value_delimiter = ',')]
    models: Vec<String>,

    /// Score threshold for convergence [default: 8.0] (range: 1.0-10.0)
    #[arg(short, long, default_value = "8.0")]
    threshold: f64,

    /// Maximum rounds [default: 5] (range: 1-20)
    #[arg(short = 'r', long, default_value = "5")]
    max_rounds: u32,

    /// Hard wall-clock timeout per call in seconds [default: 1800] (range: 1-7200)
    #[arg(long, default_value = "1800")]
    timeout: u64,

    /// Idle timeout: max seconds of silence before killing a subprocess [default: 120] (range: 1-600)
    #[arg(long, default_value = "120")]
    idle_timeout: u64,

    /// Max concurrent subprocess calls [default: 0 = unlimited] (range: 0-50)
    #[arg(long, default_value = "0")]
    max_concurrent: usize,

    /// Output format [text|json]
    #[arg(short, long, default_value = "text")]
    output_format: OutputFormat,

    /// Show per-round progress
    #[arg(short, long)]
    verbose: bool,

    /// Show raw CLI invocations and responses
    #[arg(long)]
    debug: bool,

    /// Tools to allow: `web_fetch`, `web_search`, `file_read`, `file_write`, `shell`.
    /// Mapped to each provider's native tool names automatically.
    #[arg(long = "allow-tools", value_delimiter = ',')]
    allow_tools: Vec<String>,

    /// Directory to save per-round artifacts (proposals, evaluations)
    #[arg(long = "output-dir", value_name = "DIR")]
    output_dir: Option<PathBuf>,

    /// Show estimated call count and cost, then exit
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

/// JSON output schema for successful runs.
#[derive(Serialize)]
struct JsonOutput {
    status: String,
    winner: WinnerOutput,
    final_round: u32,
    strategy: String,
    all_answers: Vec<AnswerOutput>,
    metadata: MetadataOutput,
}

#[derive(Serialize)]
struct WinnerOutput {
    model_id: String,
    answer: String,
}

#[derive(Serialize)]
struct AnswerOutput {
    model_id: String,
    answer: String,
    mean_score: f64,
}

#[derive(Serialize)]
struct MetadataOutput {
    total_rounds: u32,
    total_calls: u32,
    elapsed_ms: u128,
    models_dropped: Vec<String>,
}

/// JSON error output schema.
#[derive(Serialize)]
struct ErrorResponse {
    status: String,
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: String,
    message: String,
    provider: Option<String>,
    round: Option<u32>,
    phase: Option<String>,
    retryable: bool,
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    // Set up tracing
    let filter = if cli.debug {
        "debug"
    } else if cli.verbose {
        "info"
    } else {
        "warn"
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    // Resolve prompt text from positional arg or stdin
    let prompt_text: Option<String> = match cli.prompt.as_deref() {
        Some("-") => {
            let mut buf = String::new();
            let bytes_read = match std::io::stdin().take(1_000_001).read_to_string(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("Error reading stdin: {e}");
                    return ExitCode::from(4);
                }
            };
            if bytes_read > 1_000_000 {
                eprintln!("Error: stdin input exceeds 1MB limit");
                return ExitCode::from(4);
            }
            Some(buf)
        }
        Some(p) => Some(p.to_string()),
        None => None,
    };

    // At least one input source required
    if prompt_text.is_none() && cli.files.is_empty() {
        eprintln!("Error: a prompt or at least one --file must be provided");
        return ExitCode::from(4);
    }

    // Read and validate files (runs even during --dry-run for early validation)
    let prompt_bytes = prompt_text.as_deref().map_or(0, str::len);
    let file_budget = 1_000_000_usize.saturating_sub(prompt_bytes);
    let file_data: Vec<(String, String)> = if cli.files.is_empty() {
        Vec::new()
    } else {
        match read_and_validate_files(&cli.files, file_budget) {
            Ok(data) => data,
            Err(errors) => {
                for e in &errors {
                    eprintln!("Error: {e}");
                }
                return ExitCode::from(4);
            }
        }
    };

    // Assemble the final prompt
    let nonce = refinery_core::prompts::generate_nonce();
    let prompt =
        refinery_core::prompts::assemble_file_prompt(prompt_text.as_deref(), &file_data, &nonce);

    if cli.models.is_empty() {
        eprintln!("Error: at least one model must be specified with --models");
        return ExitCode::from(4);
    }

    let model_ids: Vec<ModelId> = match cli
        .models
        .iter()
        .map(|m| parse_model_spec(m))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(ids) => ids,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(4);
        }
    };

    let config = match EngineConfig::new(
        model_ids.clone(),
        cli.max_rounds,
        cli.threshold,
        2, // stability_rounds
        Duration::from_secs(cli.timeout),
        cli.max_concurrent,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Config error: {e}");
            return ExitCode::from(4);
        }
    };

    // Dry run: show cost estimate
    if cli.dry_run {
        let estimate = refinery_core::Engine::estimate(&config);
        println!("Dry run estimate:");
        println!("  Models: {}", estimate.model_count);
        println!("  Calls per round: {}", estimate.calls_per_round);
        println!("  Max rounds: {}", estimate.max_rounds);
        println!("  Total calls (max): {}", estimate.total_calls);
        if estimate.model_count > 5 {
            eprintln!(
                "Warning: N={} has quadratic cost scaling ({} calls/round)",
                estimate.model_count, estimate.calls_per_round
            );
        }
        return ExitCode::SUCCESS;
    }

    // Build providers
    let timeout = Duration::from_secs(cli.timeout);
    let idle_timeout = Duration::from_secs(cli.idle_timeout);

    // Animated spinner: a background task ticks the frame at ~80ms, while
    // progress events just update the shared status text.
    let spinner_state = Arc::new(Mutex::new(SpinnerState {
        label: None,
        started: std::time::Instant::now(),
        frame: 0,
        current_evals: HashMap::new(),
        round_scores: Vec::new(),
    }));

    let tick_handle = if !cli.verbose && !cli.debug && std::io::stderr().is_terminal() {
        let state = spinner_state.clone();
        Some(tokio::spawn(async move {
            const FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            loop {
                tokio::time::sleep(Duration::from_millis(80)).await;
                let mut s = state.lock().unwrap();
                if let Some(ref label) = s.label {
                    let spin = FRAMES[s.frame % FRAMES.len()];
                    let elapsed = s.started.elapsed().as_secs();
                    eprint!("\r\x1b[2K    {spin} {label}, {elapsed}s");
                    s.frame += 1;
                }
            }
        }))
    } else {
        None
    };

    let progress: Option<refinery_core::ProgressFn> = if tick_handle.is_some() {
        let state = spinner_state.clone();
        Some(Arc::new(move |event| render_progress(event, &state)))
    } else {
        None
    };

    let mut providers: Vec<Arc<dyn ModelProvider>> = Vec::new();

    for model_id in &model_ids {
        match build_provider(model_id, &cli.allow_tools, timeout, idle_timeout, progress.clone())
            .await
        {
            Ok(p) => providers.push(p),
            Err(e) => {
                eprintln!("Failed to initialize provider '{model_id}': {e}");
                return ExitCode::from(4);
            }
        }
    }

    let strategy = Box::new(refinery_core::VoteThreshold::new(cli.threshold, 2));
    let engine = refinery_core::Engine::new(providers, strategy, config, progress.clone());

    info!("Starting consensus run with {} models", cli.models.len());

    let run_result = engine.run(&prompt).await;

    // Stop the spinner tick task and clear the progress line
    if let Some(handle) = tick_handle {
        handle.abort();
        eprint!("\r\x1b[2K");
    }

    match run_result {
        Ok((outcome, rounds)) => {
            // Save per-round artifacts if --output-dir is set
            if let Some(ref dir) = cli.output_dir {
                if let Err(e) = save_round_artifacts(dir, &rounds) {
                    eprintln!("Warning: failed to save artifacts: {e}");
                }
            }

            match cli.output_format {
                OutputFormat::Json => {
                    let status_str = match serde_json::to_value(&outcome.status) {
                        Ok(serde_json::Value::String(s)) => s,
                        _ => format!("{:?}", outcome.status).to_lowercase(),
                    };
                    let json_output = JsonOutput {
                        status: status_str,
                        winner: WinnerOutput {
                            model_id: outcome.winner.to_string(),
                            answer: outcome.answer.clone(),
                        },
                        final_round: outcome.final_round,
                        strategy: "vote-threshold".to_string(),
                        all_answers: outcome
                            .all_answers
                            .iter()
                            .map(|a| AnswerOutput {
                                model_id: a.model_id.to_string(),
                                answer: a.answer.clone(),
                                mean_score: a.mean_score,
                            })
                            .collect(),
                        metadata: MetadataOutput {
                            total_rounds: outcome.final_round,
                            total_calls: outcome.total_calls,
                            elapsed_ms: outcome.elapsed.as_millis(),
                            models_dropped: vec![],
                        },
                    };
                    match serde_json::to_string_pretty(&json_output) {
                        Ok(json) => println!("{json}"),
                        Err(e) => {
                            eprintln!("Failed to serialize output: {e}");
                            return ExitCode::from(1);
                        }
                    }
                }
                OutputFormat::Text => {
                    println!("Status: {:?}", outcome.status);
                    println!("Winner: {}", outcome.winner);
                    println!("Rounds: {}", outcome.final_round);
                    println!("Total calls: {}", outcome.total_calls);
                    println!("Elapsed: {:?}", outcome.elapsed);
                    println!("\n--- Answer ---\n");
                    println!("{}", outcome.answer);
                }
            }

            match outcome.status {
                ConvergenceStatus::Converged | ConvergenceStatus::SingleModel => ExitCode::SUCCESS,
                ConvergenceStatus::MaxRoundsExceeded => ExitCode::from(2),
                ConvergenceStatus::InsufficientModels => ExitCode::from(3),
                ConvergenceStatus::Cancelled => ExitCode::from(1),
            }
        }
        Err(e) => {
            match cli.output_format {
                OutputFormat::Json => {
                    let err_response = ErrorResponse {
                        status: "error".to_string(),
                        error: converge_error_to_detail(&e),
                    };
                    match serde_json::to_string_pretty(&err_response) {
                        Ok(json) => eprintln!("{json}"),
                        Err(ser_err) => eprintln!("Error: {e} (serialization failed: {ser_err})"),
                    }
                }
                OutputFormat::Text => {
                    eprintln!("Error: {e}");
                }
            }
            ExitCode::from(1)
        }
    }
}

fn read_and_validate_files(
    paths: &[PathBuf],
    budget: usize,
) -> Result<Vec<(String, String)>, Vec<String>> {
    let mut errors: Vec<String> = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();
    let mut total_bytes: usize = 0;

    for path in paths {
        let path_str = path.display().to_string();

        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                errors.push(format!("file '{path_str}': {e}"));
                continue;
            }
        };

        if !meta.is_file() {
            errors.push(format!("file '{path_str}': not a regular file"));
            continue;
        }

        // Pre-read size guard to avoid allocating memory for huge files
        let file_size = usize::try_from(meta.len()).unwrap_or(usize::MAX);
        if file_size > budget {
            errors.push(format!("file '{path_str}': exceeds 1MB limit"));
            continue;
        }

        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                errors.push(format!("file '{path_str}': {e}"));
                continue;
            }
        };

        let Ok(text) = String::from_utf8(bytes) else {
            errors.push(format!("file '{path_str}': not valid UTF-8"));
            continue;
        };

        total_bytes += text.len();
        files.push((path_str, text));
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    if total_bytes > budget {
        return Err(vec![format!(
            "total file size ({total_bytes} bytes) exceeds 1MB limit"
        )]);
    }

    Ok(files)
}

/// Parse a CLI model spec into a `ModelId`.
///
/// Accepts `provider/model` or provider-only (applies default model).
fn parse_model_spec(input: &str) -> Result<ModelId, String> {
    if input.contains('/') {
        let (provider, model) = input.split_once('/').unwrap();
        if model.contains('/') {
            return Err(format!(
                "Model spec must be 'provider/model', got extra '/': '{input}'"
            ));
        }
        if provider.is_empty() || model.is_empty() {
            return Err(format!("Invalid model spec: '{input}'"));
        }
        Ok(ModelId::from_parts(provider, model))
    } else {
        match input {
            "claude-code" => Ok(ModelId::from_parts("claude-code", "claude-opus-4-6")),
            "codex-cli" => Ok(ModelId::from_parts("codex-cli", "gpt-5.4")),
            "gemini-cli" => Ok(ModelId::from_parts("gemini-cli", "gemini-3.1-pro-preview")),
            "claude" | "codex" | "gemini" => Err(format!(
                "Unknown provider '{input}'. The format is now 'provider/model'. \
                 Did you mean '{input}-code' or '{input}-cli'? \
                 Supported providers: claude-code, codex-cli, gemini-cli"
            )),
            _ => Err(format!(
                "Unknown provider '{input}'. Supported: claude-code, codex-cli, gemini-cli"
            )),
        }
    }
}

async fn build_provider(
    model_id: &ModelId,
    allowed_tools: &[String],
    max_timeout: Duration,
    idle_timeout: Duration,
    progress: Option<refinery_core::ProgressFn>,
) -> Result<Arc<dyn ModelProvider>, Box<dyn std::error::Error>> {
    match model_id.provider() {
        "claude-code" => {
            let provider = refinery_providers::claude::ClaudeProvider::new(
                model_id.clone(),
                allowed_tools,
                max_timeout,
                idle_timeout,
                progress,
            )
            .await?;
            Ok(Arc::new(provider))
        }
        "codex-cli" => {
            let provider = refinery_providers::codex::CodexProvider::new(
                model_id.clone(),
                "xhigh",
                allowed_tools,
                max_timeout,
                idle_timeout,
                progress,
            )
            .await?;
            Ok(Arc::new(provider))
        }
        "gemini-cli" => {
            let provider = refinery_providers::gemini::GeminiProvider::new(
                model_id.clone(),
                allowed_tools,
                max_timeout,
                idle_timeout,
                progress,
            )
            .await?;
            Ok(Arc::new(provider))
        }
        other => Err(format!(
            "Unknown provider: '{other}'. Supported: claude-code, codex-cli, gemini-cli"
        )
        .into()),
    }
}

fn save_round_artifacts(
    base_dir: &std::path::Path,
    rounds: &[RoundOutcome],
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(base_dir)?;

    for round in rounds {
        let round_dir = base_dir.join(format!("round-{}", round.round));
        std::fs::create_dir_all(&round_dir)?;

        // Proposals: one file per model
        for (model_id, text) in &round.proposals.proposals {
            let path = round_dir.join(format!("propose-{model_id}.md"));
            std::fs::write(&path, text)?;
        }

        // Evaluations: one file per (evaluator, evaluatee) pair
        for ((evaluator, evaluatee), eval) in &round.evaluations.evaluations {
            let path = round_dir.join(format!("evaluate-{evaluator}-{evaluatee}.json"));
            let content = serde_json::json!({
                "evaluator": evaluator.to_string(),
                "evaluatee": evaluatee.to_string(),
                "score": eval.score.value(),
                "rationale": eval.rationale,
                "strengths": eval.review.strengths,
                "weaknesses": eval.review.weaknesses,
                "suggestions": eval.review.suggestions,
                "overall_assessment": eval.review.overall_assessment,
            });
            std::fs::write(&path, serde_json::to_string_pretty(&content)?)?;
        }
    }

    Ok(())
}

/// Shared state between the progress callback and the background spinner tick task.
struct SpinnerState {
    /// Current in-progress label, e.g. "claude-opus-4-6 — 42 lines".
    /// The tick task appends the live elapsed time. None = spinner idle.
    label: Option<String>,
    /// When the current subprocess started (for live elapsed timer).
    started: std::time::Instant,
    /// Frame counter, advanced by the tick task.
    frame: usize,
    /// Per-model evaluation scores for the current round (cleared each round).
    current_evals: HashMap<String, Vec<f64>>,
    /// Per-round mean scores accumulated across all rounds.
    round_scores: Vec<HashMap<String, f64>>,
}

/// Handle a progress event by updating shared spinner state.
///
/// `SubprocessOutput` sets the spinner message (the tick task renders it).
/// All other events clear the spinner and print a final line.
#[allow(clippy::too_many_lines)]
fn render_progress(event: refinery_core::ProgressEvent, state: &Mutex<SpinnerState>) {
    use refinery_core::ProgressEvent;
    use std::fmt::Write;
    let mut s = state.lock().unwrap();
    match event {
        ProgressEvent::RoundStarted { round, total } => {
            s.label = None;
            s.current_evals.clear();
            eprint!("\r\x1b[2K");
            eprintln!("\n  Round {round}/{total}");
        }
        ProgressEvent::PhaseStarted { phase, .. } => {
            s.label = None;
            eprint!("\r\x1b[2K");
            eprintln!("  ── {phase} ──");
        }
        ProgressEvent::SubprocessOutput {
            model,
            lines,
            ..
        } => {
            if s.label.is_none() {
                s.started = std::time::Instant::now();
            }
            s.label = Some(format!("{model} — {lines} lines"));
        }
        ProgressEvent::ModelProposed {
            model,
            word_count,
            preview,
        } => {
            s.label = None;
            eprintln!(
                "\r\x1b[2K    \x1b[32m✓\x1b[0m {model} proposed ({word_count} words) — \"{preview}\""
            );
        }
        ProgressEvent::ModelProposeFailed { model, error } => {
            s.label = None;
            eprintln!("\r\x1b[2K    \x1b[31m✗\x1b[0m {model} failed — {error}");
        }
        ProgressEvent::EvaluationCompleted {
            reviewer,
            reviewee,
            score,
            preview,
        } => {
            s.label = None;
            s.current_evals
                .entry(reviewee.to_string())
                .or_default()
                .push(score);
            eprintln!(
                "\r\x1b[2K    \x1b[32m✓\x1b[0m {reviewer} → {reviewee}: {score:.1} — \"{preview}\""
            );
        }
        ProgressEvent::EvaluationFailed {
            reviewer,
            reviewee,
            error,
        } => {
            s.label = None;
            eprintln!(
                "\r\x1b[2K    \x1b[31m✗\x1b[0m {reviewer} → {reviewee} failed — {error}"
            );
        }
        ProgressEvent::ConvergenceCheck {
            converged,
            winner,
            best_score,
            threshold,
            stable_rounds,
            required_stable,
            ..
        } => {
            s.label = None;
            eprint!("\r\x1b[2K");

            let winner_name = winner.as_ref().map(std::string::ToString::to_string);
            if converged {
                let w = winner_name.as_deref().unwrap_or("?");
                eprintln!(
                    "  \x1b[32m→ Converged!\x1b[0m Winner: {w} ({best_score:.1} ≥ {threshold:.1}, stable {stable_rounds}/{required_stable})"
                );
            } else {
                eprintln!(
                    "  → Not converged ({best_score:.1}/{threshold:.1}, stable {stable_rounds}/{required_stable})"
                );
            }

            // Finalize current round means into the history
            if !s.current_evals.is_empty() {
                let mut means: HashMap<String, f64> = HashMap::new();
                for (model, scores) in &s.current_evals {
                    #[allow(clippy::cast_precision_loss)]
                    let mean = scores.iter().sum::<f64>() / scores.len() as f64;
                    means.insert(model.clone(), mean);
                }
                s.round_scores.push(means);
            }

            // Render progressive score table across all rounds
            if !s.round_scores.is_empty() {
                // Collect all models, sorted by latest round score desc
                let latest = s.round_scores.last().unwrap();
                let mut models: Vec<&String> = latest.keys().collect();
                models.sort_by(|a, b| {
                    latest
                        .get(*b)
                        .partial_cmp(&latest.get(*a))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                let name_w = models.iter().map(|n| n.len()).max().unwrap_or(0);
                let num_rounds = s.round_scores.len();

                // Header row with round numbers
                let mut header = format!("    {:<name_w$}", "");
                for r in 1..=num_rounds {
                    let _ = write!(header, "  R{r:<3}");
                }
                eprintln!("\x1b[2m{header}\x1b[0m");

                // One row per model
                for name in &models {
                    let is_winner = winner_name.as_deref() == Some(name.as_str());
                    let mut row = if is_winner {
                        format!("    \x1b[32m{name:<name_w$}")
                    } else {
                        format!("    {name:<name_w$}")
                    };
                    for round in &s.round_scores {
                        match round.get(*name) {
                            Some(score) => {
                                let _ = write!(row, "  {score:>4.1}");
                            }
                            None => row.push_str("     -"),
                        }
                    }
                    if is_winner {
                        row.push_str(" ★\x1b[0m");
                    }
                    eprintln!("{row}");
                }
            }
        }
    }
}

fn converge_error_to_detail(err: &refinery_core::ConvergeError) -> ErrorDetail {
    match err {
        refinery_core::ConvergeError::PhaseFailure {
            phase,
            model,
            source: _,
        } => ErrorDetail {
            code: "phase_failure".to_string(),
            message: err.to_string(),
            provider: Some(model.to_string()),
            round: None,
            phase: Some(phase.to_string()),
            retryable: true,
        },
        refinery_core::ConvergeError::InsufficientModels { round, .. } => ErrorDetail {
            code: "insufficient_models".to_string(),
            message: err.to_string(),
            provider: None,
            round: Some(*round),
            phase: None,
            retryable: false,
        },
        refinery_core::ConvergeError::ConfigInvalid { .. } => ErrorDetail {
            code: "config_invalid".to_string(),
            message: err.to_string(),
            provider: None,
            round: None,
            phase: None,
            retryable: false,
        },
        refinery_core::ConvergeError::Cancelled => ErrorDetail {
            code: "cancelled".to_string(),
            message: err.to_string(),
            provider: None,
            round: None,
            phase: None,
            retryable: false,
        },
    }
}
