# ed

A Vim-first, keyboard-only text editor for Linux, built in Rust with GPUI.

The project is being implemented one validated phase at a time. Phase 1 provides a Rope-backed file buffer, cursor and viewport navigation, visible-line rendering, atomic save, and hybrid relative line numbers.

## Dependency baseline

- GPUI `0.2.2` (exact pin)
- Ropey `1.6.1` (exact pin)

## Run

```sh
cargo run -- path/to/file
```

Without a path, `ed` opens an unnamed scratch buffer.

Phase 1 uses temporary navigation bindings until the Vim engine replaces them in Phase 2:

- Arrow keys: move the cursor
- `Home` / `End`: move to line boundaries
- `PageUp` / `PageDown`: move by one viewport
- `Ctrl-s`: atomically save an opened file

The application intentionally has no menus, toolbar, file picker, or mouse-driven controls. All UI text uses JetBrains Mono; the font must be installed on the system.

The cursor line shows its absolute 1-based line number. All other visible lines show their distance from the cursor line.
