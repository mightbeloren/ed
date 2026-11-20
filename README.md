# ed

A Vim-first, keyboard-only text editor for Linux, built in Rust with GPUI.

The project is being implemented one validated phase at a time. Phase 3 provides generic operator-plus-motion composition, word and character-find motions, registers, and line/case operators.

## Dependency baseline

- GPUI `0.2.2` (exact pin)
- Ropey `1.6.1` (exact pin)

## Run

```sh
cargo run -- path/to/file
```

Without a path, `ed` opens an unnamed scratch buffer.

Implemented Normal-mode commands:

- `h`, `j`, `k`, `l`, `w`, `W`, `b`, `B`, `e`, `E`
- `0`, `^`, `$`, `gg`, `G`, `%`
- `f`, `F`, `t`, `T`, `;`, `,`
- Counts such as `12j`, `3w`, `2$`, `5gg`, and `5G`
- Operators `d`, `c`, `y`, `>`, `<`, `=`, `g~`, `gu`, and `gU`
- Doubled line operators such as `dd`, `cc`, `yy`, `>>`, `<<`, and `==`
- Composed commands such as `dw`, `d2w`, `2dw`, `2d3w`, `c$`, `yG`, and `gUw`
- Unnamed (`\"`) and black-hole (`\"_`) registers
- `i`, `a`, `I`, `A`, `o`, and `O`

Insert mode supports printable text, Enter, Backspace, arrow keys, Home, End, and Escape. Each Insert session is grouped as one edit transaction. A change command such as `cw` keeps the same transaction open through the following Insert session. Arrow keys, Home, End, PageUp, and PageDown also remain available in Normal mode.

`Ctrl-s` atomically saves an opened file. Ex commands such as `:w` are planned for a later phase.

The application intentionally has no menus, toolbar, file picker, or mouse-driven controls. All UI text uses JetBrains Mono; the font must be installed on the system. The development profile optimizes GPUI and other dependencies to keep interactive debug builds responsive.

The cursor line shows its absolute 1-based line number. All other visible lines show their distance from the cursor line.
