use std::io::{IsTerminal as _, Read as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use serde::Serialize;
use tracing::info;

use refinery_core::types::{ConvergenceStatus, ModelId, RoundOutcome};
use refinery_core::{EngineConfig, ModelProvider};

/// Iterative multi-model consensus engine.
///
/// Given a prompt, N models independently produce answers, cross-review each other's work,
/// score all answers, then refine — repeating until a configurable convergence criterion is met.
#[derive(Parser, Debug)]
#[command(name = "refinery", version, about)]
struct Cli {
    /// The prompt to reach consensus on (or - for stdin, max 1MB). Optional when --file is used.
    #[arg(value_name = "PROMPT")]
    prompt: Option<String>,

    /// File(s) to include in the prompt, tagged by filename (repeatable, 1MB total)
    #[arg(long = "file", short = 'f', value_name = "PATH")]
    files: Vec<PathBuf>,

    /// Comma-separated model list [e.g., claude,codex,gemini]
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

    /// Directory to save per-round artifacts (proposals, evaluations, refinements)
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

    let model_ids: Vec<ModelId> = cli.models.iter().map(ModelId::new).collect();

    let config = match EngineConfig::new(
        model_ids,
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

    // Progress display when stderr is a terminal and not in verbose/debug mode
    let progress: Option<refinery_core::ProgressFn> =
        if !cli.verbose && !cli.debug && std::io::stderr().is_terminal() {
            Some(Arc::new(render_progress))
        } else {
            None
        };

    let mut providers: Vec<Arc<dyn ModelProvider>> = Vec::new();

    for model in &cli.models {
        match build_provider(model, &cli.allow_tools, timeout, idle_timeout, progress.clone()).await {
            Ok(p) => providers.push(p),
            Err(e) => {
                eprintln!("Failed to initialize provider '{model}': {e}");
                return ExitCode::from(4);
            }
        }
    }

    let strategy = Box::new(refinery_core::VoteThreshold::new(cli.threshold, 2));
    let engine = refinery_core::Engine::new(providers, strategy, config, progress.clone());

    info!("Starting consensus run with {} models", cli.models.len());

    let run_result = engine.run(&prompt).await;

    // Clear the in-place progress line before printing output
    if progress.is_some() {
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
                            model_id: outcome.winner.as_str().to_string(),
                            answer: outcome.answer.clone(),
                        },
                        final_round: outcome.final_round,
                        strategy: "vote-threshold".to_string(),
                        all_answers: outcome
                            .all_answers
                            .iter()
                            .map(|a| AnswerOutput {
                                model_id: a.model_id.as_str().to_string(),
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

async fn build_provider(
    model: &str,
    allowed_tools: &[String],
    max_timeout: Duration,
    idle_timeout: Duration,
    progress: Option<refinery_core::ProgressFn>,
) -> Result<Arc<dyn ModelProvider>, Box<dyn std::error::Error>> {
    match model {
        m if m.starts_with("claude") => {
            let model_name = m.strip_prefix("claude-").unwrap_or("opus-4-6");
            let provider = refinery_providers::claude::ClaudeProvider::new(
                model_name,
                allowed_tools,
                max_timeout,
                idle_timeout,
                progress,
            )
            .await?;
            Ok(Arc::new(provider))
        }
        m if m == "codex" || m.starts_with("codex-") => {
            let model_name = m.strip_prefix("codex-").unwrap_or("gpt-5.4");
            let provider = refinery_providers::codex::CodexProvider::new(
                model_name,
                "xhigh",
                allowed_tools,
                max_timeout,
                idle_timeout,
                progress,
            )
            .await?;
            Ok(Arc::new(provider))
        }
        m if m.starts_with("gemini") => {
            let model_name = if m == "gemini" {
                "gemini-3.1-pro-preview"
            } else {
                m
            };
            let provider = refinery_providers::gemini::GeminiProvider::new(
                model_name,
                allowed_tools,
                max_timeout,
                idle_timeout,
                progress,
            )
            .await?;
            Ok(Arc::new(provider))
        }
        _ => Err(format!(
            "Unknown model: {model}. Supported: claude[-model], codex, gemini[-model]"
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
            let path = round_dir.join(format!("propose-{}.md", model_id.as_str()));
            std::fs::write(&path, text)?;
        }

        // Evaluations: one file per (evaluator, evaluatee) pair
        for ((evaluator, evaluatee), eval) in &round.evaluations.evaluations {
            let path = round_dir.join(format!(
                "evaluate-{}-{}.json",
                evaluator.as_str(),
                evaluatee.as_str()
            ));
            let content = serde_json::json!({
                "evaluator": evaluator.as_str(),
                "evaluatee": evaluatee.as_str(),
                "score": eval.score.value(),
                "rationale": eval.rationale,
                "strengths": eval.review.strengths,
                "weaknesses": eval.review.weaknesses,
                "suggestions": eval.review.suggestions,
                "overall_assessment": eval.review.overall_assessment,
            });
            std::fs::write(&path, serde_json::to_string_pretty(&content)?)?;
        }

        // Refinements: one file per model
        for (model_id, text) in &round.refinements.refinements {
            let path = round_dir.join(format!("refine-{}.md", model_id.as_str()));
            std::fs::write(&path, text)?;
        }
    }

    Ok(())
}

fn render_progress(event: refinery_core::ProgressEvent) {
    use refinery_core::ProgressEvent;
    const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    match event {
        ProgressEvent::RoundStarted { round, total } => {
            eprintln!("\n  Round {round}/{total}");
        }
        ProgressEvent::PhaseStarted { phase, .. } => {
            eprintln!("  ── {phase} ──");
        }
        ProgressEvent::SubprocessOutput {
            model,
            lines,
            elapsed,
        } => {
            let spin = SPINNER[lines % SPINNER.len()];
            eprint!(
                "\r\x1b[2K    {spin} {model} — {lines} lines, {}s",
                elapsed.as_secs()
            );
        }
        ProgressEvent::ModelProposed {
            model,
            word_count,
            preview,
        } => {
            eprintln!(
                "\r\x1b[2K    \x1b[32m✓\x1b[0m {model} proposed ({word_count} words) — \"{preview}\""
            );
        }
        ProgressEvent::ModelProposeFailed { model, error } => {
            eprintln!("\r\x1b[2K    \x1b[31m✗\x1b[0m {model} failed — {error}");
        }
        ProgressEvent::EvaluationCompleted {
            reviewer,
            reviewee,
            score,
            preview,
        } => {
            eprintln!(
                "\r\x1b[2K    \x1b[32m✓\x1b[0m {reviewer} → {reviewee}: {score:.1} — \"{preview}\""
            );
        }
        ProgressEvent::EvaluationFailed {
            reviewer,
            reviewee,
            error,
        } => {
            eprintln!(
                "\r\x1b[2K    \x1b[31m✗\x1b[0m {reviewer} → {reviewee} failed — {error}"
            );
        }
        ProgressEvent::ModelRefined { model, word_count } => {
            eprintln!("\r\x1b[2K    \x1b[32m✓\x1b[0m {model} refined ({word_count} words)");
        }
        ProgressEvent::ModelRefineFailed { model, error } => {
            eprintln!("\r\x1b[2K    \x1b[31m✗\x1b[0m {model} refine failed — {error}");
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
            if converged {
                let w = winner.map_or_else(|| "?".to_string(), |m| m.to_string());
                eprintln!(
                    "  \x1b[32m→ Converged!\x1b[0m Winner: {w} ({best_score:.1} ≥ {threshold:.1}, stable {stable_rounds}/{required_stable})"
                );
            } else {
                eprintln!(
                    "  → Not converged ({best_score:.1}/{threshold:.1}, stable {stable_rounds}/{required_stable})"
                );
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
            provider: Some(model.as_str().to_string()),
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
