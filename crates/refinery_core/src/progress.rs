use std::sync::Arc;
use std::time::Duration;

use crate::types::{ModelId, Phase};

/// Progress events emitted during a consensus run.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    /// A model's subprocess outputted a line (streaming heartbeat).
    SubprocessOutput {
        model: ModelId,
        lines: usize,
        elapsed: Duration,
    },

    /// A new round has started.
    RoundStarted { round: u32, total: u32 },

    /// A phase within the current round has started.
    PhaseStarted { round: u32, phase: Phase },

    /// A model successfully produced a proposal.
    ModelProposed {
        model: ModelId,
        word_count: usize,
        preview: String,
    },

    /// A model failed to produce a proposal.
    ModelProposeFailed { model: ModelId, error: String },

    /// An evaluation was completed.
    EvaluationCompleted {
        reviewer: ModelId,
        reviewee: ModelId,
        score: f64,
        preview: String,
    },

    /// An evaluation failed.
    EvaluationFailed {
        reviewer: ModelId,
        reviewee: ModelId,
        error: String,
    },

    /// A model refined its answer.
    ModelRefined { model: ModelId, word_count: usize },

    /// A model failed to refine.
    ModelRefineFailed { model: ModelId, error: String },

    /// Convergence check result after the close phase.
    ConvergenceCheck {
        round: u32,
        converged: bool,
        winner: Option<ModelId>,
        best_score: f64,
        threshold: f64,
        stable_rounds: u32,
        required_stable: u32,
    },
}

/// Callback for progress events.
pub type ProgressFn = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

/// Truncate text to `max_chars` with an ellipsis suffix.
#[must_use]
pub fn preview(text: &str, max_chars: usize) -> String {
    // Operate on first line only to avoid newlines in display
    let first_line = text.lines().next().unwrap_or("");
    let trimmed: String = first_line.chars().take(max_chars).collect();
    if first_line.chars().count() > max_chars {
        format!("{trimmed}...")
    } else {
        trimmed
    }
}
