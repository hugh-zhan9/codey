# Task Plan

## Goal

Implement the first-party account-pool foundation for Codex, following the approved spec in `docs/superpowers/specs/2026-04-07-account-pool-auto-switch-design.md`, starting with the lowest-risk storage and data-model slice in `codex-login`.

## Phases

| Phase | Status | Description |
|---|---|---|
| 1 | completed | Add account-pool data model, persistent storage, and activation primitives in `codex-login` with tests |
| 2 | in_progress | Add import compatibility from `codex-acc` and `cc-switch` into the internal pool format |
| 3 | pending | Add pool manager health state and token/quota-aware active-account selection |
| 4 | pending | Expose account-pool RPCs from `codex-app-server` |
| 5 | pending | Add TUI/CLI manual account-pool management surfaces |
| 6 | pending | Add preflight auto-switch and reload integration |
| 7 | pending | Add reactive fallback switch and strict inherited-config validation |

## Constraints

- Keep `AuthManager` single-active-account scoped.
- Do not silently downgrade model or runtime settings after switching accounts.
- Reuse the existing reload-account path after activation.
- Follow TDD for each implemented behavior slice.

## Errors Encountered

| Error | Attempt | Resolution |
|---|---|---|
| None yet | 0 | N/A |
