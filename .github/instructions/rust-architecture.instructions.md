---
applyTo: "src/**/*.rs"
---

# Rust and Architecture Instructions

## Module boundaries

- `buffer`: Rope storage, positions, edits, transactions, and persistence.
- `vim`: key grammar, modes, semantic commands, motions, targets, operators, and registers.
- `editor`: executes semantic commands and coordinates buffer/view state.
- `ui`: GPUI layout and rendering only.
- Future syntax, Dired, minibuffer/shell, and plugin loading must remain separate modules.

Core modules must not import GPUI. GPUI-specific types belong at the application/UI boundary.

## Vim command flow

Follow this pipeline:

```text
GPUI Keystroke → input::Key → vim parser → vim::Command → Editor → Buffer transaction
```

- Parsers emit intent; they do not mutate buffers.
- Motions resolve through shared target logic.
- Operators consume `ResolvedTarget` values.
- Visual operations should reuse the same operator semantics where possible.
- Counts, registers, and mode transitions belong in parser/engine state rather than UI handlers.

## Buffer and position safety

- Never mutate Rope storage outside `Buffer` methods.
- Every logical text change must have an explicit transaction boundary.
- Use `CharOffset` and `BufferPosition`; do not pass unlabelled `usize` offsets across layers.
- Do not confuse UTF-8 bytes, Unicode scalar values, grapheme/display columns, and lines.
- Preserve CRLF handling and Unicode behavior when editing position code.

## Rust style

- Prefer small enums and explicit state machines over stringly typed commands.
- Prefer exhaustive matches for modes, motions, operators, and selection shapes.
- Avoid `unsafe`; dynamic grammar loading is the only planned area likely to need it, and it must be isolated.
- Do not silence Clippy with broad `allow` attributes. Fix the cause or narrowly document a justified exception.
- Do not add dependencies when the standard library or an existing dependency is sufficient.
- Keep public APIs minimal and driven by actual module boundaries.

## GPUI rendering

- Rendering must not perform file I/O, shell execution, or Vim parsing.
- Render visible lines only.
- Preserve keyboard focus through `FocusHandle`.
- All text inherits JetBrains Mono from the UI root.
- Preserve absolute numbering on the cursor line and relative numbering elsewhere.
