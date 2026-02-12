use std::io::Read as _;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use serde::Serialize;
use tracing::info;

use converge_core::types::{ConvergenceStatus, ModelId};
use converge_core::{EngineConfig, ModelProvider};

/// Iterative multi-model consensus engine.
///
/// Given a prompt, N models independently produce answers, cross-review each other's work,
/// score all answers, then refine — repeating until a configurable convergence criterion is met.
#[derive(Parser, Debug)]
#[command(name = "converge", version, about)]
struct Cli {
    /// The prompt to reach consensus on (or - for stdin, max 1MB)
    prompt: String,

    /// Comma-separated model list [e.g., claude,codex,gemini]
    #[arg(short, long, value_delimiter = ',')]
    models: Vec<String>,

    /// Score threshold for convergence [default: 8.0] (range: 1.0-10.0)
    #[arg(short, long, default_value = "8.0")]
    threshold: f64,

    /// Maximum rounds [default: 5] (range: 1-20)
    #[arg(short = 'r', long, default_value = "5")]
    max_rounds: u32,

    /// Per-call timeout in seconds [default: 120] (range: 1-600)
    #[arg(long, default_value = "120")]
    timeout: u64,

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

    // Read prompt from stdin if "-"
    let prompt = if cli.prompt == "-" {
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
        buf
    } else {
        cli.prompt
    };

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
        let estimate = converge_core::Engine::estimate(&config);
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
    let mut providers: Vec<Arc<dyn ModelProvider>> = Vec::new();

    for model in &cli.models {
        match build_provider(model, timeout).await {
            Ok(p) => providers.push(p),
            Err(e) => {
                eprintln!("Failed to initialize provider '{model}': {e}");
                return ExitCode::from(4);
            }
        }
    }

    let strategy = Box::new(converge_core::VoteThreshold::new(cli.threshold, 2));
    let engine = converge_core::Engine::new(providers, strategy, config);

    info!("Starting consensus run with {} models", cli.models.len());

    match engine.run(&prompt).await {
        Ok(outcome) => {
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

async fn build_provider(
    model: &str,
    timeout: Duration,
) -> Result<Arc<dyn ModelProvider>, Box<dyn std::error::Error>> {
    match model {
        m if m.starts_with("claude") => {
            let model_name = m.strip_prefix("claude-").unwrap_or("sonnet");
            let provider =
                converge_providers::claude::ClaudeProvider::new(model_name, timeout).await?;
            Ok(Arc::new(provider))
        }
        "codex" => {
            let provider = converge_providers::codex::CodexProvider::new(timeout).await?;
            Ok(Arc::new(provider))
        }
        m if m.starts_with("gemini") => {
            let model_name = if m == "gemini" { "gemini-2.5-pro" } else { m };
            let provider =
                converge_providers::gemini::GeminiProvider::new(model_name, timeout).await?;
            Ok(Arc::new(provider))
        }
        _ => Err(format!(
            "Unknown model: {model}. Supported: claude[-model], codex, gemini[-model]"
        )
        .into()),
    }
}

fn converge_error_to_detail(err: &converge_core::ConvergeError) -> ErrorDetail {
    match err {
        converge_core::ConvergeError::PhaseFailure {
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
        converge_core::ConvergeError::InsufficientModels { round, .. } => ErrorDetail {
            code: "insufficient_models".to_string(),
            message: err.to_string(),
            provider: None,
            round: Some(*round),
            phase: None,
            retryable: false,
        },
        converge_core::ConvergeError::ConfigInvalid { .. } => ErrorDetail {
            code: "config_invalid".to_string(),
            message: err.to_string(),
            provider: None,
            round: None,
            phase: None,
            retryable: false,
        },
        converge_core::ConvergeError::Cancelled => ErrorDetail {
            code: "cancelled".to_string(),
            message: err.to_string(),
            provider: None,
            round: None,
            phase: None,
            retryable: false,
        },
    }
}
