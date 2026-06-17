# VSN1 CLI Implementation Checklist

## Purpose

This checklist turns the decisions in `01` through `07` into an implementation sequence that can be executed across multiple agent sessions.

## Current baseline

1. `vsn1-cli/` currently contains planning documents only.
2. No Rust package exists yet.
3. The first implementation step must create a compiling crate skeleton.

## Session handoff state

- Overall status: `in progress`
- Last completed step: `step 8`
- In-progress step: `step 9`
- Last verification run: `cargo fmt --check`, `cargo test`, `cargo check` (pass on 2026-06-17 after fixing curated screen double-open handling and adding PAGEACTIVE + PAGESTORE persistence to runtime install with regression tests)`
- Last hardware validation: `2026-06-17 on Linux host: cargo run -- runtime install --dx 0 --dy 0 successfully provisioned owned LCD slots page 0 / element 13 / events 0 and 8 on /dev/ttyACM0 (VID:PID 303a:8123) and immediately verified an exact bundled-runtime match on dx=0 dy=0; current session could not run step 9 hardware validation because cargo run -- device list reported no supported VSN1/Grid USB serial devices found`
- Open blockers: `none`
- Next session start point: `hardware-validate that runtime install now survives reconnect via PAGESTORE, then replace the placeholder bundled LCD runtime with a real renderer/helper surface so screen set becomes visibly effective`

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
- [x] Hardware gate: confirm discovered topology and explicit targeting on a real device.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 4: Ship the first end-to-end screen path with `screen raw`

- [x] Add `raw.rs` and wire `screen raw` through the library.
- [x] Send raw framed Lua through the immediate path.
- [x] Reuse the same diagnostics and targeting behavior as other screen commands.
- [x] Add unit tests for raw command parsing, payload framing, and error reporting.
- [x] Hardware gate: confirm `screen raw` changes the screen on a real device.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 5: Define the runtime bundle contract

- [x] Create `assets/runtime/<bundle-version>/` and add a manifest file.
- [x] Define owned script/config locations, install order, and exact-match identity fields.
- [x] Add `runtime_bundle.rs` for manifest loading, normalization, and content hashing.
- [x] Capture the first bundled runtime/profile asset set from the validated POC inputs.
- [x] Record the initial curated dotted field inventory that the runtime will support.
- [x] Add unit tests for manifest parsing, content normalization, and hash comparison.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 6: Implement runtime inspection commands

- [x] Add `runtime.rs` read/verify/status primitives over owned slots.
- [x] Implement `runtime verify` and `runtime status`.
- [x] Fail on any missing, drifted, or mismatched owned content.
- [x] Keep exact bundled version matching as the only success condition.
- [x] Add unit tests for match, mismatch, missing-slot, and malformed-manifest cases.
- [x] Hardware gate: confirm verify catches both exact-match and drifted-runtime cases.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 7: Implement runtime install

- [x] Implement `runtime install` using the manifest install order.
- [x] Read back owned content and verify exact bundle match after install.
- [x] Keep writes limited to owned slots only.
- [x] Add clear diagnostics for install preconditions and failures.
- [x] Add unit tests for install planning, ordered writes, and post-install verification behavior.
- [x] Hardware gate: confirm install provisions a blank or repaired device successfully.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 8: Add the curated screen field registry

- [x] Add `screen.rs` field registry keyed by public names like `persistent.title`.
- [x] Model layer, value kind, runtime key, and clear behavior per field.
- [x] Reject unknown field names and invalid value types.
- [x] Support different curated field sets per layer.
- [x] Add unit tests for field lookup, parsing, value validation, and clear planning.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 9: Implement `screen set`, `screen clear`, and `screen activate`

- [x] Implement `screen set` with batched field updates in one command.
- [x] Require exact runtime verification before curated screen mutations.
- [x] Implement `screen clear <layer>` with explicit layer selection only.
- [x] Implement `screen activate slow|fast`.
- [x] Support combined `screen set ... --activate slow|fast`.
- [ ] Compile high-level screen mutations into runtime-helper Lua calls.
- [x] Add unit tests for command parsing, mixed-field validation, runtime gating, and Lua compilation.
- [ ] Hardware gate: confirm layered visibility, timeout restart, fallback, and non-preemption behavior.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

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
- Step 3: `completed on 2026-06-17 - Linux host validation confirmed real-device discovery on /dev/ttyACM0, successful 2000000-baud transport open, and explicit dx=0 dy=0 targeting flow on hardware`
- Step 4: `completed on 2026-06-17 - Linux host validation confirmed screen raw produces visible screen changes for both broadcast direct-draw and explicit dx=0 dy=0 raw update_param(...) and direct-draw commands`
- Step 5: `completed on 2026-06-12 - added the first bundled runtime contract with hashed LCD init/draw assets, manifest loading, normalized hash verification, and an initial curated dotted field inventory derived from the validated POC slot ownership and update_param(...) payload shape`
- Step 6: `completed on 2026-06-17 - software path added runtime.rs inspection, config-fetch verification, and CLI wiring; hardware validation on /dev/ttyACM0 at dx=0 dy=0 confirmed both drift detection before install and exact-match verification after bundled runtime install`
- Step 7: `completed on 2026-06-17 - implemented runtime install with manifest-ordered owned-slot writes, owned-slot-only scope, post-install exact-match verification, CLI wiring, and step 7 unit tests; fixed the bundled lcd-init asset to fit the real Grid CONFIG payload limit, added bundle-size validation, corrected exact-match verification to compare against the framed stored CONFIG representation returned by fetches, validated runtime install/verify/status on /dev/ttyACM0 at dx=0 dy=0, and later fixed missing PAGEACTIVE + PAGESTORE persistence so install can survive reconnects`
- Step 8: `completed on 2026-06-17 - added screen.rs bundled field-registry loading, typed layer/value metadata conversion from the manifest inventory, strict FIELD=VALUE parsing and validation, per-layer clear planning with runtime-default clear values, and step 8 unit tests covering lookup, parsing, value errors, and invalid bundled field specs`
- Step 9: `in progress on 2026-06-17 - implemented screen set/clear/activate CLI wiring, exact runtime gating before curated sends, persistent-field compilation through update_param(...) using current device-side state, direct temporary-layer state mutation/activation Lua, and step 9 software tests; hardware validation is still pending because no supported device was visible to the current session`
- Step 10: `not started`
- Step 11: `not started`

## Recommended session workflow

1. Pick exactly one step as `in progress`.
2. Finish code and unit tests for that step.
3. Run `cargo fmt --check`, `cargo test`, and `cargo check`.
4. Run hardware validation if the step touches real-device behavior.
5. Update both `Session handoff state` and `Step completion log` before ending the session.
