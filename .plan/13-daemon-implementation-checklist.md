# VSN1 CLI Daemon Implementation Checklist

## Purpose

This checklist turns `12-daemon-capability-plan.md` into an implementation sequence that can be executed across multiple sessions.

Use this checklist for the optional daemon-backed execution follow-up after the current main CLI work.

## Controlling plan

1. Treat `12-daemon-capability-plan.md` as the source of truth for daemon requirements and architecture.
2. When this checklist and older one-shot-only planning notes disagree, keep the cold-path fallback and follow `12` for the daemon-specific design.

## Current baseline

1. `vsn1-cli/` is currently a working one-shot Rust CLI with `device`, `runtime`, and `screen` commands.
2. The main implementation checklist through manifest-defined layer work is complete.
3. No daemon binary, IPC contract, or warm-port ownership layer exists yet.

## Session handoff state

- Overall status: `in_progress`
- Last completed step: `step 6`
- In-progress step: `none`
- Last verification run: `cargo fmt --check`, `cargo test`, `cargo check`, `cargo check --target x86_64-apple-darwin`, `cargo check --target aarch64-apple-darwin` (pass on 2026-06-22 after step 6 added a reusable per-device daemon session registry with worker threads, lazy open, `5s`-compatible configurable idle close behavior, same-device request serialization, different-device independence, and regression coverage for transport reuse, idle close plus reopen, same-device serialization, and cross-device concurrency)`
- Last hardware validation: `none for this checklist`
- Open blockers: `none`
- Next session start point: `step 7`

## Rules for every step

1. Do not mark a step complete until the application compiles.
2. Add or update unit tests in the same step as the code change.
3. Run these checks before marking a software-only step complete:
   - `cargo fmt --check`
   - `cargo test`
   - `cargo check`
4. For steps that touch daemon platform behavior, also run:
   - `cargo check --target x86_64-apple-darwin`
   - `cargo check --target aarch64-apple-darwin`
5. If a step changes real-device behavior, record hardware validation before closing the step.
6. Keep `vsn1-cli` working when no daemon is running at every stage of the rollout.
7. Preserve user-visible success and error output parity between daemon-backed and cold-path execution as closely as practical.
8. At the end of each session, update both `Session handoff state` and `Step completion log`.

## Step-by-step checklist

### Step 1: Extract a shared command model and routing classification

- [ ] Introduce semantic command/request types that are separate from `clap` parsing.
- [ ] Classify commands into local-only versus daemon-eligible groups.
- [ ] Keep `device list`, `runtime list`, and parse/help/version flows local-only.
- [ ] Keep all current command behavior unchanged on the cold path.
- [ ] Add unit tests for command classification and parse-to-request conversion.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 2: Refactor shared command execution and output rendering

- [ ] Move command execution onto shared library-level handlers that can be called by either binary.
- [ ] Centralize success output rendering so hot and cold paths can reuse the same text.
- [ ] Centralize error rendering so daemon and cold paths preserve existing wording where practical.
- [ ] Add an execution seam that can target either a one-shot transport factory or a daemon-owned transport/session.
- [ ] Add regression tests showing equivalent rendered output for the same semantic command request.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 3: Add daemon protocol types and socket-path resolution

- [ ] Add `DaemonRequest` and `DaemonResponse` library types for daemon-eligible commands.
- [ ] Add protocol versioning so client/server mismatches fail clearly.
- [ ] Implement request/response serialization and deserialization with `serde`.
- [ ] Add socket-path resolution with `VSN1_DAEMON_SOCKET` override plus Linux and macOS defaults.
- [ ] Restrict the design to one per-user host-local Unix socket.
- [ ] Add unit tests for protocol round trips, version mismatch handling, and socket-path resolution.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`, `cargo check --target x86_64-apple-darwin`, `cargo check --target aarch64-apple-darwin`.

### Step 4: Add the `vsn1-daemon` binary skeleton and health path

- [ ] Add a second binary target for `vsn1-daemon`.
- [ ] Bind the Unix socket and accept client connections.
- [ ] Decode one request and encode one response per connection.
- [ ] Add a minimal health or ping request so the client can validate live daemon reachability.
- [ ] Add tests for socket bind/listen behavior and simple request/response handling.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`, `cargo check --target x86_64-apple-darwin`, `cargo check --target aarch64-apple-darwin`.

### Step 5: Add daemon client detection and cold-path fallback in `vsn1-cli`

- [ ] Add daemon connection logic to `vsn1-cli` for daemon-eligible commands.
- [ ] Fall back to the existing cold path when the daemon socket is absent or stale.
- [ ] Treat live daemon protocol failures, execution failures, and version mismatches as errors rather than silent fallback.
- [ ] Keep local-only commands fully local even when the daemon is running.
- [ ] Add tests for no-daemon fallback, stale-socket fallback, and daemon-present routing.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`, `cargo check --target x86_64-apple-darwin`, `cargo check --target aarch64-apple-darwin`.

### Step 6: Implement per-device worker ownership and `5s` idle close

- [ ] Add a daemon-side per-device worker keyed by resolved USB port path.
- [ ] Ensure each worker owns at most one open serial transport for its device.
- [ ] Keep the port open between requests and close it after `5s` of inactivity.
- [ ] Reopen the port lazily on the next request after an idle close.
- [ ] Serialize same-device requests while allowing different-device workers to proceed independently.
- [ ] Add tests for same-device serialization, idle close timing, reopen-on-next-request, and multi-device isolation.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`, `cargo check --target x86_64-apple-darwin`, `cargo check --target aarch64-apple-darwin`.

### Step 7: Route screen commands through the daemon

- [ ] Route `screen raw`, `screen set`, `screen clear`, and `screen activate` through the daemon when it is reachable.
- [ ] Reuse the shared command handlers so daemon-backed screen output matches the cold path.
- [ ] Confirm that repeated screen commands against one device reuse the warm port inside the `5s` window.
- [ ] Preserve cold-path behavior when the daemon is not running.
- [ ] Add integration-style tests for daemon-backed screen command routing and output parity.
- [ ] Hardware gate: confirm repeated screen commands stop reopening the port between sub-`5s` invocations and still produce correct visible behavior on a real device.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`, `cargo check --target x86_64-apple-darwin`, `cargo check --target aarch64-apple-darwin`.

### Step 8: Route `device info` and runtime commands through the daemon

- [ ] Route `device info` and all serial-port-touching runtime commands through the daemon when it is reachable.
- [ ] Keep `runtime list` local-only.
- [ ] Reuse the same per-device worker ownership model for runtime install/verify/upgrade/repair/remove/status.
- [ ] Preserve the existing runtime diagnostics and verification semantics on the daemon path.
- [ ] Add integration-style tests for runtime and `device info` daemon routing plus output parity.
- [ ] Hardware gate: confirm runtime install, verify, status, and remove still behave correctly through the daemon on a real device.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`, `cargo check --target x86_64-apple-darwin`, `cargo check --target aarch64-apple-darwin`.

### Step 9: Harden failure handling and same-device contention

- [ ] Ensure daemon-backed port-open failures return clear errors and do not retry on the cold path.
- [ ] Ensure same-device concurrent requests queue safely instead of racing the transport.
- [ ] Ensure different-device requests can still progress independently.
- [ ] Add coverage for daemon crash or disconnect during request handling where practical.
- [ ] Add regression tests for busy-port failures, queued same-device work, and multi-device concurrency.
- [ ] Hardware gate: confirm the daemon returns a clear error when another process such as `grid-editor` owns the port.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`, `cargo check --target x86_64-apple-darwin`, `cargo check --target aarch64-apple-darwin`.

### Step 10: Service integration, docs, and final acceptance validation

- [ ] Add example `systemd --user` service configuration for `vsn1-daemon`.
- [ ] Add example `launchd` configuration guidance for `vsn1-daemon`.
- [ ] Update README usage docs for daemon detection, fallback behavior, and failure semantics.
- [ ] Document the socket override and expected socket locations.
- [ ] Re-run output-parity checks for representative `device`, `runtime`, and `screen` commands.
- [ ] Hardware gate: confirm `5s` idle close frees the port for `grid-editor`, and the next command reopens successfully.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`, `cargo check --target x86_64-apple-darwin`, `cargo check --target aarch64-apple-darwin`.

## Step completion log

Update this section as work lands.

- Step 1: `completed on 2026-06-22 - added src/command_model.rs with shared semantic request enums, local-only vs daemon-eligible routing classification, try_parse_command_request_from(...), and regression coverage while preserving the existing cold-path execution behavior`
- Step 2: `completed on 2026-06-22 - introduced a shared CommandExecutor trait plus OneShotCommandExecutor, added centralized CommandSuccess and render_command_success/render_command_error handling, routed the current cold path through execute_and_render_command(...), and added regression coverage for direct success rendering parity and shared error formatting`
- Step 3: `completed on 2026-06-22 - added src/daemon_protocol.rs with versioned DaemonRequest/DaemonResponse JSON encoding and decoding, explicit version-mismatch and local-only-command errors, added src/daemon_socket.rs with VSN1_DAEMON_SOCKET override plus Linux XDG_RUNTIME_DIR and macOS TMPDIR socket-path resolution, and added regression coverage for protocol round trips, mismatch handling, and socket-path resolution`
- Step 4: `completed on 2026-06-22 - added src/daemon_server.rs with Unix listener bind/accept logic, parent-directory creation, non-socket path protection, one-request/one-response serving, ping health handling, placeholder execute-request error responses, and cleanup-on-drop behavior; added src/bin/vsn1-daemon.rs plus Cargo binary registration and a daemon_main() library entrypoint; added regression coverage for ping round trips, placeholder execute responses, and bind rejection for existing non-socket paths`
- Step 5: `completed on 2026-06-22 - added src/daemon_client.rs with a real Unix-socket SystemDaemonClient, wired run() through execute_and_render_command_with_optional_daemon(...), kept local-only commands on the cold path, treated missing and stale sockets as fallback-to-local conditions, surfaced live daemon execution/protocol failures as errors, extended the top-level error type with daemon-client failures, and added regression coverage for local-only bypass, no-daemon fallback, stale-socket fallback, and live-daemon routing without local fallback`
- Step 6: `completed on 2026-06-22 - added src/daemon_session.rs with a reusable per-device session registry backed by one worker thread per device path, lazy transport open, idle-timeout transport drop, reopen-on-next-request behavior, and immediate-write support; added regression coverage proving same-device request serialization, different-device independence, transport reuse within the idle window, and reopen after idle close using a blocking test transport factory`
- Step 7: `pending`
- Step 8: `pending`
- Step 9: `pending`
- Step 10: `pending`

## Recommended session workflow

1. Pick exactly one step as `in progress`.
2. Finish code and unit tests for that step.
3. Run the required verification commands.
4. Run hardware validation if the step touches real-device behavior.
5. Update both `Session handoff state` and `Step completion log` before ending the session.
