# AGENTS.md

## Project

`ed` is a Linux-only, Vim-first, keyboard-only text editor written in Rust with GPUI.

There are no menus, toolbars, file-picker dialogs, or mouse-driven editing controls. Files are browsed through Dired buffers, and commands are entered through the bottom minibuffer.

## Dependency baseline

Keep these exact pins unless a dedicated migration is requested:

- `gpui = "=0.2.2"`
- `ropey = "=1.6.1"`

Commit `Cargo.lock`. GPUI `0.2.2` requires the local `vendor/xattr` compatibility patch with current `libc` releases.

All UI text uses the system-installed `JetBrains Mono` font. The palette is a neutral Zed-like dark gray palette.

## Architecture invariants

- Core editing logic must not depend on GPUI.
- GPUI translates keyboard input, owns focus, renders state, and requests redraws.
- Buffer mutations go through `Buffer` edit methods and transactions; do not mutate Rope storage from UI code.
- Vim keys are normalized into `input::Key`, parsed into semantic `vim::Command` values, then executed by `Editor`.
- Operator-plus-motion behavior must remain generic. Do not add separate implementations for combinations such as `dw`, `db`, or `d$`.
- Motions resolve through `vim::target`; operators consume `ResolvedTarget` values.
- Keep buffer, Vim, Ex parsing, syntax, Dired, minibuffer/shell, UI, and plugin loading as separate modules.
- Runtime language discovery and native-library loading belong in `plugin`; parsing, capture-to-color resolution, and byte-to-character span conversion belong in `syntax`. Neither module depends on GPUI.
- Retain each grammar library until all `tree_sitter::Query` and `tree_sitter::Language` values derived from it have dropped. Keep the trusted-native-code unsafe boundary small and documented.
- Ex input is parsed into typed `ex::ExCommand` values before execution; parse and validate before mutating the buffer.
- Treat byte offsets, Rope character offsets, buffer line/character positions, and display columns as distinct concepts.
- The current editor intentionally has no soft wrapping.
- The cursor line uses its absolute 1-based line number; other visible lines use relative numbers.
- Planned Dired colors interpret `LS_COLORS` semantically; do not execute or parse `ls` output to determine entries or styling.
- Planned terminal rendering interprets child-process ANSI state from a directly owned PTY. Keep terminal process/state management separate from editor buffers and GPUI rendering.
- Open-file navigation and special-buffer lifecycle are distinct: `Ctrl-Tab` / `Ctrl-Shift-Tab` cycle only ordinary file buffers.
- Planned global `Ctrl-p` opens a recursive workspace file finder in the minibuffer; it does not open Dired or a graphical file picker.

## Current phase

Phase 11 is complete:

- Rope-backed file buffer, atomic save, viewport, cursor, and relative line numbers
- Normal and Insert modes
- Generic motions, operators, counts, and operator-pending mode
- Unnamed and black-hole registers
- Visual character, line, and block modes
- Selection rendering and operators
- Block insertion/deletion
- Replace mode, `r`, `x`, and `X`
- Word, quote, delimiter, paragraph, and tag text objects
- Named, numbered, yank, small-delete, black-hole, clipboard, and primary registers
- Characterwise, linewise, and blockwise paste
- GPUI clipboard synchronization
- Branching undo tree with counted undo and redo
- Macro recording, counted playback, and `@@`
- Dot-repeat for transactional changes
- Saved/modified state tied to undo-tree history
- Wrapping literal search through the bottom prompt, including `n`, `N`, `*`, and `#`
- Local lowercase marks with exact and linewise jumps
- Counted jump-list and change-list traversal
- Reusable search/Ex minibuffer
- Typed Ex parser with line, mark, whole-buffer, and Visual ranges
- Safe `:w`, `:q`, and `:e` file lifecycle commands
- Transactional regex `:s`, `:g`, `:v`, and ranged delete commands
- Read-only Rope-backed Dired buffers with ls-style rows
- Current-directory startup, keyboard entry opening, and parent navigation
- File quit commands return to Dired; quitting Dired closes the window
- Background-only `:!cmd` execution with discarded output and exit-status messages
- Immediate PTY-backed transcript buffers through `:run cmd`
- Shell file-argument completion with Tab for `:!cmd` and `:run cmd`
- Read-only shell output buffers showing the entered command and exit status
- Existing `path`, `path:line`, and `path:line:column` output jump targets
- Output and opened-target quit behavior returns to the command working-directory Dired
- Modified-buffer protection before shell launch and when asynchronous completion arrives
- Startup discovery of runtime Tree-sitter language folders and extension-based selection
- Dynamically loaded grammar `.so` files with isolated error reporting and ABI validation
- Runtime `highlights.scm` compilation and capture-based `theme.toml` colors
- Dotted capture fallback and a built-in neutral dark theme
- Unicode-safe conversion from Tree-sitter byte spans to UI character spans
- Syntax colors in Normal/Insert/Visual rendering and the active language in the status bar

Phase 12 is next: manage ordinary open file buffers and provide complete wrapping navigation with `Ctrl-Tab` / `Ctrl-Shift-Tab`. Dired, scratch, output, and other special buffers must not appear in that cycle. Phase 12 does not implement `Ctrl-p`; Phase 13 adds a recursive workspace file finder through the minibuffer. See `README.md` for Phases 13–16 and the complete roadmap.

## Phase discipline

Implement one roadmap phase at a time. Each phase should:

1. Preserve existing behavior.
2. Add focused unit tests for parsing and editor-visible behavior.
3. Update `README.md` and relevant instructions.
4. Pass formatting, tests, Clippy, and project diagnostics.
5. Be committed as one clear Git checkpoint.

Do not begin the next phase in the same commit.

## Validation

Before every phase commit, run:

```sh
cargo fmt --all
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
git diff --check
```

Also refresh editor diagnostics. Do not claim the GUI was manually tested unless it was actually launched and exercised.

The transitive `proc-macro-error2` future-compatibility notice comes from GPUI's dependency graph and is currently expected.

## Git

- Main branch: `main`
- Do not rewrite or remove user work.
- Do not commit unrelated changes.
- Phase commit convention: `phase N: short description`
