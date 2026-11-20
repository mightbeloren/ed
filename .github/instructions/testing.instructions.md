---
applyTo: "**/*.rs"
---

# Test-Case Instructions

Every behavior change requires focused regression tests. Prefer deterministic unit tests that execute normalized keys and inspect semantic or editor-visible results.

## Parser tests

For changes under `src/vim/`, test:

- Partial key sequences emit no command until complete.
- Completed sequences produce the expected semantic `vim::Command`.
- Counts work before motions and operators.
- Operator and motion counts multiply correctly.
- Register prefixes survive until command completion.
- Escape cancels pending state.
- Invalid sequences reset safely.
- Mode transitions are explicit.

Parser tests should not assert buffer text; they test grammar and semantic intent.

## Editor behavior tests

For editing behavior, send keys through `Editor::handle_key` using the test helpers in `src/editor.rs`. Assert all relevant outcomes:

- Buffer text
- Cursor line and character
- Final Vim mode
- Selection shape when relevant
- Register text and register shape
- Transaction count/grouping
- Viewport position when movement can scroll

Do not test a user-visible Vim command only by calling its private mutation helper.

## Required edge cases

For motions, operators, text objects, paste, undo, repeat, or macros, include applicable cases from this matrix:

- Empty buffer and empty line
- Beginning/end of line
- Beginning/end of file
- One-character line
- Count of one and count greater than one
- Forward and backward direction
- Unicode text
- Whitespace and punctuation boundaries
- Cross-line behavior
- Characterwise, linewise, and blockwise selections
- Short lines in block mode
- Unnamed and black-hole register effects
- Cursor placement after success and failed/no-op commands

## Transaction tests

One logical Vim change must be one transaction. Explicitly test transaction grouping for:

- Insert sessions
- Replace sessions
- Change operator followed by Insert
- Visual block insertion
- Multi-range block deletion
- Future dot-repeat and macro playback

No-op sessions should not create empty transactions.

## Buffer and persistence tests

When buffer internals change, test:

- Character offset ↔ line/character conversion
- Unicode and line endings
- Inserted/deleted text captured by edits
- Modified-state transitions
- Atomic save behavior

Use a unique temporary directory and remove it before the test returns.

## UI tests

Keep UI logic thin and test pure helpers rather than GPUI internals. Test line-number, selection-range, and geometry calculations. Do not introduce screenshot tests without a stable rendering harness.

## Required validation

Run all of the following before declaring the change complete:

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
git diff --check
```

Do not claim a command passed unless it was actually run and succeeded.
