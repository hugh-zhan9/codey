# Findings

## Approved product constraints

- Multi-account support must be internal, not an external wrapper.
- Account pool must support proactive token refresh.
- Automatic switching must happen both before requests and as a single reactive fallback after recognized quota/auth failures.
- After switching, the request must inherit the previous model and session-level settings.
- If the new account does not support the inherited model or relevant settings, Codex must fail explicitly and must not auto-downgrade.
- Internal storage should own the canonical account-pool format, while import compatibility with `codex-acc` and `cc-switch` is required.

## Existing codebase facts

- Active account auth handling currently centers on `codex-rs/login`.
- `/reload` plumbing already exists in `codex-rs/tui/src/app.rs` and `codex-rs/tui/src/chatwidget.rs`.
- `codex-acc` works as an external wrapper that stores multiple account snapshots and switches by rewriting active auth/config files.
- `codex-acc` already contains proactive token refresh logic and import/export compatibility with `cc-switch`.

## Initial implementation strategy

- Start with `codex-login` account-pool storage and model types because that slice is self-contained, low risk, and testable.
- Keep current active-account flow unchanged while introducing a separate persisted pool file.
