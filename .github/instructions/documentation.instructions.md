---
applyTo: "**/*.md"
---

# Documentation and Roadmap Instructions

Keep project documentation synchronized with implementation.

## Sources of truth

- `AGENTS.md`: architecture invariants, dependency policy, current phase, validation, and Git rules.
- `README.md`: user-visible features and complete phase roadmap.
- `INSTRUCTIONS.md`: continuation workflow and testing checklist.
- `.github/copilot-instructions.md`: concise repository-wide guidance for GitHub coding agents.
- `.github/instructions/*.instructions.md`: scoped guidance for matching files.

Do not introduce conflicting phase numbers, dependency versions, validation commands, or architectural rules across these files.

## Phase updates

When completing a phase:

1. Mark only that phase complete in `README.md`.
2. Set the next phase accurately.
3. Update the implemented command list.
4. Update `AGENTS.md` current-phase summary.
5. Update testing instructions only if the testing strategy changed.
6. Do not describe planned behavior as implemented.

## Feature claims

- Distinguish implemented, partial, and planned Vim behavior.
- State that search begins in Phase 7 until it is actually implemented.
- State that Ex/minibuffer execution begins in Phase 8 until implemented.
- Do not claim complete Vim compatibility before compatibility hardening is complete.
- Do not claim GUI/manual testing unless it happened.

## Style

- Use concise Markdown headings and checklists.
- Use backticks for commands, keys, modules, and paths.
- Keep examples consistent with actual parser behavior.
- Mention migration or compatibility implications when dependencies or file formats change.
