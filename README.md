# ed

A Vim-first, keyboard-only text editor for Linux, built in Rust with GPUI.

The project is implemented one validated, committed phase at a time. **Phase 9 is complete.**

## Dependency baseline

- GPUI `0.2.2` (exact pin)
- Ropey `1.6.1` (exact pin)
- Regex `1.13.2` (exact pin)

All UI text uses JetBrains Mono. The font must be installed on the system.

## Run

```sh
cargo run -- path/to/file
```

Without a path, `ed` opens a Dired buffer for the current working directory. Passing a directory opens it in Dired; passing a file opens that file. `Ctrl-s` and `:w` atomically save opened files.

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

## UI

- JetBrains Mono throughout
- Neutral Zed-like dark palette
- Absolute cursor line number with relative surrounding line numbers
- Keyboard-only cursor and viewport navigation
- Status bar showing mode, file, modified state, and cursor position
- Bottom minibuffer row reserved for search, Ex, and shell commands

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
| 10 | Next | Async shell runner, output buffers, path jump targets |
| 11 | Planned | Runtime Tree-sitter language plugins and themes |
| 12 | Planned | Vim compatibility hardening, Unicode/large-file testing, profiling |

Async shell execution and navigable output buffers are the next implementation checkpoint in Phase 10.

See `AGENTS.md` for architecture constraints and `INSTRUCTIONS.md` for the development and testing workflow.
