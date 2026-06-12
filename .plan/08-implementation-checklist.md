# VSN1 CLI Implementation Checklist

## Purpose

This checklist turns the decisions in `01` through `07` into an implementation sequence that can be executed across multiple agent sessions.

## Current baseline

1. `vsn1-cli/` currently contains planning documents only.
2. No Rust package exists yet.
3. The first implementation step must create a compiling crate skeleton.

## Session handoff state

- Overall status: `in progress`
- Last completed step: `step 2`
- In-progress step: `step 4 - ship the first end-to-end screen path with screen raw`
- Last verification run: `cargo fmt --check`, `cargo test`, `cargo check` (pass on 2026-06-12 after wiring screen raw through the immediate path)
- Last hardware validation: `step 3 attempted on 2026-06-12: cargo run -- device list now correctly reports no supported devices in this container after filtering out enumerated-but-missing /dev paths; step 4 hardware validation not run yet because this container still lacks direct access to the real /dev/ttyACM* node`
- Open blockers: `step 3 real-device validation still required for discovered topology and explicit targeting; step 4 real-device validation still required to confirm screen raw changes the screen on hardware`
- Next session start point: `step 4 - run hardware validation for screen raw on the host with direct serial-device access, then close the remaining hardware gate and revisit step 3 host validation if needed`

## Rules for every step

1. Do not mark a step complete until the application compiles.
2. Add or update unit tests in the same step as the code change.
3. Run these checks before marking the step complete:
   - `cargo fmt --check`
   - `cargo test`
   - `cargo check`
4. If a step changes runtime behavior that depends on hardware, record the hardware result before closing the step.
5. At the end of each session, update the handoff state in this file.

## Step-by-step checklist

### Step 1: Bootstrap the crate and CLI shell

- [x] Create `Cargo.toml`, `src/lib.rs`, and `src/main.rs`.
- [x] Add shared error handling and a thin CLI entrypoint.
- [x] Add top-level command groups: `device`, `runtime`, `screen`.
- [x] Stub subcommands so parsing is stable even before behavior is implemented.
- [x] Add unit tests for CLI parsing and library entrypoints.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 2: Build protocol, transport, and test seams

- [x] Add `protocol.rs` for Grid packet framing and command encoding.
- [x] Add `transport.rs` with a serial transport trait plus a fake transport for tests.
- [x] Add deterministic Lua framing for `<?lua ... ?>` payloads.
- [x] Keep provisioning writes and immediate writes separated in the API.
- [x] Add unit tests for packet encoding, framing, and transport error mapping.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 3: Add device discovery and targeting

- [x] Add `device.rs` and `targeting.rs`.
- [x] Implement device enumeration and target selection rules.
- [x] Support broadcast-first defaults plus explicit `--dx` and `--dy` overrides.
- [x] Implement `device list` and `device info` using the transport abstraction.
- [x] Add unit tests for targeting resolution, ambiguous-target failures, and CLI parsing.
- [ ] Hardware gate: confirm discovered topology and explicit targeting on a real device.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 4: Ship the first end-to-end screen path with `screen raw`

- [x] Add `raw.rs` and wire `screen raw` through the library.
- [x] Send raw framed Lua through the immediate path.
- [x] Reuse the same diagnostics and targeting behavior as other screen commands.
- [x] Add unit tests for raw command parsing, payload framing, and error reporting.
- [ ] Hardware gate: confirm `screen raw` changes the screen on a real device.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 5: Define the runtime bundle contract

- [ ] Create `assets/runtime/<bundle-version>/` and add a manifest file.
- [ ] Define owned script/config locations, install order, and exact-match identity fields.
- [ ] Add `runtime_bundle.rs` for manifest loading, normalization, and content hashing.
- [ ] Capture the first bundled runtime/profile asset set from the validated POC inputs.
- [ ] Record the initial curated dotted field inventory that the runtime will support.
- [ ] Add unit tests for manifest parsing, content normalization, and hash comparison.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 6: Implement runtime inspection commands

- [ ] Add `runtime.rs` read/verify/status primitives over owned slots.
- [ ] Implement `runtime verify` and `runtime status`.
- [ ] Fail on any missing, drifted, or mismatched owned content.
- [ ] Keep exact bundled version matching as the only success condition.
- [ ] Add unit tests for match, mismatch, missing-slot, and malformed-manifest cases.
- [ ] Hardware gate: confirm verify catches both exact-match and drifted-runtime cases.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 7: Implement runtime install

- [ ] Implement `runtime install` using the manifest install order.
- [ ] Read back owned content and verify exact bundle match after install.
- [ ] Keep writes limited to owned slots only.
- [ ] Add clear diagnostics for install preconditions and failures.
- [ ] Add unit tests for install planning, ordered writes, and post-install verification behavior.
- [ ] Hardware gate: confirm install provisions a blank or repaired device successfully.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 8: Add the curated screen field registry

- [ ] Add `screen.rs` field registry keyed by public names like `persistent.title`.
- [ ] Model layer, value kind, runtime key, and clear behavior per field.
- [ ] Reject unknown field names and invalid value types.
- [ ] Support different curated field sets per layer.
- [ ] Add unit tests for field lookup, parsing, value validation, and clear planning.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 9: Implement `screen set`, `screen clear`, and `screen activate`

- [ ] Implement `screen set` with batched field updates in one command.
- [ ] Require exact runtime verification before curated screen mutations.
- [ ] Implement `screen clear <layer>` with explicit layer selection only.
- [ ] Implement `screen activate slow|fast`.
- [ ] Support combined `screen set ... --activate slow|fast`.
- [ ] Compile high-level screen mutations into runtime-helper Lua calls.
- [ ] Add unit tests for command parsing, mixed-field validation, runtime gating, and Lua compilation.
- [ ] Hardware gate: confirm layered visibility, timeout restart, fallback, and non-preemption behavior.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 10: Complete runtime lifecycle commands

- [ ] Implement `runtime upgrade` for older owned bundle versions.
- [ ] Implement `runtime repair` for drifted owned content.
- [ ] Implement `runtime remove` without touching unrelated device state.
- [ ] Keep ownership checks explicit in the runtime manifest and code paths.
- [ ] Add unit tests for upgrade eligibility, repair planning, and remove safety boundaries.
- [ ] Hardware gate: confirm upgrade, repair, and remove work safely on real hardware.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 11: Hardening and release readiness

- [ ] Polish CLI help text and diagnostics.
- [ ] Add regression tests for packet encoding, manifest verification, field parsing, and command parsing.
- [ ] Confirm Linux and macOS build health.
- [ ] Write user-facing usage docs for `device`, `runtime`, and `screen`.
- [ ] Record the hardware validation matrix and known limits such as the `5-10` visible updates/sec budget.
- [ ] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

## Step completion log

Update this section as work lands.

- Step 1: `completed on 2026-06-12`
- Step 2: `completed on 2026-06-12`
- Step 3: `in progress - software complete, discovery hardened against missing device nodes, hardware validation still pending on a host runtime with direct serial access`
- Step 4: `in progress - software complete, hardware validation pending on a host runtime with direct serial access`
- Step 5: `not started`
- Step 6: `not started`
- Step 7: `not started`
- Step 8: `not started`
- Step 9: `not started`
- Step 10: `not started`
- Step 11: `not started`

## Recommended session workflow

1. Pick exactly one step as `in progress`.
2. Finish code and unit tests for that step.
3. Run `cargo fmt --check`, `cargo test`, and `cargo check`.
4. Run hardware validation if the step touches real-device behavior.
5. Update both `Session handoff state` and `Step completion log` before ending the session.
