# Issue 3093 Evaluation: Dynamic Node Discovery And Capability Advertisement

This note captures the current repository state for [issue #3093](https://github.com/zeroclaw-labs/zeroclaw/issues/3093) and the reason this branch does not attempt a second competing implementation.

## Current State In This Checkout

- `src/gateway/mod.rs` already contains an experimental node-control scaffold:
  - `POST /api/node-control`
  - `node.list`
  - `node.describe`
  - `node.invoke` returns a stubbed not-implemented response
- `src/config/schema.rs` already exposes `[gateway.node_control]` with:
  - `enabled`
  - `auth_token`
  - `allowed_node_ids`
- There is no runtime node registry, heartbeat protocol, persistent node transport, or dynamic capability advertisement path in the checked-in `origin/dev` code.

## Related Existing Work

- Issue `#2991` already defines a concrete implementation track for a multi-machine node system.
- PR `#3006` (`feat(nodes): implement functional multi-machine node system`) is already open and tied to `#2991`.
- A comment on `#3093` explicitly points to `#2991` and PR `#3006` as the active implementation thread.

## Why This Branch Does Not Reimplement The Feature

Creating a second independent node-system implementation here would duplicate an already-open PR with overlapping goals:

- dynamic node registration
- remote invocation
- node registry / lifecycle handling
- cluster-style capability exposure

That would increase review noise and create merge risk without adding new signal.

## Recommended Follow-Up If PR #3006 Moves Forward

If the existing node-system PR is merged or needs a targeted follow-up, the highest-value next steps would be:

1. Add explicit capability-advertisement tests at the gateway boundary so node-reported tools are validated end-to-end.
2. Define a stable capability schema/versioning story for node tool metadata to prevent node/gateway drift.
3. Add prompt/runtime integration tests showing dynamically advertised node capabilities are exposed safely to the agent.
4. Tighten auth and replay protection around node registration, heartbeat, and invoke flows before treating the feature as generally available.

## Repository Guidance

Treat `#3093` as a duplicate or umbrella request until PR `#3006` is reviewed. New work should build on that branch or land as narrow follow-up changes after it, rather than starting a parallel implementation from the current scaffold.
