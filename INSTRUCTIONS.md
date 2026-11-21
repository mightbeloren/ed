# Development and Testing Instructions

This document is the continuation checklist for future implementation sessions.

## Start of a session

1. Read `AGENTS.md` and `README.md`.
2. Inspect the latest commits and working tree:

   ```sh
   git --no-optional-locks status --short --branch
   git --no-pager log -5 --oneline
   ```

3. Confirm which single roadmap phase is next.
4. Read the affected modules before editing.
5. Keep the phase scope explicit; do not pull later features forward unless a prerequisite requires it.

## Core execution path

Keyboard behavior should follow this path:

```text
GPUI Keystroke
→ input::Key
→ Vim parser / mode state
→ semantic vim::Command
→ Editor execution
→ Buffer transaction
→ GPUI render
```

Do not place Vim semantics directly in GPUI event handlers.

Completed Ex commands follow a parallel core-only path:

```text
normalized input::Key
→ minibuffer text
→ typed ex::ExCommand
→ Editor execution
→ Buffer transaction or editor lifecycle request
```

Parse and validate an entire Ex command before mutating the buffer. Keep GPUI-specific close behavior behind `EditorRequest` rather than importing GPUI into editor core logic.

Dired is a read-only Rope-backed buffer with filesystem metadata held separately in `dired::DiredState`. Normal Vim navigation still flows through `VimEngine`; only Dired-specific Enter and Backspace keys are intercepted before Vim parsing. Keep filesystem loading in the Dired/editor core rather than UI rendering.

Shell execution follows an asynchronous request/completion path:

```text
normalized input::Key
→ minibuffer `:!cmd` or `:run cmd`
→ typed ex::ExCommand::Shell
→ EditorRequest::RunShell
→ GPUI background executor
├─ `:!` → discarded output → completion status message
└─ `:run` → PTY chunks → read-only output::OutputState
→ GPUI render
```

Keep shell process execution out of the UI thread and keep GPUI imports out of `editor`, `shell`, and `output`. `:!cmd` runs `/bin/sh -c` in the background with stdout/stderr redirected to null and reports only completion status; it must never replace the active buffer. `:run cmd` opens a request-identified output buffer immediately and uses util-linux `/usr/bin/script -qefc` to allocate a PTY, forwarding normalized transcript chunks through `async-channel`. Apply chunks and completion only while the matching running output remains active, so stale tasks cannot pull the user back after navigation. PTY normalization strips ANSI control sequences and turns carriage-return redraws into transcript lines; this is not a fully interactive terminal emulator. Output buffers begin with `$ cmd`. Tab completion for both forms resolves the current whitespace-delimited filesystem argument relative to the command working directory; it does not complete executable names. Output jump targets use the same working directory. Before `:run` replaces the active buffer, preserve modified user data; background `:!` is safe from modified buffers.

Dired listing text remains read-only. If writable Dired is added later, implement it as an explicit `wdired`-style filesystem-operation mode with validated rename operations, not by treating arbitrary listing edits as a normal buffer save.

Runtime syntax highlighting follows this core-only path:

```text
${XDG_CONFIG_HOME:-$HOME/.config}/ed/languages/*/language.toml
→ plugin manifest validation
→ trusted grammar `.so` load and Tree-sitter ABI validation
→ highlights.scm query compilation
→ file-extension language selection
→ source parse and capture query
→ byte-range to per-line Unicode character-range conversion
→ GPUI applies precomputed colors while rendering
```

Keep `plugin` and `syntax` independent from GPUI, and never load files or parse source during `Render`. Dynamic grammar loading is the project's narrow unsafe boundary: grammar libraries are trusted native code, symbol pointers must be checked, and the `Library` must outlive every `Language` and `Query` derived from it. Preserve the `LanguagePlugin` field/drop order so the query and language drop before the library unloads. Tree-sitter reports UTF-8 byte offsets; never pass them directly to Rope character offsets or UI columns. Invalid plugins and themes must produce actionable errors while allowing startup with the built-in theme and remaining valid plugins.

Phase 11 intentionally reparses the complete buffer after each revision. Incremental `Tree::edit`, background parsing, cancellation, and large-file performance work belong to Phase 16. Tests must construct an in-memory/static registry and must not read the developer's real `~/.config/ed` directory.

## Planned Phase 12–16 architecture

Phase 12 introduces ordinary file-buffer management with stable identities and canonical-path reuse. Each open file retains its cursor, viewport, Vim state, undo history, syntax state, and modified state. `Ctrl-Tab` and `Ctrl-Shift-Tab` must traverse every open file in opposite directions and wrap. Dired, scratch, output, and other special buffers remain outside this navigation cycle. Preserve modified-buffer protection and the existing special-buffer lifecycle. Do not implement `Ctrl-p` in Phase 12.

Phase 13 adds a global `Ctrl-p` recursive workspace file finder through the existing minibuffer system. Determine the workspace root from startup context or a containing Git root through filesystem traversal. Index nested files outside GPUI rendering, exclude `.git` internals and ignored paths, and expose deterministic fuzzy-ranked matches to a compact result list above the minibuffer. Up/Down and `Ctrl-n`/`Ctrl-p` select matches, Enter opens or reuses the canonical file buffer, and Escape cancels. Keep matching, indexing, and result state independent from GPUI; the UI only renders state and routes focus/input.

Phase 14 expands Dired metadata and filesystem operations. Read directory entries and metadata through filesystem APIs, never by parsing `ls`. Resolve filename styling through `LS_COLORS`, including indexed and truecolor values, with the neutral palette as fallback. Keep metadata and row spans separate from listing text. Writable Dired edits only filename fields and turns differences into validated rename operations. Create, copy, move, and delete operations require collision checks; recursive/destructive actions require confirmation. Test all filesystem behavior in unique temporary directories, including symlinks and partial failures.

Phase 15 removes `:!cmd`, `:run`, transcript output buffers, and their completion path after the replacement is ready. A terminal session is persistent UI state, not an editor buffer: `Ctrl-Backtick` toggles visibility/focus while the child and scrollback remain alive. Own the PTY directly, resize it with the panel, process I/O off the UI thread, and feed output into a terminal state machine. Render the child's actual ANSI 16-color, 256-color, and truecolor attributes; use the neutral palette only for terminal defaults/fallbacks. Never parse terminal display rows as shell output. Keep PTY/process code independent from GPUI and expose renderable terminal cells/state to `ui`.

Phase 16 performs Vim compatibility hardening, incremental syntax parsing, Unicode and large-file tests, profiling, and cleanup. Do not pull these scopes into earlier phase commits unless an earlier phase requires a narrowly documented prerequisite.

## Test strategy

Every new Vim feature needs tests at two levels when practical.

### Parser tests

Add table-like parser tests under `src/vim/` for:

- Partial sequences that should not emit a command yet
- Completed semantic commands
- Counts and multiplied operator/motion counts
- Escape/cancellation behavior
- Register prefixes
- Mode transitions

Parser tests should assert semantic command values, not resulting text.

### Editor behavior tests

Add tests under `src/editor.rs` for:

- Resulting buffer text
- Final cursor position
- Final mode
- Register contents and selection shape
- Transaction count/grouping
- Viewport movement when relevant

Exercise behavior through normalized key input rather than calling private edit helpers directly. Existing `type_keys`, `named`, and `control_named` test helpers demonstrate the preferred pattern.

### Buffer tests

Add tests under `src/buffer/` for:

- Unicode character-offset conversion
- Line-ending behavior
- Reversible edit data
- Transaction boundaries
- Save behavior and modified state

Use unique temporary directories and clean them up in the test.

### Syntax/plugin tests

Cover manifest validation, broken-plugin isolation, theme parsing and fallback, capture-parent fallback, language selection, query execution, and multibyte UTF-8 byte-to-character conversion. Static grammar crates are acceptable as dev-dependencies; production language support must remain runtime-loaded.

### UI tests

Keep rendering logic thin. Test pure helpers such as line-number and selection calculations. Do not use screenshot tests unless a stable GPUI harness is introduced.

## Vim compatibility checklist

For each motion or operator, cover:

- Beginning and end of line/file
- Empty lines
- Counts
- Forward and backward direction
- Unicode text
- Operator composition
- Characterwise versus linewise versus blockwise shape
- Register effects
- Cursor placement after the command

For insertion/replacement, verify that one logical Vim change is one buffer transaction.

## GPUI guidance

- `Entity<T>` is GPUI-managed state; keep most editor-core types as plain Rust values.
- `Context<T>` is used for short mutations, notifications, subscriptions, and task spawning.
- `Render` describes the current visual tree and must not perform file I/O or parse Vim commands.
- Focus is controlled by `FocusHandle`; editor focus is the default, and the future minibuffer will take and restore focus.
- Use GPUI actions/keybindings only for stable application routing. Multi-key Vim grammar belongs in the Vim parser.
- Keep version-sensitive custom rendering isolated under `src/ui/`.

## Completion checklist

Before committing a phase:

- [ ] Required behavior is implemented.
- [ ] Regression and edge-case tests were added.
- [ ] `README.md` roadmap/status is updated.
- [ ] `AGENTS.md` is updated if architecture or phase state changed.
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo test --all-targets` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] Editor diagnostics have no project errors or warnings.
- [ ] `git diff --check` passes.
- [ ] The phase is committed with a focused message.
- [ ] The working tree is clean afterward.
