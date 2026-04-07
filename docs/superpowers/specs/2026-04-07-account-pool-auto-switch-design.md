# Account Pool Auto-Switch Design

## Summary

Add an internal multi-account pool to Codex so the app can:

- persist multiple ChatGPT/Codex account snapshots
- proactively refresh account tokens
- pre-check quota and auth health before each request
- automatically switch to another healthy account when the current account is exhausted
- trigger the existing account reload flow after a switch
- preserve the previous request's model and session-level runtime settings after a switch
- fail explicitly if the new account does not support the inherited model or settings

This design intentionally does not embed `codex-acc`'s runtime file-overwrite flow. Instead, Codex gets a first-party account-pool layer and supports importing existing accounts from `codex-acc` and `cc-switch`.

## Goals

- Make multi-account usage a native Codex capability.
- Keep the current single-active-account auth model intact for the rest of the codebase.
- Support both proactive switching before requests and reactive switching after quota/auth failures.
- Reuse the current `/reload` account refresh path after a switch instead of inventing a parallel state-sync flow.
- Preserve the active thread's model and runtime settings across account switches.
- Avoid silent model/config downgrades after switching accounts.

## Non-Goals

- Replacing `AuthManager` with a fully multi-account-native auth core.
- Re-implementing `codex-acc` as an external wrapper inside this repo.
- Automatically downgrading to a different model when the new account does not support the inherited one.
- Supporting unlimited retry loops for failed requests.
- Building a complex quota prediction engine in the first iteration.
- Shipping `cc-switch` export in the first milestone.

## User Experience

### Manual account management

Codex exposes an account-pool UI and CLI surface that lets users:

- list accounts
- manually switch the active account
- add or import accounts
- mark accounts that require relogin
- inspect token health and last-known quota state

### Automatic switching

Codex evaluates the current active account before starting a new model request.

- If the current account is healthy and quota is sufficient, nothing changes.
- If the current account should not be used, Codex switches to the next healthy account, updates the active auth/config snapshots, and runs the existing account reload flow.

If a request still fails with a recognized quota exhaustion or auth-expired error:

- Codex may perform one reactive fallback switch.
- If the request is still in a safe-to-retry phase, Codex retries once with the new account.
- If the failure is already terminal or the request is not safe to replay, Codex switches the active account but does not automatically replay that request.

### Session inheritance after switching

After a switch, Codex must keep using the last request's active session-level settings:

- model
- reasoning effort
- approval/review policy
- sandbox settings
- collaboration mode
- other session-scoped runtime settings already active on the thread

If the new account cannot support the inherited model or relevant settings, Codex must not silently degrade. It reports that the switch succeeded but the inherited request configuration is not valid on the new account.

## Architecture

The design adds an account-pool layer above the existing active-account auth system.

### Existing contract that remains unchanged

`AuthManager` remains responsible for the single currently active account. Existing code that reads the active account from local auth storage should continue to work.

### New layer

Introduce an `AccountPoolManager` in `codex-login` that owns:

- multi-account storage
- token refresh and token-health state
- quota-health state
- next-account selection
- activation of the selected account into the existing active auth/config files

This lets the rest of the app continue to see one active account at a time while adding first-party multi-account management.

## Data Model

Store the pool separately from the current active auth snapshot. Proposed location:

- `~/.codex/account-pool.json`

This avoids overloading `auth.json`, which should continue to represent only the currently active account.

Each account entry contains:

- `alias`
- `authSnapshot`
- `configSnapshot`
- `source`
  - `native`
  - `codexAccImport`
  - `ccSwitchImport`
- `accountIdentity`
  - `accountId`
  - `userId`
  - `email`
  - `planType`
- `tokenHealth`
  - `lastRefreshAt`
  - `expiresAt`
  - `refreshStatus`
  - `needsRelogin`
- `usageHealth`
  - `fiveHourRemainingPercent`
  - `weeklyRemainingPercent`
  - `lastCheckedAt`
  - `quotaExhausted`
- `switchPolicyState`
  - `priority`
  - `cooldownUntil`
  - `lastSelectedAt`
  - `lastFailureReason`

The file also stores:

- `currentAlias`
- optional global account-pool settings
- import metadata

## Switching Policy

Selection is deterministic and conservative.

Candidate filtering order:

1. Exclude accounts marked `needsRelogin`.
2. Exclude accounts whose token refresh failed or whose auth snapshot is invalid.
3. Exclude accounts marked quota exhausted.
4. Exclude accounts still inside cooldown.
5. Prefer the current account if it is still valid.
6. Otherwise choose the highest-priority healthy account with the best recent quota state.

This policy avoids oscillation when several accounts are close to exhaustion.

## Token Refresh

The system must proactively refresh token validity for pooled accounts. This behavior is required and is based on the same functional need already handled in `codex-acc`.

### Active account refresh

Before a request starts:

- inspect token expiration/refresh state
- refresh if needed
- fail over if refresh is impossible or indicates relogin is required

### Background refresh for non-active accounts

Run low-frequency health refreshes on non-active accounts so failover targets are still usable when needed.

### Refresh failure handling

If refresh fails in a way that indicates the account is no longer valid:

- mark `needsRelogin = true`
- exclude the account from automatic switching
- keep the account visible in the pool UI for manual repair

## Quota Health

Use recent rate-limit data as the primary source for switch decisions.

For MVP:

- store last-known five-hour and weekly remaining percentages when available
- mark an account quota-exhausted when the active request flow or explicit quota fetch says it is exhausted
- allow background refresh of quota state, but do not require continuous polling

Quota decisions should stay simple in the first version. There is no need for predictive routing.

## Request Lifecycle Integration

### Preflight path

Before a new request is submitted to the model:

1. refresh token state for the active account if needed
2. evaluate quota/auth health
3. if the account is unsuitable, switch to a new account
4. activate the new account into the normal active auth/config storage
5. trigger the existing reload-account path
6. validate that the new account supports the inherited model/settings
7. if validation succeeds, submit the request
8. if validation fails, surface an explicit error without silent downgrade

### Reactive fallback path

If a request fails in-flight with a recognized quota exhaustion or auth-expired failure:

1. decide whether the failure is eligible for automatic recovery
2. select and activate the next account
3. trigger reload-account
4. re-validate model/settings compatibility
5. if safe to retry and still pre-terminal, replay once
6. otherwise stop and tell the user that the account changed but the failed request was not retried

Reactive fallback is limited to a single automatic retry attempt per user request.

## Safe Retry Rules

Automatic retry is only allowed when all of the following are true:

- the failure is clearly due to quota exhaustion or token/auth invalidation
- the request has not produced a final terminal result
- the request is still in a replay-safe phase
- no non-idempotent external side effects have already been committed
- no prior automatic account-switch retry has already happened for the same request

Automatic retry is not allowed when:

- the failed turn is already committed as terminal
- the request has already executed a user-visible, non-idempotent external side effect
- the new account cannot support the inherited model/settings
- the request has already consumed its one automatic retry

## Model And Configuration Inheritance

This is a hard requirement.

When an account switch occurs, Codex must preserve the last request's thread/session-level configuration. The new account does not redefine the request.

Inherited state includes:

- model
- reasoning effort
- approval or review settings
- sandbox settings
- collaboration mode
- other thread-scoped runtime settings already active in the current session

### Strict compatibility rule

If the new account does not support the inherited model or required runtime settings:

- do not auto-downgrade
- do not silently switch to a nearby model
- fail explicitly
- keep the newly switched account active for future requests
- tell the user that the account switch succeeded but the inherited configuration is unsupported

## Import Compatibility

Codex owns its own internal account-pool format, but it can import external account stores.

### `codex-acc`

Support one-way import of stored account snapshots and config snapshots from `codex-acc`.

### `cc-switch`

Support one-way import of compatible Codex account snapshots from `cc-switch`.

MVP does not require export back to either system.

## Module-Level Changes

### `codex-login`

Add:

- `account_pool.rs`
- `AccountPoolStore`
- `AccountPoolManager`
- import adapters for `codex-acc` and `cc-switch`

Responsibilities:

- load/save account pool
- token refresh for pooled accounts
- health state updates
- candidate selection
- activation of selected account into the existing active auth/config storage

`AuthManager` remains single-account and active-account-scoped.

### `codex-app-server`

Add account-pool RPCs and notifications so TUI and CLI do not manipulate storage directly.

Likely methods:

- `accountPool/list`
- `accountPool/import`
- `accountPool/switch`
- `accountPool/refresh`
- `accountPool/autoSwitchStatus`

Likely notification:

- `AccountSwitchedNotification`

Responsibilities:

- expose account-pool state
- execute switch requests
- notify consumers about active-account changes
- trigger the existing reload flow after activation

### `codex-tui`

Extend current request submission and account reload flows to:

- preflight-check active account health
- reactively fail over on eligible quota/auth failures
- reuse the existing reload-account path after a switch
- add account-pool management UI surfaces

Minimum UI:

- account list
- manual switch
- import
- relogin-needed visibility
- switch/retry status messaging

### `codex-exec` / CLI

Expose account-pool commands for non-TUI users and ensure non-TUI requests use the same preflight policy as TUI.

## Error Handling

Explicit user-visible errors are required in these cases:

- no healthy account is available
- token refresh fails and the current account cannot be used
- switch succeeds but inherited model/settings are unsupported on the new account
- reactive retry is not safe or already consumed
- imported account snapshot is malformed or incomplete

The system must not silently downgrade models or loop through the full pool indefinitely.

## Testing Strategy

### `codex-login`

- account-pool persistence round trips
- token refresh success/failure
- account selection and cooldown logic
- relogin-required marking
- activation of selected account into current auth/config storage
- import from `codex-acc`
- import from `cc-switch`

### `codex-app-server`

- RPC behavior for list/import/switch/refresh
- account-switched notifications
- reload trigger after activation

### `codex-tui`

- preflight auto-switch before request submit
- reactive single failover after quota exhaustion
- no silent downgrade when new account lacks the inherited model
- `/reload`-driven state refresh after switch
- snapshot coverage for account-pool UI/status text

### Integration

- active account exhausted -> switches to healthy account -> next request succeeds
- active account receives auth failure -> token refresh succeeds
- token refresh fails -> account marked relogin-required and skipped
- no suitable account exists -> request fails clearly
- inherited model unsupported on switched account -> explicit error and no auto-downgrade

## Rollout Plan

Recommended implementation sequence:

1. account-pool storage and manual activation in `codex-login`
2. app-server RPC surface
3. TUI and CLI manual account management
4. preflight auto-switch
5. reactive single-fallback switching
6. importers for `codex-acc` and `cc-switch`

This ordering keeps the feature incremental and testable.

## Risks

### Auth consistency

The pool snapshot, active `auth.json`, and active `config.toml` must stay aligned. A partial activation can leave the UI and request path using different account assumptions.

### Retry safety

Automatic replay after failover must remain tightly constrained. The system cannot assume every failed turn is safe to replay.

### Switch oscillation

If several accounts are near exhaustion, the app can thrash between them unless cooldown and one-shot retry rules are enforced.

### Compatibility drift

Imports from `codex-acc` and `cc-switch` are best-effort compatibility layers. Their formats may evolve independently, so import code must validate aggressively and degrade clearly.

## MVP Scope

The first implementation should include:

- internal account-pool storage
- manual account add/import/list/switch
- token refresh health management
- preflight account switching
- single reactive fallback switch
- reload integration
- strict inheritance of model/settings with explicit incompatibility failure

The first implementation should exclude:

- automatic model fallback
- export back to external tools
- advanced quota prediction
- multi-hop retry chains
- heavy background polling
