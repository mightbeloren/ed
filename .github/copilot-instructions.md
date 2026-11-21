# Repository Instructions

Before changing this repository, read `AGENTS.md`, `README.md`, and `INSTRUCTIONS.md`.

## Project identity

`ed` is a Linux-only, Vim-first, keyboard-only text editor written in Rust with GPUI. It intentionally has no menus, toolbar, file-picker dialogs, or mouse-driven editing controls.

## Required behavior

- Implement exactly one roadmap phase at a time.
- Preserve existing behavior and keep changes focused on the active phase.
- Keep core editing logic independent from GPUI.
- Normalize GPUI input into `input::Key`, parse semantic `vim::Command` values, and execute them through `Editor`.
- Keep operator-plus-motion behavior generic through `vim::target`; never hardcode combinations such as `dw`, `db`, or `d$`.
- Route all text mutations through `Buffer` edit methods and transactions.
- Keep byte offsets, Rope character offsets, buffer positions, and display columns distinct.
- Use JetBrains Mono and the existing neutral dark palette for all UI.
- Do not add mouse-driven editing or soft wrapping.
- Follow the active roadmap exactly: Phase 12 is ordinary file-buffer navigation; Phase 13 is the recursive minibuffer file finder; Phase 14 is rich/writable Dired; Phase 15 is the persistent terminal panel; Phase 16 is final hardening.
- `Ctrl-Tab` and `Ctrl-Shift-Tab` cycle only ordinary file buffers; Dired, scratch, output, and other special buffers are excluded.
- Global `Ctrl-p` opens the recursive workspace file finder in the minibuffer; it does not open Dired or a graphical file picker.
- Dired styling interprets `LS_COLORS` over filesystem metadata; never parse `ls` output.
- Terminal colors come from interpreted child-process ANSI state over a direct PTY; terminal session state is not an editor buffer.

## Dependencies

Do not change these exact pins unless the task is a dedicated migration:

- `gpui = "=0.2.2"`
- `ropey = "=1.6.1"`

Keep `Cargo.lock` and the local `vendor/xattr` compatibility patch.

## Validation

Before a phase commit, run:

```sh
cargo fmt --all
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
git diff --check
```

Refresh project diagnostics as well. The transitive `proc-macro-error2` future-compatibility notice is expected.

## Git

- Main branch: `main`; never push phase development directly to it.
- Use one branch and pull request per phase, and require the `CI / quality` check before merge.
- Phase commit format: `phase N: short description`
- Release tags use immutable `vX.Y.Z` versions matching `Cargo.toml`.
- Do not rewrite user work or commit unrelated changes.
- Do not claim manual GUI validation unless the GUI was launched and exercised.
