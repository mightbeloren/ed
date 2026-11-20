# ed

A Vim-first, keyboard-only text editor for Linux, built in Rust with GPUI.

The project is being implemented one validated phase at a time. Phase 2 provides Normal and Insert modes, counted core motions, Vim insertion commands, and insert-session edit transactions.

## Dependency baseline

- GPUI `0.2.2` (exact pin)
- Ropey `1.6.1` (exact pin)

## Run

```sh
cargo run -- path/to/file
```

Without a path, `ed` opens an unnamed scratch buffer.

Implemented Normal-mode commands:

- `h`, `j`, `k`, `l`
- `0`, `^`, `$`, `gg`, `G`
- Counts such as `12j`, `3l`, `2$`, `5gg`, and `5G`
- `i`, `a`, `I`, `A`, `o`, and `O`

Insert mode supports printable text, Enter, Backspace, arrow keys, Home, End, and Escape. Each Insert session is grouped as one edit transaction. Arrow keys, Home, End, PageUp, and PageDown also remain available in Normal mode.

`Ctrl-s` atomically saves an opened file. Ex commands such as `:w` are planned for a later phase.

The application intentionally has no menus, toolbar, file picker, or mouse-driven controls. All UI text uses JetBrains Mono; the font must be installed on the system. The development profile optimizes GPUI and other dependencies to keep interactive debug builds responsive.

The cursor line shows its absolute 1-based line number. All other visible lines show their distance from the cursor line.
