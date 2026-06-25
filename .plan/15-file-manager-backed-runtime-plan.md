# File-Manager-Backed Runtime Plan

## Purpose

This plan describes a scoped follow-up that lets `vsn1-cli` provision and manage the VSN1 runtime through the module file-manager path instead of the legacy `CONFIG` slot-write path.

The immediate goal is intentionally narrow:

1. keep the public CLI porcelain unchanged
2. use file-manager access only behind the existing `runtime` commands
3. focus only on the two runtime-owned event files at `/00/0d/00.lua` and `/00/0d/08.lua`
4. prove that the same runtime assets currently installed as `page=0 element=13 event=0` and `page=0 element=13 event=8` can be installed, verified, and removed through the file-manager path instead

This is a runtime lifecycle follow-up, not a general filesystem product feature.

## Scope Summary

### In scope

1. internal host support for raw `EVALUATE` requests and Lua-value response decoding
2. an internal runtime provisioning backend that reads and writes `/00/0d/00.lua` and `/00/0d/08.lua`
3. runtime install, upgrade, verify, repair, status, and remove/uninstall behavior for that backend
4. pre-install backup and frozen-runtime persistence for that backend
5. hardware validation that the screen behaves the same after file-backed install as after the old slot-backed install

### Out of scope

1. public `file` or `filesystem` CLI commands
2. arbitrary browsing, copy, rename, or delete porcelain
3. broad support for arbitrary module paths in the first pass
4. mixing slot-backed and file-backed owned slots within one runtime in the first pass
5. redesigning `screen set`, `screen clear`, or `screen activate`

## Proposed User-Facing Behavior

The CLI surface should stay the same.

Users should still run commands such as:

1. `vsn1-cli runtime install <name>`
2. `vsn1-cli runtime verify`
3. `vsn1-cli runtime status`
4. `vsn1-cli runtime repair`
5. `vsn1-cli runtime upgrade <name>`
6. `vsn1-cli runtime remove`

The difference is internal:

1. a normal runtime continues to use the existing `CONFIG` slot path
2. a file-manager-backed runtime uses the module filesystem path for its owned runtime assets

No new public runtime flags are required for the first pass.

## Recommended Product Shape

Ship this first as a separate runtime variant rather than replacing the current proven default runtime immediately.

Recommended first runtime name:

1. `default-file-manager-poc`

Rationale:

1. it preserves the currently validated slot-backed `default` runtime as a fallback
2. it gives hardware validation a clean A/B comparison target
3. it avoids mixing two provisioning strategies into the same runtime during the proof stage

## Proposed Runtime Model

### Runtime manifest addition

Add one new runtime-level manifest field to select the provisioning backend.

Recommended shape:

```toml
provisioning_backend = "config-slots" | "module-files"
```

Recommended first-pass rules:

1. default to `config-slots` when the field is omitted
2. `module-files` means all owned runtime assets in that runtime are provisioned through the file-manager path
3. do not support mixed backends within one runtime yet

### Preserve the existing owned-slot contract

Keep `owned_slots` with:

1. `name`
2. `page`
3. `element`
4. `event`
5. `asset`
6. `install_order`

This keeps the curated runtime metadata and logical ownership model stable.

### Derive module file paths from slot metadata

For the first pass, do not add arbitrary per-slot path fields.

Instead, derive the target file path from the existing slot location:

```text
/{page:02x}/{element:02x}/{event:02x}.lua
```

Examples for the current runtime-owned slots:

1. `page=0 element=13 event=0` -> `/00/0d/00.lua`
2. `page=0 element=13 event=8` -> `/00/0d/08.lua`

Why this is the right first step:

1. it matches the user's stated target files exactly
2. it preserves the 1:1 mapping between existing owned-slot metadata and the file-manager-backed layout
3. it avoids inventing a second source of truth for the same logical runtime locations

## Proposed Internal Architecture

### 1. Add a raw `EVALUATE` protocol path

Add a new low-level internal request path alongside the existing `IMMEDIATE`, `CONFIG`, `PAGEACTIVE`, and `PAGESTORE` helpers.

Responsibilities:

1. encode the raw `EVALUATE` packet shape confirmed from `grid-editor`
2. send targeted evaluate requests over the existing serial transport
3. parse the returned Lua values
4. fail fast on timeout or malformed responses

Important constraint:

1. this should be an internal library helper only
2. it should not create a new public CLI command in this pass

### 2. Add a minimal internal module-files API

Build a very small internal API on top of `evaluate()` for runtime lifecycle work.

Recommended first-pass operations:

1. `read_file(path) -> Option<String>`
2. `write_file_atomic(path, content)`
3. `delete_file(path)` or `clear_file(path)`
4. `ensure_parent_dirs(path)` only as needed for `/00/0d`

Do not implement general-purpose browse/copy/rename/list helpers unless runtime lifecycle work actually needs them.

### 3. Abstract runtime provisioning behind a backend interface

The current runtime code already centralizes lifecycle operations behind reader/writer traits. Extend that idea one level upward.

Recommended shape:

1. keep runtime lifecycle orchestration in `runtime.rs`
2. add a backend selection layer based on the runtime manifest's `provisioning_backend`
3. implement two backends:
   - existing `config-slots`
   - new `module-files`

The lifecycle code should continue to think in terms of owned runtime assets and logical slot ownership, while the backend decides whether that means `CONFIG` reads/writes or file reads/writes.

### 4. Keep `screen` commands unchanged

`screen set`, `screen clear`, and `screen activate` should continue using the same immediate runtime-helper path.

The only required integration is that runtime verification and installed-runtime gating must understand the file-backed runtime variant.

## Implementation Phases

## Phase 1: Protocol and parsing groundwork

### Goal

Teach `vsn1-cli` how to send module-targeted `EVALUATE` requests and decode the response safely.

### Work

1. Add `protocol.rs` support for encoding raw `EVALUATE` frames.
2. Reuse the existing BRC/header machinery where possible instead of duplicating packet framing logic.
3. Add a small Lua-value response decoder for:
   - nil
   - boolean
   - number
   - string
   - table
4. Add a targeted request/response helper in the transport/runtime layer.
5. Keep response matching stricter than the current `grid-editor` implementation.

### Verification

1. unit tests for `EVALUATE` packet encoding
2. unit tests for Lua-value response parsing
3. unit tests for timeout and malformed-response failures

### Exit criteria

1. Rust code can send `return 1` to one module and decode the response reliably on hardware
2. a simple `io.open`-based file size or file existence probe works end-to-end on hardware

## Phase 2: Minimal module-files runtime backend

### Goal

Implement the smallest internal file API needed to manage `/00/0d/00.lua` and `/00/0d/08.lua`.

### Work

1. Add a small internal module-files helper module.
2. Implement read as repeated `io.open(..., "r")` plus chunked reads through `EVALUATE`.
3. Implement write as chunked temp-file upload plus `os.rename(tmp, final)`.
4. Implement delete/clear behavior for owned runtime files.
5. Add path derivation from `page/element/event` using lowercase two-digit hex.
6. Restrict the first pass to derived runtime-owned `.lua` event files only.

### Verification

1. unit tests for slot-location to device-file path mapping
2. unit tests for chunking and Lua escaping helpers
3. hardware validation that `lcd-init.lua` and `lcd-draw.lua` can be uploaded to `/00/0d/00.lua` and `/00/0d/08.lua`

### Exit criteria

1. `vsn1-cli` can read back the exact two target files from a real device
2. `vsn1-cli` can replace those files on a real device without using `CONFIG` writes for those assets

## Phase 3: Runtime manifest and bundle integration

### Goal

Make runtime bundles explicitly choose between slot-backed and file-backed provisioning.

### Work

1. Add `provisioning_backend` to runtime manifest parsing and validation.
2. Keep `config-slots` as the implicit default.
3. Reject invalid or unknown backend names.
4. Add a new proof runtime directory such as `assets/runtimes/default-file-manager-poc`.
5. Reuse the same `lcd-init.lua`, `lcd-draw.lua`, layer metadata, and field registry shape as the existing default runtime.

### Recommended validation rule

For the first pass, when `provisioning_backend = "module-files"`:

1. require every owned slot asset to end in `.lua`
2. derive its device file path from `page/element/event`
3. reject runtime bundles that try to use non-event or non-Lua owned assets through this backend

### Verification

1. manifest parsing tests for both backend values
2. regression tests for invalid backend names
3. runtime-bundle tests for derived device-file paths

### Exit criteria

1. the repo can load a file-manager-backed runtime bundle cleanly
2. the old `default` runtime remains unchanged and still valid

## Phase 4: Runtime lifecycle integration

### Goal

Make the existing runtime commands operate through the selected provisioning backend.

### Work

1. Teach `runtime install` to:
   - capture pre-install backup through the selected backend
   - write owned assets through the selected backend
   - freeze the selected runtime locally as today
   - verify through the selected backend
2. Teach `runtime upgrade` to reuse the same backend path without refreshing backup.
3. Teach `runtime verify` and `runtime status` to inspect owned runtime assets through the selected backend.
4. Teach `runtime repair` to reapply the frozen installed runtime through the selected backend.
5. Teach `runtime remove` / `runtime uninstall` to restore the backup through the selected backend or fall back to clear/delete behavior.

### Important design point

The frozen installed runtime copy under `~/.config/vsn1-cli/runtime` should stay manifest-driven exactly as today.

That means:

1. the local installed-runtime record does not need a second format
2. the backend choice should come from the frozen runtime manifest
3. screen-field metadata can keep loading from the frozen runtime copy as it does now

### Verification

1. unit tests for backend selection during install/verify/remove
2. unit tests for backup generation and restore under `module-files`
3. unit tests for drift detection through file reads

### Exit criteria

1. `runtime install <file-backed-name>` succeeds end-to-end
2. `runtime verify` reports exact-match compatible after install
3. `runtime status` reports the file-backed runtime correctly
4. `runtime remove` restores the pre-install state or clearly reports fallback clear behavior

## Phase 5: Behavior parity validation

### Goal

Prove that the file-backed runtime behaves the same on hardware as the current slot-backed runtime for the two owned runtime assets.

### Work

1. Install the current proven slot-backed runtime and record baseline behavior.
2. Install the file-manager-backed runtime variant.
3. Compare:
   - visible initialization behavior
   - visible draw behavior
   - `screen set` updates
   - `screen activate` behavior
   - timeout fallback behavior for temporary layers
4. Confirm that `runtime verify` and `runtime repair` work after intentional drift introduced through the file path.

### Exit criteria

1. the device visibly behaves the same after file-backed install as after legacy slot-backed install
2. the runtime helper contract used by `screen` commands still works unchanged
3. a deliberate edit to `/00/0d/08.lua` is detected by `runtime verify`

## Recommended Step-By-Step Checklist

Use this as the execution order if implementation begins.

### Step 1: Add `EVALUATE` packet encode/decode support

- [x] Encode targeted raw `EVALUATE` packets in `protocol.rs`.
- [x] Add Lua-value response decoding for nil/bool/number/string/table.
- [x] Add tests for packet encoding and response parsing.
- [ ] Hardware gate: confirm `return 1` evaluates successfully on a real device.

### Step 2: Add an internal module-files helper

- [x] Implement chunked file read for targeted module paths.
- [x] Implement chunked temp-file write plus rename.
- [x] Implement fixed-path clear/delete semantics for runtime-owned files.
- [x] Add tests for path derivation and chunking helpers.
- [ ] Hardware gate: confirm read/write of `/00/0d/00.lua` and `/00/0d/08.lua`.

### Step 3: Extend runtime manifests with backend selection

- [x] Add `provisioning_backend` parsing and validation.
- [x] Default omitted backend to `config-slots`.
- [x] Add a new file-backed proof runtime directory.
- [x] Add manifest regression tests for valid and invalid backend values.

### Step 4: Integrate backend selection into runtime install/upgrade

- [x] Route runtime install writes through the selected backend.
- [x] Route pre-install backup capture through the selected backend.
- [x] Route runtime upgrade through the selected backend.
- [x] Add tests for file-backed install planning and backup capture.
- [ ] Hardware gate: confirm `runtime install <file-backed-name>` works.

### Step 5: Integrate backend selection into verify/status/repair

- [x] Route runtime inspection through the selected backend.
- [x] Detect drift by file content mismatch on the module.
- [x] Route runtime repair through the selected backend.
- [x] Add tests for exact-match, drift, and missing-file cases.
- [ ] Hardware gate: confirm drifted `/00/0d/08.lua` is detected and repaired.

### Step 6: Integrate backend selection into remove/uninstall

- [x] Restore pre-install file contents through the selected backend when backup exists.
- [x] Define and implement fallback clear behavior when no backup exists.
- [x] Add tests for restore and fallback-clear behavior.
- [ ] Hardware gate: confirm uninstall restores prior state or performs the documented fallback.

### Step 7: Run parity validation against the proven slot-backed runtime

- [ ] Compare visible LCD behavior after slot-backed install vs file-backed install.
- [ ] Confirm `screen set`, `screen clear`, and `screen activate` still function.
- [ ] Record validation notes and any remaining constraints.
- [ ] Update `08-implementation-checklist.md` with session handoff state if this work is executed.

## Key Design Decisions To Preserve

1. Do not expose arbitrary filesystem commands publicly in this pass.
2. Keep ownership and curated field metadata manifest-driven.
3. Keep runtime lifecycle semantics the same from the user's point of view.
4. Keep the old slot-backed runtime path available until hardware validation proves the file-backed path is equally reliable.
5. Keep the file-backed proof narrowly scoped to `/00/0d/00.lua` and `/00/0d/08.lua` before broadening.

## Open Questions And Uncertainties

These are not blockers to the plan, but they should be resolved during implementation.

### 1. Separate proof runtime or replace `default`?

Recommendation:

1. start with a separate runtime such as `default-file-manager-poc`
2. only consider replacing `default` after hardware parity is proven

### 2. What should fallback clear mean for file-backed owned runtime assets?

Open choice:

1. delete `/00/0d/00.lua` and `/00/0d/08.lua`
2. overwrite them with empty files

This should be decided by hardware behavior, not guesswork.

### 3. Is a page-store or reload step needed after direct file writes?

The old slot-backed path explicitly sends `PAGEACTIVE` plus `PAGESTORE`.

Unknowns to validate:

1. whether direct file writes become active immediately
2. whether they require an equivalent persistence or page reload action
3. whether `runtime install` should still send page-store commands after file-backed writes for consistency

### 4. Should verification compare normalized Lua text or exact file bytes?

Recommendation:

1. keep normalized text comparison first for parity with the existing runtime model
2. only move to exact-byte verification if hardware behavior proves normalization is masking real drift

### 5. How broad is the derived path rule really?

The current plan assumes the event file path is:

```text
/{page:02x}/{element:02x}/{event:02x}.lua
```

That matches the requested `/00/0d/00.lua` and `/00/0d/08.lua` target paths, but this should still be confirmed on hardware before the rule is generalized beyond this POC.

## Success Criteria

Treat this follow-up as successful when all of the following are true:

1. a new runtime can be installed entirely through the file-manager path without public filesystem porcelain
2. the installed file-backed runtime uses `/00/0d/00.lua` and `/00/0d/08.lua` for the same logical runtime assets now owned through `CONFIG`
3. `runtime verify`, `runtime repair`, `runtime status`, and `runtime remove` all work through that backend
4. the screen behaves the same on hardware as it did with the legacy slot-backed install
