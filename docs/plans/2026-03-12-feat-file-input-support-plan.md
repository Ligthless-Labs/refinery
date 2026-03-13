---
title: "feat: Add file input support to CLI"
type: feat
date: 2026-03-12
brainstorm: docs/brainstorms/2026-03-11-file-input-support-brainstorm.md
completed: 2026-03-12
---

# feat: Add file input support to CLI

## Overview

Add a repeatable `--file <path>` flag to the `converge` CLI so users can pass one or more files alongside or instead of a text prompt. File contents are read at the CLI layer, wrapped in nonce-based XML tags with filenames, and assembled into the prompt string before passing to the engine. No changes to the engine or providers.

## Problem Statement / Motivation

Currently `converge` only accepts a text prompt (positional arg or stdin). Users working with code review, document analysis, or multi-file reasoning must manually cat/paste file contents into the prompt. This is cumbersome for real-world usage where the subject matter lives in files.

## Proposed Solution

CLI-layer only. Read files, wrap each in `<file-{nonce} path="...">...</file-{nonce}>` tags, assemble with any text prompt, and pass the combined string to `engine.run()`. The engine and providers remain unchanged — they only see a `&str`.

### User Flows

```sh
# Files + prompt
converge "review these for security issues" --file src/auth.rs --file src/crypto.rs -m claude,codex

# Files only (no text prompt)
converge --file src/main.rs -m claude,gemini

# Stdin + files
echo "analyze this code" | converge - --file src/context.rs -m claude,codex

# Existing flows unchanged
converge "question" -m claude,codex
echo "question" | converge - -m claude,codex
```

### Assembly Format

Text prompt first (if present), then file blocks separated by double newlines:

```
{text_prompt}

<file-a7f3c1 path="src/auth.rs">
...file contents (sanitized)...
</file-a7f3c1>

<file-a7f3c1 path="src/crypto.rs">
...file contents (sanitized)...
</file-a7f3c1>
```

When files-only (no text prompt), the file blocks are the entire user message.

## Technical Considerations

### Security: Nonce-based file tags

The codebase already uses nonce-based XML delimiters for `<answer-{nonce}>` and sanitizes injected tags (`prompts.rs`). File content is user-provided input that could contain `</file>` strings to break tag boundaries. Using `<file-{nonce}>` tags is consistent with the existing security posture.

- Reuse `prompts::nonce()` for generating the 6-hex-char random nonce
- Add `sanitize_for_file_tag(content, nonce)` alongside existing sanitizers
- Sanitize both `<file-{nonce}` and `</file-{nonce}>` in file contents
- Escape `"`, `<`, `>` in the `path` attribute value

### Size validation

- 1MB total limit measured on **raw inputs**: `prompt_text.len() + sum(file_bytes.len())`
- Check `fs::metadata().len()` **before** reading to avoid allocating memory for huge files
- If a single file exceeds 1MB, reject immediately without reading
- After reading all files, check aggregate total

### Error handling

- **Report all file errors before exiting** (no whack-a-mole)
- Exit code 4 for all validation errors (consistent with existing stdin/config errors)
- Error format: `eprintln!("Error: file '{path}': {reason}")` per file
- Distinct messages for: not found, permission denied, not a regular file, non-UTF-8, exceeds size limit

### Edge cases

- **Empty files**: included as `<file-{nonce} path="empty.rs"></file-{nonce}>`
- **Symlinks**: followed (standard `fs::read` behavior)
- **Non-regular files** (FIFOs, devices): rejected via `metadata().is_file()` check
- **Duplicate paths**: included as-is, no deduplication (user explicitly requested them)
- **`--dry-run` + `--file`**: validates files exist and are readable before showing estimate

## Acceptance Criteria

- [x] `--file <path>` flag is repeatable, accepts `PathBuf`
- [x] `prompt` positional arg becomes `Option<String>` — at least one of prompt or `--file` required
- [x] Files wrapped in `<file-{nonce} path="...">` tags with sanitization
- [x] 1MB total size limit (raw inputs) with pre-read metadata check
- [x] All file errors reported before exit (not fail-fast)
- [x] Existing flows (prompt-only, stdin-only) unchanged
- [x] `--dry-run` validates file existence
- [x] Non-UTF-8 files produce a clear error
- [x] Non-regular files (FIFOs, devices) rejected

## Implementation

### Phase 1: Core file reading and tag wrapping (`converge_core`)

Add to `crates/converge_core/src/prompts.rs`:

- `sanitize_for_file_tag(content: &str, nonce: &str) -> String` — escapes `<file-{nonce}` and `</file-{nonce}>` occurrences
- `wrap_file_content(content: &str, path: &str, nonce: &str) -> String` — wraps content in `<file-{nonce} path="...">...</file-{nonce}>`
- `escape_xml_attr(value: &str) -> String` — escapes `"`, `<`, `>`, `&` in attribute values
- `assemble_file_prompt(prompt: Option<&str>, files: &[(String, String)], nonce: &str) -> String` — assembles the final prompt string from optional text + tagged files

Tests in same file:
- `wrap_file_content_basic`
- `wrap_file_content_sanitizes_closing_tag`
- `escape_xml_attr_special_chars`
- `assemble_with_prompt_and_files`
- `assemble_files_only`
- `assemble_prompt_only`

### Phase 2: CLI argument and file reading (`converge_cli`)

Modify `crates/converge_cli/src/main.rs`:

- Change `prompt: String` to `prompt: Option<String>` in `Cli` struct
- Add `#[arg(long = "file", short = 'f')] files: Vec<PathBuf>`
- Add validation: at least one of `prompt` or `files` must be present
- Add `read_and_validate_files(paths: &[PathBuf], remaining_budget: usize) -> Result<Vec<(String, String)>, Vec<String>>`:
  - Check `metadata().is_file()` for each path
  - Check `metadata().len()` against remaining budget
  - Read file, validate UTF-8
  - Collect all errors, return them together
- Update prompt assembly block (lines ~134-151) to handle the new flows
- Update `--dry-run` to validate files before printing estimate
- Wire into `assemble_file_prompt()` from Phase 1

Tests (integration-level in main.rs or a separate test module):
- `cli_file_flag_parsed_correctly`
- `cli_no_prompt_no_files_errors`
- `cli_files_only_accepted`

### Phase 3: Documentation

- Update README.md usage section with `--file` examples
- Add `--file` to the options table

## Dependencies & Risks

- **No engine/provider changes** — blast radius is small
- **Risk**: nonce-based tags add ~20 chars overhead per file — negligible vs 1MB budget
- **Risk**: `prompt` becoming `Option<String>` could break any external scripts — mitigated by being a new feature on a pre-1.0 tool

## References

- Brainstorm: `docs/brainstorms/2026-03-11-file-input-support-brainstorm.md`
- Existing nonce pattern: `crates/converge_core/src/prompts.rs` (lines 8-33)
- Existing sanitizers: `sanitize_for_delimiter`, `sanitize_for_review_tag` in same file
- Existing stdin handling: `crates/converge_cli/src/main.rs` (lines 134-151)
- Security learnings: `docs/solutions/security-issues/prompt-injection-prevention-multi-model.md`
- CLI flag learnings: `docs/solutions/integration-issues/cli-provider-flags-and-sandboxing.md`
