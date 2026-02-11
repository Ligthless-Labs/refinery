# ConVerge Refinery

Rust Library and CLI for iteratively reaching consensus across multiple AI models

## Tools

The crate relies on Cargo for building and dependency management.

The CLI relies on Bazel for building and dependency management.

## Environment Setup

Copy `.env.example` to `.env` and fill in your credentials:

```bash
cp .env.example .env
# Edit .env with your actual credentials
```

## Quick Commands

- `cargo test --workspace` - Run all tests
- `cargo build --workspace` - Build all crates
- `bazel build //...` - Build with Bazel

### Running the Linear Agent

```bash
# Load environment variables
source .env

# Run the agent (polls Linear for Todo issues, runs brainstorms)
cargo run -p linear_agent --release -- --team-id $LINEAR_TEAM_ID --poll-interval 60
```

## Crate Structure

[TK]

## Key Directories

- `todos/` - Pending work items
- `docs/plans/` - Implementation plans
- `docs/solutions/` - Documented learnings

## Process

Use sub-agents for each task. Parallelize tasks that can be parallelized.
When picking up a milestone from a roadmap or general plan, if the milestone does not have a dedicated plan, a dedicated plan should be created.
When a plan is deepened, the plan should be updated to reflect it (eg **Enhanced:** 2026-01-29 (via `/deepen-plan`) in the header).
When a plan is reviewed, the plan should be updated to reflect it (eg **Reviewed:** 2026-01-29 (via `/$SKILL / $COMMAND`) in the header).
When a plan is completed, the plan should be updated to reflect it (eg **Completed:** 2026-01-29 in the header).
When a gap is discovered during execution, the plan should be updated with an addendum (eg **Addendum:** 2026-02-07 — description of what was added and why).

## Conventions

Favour ast-grep over grep when researching and operating over code.
Commit early and eagerly. Favour atomic commits.
Use a TDD approach.
Run checks and gates (tests, linting,...) regularly to tighten your feedback loop.
