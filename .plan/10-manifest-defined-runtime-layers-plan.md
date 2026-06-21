# VSN1 CLI Manifest-Defined Runtime Layers Plan

## Purpose

This document captures a follow-up implementation plan for moving `vsn1-cli` from a hard-coded three-layer model to a manifest-defined layer model.

It is written so a new session with no prior conversational context can pick up the work directly from disk.

## Current state snapshot

As of the completion of step 16, the implementation still hard-codes the layer model even though field inventory already comes from the installed runtime manifest.

Current hard-coded constraints:

1. Host-side layer names are fixed to `persistent`, `slow`, and `fast` in `src/screen.rs`.
2. Only `slow` and `fast` are activatable from the CLI.
3. Activation timeouts are fixed at `5s` and `1s`.
4. Clear defaults are hard-coded by public field name.
5. Host compilation is split between one persistent update path and two overlay-specific update helpers.
6. The device-side default runtime Lua stores state in three fixed buckets and exposes fixed helper functions that match those three layers.

This means the manifest currently defines field inventory within a fixed layer framework, not the layer framework itself.

## Goal

Make the runtime manifest define its own layers so a runtime can choose:

1. layer names
2. layer priority order
3. which layers are persistent versus temporary
4. per-layer activation timeout behavior
5. per-layer field inventory

The host CLI should stop assuming that the public layer set is always `persistent`, `slow`, and `fast`.

## Recommended scope boundary

Recommended scope for this follow-up:

1. Let the manifest define arbitrary layer names.
2. Allow one or more non-expiring persistent layers.
3. Define persistent-layer selection by activation recency: the last activated persistent layer wins until another persistent layer is activated.
4. Allow zero or more temporary activatable layers.
5. Keep the public command surface as `screen set`, `screen clear`, and `screen activate`.
6. Replace the current special-case host compiler with a generic runtime helper surface so layer names no longer matter to the host.

Avoid a half-measure where only the names become dynamic but the compiler still assumes one persistent path plus two special overlays. That would add complexity without actually removing the architectural coupling.

## Recommended manifest extension

Add an explicit layer inventory to the runtime manifest.

Recommended shape:

```toml
[[layers]]
name = "base"
priority = 0
activation = "persistent"
notes = "Primary long-lived content layer."

[[layers]]
name = "alt"
priority = 1
activation = "persistent"
notes = "Alternate long-lived content layer that becomes visible when activated."

[[layers]]
name = "notice"
priority = 10
activation = "temporary"
timeout_ms = 5000
notes = "Medium-priority notification layer."

[[layers]]
name = "reaction"
priority = 20
activation = "temporary"
timeout_ms = 1000
notes = "High-priority transient reaction layer."
```

Recommended validation rules:

1. `name` must be unique.
2. `priority` must be unique.
3. At least one layer must have `activation = "persistent"`.
4. `temporary` layers must define `timeout_ms`.
5. `persistent` layers must not define `timeout_ms`.
6. Field entries must reference an existing layer name.
7. Public field names must still use `<layer>.<field>` syntax where the prefix matches the declared layer name.
8. Multiple persistent layers are valid; their visibility order is not priority-based between themselves and instead follows activation recency, with the most recently activated persistent layer acting as the active base layer.

## Recommended host/runtime helper model

The current host compiler is tightly coupled to fixed helper names such as `P`, `S`, `F`, and `A`.

To make layers truly manifest-defined, move to a generic device-side helper contract.

Recommended runtime helper surface:

1. `set_field(layer_name, runtime_key, value)`
2. `activate_layer(layer_name)`
3. optional `clear_layer(layer_name)` if runtime-side clear becomes preferable later

Recommended host behavior:

1. `screen set base.title=Tempo` compiles to one or more `set_field(...)` calls.
2. `screen set alt.title=Pitch --activate alt` compiles to `set_field(...)` plus `activate_layer(...)`, making `alt` the active persistent base layer.
3. `screen set notice.message='Disk full' --activate notice` compiles to `set_field(...)` plus `activate_layer(...)`.
4. `screen activate reaction` compiles to `activate_layer("reaction")`.
5. `screen clear notice` can remain host-side by sending manifest-derived default values back through `set_field(...)` unless a runtime-side clear primitive is later justified.

This is the key simplification that removes hard-coded layer names from the host compiler.

## Recommended device-side runtime model

The runtime Lua should stop storing state in fixed `p`, `s`, and `f` buckets.

Recommended direction:

1. Store layers in a table keyed by manifest layer name.
2. Store current values per layer.
3. Store the currently active persistent layer name.
4. Store activation expiry per temporary layer.
5. Resolve visible content by descending priority over currently active temporary layers.
6. When no temporary layer is active, render the currently active persistent layer.
7. Activating a persistent layer replaces the current persistent base immediately and does not expire.

This implies the default runtime assets under `assets/runtimes/default/` must be rewritten alongside the host changes.

## Public CLI implications

### `screen set`

1. Keep `FIELD=VALUE` syntax.
2. Validate the field name against the installed runtime manifest.
3. Validate `--activate <layer>` against the installed runtime layer inventory.
4. Allow activation for both persistent and temporary layers.
5. For persistent layers, `--activate` selects the active base layer without a timeout.

### `screen clear`

1. Replace the current compile-time `ValueEnum` layer parsing with runtime-validated string parsing.
2. Accept any installed manifest layer name.
3. Continue requiring an explicit layer argument.

### `screen activate`

1. Replace the current compile-time `ValueEnum` parsing with runtime-validated string parsing.
2. Accept any installed manifest layer name.
3. Persistent-layer activation should switch the active base layer.
4. Temporary-layer activation should start or restart that layer's timeout.
5. Reject unknown layer names with clear diagnostics.

## Compatibility and migration recommendation

Recommended migration strategy:

1. Introduce explicit `[[layers]]` entries while keeping the current default runtime behavior unchanged semantically.
2. Relax the manifest model to allow one or more persistent layers so the future runtime contract does not bake in a single-base assumption.
3. Rewrite the default runtime manifest to declare three layers explicitly.
4. Rewrite the default runtime Lua helpers to the generic helper surface.
5. Rewrite the host compiler and CLI to consume the generic manifest-driven model.
6. Remove the remaining hard-coded layer enum and special-case compiler branches.

Do not try to support both the old fixed-helper runtime contract and the new generic-helper contract indefinitely. That would create a dual model with little value.

## Risks and constraints

1. **Runtime Lua size risk**
   - A generic table-driven layer engine may increase stored script size.
   - This must be checked against the Grid CONFIG payload limit after framing.

2. **Host compiler complexity**
   - The host should not evolve into a small template engine with arbitrary manifest-defined Lua snippets unless there is a concrete need.
   - Prefer one generic helper contract instead.

3. **Dynamic CLI parsing**
   - Moving from `ValueEnum` to runtime-validated string parsing will shift some errors from parse-time to execution-time.
   - That is acceptable here because the valid layer set becomes runtime-dependent.

4. **Hardware behavior coupling**
   - Layer priority, timeout, fallback, and redraw behavior all need hardware validation after the runtime rewrite.

## Recommended implementation phases

### Phase 1: Manifest schema and validation

1. Add `[[layers]]` support to `runtime_bundle.rs`.
2. Validate layer uniqueness, priority, activation kind, and timeout rules, while allowing one or more persistent layers.
3. Validate that every field references a declared layer.
4. Keep the current default runtime loading by adding explicit layer entries to the default manifest.

### Phase 2: Screen registry becomes layer-inventory-driven

1. Replace `ScreenLayer` enum usage with manifest-backed layer identifiers.
2. Replace `Layer` and `ActivationLayer` `ValueEnum` parsing in the CLI with string arguments.
3. Add runtime validation for `screen clear <layer>` and `screen activate <layer>`.
4. Allow persistent-layer activation to switch the active base layer.
5. Update tests for unknown layer names, invalid activation targets, and persistent-layer activation success.

### Phase 3: Generic host compiler contract

1. Replace the current persistent/slow/fast compiler split with generic helper calls.
2. Remove special-cased `compile_persistent_update`, `compile_overlay_update`, and fixed activation mappings.
3. Keep value-kind validation from the field registry.
4. Ensure combined `screen set ... --activate <layer>` works for both persistent and temporary layers.

### Phase 4: Device-side runtime rewrite

1. Rewrite the default runtime Lua to use manifest-defined layer tables.
2. Add generic `set_field(...)` and `activate_layer(...)` helpers.
3. Track the active persistent layer separately from temporary expiries.
4. Implement visibility resolution where temporary layers use priority ordering and the active persistent layer is the fallback base.
5. Verify the rewritten runtime still fits the Grid CONFIG size limit.

### Phase 5: Docs, migration, and hardware validation

1. Update README and help text to stop implying fixed `slow` and `fast` layer names.
2. Record the manifest layer contract in the runtime planning docs.
3. Run hardware validation for:
   - persistent-layer activation and base-layer switching
   - temporary layer activation
   - timeout expiry
   - priority ordering among temporary layers
   - fallback from temporary layers to the active persistent layer
   - reactivation timer restart
   - mixed field updates across manifest-defined layer names

## Checklist

### Step 17: Add manifest-defined layer inventory

- [x] Add `[[layers]]` to the runtime manifest contract.
- [x] Validate layer uniqueness, priority, and activation rules.
- [x] Require fields to reference declared layers.
- [x] Update the default runtime manifest to declare its current three layers explicitly.
- [x] Add unit tests for invalid layer definitions and invalid field-to-layer references.

### Step 18: Make CLI layer parsing runtime-driven

- [x] Replace compile-time screen layer enums in CLI parsing with string-based runtime validation.
- [x] Allow `screen clear <layer>` for any declared manifest layer name.
- [x] Allow `screen activate <layer>` for any declared manifest layer name, including persistent layers.
- [x] Add tests for unknown layer names, invalid activation targets, persistent-layer activation success, and successful dynamic layer parsing.

### Step 19: Replace hard-coded host compiler layer logic

- [ ] Introduce a generic runtime helper contract for field updates and activation.
- [ ] Remove persistent/slow/fast-specific compile branches from `screen.rs`.
- [ ] Keep field-type validation and clear-value handling manifest-driven.
- [ ] Add regression tests for generic set, clear, and set-and-activate compilation.

### Step 20: Rewrite the default runtime to the generic layer engine

- [ ] Rewrite `assets/runtimes/default/lcd-init.lua` and any dependent runtime assets to use manifest-defined layer tables.
- [ ] Preserve the current visible behavior using explicit manifest layer declarations.
- [ ] Keep the rewritten runtime under the Grid CONFIG size limit.
- [ ] Add tests or fixture coverage for the new default runtime manifest and helper contract.

### Step 21: Validate manifest-defined layers end to end

- [ ] Update README/help text for dynamic layer names.
- [ ] Run `cargo fmt --check`, `cargo test`, `cargo check`, `cargo check --target x86_64-apple-darwin`, and `cargo check --target aarch64-apple-darwin`.
- [ ] Run hardware validation on a real device for dynamic layer priority, activation, timeout, fallback, and reactivation behavior.
- [ ] Record the results in `docs/validation-matrix.md` and `08-implementation-checklist.md`.

## Recommended next-session starting point

Start with Step 18.

Specifically:

1. continue with runtime-driven CLI layer parsing and activation validation
2. allow persistent-layer activation in the runtime-driven model
3. preserve the new manifest rule that one or more persistent layers are valid

Do not start with the Lua rewrite first. The manifest contract needs to be stable before the host compiler and device runtime are rewritten around it.
