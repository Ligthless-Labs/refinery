---
title: "feat: setsid process groups for child process cleanup"
priority: high
milestone: v0.1
deferred_from: PR #1 review
---

# setsid process groups for child process cleanup

## Problem

Currently child CLI processes (claude, codex, gemini) are killed via `kill_on_drop(true)` only. This sends SIGKILL to the direct child but does **not** kill grandchildren spawned by the CLI tools. If ConVerge is interrupted, orphaned grandchild processes can linger.

## Solution

Use `setsid` (Unix) to place each spawned CLI process in its own process group, then send `SIGTERM` / `SIGKILL` to the entire group on cancellation or timeout.

## Notes

- `unsafe_code = "deny"` (not `"forbid"`) was chosen specifically to allow `setsid` via `pre_exec`
- macOS and Linux both support `setsid`
- Windows is out of scope for v0

## References

- `crates/refinery_providers/src/process.rs` — `spawn_cli()` function
- Plan: `docs/plans/2026-02-10-feat-consensus-loop-engine-plan.md` (§ process management)
