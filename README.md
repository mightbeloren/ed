# ed

A Vim-first, keyboard-only text editor for Linux, built in Rust with GPUI.

The project is implemented one validated, committed phase at a time. **Phase 4 is complete.**

## Dependency baseline

- GPUI `0.2.2` (exact pin)
- Ropey `1.6.1` (exact pin)

All UI text uses JetBrains Mono. The font must be installed on the system.

## Run

```sh
cargo run -- path/to/file
```

Without a path, `ed` opens an unnamed scratch buffer. `Ctrl-s` atomically saves an opened file; `:w` arrives with Ex commands in Phase 8.

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
- `0`, `^`, `$`, `gg`, `G`, `%`
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
- Unnamed (`"`) and black-hole (`"_`) registers

Insert, Replace, change, and block-insert sessions are grouped into logical edit transactions in preparation for the undo tree.

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
| 5 | Next | Text objects and full register/paste behavior |
| 6 | Planned | Undo tree, macros, and dot-repeat |
| 7 | Planned | Search (`/`, `?`, `n`, `N`, `*`, `#`), marks, jump/change lists |
| 8 | Planned | Minibuffer and Ex commands (`:w`, `:q`, `:e`, `:s`, `:g`, ranges) |
| 9 | Planned | Dired directory buffers |
| 10 | Planned | Async shell runner, output buffers, path jump targets |
| 11 | Planned | Runtime Tree-sitter language plugins and themes |
| 12 | Planned | Vim compatibility hardening, Unicode/large-file testing, profiling |

Search and `:` commands are therefore expected to be unavailable at Phase 4.

See `AGENTS.md` for architecture constraints and `INSTRUCTIONS.md` for the development and testing workflow.
