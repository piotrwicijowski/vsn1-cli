# VSN1 CLI Runtime Discovery And Lifecycle Redesign

## Purpose

This document replaces the earlier hash-centric runtime assumptions with a named-runtime model that is friendlier to power users, local tinkering, and package upgrades.

Use this as the runtime implementation source of truth for all follow-up work after step 11.

## Decision summary

1. `vsn1-cli` should support multiple named runtimes.
2. Runtime names come from directory names.
3. System runtimes live under `/usr/share/vsn1-cli/runtimes`.
4. User-created runtimes live under `~/.local/share/vsn1-cli/runtimes`.
5. When running from a dev checkout, include `vsn1-cli/assets/runtimes` in discovery.
6. Name collisions resolve by source precedence: `dev` > `user` > `system`.
7. The repository should stop using `assets/runtime/<bundle-version>/...` for active development and instead ship `assets/runtimes/default/...` first.
8. Runtime manifests remain required, but runtime lifecycle operations must not rely on content hashes.
9. `runtime install <name>` should install the selected runtime and freeze a copy under `~/.config/vsn1-cli/runtime`.
10. `runtime verify` should compare the device slot contents against the frozen runtime copy in `~/.config/vsn1-cli/runtime`.
11. `runtime install` should also save the pre-install contents of the owned slots under `~/.config/vsn1-cli/pre-install`.
12. `runtime upgrade <name>` should overwrite the device from the selected runtime like install, but it must not refresh the pre-install backup.
13. `runtime remove` and `runtime uninstall` should be aliases that restore from `~/.config/vsn1-cli/pre-install`.
14. If a pre-install backup is missing or incomplete, uninstall should clear the owned slots, print a warning, and continue.
15. Install, upgrade, verify, remove, and uninstall should continue to allow broadcast mode for now under the simplifying assumption that the user has only one VSN1 module.

## Scope notes

1. This redesign intentionally prioritizes user control over strict safety checks.
2. The goal is operational predictability, not preventing users from modifying installed runtimes.
3. The manifest remains the ownership and field-registry contract even when the Lua asset contents are edited manually.
4. Multi-module persistence and per-target runtime state are explicitly out of scope for this pass.

## Runtime discovery model

Discovery should scan these roots in increasing priority order:

1. `/usr/share/vsn1-cli/runtimes`
2. `~/.local/share/vsn1-cli/runtimes`
3. `<dev-checkout>/vsn1-cli/assets/runtimes`

Resolution rules:

1. Each immediate child directory is one runtime.
2. A runtime is loadable only when its directory contains a valid `manifest.toml`.
3. The runtime name is the directory basename.
4. If the same name exists in multiple roots, keep only the highest-precedence copy.
5. Discovery results should carry both the runtime name and the source root so diagnostics can explain which copy won.

### Dev-checkout detection

Treat the dev runtime root as available when the compile-time crate root contains `assets/runtimes` at runtime.

That keeps `cargo run` and other source-tree executions useful without requiring special flags, while installed builds can simply ignore the dev root when that directory is absent.

## Runtime manifest direction

The runtime manifest should stay close to the current structure, but drop hash-driven identity.

The required runtime manifest data should be:

1. compatibility reference or description text
2. owned slots
3. script asset filenames
4. install order
5. curated field inventory
6. compatibility notes

The owned slot entries should continue to identify:

1. slot name
2. page
3. element
4. event
5. asset filename
6. install order

Fields such as `normalized_sha256` and runtime markers should be removed from lifecycle-critical logic.

## Frozen installed runtime

After `runtime install <name>` succeeds, copy the full selected runtime directory to:

`~/.config/vsn1-cli/runtime`

This frozen copy becomes the installed-runtime record for later commands.

Rules:

1. `runtime verify` should read slot ownership and asset contents from this frozen copy.
2. `screen`-side curated runtime metadata should eventually be able to load from this frozen copy too, so package upgrades do not silently change the host/runtime contract.
3. `runtime upgrade <name>` should replace the frozen runtime copy with the newly selected runtime after a successful device write.
4. `runtime remove` or `runtime uninstall` should delete this directory after the device restore/clear path completes.

## Pre-install backup model

Before `runtime install <name>` overwrites the device, save the current contents of every owned slot to:

`~/.config/vsn1-cli/pre-install`

This directory should contain:

1. a small backup manifest that records the owned slots and asset filenames
2. one file per saved slot content

The backup manifest can be a subset of the runtime manifest because it only needs to support restoration.

Recommended backup manifest shape:

1. owned slots with `name`, `page`, `element`, `event`, `asset`, and `install_order`
2. optional notes about when or from which runtime the backup was captured

Backup behavior:

1. `runtime install <name>` refreshes the pre-install backup before writing the new runtime.
2. `runtime upgrade <name>` does not refresh the pre-install backup.
3. Blank or missing slots should still be representable in the backup so uninstall can restore them to empty content.

## Command semantics

### `runtime install <name>`

1. discover runtimes and resolve `<name>` using `dev` > `user` > `system`
2. load the runtime manifest and assets
3. read the current contents of the owned slots from the device
4. write the pre-install backup under `~/.config/vsn1-cli/pre-install`
5. install the selected runtime to the device in manifest order
6. store the touched pages
7. copy the full selected runtime directory to `~/.config/vsn1-cli/runtime`
8. verify the device contents against the frozen runtime copy

### `runtime upgrade <name>`

1. resolve and load the selected runtime exactly like install
2. do not refresh `~/.config/vsn1-cli/pre-install`
3. overwrite the device slots from the selected runtime
4. store the touched pages
5. replace `~/.config/vsn1-cli/runtime`
6. verify against the new frozen runtime copy

### `runtime verify`

1. require `~/.config/vsn1-cli/runtime` to exist
2. load the frozen runtime manifest and assets
3. read the owned slots from the device
4. compare normalized stored script contents directly against the frozen asset contents
5. fail on missing slots, content mismatch, or inconsistent target responses

### `runtime status`

Status should inspect the runtime relative to the frozen installed copy when present.

Recommended reporting states:

1. no frozen runtime installed locally
2. installed runtime present and exact content match
3. installed runtime present but one or more owned slots drifted
4. installed runtime present but one or more owned slots are missing
5. target responses were inconsistent

### `runtime remove` and `runtime uninstall`

Both commands should behave identically.

Preferred flow:

1. load `~/.config/vsn1-cli/pre-install` when present
2. restore those saved scripts into the recorded owned slots
3. store the touched pages
4. remove `~/.config/vsn1-cli/runtime`

Fallback flow when the backup is missing or incomplete:

1. determine the owned slots from the frozen runtime copy when possible
2. clear those owned slots explicitly to empty scripts
3. print a warning that the pre-install backup was unavailable and the owned slots were cleared instead of restored
4. store the touched pages
5. remove `~/.config/vsn1-cli/runtime`

## Verification model without hashes

Verification should continue to normalize text content and compare the framed stored Lua bodies exactly, but the expected values come from the actual frozen asset files rather than from manifest hashes.

This preserves deterministic verification while letting users edit runtime assets without recalculating metadata.

## Recommended storage helpers

Introduce explicit host-side storage helpers for:

1. locating the runtime config root under `~/.config/vsn1-cli`
2. locating the user runtime root under `~/.local/share/vsn1-cli/runtimes`
3. discovering the system runtime root
4. atomically replacing the frozen runtime directory
5. atomically replacing the pre-install backup directory

## Migration notes

1. Move the current active repo runtime to `assets/runtimes/default`.
2. Remove the old historical version directories from `assets/runtime` after any needed content migration is complete.
3. Update all docs, tests, and CLI help text that currently say `bundled runtime` or refer to exact hash matching.
4. Replace version-centric output text with runtime-name and runtime-source text.

## Checklist

### Phase 1: Runtime asset layout and discovery

- [ ] Move the current repo runtime assets from `assets/runtime/<bundle-version>` to `assets/runtimes/default`.
- [ ] Remove the legacy versioned runtime directories from `assets/runtime` after migration.
- [ ] Add runtime-root discovery for system, user, and dev locations.
- [ ] Implement runtime-name resolution by directory name with `dev` > `user` > `system` precedence.
- [ ] Add unit tests for root scanning, invalid runtime directories, and name-collision resolution.

### Phase 2: Manifest and verification redesign

- [ ] Remove lifecycle-critical hash fields from the runtime manifest contract and parsing path.
- [ ] Keep text normalization and stored-Lua framing, but compare actual script contents instead of expected hashes.
- [ ] Update runtime verification logic to use the frozen installed runtime copy.
- [ ] Update manifest and runtime-bundle tests to cover the no-hash path.

### Phase 3: Frozen runtime and backup persistence

- [ ] Add host-side path helpers for `~/.config/vsn1-cli/runtime` and `~/.config/vsn1-cli/pre-install`.
- [ ] Add full-directory copy/replace logic for the frozen installed runtime.
- [ ] Add backup manifest generation plus slot-content capture for pre-install state.
- [ ] Represent blank or missing slots in the backup so uninstall can restore them to empty content.
- [ ] Add tests for frozen-runtime replacement and pre-install backup replacement.

### Phase 4: Runtime command behavior

- [ ] Change `runtime install` to require a runtime name and write the pre-install backup.
- [ ] Change `runtime upgrade` to require a runtime name and skip backup refresh.
- [ ] Change `runtime verify` to compare against the frozen installed runtime.
- [ ] Rework `runtime status` to report local-frozen-runtime state.
- [ ] Make `runtime remove` restore from backup or clear with a warning.
- [ ] Add `runtime uninstall` as an alias for `runtime remove`.
- [ ] Update unit tests for install, upgrade, verify, status, remove, and alias behavior.

### Phase 5: Screen/runtime integration and docs

- [ ] Make the curated screen registry load from the frozen installed runtime instead of a compile-time bundled runtime.
- [ ] Update CLI help text, README usage, and validation docs for named runtimes and backup-based uninstall.
- [ ] Run `cargo fmt --check`, `cargo test`, and `cargo check`.
- [ ] Run hardware validation covering install, verify, upgrade, uninstall with backup restore, and uninstall fallback-to-clear.
- [ ] Record the validation results and final handoff state in `08-implementation-checklist.md`.

## Recommended next implementation step

Start with runtime discovery and asset-layout migration first.

That creates the named-runtime foundation before touching the lifecycle commands or the on-disk config state.
