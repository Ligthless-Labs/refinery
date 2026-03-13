# File Input Support

**Date:** 2026-03-11
**Status:** Ready for planning

## What We're Building

Add the ability to pass one or more files to `converge` alongside or instead of a text prompt. File contents are read at the CLI layer, tagged with their filenames, and injected into the user message. The engine and providers remain unchanged — they continue to receive a plain string.

## Why This Approach

The engine already operates on `&str` prompts. File support is purely a CLI-layer concern: read files, format them, pass the assembled string downstream. This avoids any changes to the core engine, provider adapters, or message types.

## Key Decisions

1. **Both modes supported** — files can be the entire prompt (no text prompt needed) or serve as context alongside a text prompt.
2. **Tagged with filenames** — each file wrapped in `<file path="...">...</file>` tags so models know which content came from where.
3. **Repeated `--file` flags** — `converge "review this" --file src/main.rs --file lib.rs --models claude,codex`. Shell glob expansion works naturally.
4. **1MB total size limit** — sum of all file contents (and prompt text) must stay under 1MB. Note: experiment with per-provider limits later.
5. **Files-only mode** — when no prompt text is given but files are provided, the tagged file contents become the user message directly (no auto-generated wrapper).

## Scope

### In scope
- `--file <path>` CLI argument (repeatable)
- File reading with error handling (not found, unreadable, size exceeded)
- XML-tagged formatting of file contents
- Files-only mode (prompt becomes optional when files are provided)
- Stdin (`-`) remains supported and composable with `--file`

### Out of scope
- Binary file support (images, PDFs)
- Per-provider context window awareness
- Streaming file reads for very large files
- Glob expansion in the tool itself (shell handles this)

## Open Questions

- Should the 1MB limit be configurable via a CLI flag?
- How should file encoding errors (non-UTF-8) be handled — skip with warning or hard error?
