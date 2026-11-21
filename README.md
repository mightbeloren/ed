# ed

[![CI](https://github.com/mightbeloren/ed/actions/workflows/ci.yml/badge.svg)](https://github.com/mightbeloren/ed/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/mightbeloren/ed?display_name=tag)](https://github.com/mightbeloren/ed/releases)

A Vim-first, keyboard-only text editor for Linux, built in Rust with GPUI.

The project is implemented one validated, committed phase at a time. **Phase 11 is complete.**

## Dependency baseline

- GPUI `0.2.2` (exact pin)
- Ropey `1.6.1` (exact pin)
- Regex `1.13.2` (exact pin)
- Async-channel `2.5.0` (exact pin)
- Tree-sitter `0.25.10` (exact pin)
- Libloading `0.8.9` (exact pin)
- Serde `1.0.229` and TOML `0.8.23` (exact pins)

All UI text uses JetBrains Mono. The font must be installed on the system.

## Run

```sh
cargo run -- path/to/file
```

Without a path, `ed` opens a Dired buffer for the current working directory. Passing a directory opens it in Dired; passing a file opens that file. `Ctrl-s` and `:w` atomically save opened files.

## Releases

Versioned Linux x86-64 archives and SHA-256 checksums are published on the [GitHub Releases page](https://github.com/mightbeloren/ed/releases). Releases use annotated tags matching the package version in `Cargo.toml`, such as `v0.1.0`.

Each pull request must pass formatting, tests, strict Clippy, whitespace checks, and a release build. The CI run also provides a temporary downloadable Linux binary. The protected `main` branch accepts changes only through pull requests with required checks; phase work belongs on a dedicated branch.

## Implemented Vim behavior

### Modes

- Normal
- Insert
- Visual character (`v`)
- Visual line (`V`)
- Visual block (`Ctrl-v`)
- Replace (`R`)
- Operator-pending

### Motions and editing

- `h`, `j`, `k`, `l`, `w`, `W`, `b`, `B`, `e`, `E`
- `0`, `^`, `$`, `_`, `+`, `-`, `gg`, `G`, `%`
- `f`, `F`, `t`, `T`, `;`, `,`
- Counts such as `12j`, `3w`, `2$`, `5gg`, and `5G`
- Insert commands `i`, `a`, `I`, `A`, `o`, and `O`
- Character deletion `x`, `X`
- Single-character replacement `r`
- Continuous Replace mode `R`

### Operators and registers

- Operators `d`, `c`, `y`, `>`, `<`, `=`, `g~`, `gu`, and `gU`
- Doubled operators such as `dd`, `cc`, `yy`, `>>`, `<<`, and `==`
- Generic compositions such as `dw`, `2d3w`, `c$`, `yG`, and `gUw`
- Operators over Visual character, line, and block selections
- Visual block insertion with `I` and `A`
- Text objects: `iw`, `aw`, `iW`, `aW`, quotes, delimiters, paragraphs, and tags
- Characterwise, linewise, and blockwise `p` / `P`
- Unnamed, named/append, numbered, yank (`0`), small-delete (`-`), black-hole (`_`), clipboard (`+`), and primary (`*`) registers
- GPUI system clipboard synchronization for `+` and `*`

Insert, Replace, change, and block-insert sessions are grouped into logical edit transactions.

### History and replay

- Branching undo tree with `u` and `Ctrl-r`
- Counted undo and redo
- Macro recording with `q{register}`
- Macro playback with `@{register}`, `@@`, and counts
- Dot-repeat with `.`, including counted repeat
- Saved/modified state follows the current undo-tree node

### Search and navigation history

- Forward and backward wrapping literal search with `/` and `?`
- Search prompt editing with Backspace, Enter, and Escape in the bottom row
- Counted `n` / `N` repetition
- Counted whole-keyword search with `*` / `#`
- Search while a Visual selection remains active
- Local lowercase marks with `m{mark}`, exact-position backtick jumps, and linewise `'{mark}` jumps
- Jump list traversal with counted `Ctrl-o` / `Ctrl-i`
- Change list traversal with counted `g;` / `g,`

### Minibuffer and Ex commands

- Reusable bottom minibuffer for search and `:` commands
- Prompt editing with Backspace, `Ctrl-u`, Enter, and Escape
- `:w`, `:w!`, `:wq`, and writing to a path
- `:q` / `:q!` with modified-buffer protection and return-to-Dired behavior
- `:e` / `:e!` with safe failure behavior and reload support
- Line jumps such as `:42`
- Ranges using line numbers, `.`, `$`, `%`, lowercase marks, and Visual `'<,'>` addresses
- Transactional `:s` with `g` and `i` flags
- `:g` / `:v` matching with count/print, delete, or nested substitution actions
- Ranged `:delete`

Ex matching currently uses Rust regex syntax rather than Vim's distinct regex dialect. Ranged writes and the broader Vim Ex grammar remain compatibility-hardening work.

### Dired

- Startup without a path opens the current directory
- Directory arguments and `:e directory` open Dired buffers
- Rope-backed, read-only, ls-style directory rows with directories sorted before files
- Normal Vim motions, counts, search, marks, and yanks remain available
- Enter opens the selected file or directory
- Backspace opens the parent directory
- Mutating Vim and Ex commands are rejected
- `:q`, `:q!`, and successful `:wq` return file buffers to their containing directory
- `:q` from Dired closes the editor window

Dired currently remains read-only. Phase 14 will add richer filesystem and directory summaries, `LS_COLORS`-compatible styling, marked operations, file/directory creation, copy/move/delete, and an explicit writable Dired mode with validated rename operations rather than saving arbitrary listing text.

### Shell commands and output buffers

- `:!cmd` runs asynchronously through `/bin/sh -c`, keeps the current buffer visible, discards stdout/stderr, and reports only its final exit status
- `:run cmd` immediately opens a read-only PTY-backed transcript buffer and streams output while the process runs
- `:run` output begins with `$ cmd`, preserving the exact command entered
- Tab completes the current filesystem argument for both forms, such as `:!cat src/fi<Tab>` → `:!cat src/file.txt `
- Leaving a running `:run` output buffer prevents chunks or late completion from pulling focus back to that buffer
- The working directory follows the current Dired directory, output-buffer directory, or file directory
- PTY output is normalized into readable transcript text in a read-only output buffer
- `:run` exit status and success/failure appear in the status bar
- Enter opens an existing `path`, `path:line`, or `path:line:column` target on the current output line
- Relative targets resolve against the command's working directory
- `:q` and `:q!` from output, or from a target opened through output, return to Dired at that working directory
- Modified file buffers must be written or discarded before `:run` replaces them; background `:!cmd` is allowed without writing

`:run` uses Linux util-linux `/usr/bin/script` to allocate a PTY, so programs detect a terminal and flush terminal-oriented output in real time. ANSI control sequences are removed and carriage-return redraws become readable transcript lines. This is a PTY-backed command transcript, not a fully interactive terminal emulator: keyboard input is not forwarded to the child and terminal colors/cursor positioning are not rendered. File completion and target parsing currently focus on whitespace-delimited shell/compiler formats; quoting, escaped spaces, and paths containing spaces are not comprehensively supported.

Phase 15 will replace `:!cmd`, `:run`, and transcript buffers with one persistent bottom terminal panel toggled by `Ctrl-Backtick`. The panel will own a direct PTY, remain alive while hidden, accept interactive input, and interpret terminal ANSI control sequences. Program-supplied 16-color, 256-color, and truecolor output will be preserved; the neutral dark palette supplies the default terminal foreground, background, cursor, and fallback ANSI colors.

## Runtime syntax plugins

At startup, `ed` scans `${XDG_CONFIG_HOME:-$HOME/.config}/ed/languages/`. Each immediate child directory is one language plugin:

```text
~/.config/ed/
├── theme.toml
└── languages/<name>/
    ├── language.toml
    ├── grammar.so
    └── highlights.scm
```

A plugin manifest uses this schema:

```toml
name = "rust"
extensions = ["rs"]
grammar = "grammar.so"
highlights = "highlights.scm"
symbol = "tree_sitter_rust" # optional
```

`extensions` omit the leading dot and select a plugin from the opened file's extension. Plugin paths must be relative to the language folder. If `symbol` is omitted, it defaults to `tree_sitter_<name>`, with `-` and `.` converted to `_`. The shared library must export that generated Tree-sitter C function, and its grammar must be compatible with the supplied `highlights.scm` query.

The optional `~/.config/ed/theme.toml` maps Tree-sitter capture names to `#RRGGBB` colors:

```toml
[captures]
keyword = "#c586c0"
string = "#ce9178"
"function.call" = "#dcdcaa"
```

Capture lookup falls back through dotted parents, so `function.call` uses `function` when no exact mapping exists. A built-in neutral dark theme is used when no theme exists or the theme is invalid. Broken language plugins are reported without preventing the editor from starting. See `examples/language-plugin/` and `examples/theme.toml` for templates.

Language plugins and the theme are loaded only when `ed` starts, so changes require a restart. Parsing currently reparses the full buffer after each edit; incremental parsing and large-file profiling are Phase 16 work.

**Security and compatibility:** a grammar `.so` is trusted native code, not sandboxed data. Loading one can execute arbitrary code or crash the process. `ed` validates the Tree-sitter language ABI version, but plugin authors are still responsible for compiling a compatible grammar and matching query. Install plugins only from sources you trust.

## Planned file-buffer navigation

Phase 12 introduces open-file buffer management. Every ordinary file buffer retains its own contents, cursor, viewport, Vim state, undo history, syntax state, and modified state.

- `Ctrl-Tab` selects the next open file buffer and wraps through all open files.
- `Ctrl-Shift-Tab` selects the previous open file buffer and wraps in the opposite direction.
- Dired, scratch, shell-output, and other special buffers are excluded from the `Ctrl-Tab` cycle.
- Opening an already-open canonical file path activates its existing buffer instead of creating a duplicate.
- `:q` protects modified files, closes the active file buffer when allowed, and activates another open file or returns to the existing Dired lifecycle when no file remains.
- Phase 12 does not add `Ctrl-p`, fuzzy matching, rich Dired, or terminal-panel behavior.

## Planned recursive file finder

Phase 13 binds global `Ctrl-p` to a keyboard-only file finder built on the bottom minibuffer. It does not open Dired or a graphical file-picker dialog.

- Typing updates fuzzy matches against paths recursively discovered below the workspace root, including nested directories.
- The workspace root is the startup directory, an explicitly opened directory, the containing Git root when one is available, or otherwise the opened file's parent directory.
- `.git` internals and ignored paths are excluded; matching and indexing run outside GPUI rendering.
- A compact result list appears directly above the minibuffer while the prompt is active.
- Up/Down and `Ctrl-n`/`Ctrl-p` move through matches, Enter opens or reuses the selected file buffer, and Escape cancels and restores editor focus.
- Results use deterministic fuzzy scoring with path and recent-use tie breaking.

## Planned rich Dired

Phase 14 keeps Dired as a structured Rope-backed buffer while adding an Emacs-like header and metadata columns: directory path, entry counts, apparent size, filesystem availability, permissions, link count, owner, group, size, modification time, filename, and symlink target.

Filename colors will interpret the standard `LS_COLORS` environment variable, including terminal 16-color, 256-color, and truecolor entries, so Dired matches the user's terminal `ls` colors without executing or parsing `ls` output. Missing or invalid `LS_COLORS` entries fall back to the existing neutral dark palette. Metadata, marks, selections, diagnostics, and the cursor retain editor-owned styles.

Dired operations will include marked deletion, create file/directory, copy, move/rename, refresh, and writable-Dired filename editing. Filesystem changes must be represented as validated operations with collision checks and confirmation for destructive or recursive actions.

## UI

- JetBrains Mono throughout
- Neutral Zed-like dark palette
- Absolute cursor line number with relative surrounding line numbers
- Keyboard-only cursor and viewport navigation
- Status bar showing mode, file, modified state, and cursor position
- Bottom minibuffer row with a visible caret for search, Ex, and shell commands

There are intentionally no menus, toolbar, file-picker dialogs, or mouse-driven editing controls.

## Roadmap

| Phase | Status | Scope |
|---|---|---|
| 0 | Complete | GPUI shell, focus, normalized keyboard input |
| 1 | Complete | Rope buffer, file open/save, viewport, cursor, relative line numbers |
| 2 | Complete | Normal/Insert modes, core motions, counts, insert transactions |
| 3 | Complete | Generic operators and motions, registers, operator-pending mode |
| 4 | **Complete** | Visual char/line/block, selection operators, block edits, Replace mode, `x`/`r` |
| 5 | **Complete** | Text objects, full register families, clipboard bridge, character/line/block paste |
| 6 | **Complete** | Branching undo tree, macros, and dot-repeat |
| 7 | **Complete** | Search (`/`, `?`, `n`, `N`, `*`, `#`), local marks, jump/change lists |
| 8 | **Complete** | Reusable minibuffer and Ex commands (`:w`, `:q`, `:e`, `:s`, `:g`, ranges) |
| 9 | **Complete** | Dired directory buffers, startup browsing, file-return quit behavior |
| 10 | **Complete** | Async shell runner, read-only output buffers, path jump targets |
| 11 | **Complete** | Runtime Tree-sitter language plugins and capture-based themes |
| 12 | Next | Ordinary file-buffer lifecycle and complete `Ctrl-Tab` / `Ctrl-Shift-Tab` navigation; exclude special buffers |
| 13 | Planned | Recursive workspace file finder through the minibuffer with global `Ctrl-p` |
| 14 | Planned | Rich `LS_COLORS` Dired, metadata, marks, create/copy/move/delete, writable Dired |
| 15 | Planned | Persistent `Ctrl-Backtick` terminal panel, direct PTY, ANSI emulation; remove `:!` and `:run` |
| 16 | Planned | Vim compatibility hardening, incremental syntax, Unicode/large-file testing, profiling |

The next implementation checkpoint is Phase 12: ordinary file-buffer lifecycle and complete keyboard navigation. Each later phase remains a separate validated commit.

See `AGENTS.md` for architecture constraints and `INSTRUCTIONS.md` for the development and testing workflow.
