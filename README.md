# ed

A Vim-first, keyboard-only text editor for Linux, built in Rust with GPUI.

The project is being implemented one validated phase at a time. The current Phase 0 spike provides the GPUI application shell, keyboard focus, and normalized key input.

## Dependency baseline

- GPUI `0.2.2` (exact pin)
- Ropey `1.6.1` (exact pin)

## Run

```sh
cargo run
```

The application intentionally has no menus, toolbar, file picker, or mouse-driven controls.

Relative line numbers will be added with the Rope-backed editor viewport in Phase 1. The cursor line will show its absolute 1-based line number; all other visible lines will show their distance from the cursor line.
