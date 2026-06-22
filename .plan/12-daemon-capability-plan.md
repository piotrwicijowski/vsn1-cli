# VSN1 CLI Daemon Capability Plan

### Purpose

This document captures a follow-up plan for adding optional daemon-backed execution to `vsn1-cli` without removing the existing one-shot cold path.

The goal is not to redesign the product around a required background service. The goal is to add a host-local daemon that can own USB access, keep per-device ports warm between commands, and serialize conflicting work so rapid repeated invocations stop fighting each other.

Use `13-daemon-implementation-checklist.md` to execute this plan step by step.

### Relationship To Earlier Plans

Earlier planning for v1 intentionally chose a one-shot CLI-only model.

That decision remains important in one specific sense:

1. `vsn1-cli` must still work correctly when no daemon is running.
2. The daemon is an optional acceleration and ownership layer, not a new hard requirement for basic operation.
3. Where earlier plan files say "one-shot only," treat that as the required fallback behavior, not as a prohibition on this follow-up.

This follow-up extends the current implementation after manifest-defined runtime layer work, not before it.

### Confirmed Requirements

1. Add a separate binary named `vsn1-daemon`.
2. Daemon lifecycle will be managed externally by `systemd` on Linux and `launchd` on macOS.
3. If the daemon is running, `vsn1-cli` should autodetect it and send eligible commands to it.
4. If the daemon is not running, `vsn1-cli` should use the current cold path.
5. The daemon should keep each device port open between requests, similar to the warm path.
6. If no new request arrives for a given device for `5s`, the daemon should close that serial port but remain alive.
7. The daemon should manage multiple USB devices concurrently on one host.
8. Each explicit `--device` target should have its own internal port lifecycle and `5s` idle timeout.
9. Commands that fail because the daemon cannot open or use the port should return an error rather than bypassing the daemon.
10. The daemon should centralize ownership and serialization to reduce dropped messages and race conditions from rapid cold-path invocations.
11. IPC between `vsn1-cli` and `vsn1-daemon` should use a Unix socket.
12. Hot-path and cold-path command behavior and output should stay as similar as practical.
13. Commands with no daemon benefit should remain local instead of being needlessly proxied.

### Why This Follow-Up Is Worth Doing

The existing POC findings support this scope, but for narrower reasons than raw throughput alone:

1. The cold path pays a meaningful per-command close penalty of roughly `35 ms`.
2. The POC showed that a warm session can remove that host-side reopen/close churn.
3. The POC also showed that visible screen cadence is still much slower than raw host send cadence, so the daemon should not be justified as a magical frame-rate unlock.
4. The stronger reason to productize the daemon is command ownership, request serialization, reduced port churn, and fewer host-side races.

### Scope Boundary

#### In scope

1. Optional daemon forwarding for commands that touch the serial port.
2. Per-device request serialization inside the daemon.
3. Per-device warm transport reuse with `5s` idle close.
4. Multi-device concurrency at the host level.
5. Output and error parity between daemon-backed and cold-path execution.

#### Out of scope for the first daemon pass

1. Auto-starting the daemon from `vsn1-cli`.
2. Replacing `systemd` or `launchd` lifecycle management with CLI subcommands.
3. Reworking the USB protocol around a new long-lived heartbeat/session simulation unless hardware later proves it is necessary.
4. Device-side coalescing, screen-rate smoothing, or new rendering semantics.
5. Routing commands that do not benefit from a warm serial owner.

### Command Routing Model

Split the public command surface into two groups.

#### Always local

These commands should stay inside `vsn1-cli` even when the daemon is running:

1. `device list`
2. `runtime list`
3. `--help`, `--version`, and argument parsing failures

Rationale:

1. They do not need the serial port.
2. Routing them through the daemon adds failure modes without solving the ownership problem.

#### Daemon-eligible

These commands should attempt daemon forwarding first:

1. `device info`
2. `screen raw`
3. `screen set`
4. `screen clear`
5. `screen activate`
6. `runtime install`
7. `runtime verify`
8. `runtime upgrade`
9. `runtime repair`
10. `runtime remove` / `runtime uninstall`
11. `runtime status`

Routing rule:

1. Parse CLI arguments locally as today.
2. If the parsed command is daemon-eligible, try connecting to the daemon socket.
3. If the socket is not reachable because no daemon is running, execute the current cold path locally.
4. If the socket is reachable, send the semantic command request to the daemon and return its response.
5. If the daemon accepts the request and returns an execution error, surface that error directly and do not retry locally.

### Daemon Detection Semantics

Use daemon reachability, not socket-file existence alone, as the detection signal.

Recommended behavior:

1. `ENOENT` on socket connect means "daemon not running" and should fall back to the cold path.
2. `ECONNREFUSED` or a stale socket with no live peer should also fall back to the cold path.
3. A successful socket connection means "daemon is running," and the request should stay on the daemon path.
4. Protocol decode failures, version mismatches, or mid-request daemon failures should be treated as errors, not as silent cold-path fallbacks.

That keeps the no-daemon experience simple while preventing split-brain behavior when a real daemon is available but unhealthy.

### Host-Local IPC Contract

The daemon protocol should be semantic, not shell-text based.

Recommended shape:

1. Define a `DaemonRequest` enum that mirrors the daemon-eligible command set.
2. Define a `DaemonResponse` enum that carries protocol version plus either success output text or structured error text.
3. Serialize requests and responses with `serde` using a stable wire format such as JSON.
4. Keep the first protocol synchronous request/response per connection.

Important constraint:

The daemon should execute the same library-level command handlers as the cold path so behavior divergence stays small.

### Socket Path Recommendation

Use one per-user Unix socket, not one per device.

Recommended resolution order:

1. `VSN1_DAEMON_SOCKET` override for development and tests.
2. Linux default: `$XDG_RUNTIME_DIR/vsn1-cli/daemon.sock`.
3. macOS default: `${TMPDIR}/vsn1-cli/daemon.sock`.

Operational rules:

1. Create the parent directory if needed.
2. Restrict permissions to the current user.
3. Treat the daemon as a per-user host service.

### Internal Daemon Architecture

Use one process with a host-global router and per-device workers.

#### 1. Listener layer

Responsibilities:

1. bind the Unix socket
2. accept client connections
3. decode `DaemonRequest`
4. route each request to the correct execution path
5. encode `DaemonResponse`

#### 2. Command router

Responsibilities:

1. keep local-only logic out of the daemon
2. resolve which requests need a device worker
3. run host-only daemon-side helpers such as device discovery and runtime name resolution before acquiring a device worker when needed

#### 3. Per-device worker

Responsibilities:

1. own one selected USB serial device path
2. own at most one open serial transport for that port
3. serialize all commands targeting that device
4. close the transport after `5s` of inactivity
5. reopen the transport lazily on the next request

Recommended key:

1. key workers by canonical resolved USB port path, not by `dx/dy`
2. treat broadcast vs explicit module targeting as request metadata inside a device worker, not as separate workers

### Device Resolution Inside The Daemon

To preserve parity with the current CLI, keep device selection rules the same.

Recommended flow per daemon-eligible request:

1. Resolve `TargetArgs` semantics exactly as today.
2. Discover supported USB devices exactly as today.
3. Select the device exactly as today:
   - explicit `--device` wins
   - otherwise require exactly one matching supported device
   - otherwise return the same ambiguity or missing-device error
4. Route the request to the worker for that resolved USB port name.

This keeps multi-device behavior consistent between hot and cold paths.

### Transport Lifecycle Inside A Device Worker

Each device worker should behave like a small actor with one mutable transport owner.

Recommended lifecycle:

1. When the first request arrives, open the serial port at `2000000` baud.
2. Execute the full request while holding exclusive mutable access to that transport.
3. When the request completes, leave the port open and arm a `5s` idle deadline.
4. If another request for the same device arrives before the deadline, cancel the pending close and reuse the open transport.
5. If the deadline expires with no new request, close the port and return to an idle worker state.
6. Keep the worker object alive after closing the port so the next request only pays reopen cost, not router recreation cost.

Important serialization rule:

1. Only one request may execute against a given device at a time.
2. Requests for different devices may execute concurrently.

### Command Execution Refactor Needed In The Library

The current CLI library is close to reusable, but it still assumes one-shot direct execution and `String` rendering at the top level.

The daemon follow-up should first refactor command execution into a shared library surface with these properties:

1. parse CLI arguments into semantic command types
2. classify commands as local-only or daemon-eligible
3. expose shared command handlers that return the same success text and the same user-facing errors regardless of caller
4. allow those handlers to execute either against a direct one-shot `SerialTransportFactory` or against a daemon-owned already-open transport

Recommended direction:

1. Introduce a request model separate from `clap` parsing.
2. Keep output rendering centralized so daemon and cold path reuse the same formatting.
3. Keep the `screen` and `runtime` command logic in library code, not in either binary.

### Runtime Command Implications

Runtime operations are the part of the current codebase most likely to expose hidden daemon complexity.

Reasons:

1. They perform multiple transport reads and writes, not just one immediate send.
2. They already maintain per-command packet identity state in `TransportRuntimeSlotReader`.
3. They depend on delays, read-back windows, and exact response matching.

Plan implication:

1. Do not special-case the daemon only for `screen` commands.
2. Make runtime commands first-class daemon requests from the beginning.
3. Keep runtime command execution command-scoped even when the underlying serial port stays open.
4. Do not introduce a process-global packet-identity simulation layer unless real hardware proves it is necessary.

The worker should own the port, but each runtime command should still own its own request/response transaction logic.

### Output And Error Parity

Parity matters because users should not have to care whether the command was handled locally or by the daemon.

Recommended rules:

1. Success output should remain byte-for-byte identical where practical.
2. Existing error wording should be preserved where practical.
3. The daemon should not inject extra chatter into normal command output.
4. If needed for debugging, add opt-in daemon diagnostics through logs or a dedicated verbose mode, not default stdout.

### Concurrency And Backpressure Rules

To reduce races, define simple rules early.

1. A device worker processes one request at a time.
2. Additional requests for that device wait in order.
3. Requests for different devices can progress independently.
4. The first pass should prefer correctness over queue sophistication.
5. If a request is cancelled because the client disconnects, treat that as a future enhancement unless the implementation cost stays very small.

### Failure Model

Recommended failure behavior:

1. No daemon reachable: use the cold path.
2. Daemon reachable but device open fails: return daemon error.
3. Daemon reachable but command execution fails: return daemon error.
4. Daemon reachable but request is malformed or unsupported: return daemon protocol error.
5. Daemon crashes mid-request: surface an execution failure; do not silently retry locally.

This avoids double-executing mutable runtime commands and avoids racing the daemon for ownership.

### Suggested Codebase Shape

Keep the single Rust package and add a second binary target instead of splitting into a multi-crate workspace immediately.

Recommended additions:

```text
vsn1-cli/
  src/
    lib.rs
    main.rs
    bin/
      vsn1-daemon.rs
    daemon_protocol.rs
    daemon_client.rs
    daemon_server.rs
    daemon_router.rs
    command_model.rs
```

Possible internal helper types:

1. `CommandRequest`
2. `DaemonRequest`
3. `DaemonResponse`
4. `DeviceWorker`
5. `DaemonTransportPool` or `DeviceSessionRegistry`

The exact filenames can stay flexible. The important point is that daemon protocol, client code, and execution routing remain separate from transport and screen/runtime logic.

### Implementation Phases

#### Phase 1: Shared command model

1. Introduce semantic command/request types independent of `clap`.
2. Split local-only versus daemon-eligible commands.
3. Centralize shared output rendering.
4. Keep all current commands working on the cold path.

#### Phase 2: IPC foundations

1. Add Unix socket path resolution.
2. Add request/response protocol types.
3. Add a minimal `vsn1-daemon` listener that can answer a health check and one simple proxied command.
4. Add daemon client connection logic in `vsn1-cli`.

#### Phase 3: Immediate screen commands through the daemon

1. Route `screen raw`, `screen set`, `screen clear`, and `screen activate` through the daemon when available.
2. Add one per-device worker with warm-port reuse and `5s` idle close.
3. Validate that repeated screen commands stop reopening the port on every invocation.

#### Phase 4: Runtime and device-info commands through the daemon

1. Route `device info` and all port-using runtime commands through the daemon when available.
2. Reuse the same per-device worker abstraction.
3. Confirm that runtime commands still preserve the current diagnostics and verification behavior.

#### Phase 5: Multi-device concurrency hardening

1. Allow concurrent workers for distinct USB port paths.
2. Add queueing and serialization tests for same-device contention.
3. Confirm independent idle close behavior per device.

#### Phase 6: Service integration and docs

1. Add example `systemd --user` unit files.
2. Add example `launchd` plist guidance.
3. Document daemon detection, fallback, and failure semantics in the README.

### Verification Strategy

#### Software verification

At minimum, add tests for:

1. command classification into local-only vs daemon-eligible
2. socket detection fallback behavior
3. protocol encode/decode round trips
4. same output rendering for local and daemon-backed success cases
5. same error rendering for local and daemon-backed failure cases
6. per-device request serialization
7. idle close after `5s`
8. reopen on next request after idle close
9. multi-device concurrency isolation

#### Hardware validation

Major milestones should be validated on a real VSN1 device.

Recommended hardware matrix:

1. cold path still works with daemon absent
2. daemon-backed repeated `screen set` does not reopen the port between sub-`5s` invocations
3. daemon closes the port after `5s` idle and `grid-editor` can then claim it
4. daemon reopens successfully on the next command after idle close
5. daemon returns a clear error when `grid-editor` already owns the port
6. runtime install/verify/status still work correctly through the daemon
7. two supported USB devices can be driven independently if available

### Recommended Initial Acceptance Criteria

Treat the first daemon-capable implementation as successful when all of these are true:

1. `vsn1-cli` still works unchanged when no daemon is running.
2. When the daemon is running, all serial-port-touching commands use it automatically.
3. Discovery-only commands remain local.
4. Same-device requests are serialized inside the daemon.
5. Different-device requests can proceed concurrently.
6. A device port stays open across quick repeated commands and closes after `5s` idle.
7. Once closed by idle timeout, the port becomes available for `grid-editor`.
8. User-visible success and failure output stays materially aligned with the cold path.

### Recommendation

Implement this as an ownership and transport-lifecycle follow-up, not as a rendering-performance project.

The simplest correct design is:

1. keep `vsn1-cli` as the parsing and fallback entrypoint
2. add `vsn1-daemon` as an externally managed host-local owner
3. forward only serial-port-touching commands
4. serialize work per USB device
5. reuse the existing cold-path command logic as much as possible
6. close each port after `5s` idle so `grid-editor` can still reclaim it
