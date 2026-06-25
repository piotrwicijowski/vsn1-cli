# VSN1 CLI Implementation Checklist

## Purpose

This checklist turns the decisions in `01` through `09` into an implementation sequence that can be executed across multiple agent sessions.

## Current baseline

1. `vsn1-cli/` is now a working Rust crate with device, runtime, and screen commands.
2. Steps `1` through `11` are complete and validated against the earlier bundled-hash runtime model.
3. Steps `12+` track the named-runtime redesign described in `09-runtime-discovery-and-lifecycle-redesign.md`.

## Session handoff state

- Overall status: `in_progress`
- Last completed step: `step 21`
- In-progress step: `file-manager follow-up step 6`
- Last verification run: `cargo fmt --check`, `cargo test`, `cargo check` (pass on 2026-06-25 after marking Step 6 software items complete in the file-manager plan and adding module-files remove/uninstall regression coverage for backup-restore and fallback-delete behavior)`
- Last hardware validation: `2026-06-21 on Linux host: step 21 validation passed on /dev/ttyACM0 at dx=0 dy=0. runtime install default, runtime verify, and runtime status all reported exact-match compatible on installed runtime 2026-06-21-manifest-layers.1; screen set/activate validation confirmed persistent activation acceptance, slow/fast activation, fast-over-slow temporary priority, fallback from fast to slow to persistent, slow and fast timer restart behavior, lower-layer non-preemption while a higher layer remained active, and successful screen clear fallback for slow and fast.`
- Open blockers: `hardware validation remains pending for the file-manager follow-up Step 1/Step 2 gates, plus the Step 4, Step 5, and Step 6 hardware validations for file-backed install, drift-detection/repair, and remove/uninstall behavior`
- Next session start point: `main implementation plan remains complete through step 21; for the file-manager follow-up, Step 6 software items are now implemented, its hardware gate still remains, and the next code step after that validation is Step 7 in 15-file-manager-backed-runtime-plan.md for slot-backed vs file-backed behavior parity coverage`

## Rules for every step

1. Do not mark a step complete until the application compiles.
2. Add or update unit tests in the same step as the code change.
3. Run these checks before marking the step complete:
   - `cargo fmt --check`
   - `cargo test`
   - `cargo check`
4. If a step changes runtime behavior that depends on hardware, record the hardware result before closing the step.
5. At the end of each session, update the handoff state in this file.
6. For runtime work after step 11, treat `09-runtime-discovery-and-lifecycle-redesign.md` as the controlling plan when older steps mention bundled-hash behavior.

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
- [x] Compile high-level screen mutations into runtime-helper Lua calls.
- [x] Add unit tests for command parsing, mixed-field validation, runtime gating, and Lua compilation.
- [x] Hardware gate: confirm layered visibility, timeout restart, fallback, and non-preemption behavior.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 10: Complete runtime lifecycle commands

- [x] Implement `runtime upgrade` for older owned bundle versions.
- [x] Implement `runtime repair` for drifted owned content.
- [x] Implement `runtime remove` without touching unrelated device state.
- [x] Keep ownership checks explicit in the runtime manifest and code paths.
- [x] Add unit tests for upgrade eligibility, repair planning, and remove safety boundaries.
- [x] Hardware gate: confirm upgrade, repair, and remove work safely on real hardware.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 11: Hardening and release readiness

- [x] Polish CLI help text and diagnostics.
- [x] Add regression tests for packet encoding, manifest verification, field parsing, and command parsing.
- [x] Confirm Linux and macOS build health.
- [x] Write user-facing usage docs for `device`, `runtime`, and `screen`.
- [x] Record the hardware validation matrix and known limits such as the `5-10` visible updates/sec budget.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 12: Introduce named runtime discovery and repo layout migration

- [x] Move the active repo runtime from `assets/runtime/<bundle-version>` to `assets/runtimes/default`.
- [x] Remove the legacy versioned runtime directories from `assets/runtime` once the new root is wired.
- [x] Add runtime discovery across system, user, and dev roots.
- [x] Resolve runtime-name collisions by `dev` > `user` > `system` precedence.
- [x] Add unit tests for discovery, invalid runtime directories, and collision resolution.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 13: Drop hash-driven runtime identity and verify against script contents

- [x] Remove manifest hash requirements from the runtime bundle contract.
- [x] Keep content normalization and framed-script comparison, but compare against actual runtime assets.
- [x] Rework `runtime verify` and `runtime status` around the frozen installed runtime copy.
- [x] Add unit tests for content-match, content-drift, missing-slot, and malformed-manifest cases without hashes.
- [x] Hardware gate: confirm verify catches both matching and drifted script-content cases using the frozen runtime copy.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 14: Add frozen runtime and pre-install backup persistence

- [x] Add host-side storage helpers for `~/.config/vsn1-cli/runtime` and `~/.config/vsn1-cli/pre-install`.
- [x] Freeze the installed runtime by copying the selected runtime directory locally after successful install or upgrade.
- [x] Capture pre-install owned-slot contents plus a restore manifest before `runtime install` overwrites the device.
- [x] Represent blank or missing slots in the backup so uninstall can restore them to empty content.
- [x] Add unit tests for frozen-runtime replacement, backup replacement, and missing-slot backup behavior.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 15: Rework runtime install, upgrade, remove, and uninstall semantics

- [x] Change `runtime install` to require a runtime name and refresh the pre-install backup.
- [x] Change `runtime upgrade` to require a runtime name and skip backup refresh.
- [x] Make `runtime remove` restore from backup when present.
- [x] Add `runtime uninstall` as an alias of `runtime remove`.
- [x] When backup data is missing or incomplete, clear owned slots, print a warning, and continue cleanup.
- [x] Add unit tests for install-name resolution, upgrade-without-backup-refresh, backup-restore removal, and fallback-to-clear removal.
- [x] Hardware gate: confirm install, upgrade, uninstall-with-restore, and uninstall-fallback-to-clear on a real device.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`.

### Step 16: Reconnect curated screen behavior and docs to the frozen runtime model

- [x] Load curated screen field metadata from the frozen installed runtime instead of a compile-time bundled runtime.
- [x] Update CLI help text, README usage, and validation docs for named runtimes and backup-based uninstall.
- [x] Add regression tests covering frozen-runtime registry loading and runtime command parsing.
- [x] Confirm Linux and macOS build health after the runtime redesign.
- [x] Verify: `cargo fmt --check`, `cargo test`, `cargo check`, `cargo check --target x86_64-apple-darwin`, `cargo check --target aarch64-apple-darwin`.

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
- Step 9: `completed on 2026-06-17 - implemented screen set/clear/activate CLI wiring, exact runtime gating before curated sends, replaced the placeholder bundled lcd-draw asset with a minimal real renderer, compared the curated IMMEDIATE path against the working POC and found that packet sending still matches while the script body differed materially, rewrote the bundled runtime plus host compiler around compact stored helper calls, then used fresh Grid Editor hardware feedback to confirm draw-slot invocation on dx=0 dy=0, fixed the missing palette global c in bundled runtime 2026-06-17-screen-first.4, restored visible output by adding glsb(255) to the owned lcd-init runtime in bundled version 2026-06-17-screen-first.5, addressed garbled slow-layer rendering by switching owned temporary-layer drawing to literal RGB colors in bundled version 2026-06-17-screen-first.6, fixed the temporary-layer activation regression introduced there in bundled version 2026-06-17-screen-first.7, simplified the slow-layer renderer in bundled version 2026-06-17-screen-first.8 to remove the remaining white-panel/black-bar artifact while staying under the Grid CONFIG payload limit, and completed hardware validation for layered visibility, timeout restart, fallback, and non-preemption behavior on dx=0 dy=0`
- Step 10: `completed on 2026-06-17 - implemented runtime upgrade, runtime repair, and runtime remove; added explicit managed-slot lifecycle gating, managed-hash-based remove safety checks, then corrected runtime remove on hardware by switching from zero-length CONFIG writes to explicit empty-script writes; hardware validation on /dev/ttyACM0 at dx=0 dy=0 confirmed remove, reinstall, negative repair/upgrade checks, positive repair after out-of-band drift, and positive upgrade from exact older bundled runtime 2026-06-17-screen-first.5 to 2026-06-17-screen-first.8`
- Step 11: `completed on 2026-06-17 - polished clap help text across device/runtime/screen commands, tightened actionable diagnostics for targeting and field errors, added regression coverage for Lua framing, bundled runtime family loading, field parsing with embedded equals, command help text, and command parsing, confirmed Linux baseline checks plus cross-target macOS cargo check on x86_64-apple-darwin and aarch64-apple-darwin, and added user-facing README usage docs plus a validation matrix capturing current hardware coverage and the 5-10 visible updates/sec budget`
- Step 12: `completed on 2026-06-20 - moved the checked-in dev runtime to assets/runtimes/default, removed the legacy versioned assets/runtime tree, added runtime discovery across system/user/dev roots with dev > user > system name-collision precedence, added discovery regression tests, and refactored runtime-family tests so they no longer depend on checked-in historical bundles`
- Step 13: `completed on 2026-06-20 - removed manifest hash requirements from the runtime bundle contract, switched verify/status to compare device contents against the frozen installed runtime copy, added regression coverage for no-installed-runtime and frozen-runtime inspection behavior, and validated on Linux hardware at /dev/ttyACM0 with exact-match, drifted-content, and missing-local-runtime cases on dx=0 dy=0`
- Step 14: `completed on 2026-06-20 - added ~/.config/vsn1-cli runtime storage helpers, froze successful installs/upgrades/repairs into ~/.config/vsn1-cli/runtime, captured pre-install owned-slot contents into a loadable ~/.config/vsn1-cli/pre-install bundle with empty-slot representation, added replacement semantics for both directories, and added regression coverage for backup capture and persistence replacement`
- Step 15: `completed on 2026-06-20 - switched runtime install/upgrade to named runtime selection and runtime discovery resolution, changed remove/uninstall to backup-restore-or-clear semantics with frozen-runtime cleanup, added command and lifecycle regression coverage, and validated on Linux hardware at /dev/ttyACM0 for install default, remove restore, and fallback-to-clear remove behavior on dx=0 dy=0`
- Step 16: `completed on 2026-06-20 - switched curated screen set/clear metadata loading from compile-time bundled assets to the frozen installed runtime copy, updated help text plus README and validation docs for the named-runtime lifecycle, added installed-runtime screen registry regression coverage, and reconfirmed Linux plus macOS cross-target build health`
- Step 17: `completed on 2026-06-21 - added explicit manifest [[layers]] support and validation to runtime bundle loading, enforced that fields reference declared layers, updated the default runtime manifest to declare persistent/slow/fast explicitly, carried layer metadata into pre-install backup manifests, added regression coverage for invalid layer definitions and undeclared field-layer references, and then relaxed the persistent-layer rule to allow one or more persistent layers so the follow-up runtime model can support last-activated-persistent-layer-wins semantics`
- Step 18: `completed on 2026-06-21 - replaced compile-time CLI layer enums with runtime-validated string layer names, taught the screen registry to load manifest layer inventory and validate layer names at runtime, allowed screen clear and screen activate to resolve declared manifest layers from the installed runtime copy, added persistent-layer activation success coverage for the current fixed contract where activating the default persistent layer is a validated no-op, and updated help text plus regression coverage for unknown layers and dynamic layer parsing`
- Step 19: `completed on 2026-06-21 - replaced the remaining persistent/slow/fast-specific host compiler branches with generic set_field(layer, runtime_key, value) and activate_layer(layer) helper calls, removed the fixed persistent activation no-op path so all declared layers now compile through the same host contract, and added regression coverage for generic set, clear, activate, and set-and-activate Lua compilation`
- Step 20: `completed on 2026-06-21 - rewrote the default runtime manifest/assets to the generic manifest-defined layer engine, replaced the old fixed P/S/F/A helper surface with generic set_field(...) and activate_layer(...) entrypoints backed by layer tables plus an active persistent base, preserved the current persistent/slow/fast visuals and timeout behavior, trimmed the lcd-draw asset back under the Grid CONFIG payload limit, and added regression coverage for the bundled runtime helper contract and payload-budget enforcement`
- Step 21: `completed on 2026-06-21 - updated README/help text for runtime-defined layer names and the generic helper contract, re-ran Linux plus macOS cross-target verification, and recorded real-device validation on /dev/ttyACM0 at dx=0 dy=0 covering runtime install/verify/status exact-match behavior, persistent activation acceptance, slow/fast activation, temporary priority, timeout expiry, fallback, timer restart, lower-layer non-preemption, and slow/fast clear fallback for the shipped default runtime`

## Recommended session workflow

1. Pick exactly one step as `in progress`.
2. Finish code and unit tests for that step.
3. Run `cargo fmt --check`, `cargo test`, and `cargo check`.
4. Run hardware validation if the step touches real-device behavior.
5. Update both `Session handoff state` and `Step completion log` before ending the session.

## Follow-up scoped work

1. For future work to make runtime layers manifest-defined instead of fixed to `persistent`, `slow`, and `fast`, use `10-manifest-defined-runtime-layers-plan.md` as the starting point.
2. The manifest-defined layer follow-up now assumes one or more persistent layers are valid and that the most recently activated persistent layer should become the active base layer.
3. The manifest-defined layer follow-up is complete for the shipped default runtime; future hardware coverage for persistent-to-persistent base switching will require a runtime that declares multiple persistent layers.
4. For future work to add optional daemon-backed command execution with per-device warm-port reuse and `5s` idle close, use `12-daemon-capability-plan.md`.
5. For step-by-step execution of the daemon follow-up, use `13-daemon-implementation-checklist.md`.
6. For future work to add file-manager-backed runtime provisioning through on-module event files instead of `CONFIG` slot writes, use `15-file-manager-backed-runtime-plan.md`.
