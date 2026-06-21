# VSN1 CLI Architecture And Implementation Plan

## Runtime redesign note

The original runtime sections in this document were written around one bundled runtime family plus exact hash matching.

That is no longer the intended direction.

Use `09-runtime-discovery-and-lifecycle-redesign.md` as the controlling runtime plan for all new runtime work. When this file and `09` disagree, follow `09`.

For the follow-up scope that makes runtime layers manifest-defined instead of fixed to `persistent`, `slow`, and `fast`, use `10-manifest-defined-runtime-layers-plan.md`, which now assumes one or more persistent layers are valid and that the last activated persistent layer becomes the active base layer.

## Decision summary

The current planned shape for `vsn1-cli` v1 is:

1. screen-first
2. one-shot CLI only
3. public commands: `device`, `runtime`, `screen`
4. public input surface: subcommands only
5. high-level screen API centered on curated parameter slots
6. runtime/profile should expose `persistent`, `slow`, and `fast` display layers
7. `slow` and `fast` are explicitly activated and auto-revert after delay
8. lower-layer updates must not preempt a currently visible higher layer
9. `fast` falls back to active `slow` when present, otherwise `persistent`
10. layer field names should use dotted public names such as `persistent.title`
11. layers may expose different curated parameter sets
12. multiple named runtime/profile bundles discovered from system, user, and dev roots
13. runtime verification should compare device slots against the frozen installed runtime copy instead of relying on manifest hashes
14. fail-fast behavior with clear diagnostics
15. reusable Rust library plus thin CLI binary
16. hardware-in-loop validation required for major milestones

## Grounding from the POC

The production architecture should be built around these confirmed findings:

1. The viable live-update path is runtime-driven.
   - Stored config installs the LCD runtime and draw handler.
   - Live screen updates should primarily call runtime helpers such as `update_param(...)`.
   - The stored LCD init must also establish renderer-required globals and visible-state init such as palette globals like `c[...]` and explicit brightness init via `glsb(255)`; helper calls can succeed without visible output if that init surface is incomplete.

2. Live `IMMEDIATE` payloads must be framed as `<?lua --[[@cb]] ... ?>`.

3. Host send rate is not the same as visible screen-update rate.
   - The reliable visible budget is currently about `5-10` updates/sec depending on payload shape.
   - Repeated callers should send the latest desired state, not every transient intermediate state.

4. Provisioning/config writes are a separate path from fast live updates.
   - Config writes are slower and should be treated as install/repair operations, not the normal update path.

5. The runtime needs layered visibility semantics.
   - `persistent` is the base content layer.
   - `slow` is a temporary overlay with a `5s` timeout.
   - `fast` is a temporary overlay with a `1s` timeout.
   - lower-layer state changes must not steal visibility from a higher active layer.

## Proposed codebase structure

Keep the initial implementation as a single Rust package with both a library and a binary.

```text
vsn1-cli/
  Cargo.toml
  assets/
    runtimes/
      default/
        manifest.toml
        lcd-init.lua
        lcd-draw.lua
        system-init.lua
        ...
  src/
    lib.rs
    main.rs
    error.rs
    protocol.rs
    transport.rs
    device.rs
    runtime.rs
    runtime_bundle.rs
    targeting.rs
    screen.rs
    raw.rs
```

Rationale:

1. This satisfies the library-plus-binary decision without overcommitting to a multi-crate workspace too early.
2. It keeps provisioning, protocol, and screen logic separate enough to evolve independently.
3. It stays minimal while the runtime surface is still being discovered.

## Architectural layers

### 1. Transport and protocol layer

Responsibilities:

1. discover compatible USB serial devices
2. open serial at `2000000` baud
3. encode/decode Grid packet frames
4. send `IMMEDIATE` and `CONFIG` packets
5. expose low-level request/response helpers for library internals

Important rules:

1. Keep the fast one-shot send path separate from the slower provisioning/config path.
2. Treat framing and packet encoding as deterministic library internals.
3. Preserve debug/response decoding for diagnostics, but do not block the normal fast path on unnecessary read-back behavior.

### 2. Device layer

Responsibilities:

1. enumerate reachable VSN1-compatible devices/modules
2. expose device metadata for `device list` and `device info`
3. centralize default targeting rules

Planned targeting policy:

1. default screen behavior is broadcast-first
2. explicit overrides should be supported via flags such as `--dx` and `--dy`
3. device-oriented commands should surface the actual discovered topology so the user can move from broadcast to explicit targeting when needed

### 3. Runtime bundle and provisioning layer

Responsibilities:

1. own the bundled known-good runtime/profile asset set
2. install runtime assets into the correct stored script/config locations
3. verify exact installed version/content match
4. support `install`, `verify`, `upgrade`, `repair`, and `remove`

Recommended runtime bundle format:

1. a manifest with:
   - bundle version
   - supported device/runtime compatibility notes
   - owned script/config locations
   - install order
   - expected content hashes
2. one file per provisioned script body
3. one exact-match identity mechanism:
   - exact normalized content hash comparison is preferred
   - also embed a human-readable runtime version marker inside the installed runtime where practical

Ownership model:

1. `vsn1-cli` should explicitly own only the script/config slots it provisions.
2. `remove` should only clear or restore slots owned by the manifest.
3. The CLI should not touch unrelated device state.

### 4. Screen abstraction layer

Responsibilities:

1. define the curated parameter-slot API
2. validate user-provided slot names and value types
3. compile high-level updates into one or more runtime-helper calls
4. expose `set` and `clear` as the stable first screen operations
5. model layered state and activation separately

Recommended model:

1. represent supported screen fields as a curated enum or registry
2. map each supported field to a known runtime helper invocation shape
3. allow one `screen set` command to update multiple curated fields in one invocation
4. encode layer in the public field names via prefixes such as `persistent_`, `slow_`, and `fast_`

That last point matters because:

1. v1 is one-shot only
2. the visible update budget is limited
3. batching multiple field changes into a single command is better than forcing callers to spawn the CLI repeatedly for each individual field

Layer model recommendation:

1. `persistent` holds the default long-lived state
2. `slow` holds temporary notification state
3. `fast` holds short reaction state
4. the runtime should render the highest-priority currently active layer
5. activation deadlines should live on the device-side runtime so visibility survives one-shot CLI process exit
6. `fast` should fall back to active `slow` when `slow` has not yet expired, otherwise to `persistent`
7. re-activating `slow` or `fast` should restart that layer's timer from now

### 5. Raw operation layer

Responsibilities:

1. expose the public expert-facing `screen raw` escape hatch
2. send arbitrary framed Lua without pretending it is part of the stable high-level API
3. keep raw behavior clearly documented as advanced and less stable than curated slot operations

Recommended safety boundary:

1. `screen raw` should be explicit and obviously expert-facing
2. high-level runtime checks should remain strict for `screen set` and `screen clear`
3. raw commands should still use the same transport/protocol machinery and diagnostics path

## Proposed command surface

### `device`

Initial subcommands:

1. `device list`
2. `device info`

Purpose:

1. enumerate available devices and modules
2. show targetable `dx/dy` information
3. make broadcast behavior observable instead of opaque

### `runtime`

Initial subcommands:

1. `runtime install`
2. `runtime verify`
3. `runtime upgrade`
4. `runtime repair`
5. `runtime remove`
6. `runtime status`

Expected behavior:

1. `install` provisions the bundled runtime when absent
2. `verify` checks exact manifest/content match
3. `upgrade` moves from an older owned runtime to the bundled one
4. `repair` reapplies the bundled runtime when verification fails
5. `remove` deletes only the runtime bundle's owned slots
6. `status` reports whether the expected runtime is present and exact-match compatible

### `screen`

Initial subcommands:

1. `screen set`
2. `screen clear`
3. `screen raw`
4. `screen activate`

Recommended behavior:

1. `screen set`:
   - accepts one or more curated slot assignments
   - validates names and value types
   - requires exact runtime match before sending runtime-helper updates
   - updates stored state for the addressed layer without implicitly changing visible priority
   - should also support an explicit activate option for temporary layers via `--activate slow|fast`
2. `screen clear`:
   - resets supported curated screen state to a known blank/default state
   - requires exact runtime match
   - should require an explicit target layer
3. `screen raw`:
   - accepts a raw Lua snippet
   - frames it as `<?lua ... ?>`
   - is intended for experts and diagnostics
4. `screen activate`:
   - explicitly activates `slow` or `fast`
   - triggers the runtime to show that layer temporarily
   - relies on device-side timeout behavior to revert automatically

## Data model recommendations

### Curated field registry

Define a stable host-side registry for supported fields, for example:

1. field name
2. layer
3. value type
4. runtime mapping
5. clear/default behavior

Example internal shape:

```rust
pub enum ScreenLayer {
    Persistent,
    Slow,
    Fast,
}

pub struct ScreenFieldSpec {
    pub public_name: &'static str,
    pub layer: ScreenLayer,
    pub value_kind: ScreenValueKind,
    pub runtime_key: &'static str,
}

pub enum ScreenValueKind {
    Text,
    Int,
    Float,
    Bool,
}
```

The exact field list should come from the bundled runtime asset inventory, not from guesswork, and it may differ by layer.

### Runtime bundle manifest

The runtime bundle manifest should minimally capture:

1. bundle version
2. owned target locations
3. script asset filenames
4. normalized content hashes
5. install sequence
6. compatibility notes

This manifest becomes the single source of truth for provisioning and exact-match verification.

### Layered runtime state

The runtime bundle should likely install a device-side state model that includes:

1. parameter values for each layer
2. current active layer
3. activation expiry for temporary layers
4. redraw logic that picks the highest visible layer by priority and timeout state
5. fallback logic where `fast` can reveal still-active `slow` without reconstructing host state

This is important because the CLI is one-shot only. Auto-reversion cannot depend on a long-lived host process.

## Execution flow

### Screen update flow

1. parse CLI command
2. resolve targeting: broadcast by default, explicit target if provided
3. verify exact runtime match
4. validate curated field assignments such as `persistent.title=...` or `fast.action=...`
5. compile to runtime-helper Lua
6. frame as `<?lua ... ?>`
7. open serial port
8. send one `IMMEDIATE` packet
9. return success or fail with diagnostics

### Runtime install/repair/upgrade flow

1. parse CLI command
2. resolve target device/module
3. load bundled manifest and assets
4. write owned config/runtime scripts in manifest order
5. read back or otherwise verify owned slots
6. report exact installed bundle version/hash status

### Layer activation flow

1. parse `screen activate <layer>`
2. resolve targeting
3. verify exact runtime match
4. reject unsupported layers such as `persistent` if activation is only for temporary overlays
5. call the runtime helper that sets the active layer and expiry behavior
6. return immediately; runtime owns the timed reversion

### Combined set-and-activate flow

1. parse `screen set ... --activate slow|fast`
2. resolve targeting
3. verify exact runtime match
4. validate that all assigned fields belong to the activated layer or to a supported mixed command shape
5. update layer state
6. activate the temporary layer and restart its timer
7. return immediately; runtime owns visibility and reversion

This combined flow is preferred over adding a separate `screen show` command.

### Runtime verify flow

1. read owned runtime/config locations
2. normalize contents
3. compare exact hashes against the bundled manifest
4. fail if any slot differs, is missing, or is attached to the wrong target

## Failure semantics

The CLI should be intentionally strict.

1. no suitable device found: fail
2. multiple candidate targets for a non-broadcast command: fail
3. runtime mismatch on curated screen commands: fail
4. unknown slot name or invalid value type: fail
5. `screen clear` with no explicit layer: fail
6. provisioning verification mismatch: fail
7. transport write/open error: fail

Diagnostics should report:

1. what failed
2. which target was involved
3. whether the failure happened in discovery, transport, provisioning, or runtime compatibility
4. the next likely action, such as `runtime install`, `runtime verify`, or `device list`

## Phased implementation plan

### Phase 1: library skeleton and protocol baseline

Deliver:

1. Cargo package with `lib.rs` and `main.rs`
2. serial discovery/open helpers
3. Grid packet encode/send path
4. shared error types and diagnostics formatting
5. `device list` and `screen raw`

Exit criteria:

1. raw framed Lua send works on hardware
2. target override flags work
3. basic diagnostics are readable

### Phase 2: runtime bundle and verification core

Deliver:

1. bundled runtime assets and manifest format
2. `runtime install`
3. `runtime verify`
4. exact-match hashing/identity logic
5. `runtime status`

Exit criteria:

1. a device can be provisioned from the bundled assets
2. exact verify detects both match and mismatch cases on hardware

### Phase 3: screen-first stable API

Deliver:

1. curated field registry
2. `screen set`
3. `screen clear`
4. `screen activate`
5. runtime-match gate on curated screen commands
6. ability to batch multiple field updates in one command
7. device-side layered visibility logic
8. combined set-and-activate flow for temporary layers

Exit criteria:

1. supported field updates reliably change the screen on hardware
2. `screen clear` returns the display to a known blank/default state
3. high-level commands fail cleanly on mismatch or invalid input
4. `slow` reliably reverts after `5s`
5. `fast` reliably reverts after `1s`
6. lower-layer updates do not preempt a higher active layer
7. `fast` correctly reveals still-active `slow` after expiry

### Phase 4: runtime lifecycle completion

Deliver:

1. `runtime upgrade`
2. `runtime repair`
3. `runtime remove`
4. ownership-safe handling of managed slots

Exit criteria:

1. upgrade from an older owned bundle works
2. repair restores a tampered runtime
3. remove only touches owned content

### Phase 5: hardening and release readiness

Deliver:

1. hardware validation matrix for Linux and macOS
2. stable CLI help text and docs
3. error-message polish
4. regression tests for packet encoding, manifest verification, and CLI parsing

Exit criteria:

1. major commands are hardware-validated
2. the bundled runtime story is reproducible end-to-end
3. the CLI surface is stable enough to document as v1

## Testing strategy

### Non-hardware tests

Use for:

1. packet framing and encoding
2. manifest parsing and exact-hash verification logic
3. curated field parsing and validation
4. command parsing and diagnostics

### Hardware-in-loop tests

Required for:

1. raw framed `IMMEDIATE` sends
2. runtime provisioning and verification
3. reliable visible `screen set` behavior
4. clear/reset behavior
5. broadcast vs explicit targeting behavior
6. lifecycle commands that mutate stored config
7. layered activation and timed reversion behavior

### Suggested milestone checks

1. confirm the exact curated field set against real installed runtime scripts
2. confirm the bundled runtime installs cleanly on a blank or repaired target
3. confirm repeated one-shot `screen set` invocations behave acceptably within the proven visible update budget
4. confirm exact-match verification catches even small script drift
5. confirm layered fallback and timer restart behavior on hardware

## Recommended immediate next work

1. create the Rust package skeleton in `vsn1-cli/`
2. inventory the runtime/profile assets that will become the bundled known-good set
3. define the manifest format for owned slots and exact-match verification
4. identify and document the first curated dotted field names from the installed runtime
5. design the device-side layered runtime state and helper function surface
6. implement `screen raw` and `device list` first as the smallest end-to-end hardware path

## Main architectural risk

The main remaining risk is not transport viability.

It is the exact shape of the curated parameter-slot surface and the provisioning bundle:

1. which runtime scripts the CLI will own
2. which slot names are stable enough to expose publicly
3. how safely `remove` can restore owned state without touching unrelated config
4. how the layered timeout and fallback logic should behave at runtime boundaries

That risk is now narrower: the high-level layer semantics are mostly decided, but the exact curated dotted field inventory still needs to be derived from the bundled runtime design.
