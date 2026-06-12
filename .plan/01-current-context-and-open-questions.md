# VSN1 CLI Planning: Current Context And Open Questions

## Purpose

This document captures the current high-confidence context for `vsn1-cli` and the architecture decisions that still need user input.

## Confirmed constraints

1. `vsn1-cli` should be a standalone Rust CLI for driving the VSN1 screen directly over USB.
2. The production path must not depend on `grid-editor`.
3. Current target platforms are Linux and macOS.
4. The Rust POC proved that live `IMMEDIATE` Lua updates are viable when framed as `<?lua ... ?>`.
5. The currently meaningful display budget is roughly `5-10` reliable visible updates per second, depending on payload shape.
6. The implementation should not assume that every successful serial write becomes a visible screen update.

## Working implementation assumptions

1. Treat `grid-editor v1.6.5` behavior as the compatibility reference unless later work disproves it.
2. Keep the fast live-update path simple unless hardware behavior forces more session logic.
3. Prefer a stateful host-side model that coalesces changes instead of attempting to push every intermediate state.
4. Treat runtime-helper updates as the safest primary path for production planning, while keeping direct draw available for targeted capability probes.

## Architecture questions to resolve

1. Should the first production scope be screen-only, or should the CLI also own broader device concerns such as module targeting, discovery UX, or future knob/LED integration boundaries?
2. Should `vsn1-cli` be designed only as a one-shot command-line tool, or should it also support a long-lived daemon/session mode for lower-latency repeated updates?
3. What should the first public interface be: direct subcommands, JSON input, a script format, stdin streaming, or some combination?
4. What is the canonical host-side rendering model: high-level widgets/state, raw Lua snippets, direct draw primitives, runtime helper calls, or a layered abstraction that can target multiple backends?
5. What reliability guarantees matter most: best-effort updates, ack/verification on some commands, retry policy, device reconnect behavior, and failure reporting semantics?
6. How much device/runtime setup should the CLI own versus assuming a compatible runtime is already installed on the VSN1?
7. What should be considered stable public API surface in v1: user-facing commands only, or also a reusable Rust library crate?
8. What kinds of testing and simulation are required before implementation is considered production-ready?

## Next step

Collect the user's architecture decisions, then split them into numbered plan files covering:

1. scope and product shape
2. runtime model and transport lifecycle
3. command/input surface
4. rendering/update model
5. implementation phases and test strategy

## Cross-session note

No new critical architectural decisions have been made yet, so `AGENTS.md` does not need updating at this stage.
