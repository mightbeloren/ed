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
→ minibuffer `:!cmd`
→ typed ex::ExCommand::Shell
→ EditorRequest::RunShell
→ GPUI background executor
→ shell::ShellCompletion
→ read-only output::OutputState
→ GPUI render
```

Keep `/bin/sh -c` process execution out of the UI thread and keep GPUI imports out of `editor`, `shell`, and `output`. `:!cmd` keeps the active buffer visible and captures output at completion; `:run cmd` opens a request-identified output buffer immediately and forwards stdout/stderr chunks through `async-channel`. Apply chunks and completion only while the matching running output remains active, so stale tasks cannot pull the user back after navigation. Output buffers begin with `$ cmd`. Tab completion for both forms resolves the current whitespace-delimited filesystem argument relative to the command working directory; it does not complete executable names. Output jump targets use the same working directory. Before replacing the active buffer with completed output, preserve modified user data; tests must cover launch-time and completion-time modification races. Delayed `:!` output places stdout before stderr; streamed `:run` output uses observed chunk arrival order.

Dired listing text remains read-only. If writable Dired is added later, implement it as an explicit `wdired`-style filesystem-operation mode with validated rename operations, not by treating arbitrary listing edits as a normal buffer save.

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
