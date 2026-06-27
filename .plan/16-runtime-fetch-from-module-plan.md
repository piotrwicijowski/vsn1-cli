# Runtime Fetch-From-Module Plan

## Purpose

This plan describes a follow-up that lets `vsn1-cli` rebuild the frozen installed runtime under `~/.config/vsn1-cli/runtime` directly from files stored on a VSN1 module.

Primary user story:

1. install a runtime from computer A
2. later connect the same module to computer B
3. run one command on computer B to repopulate `~/.config/vsn1-cli/runtime`
4. immediately regain `runtime verify`, `runtime status`, and curated `screen` command support without needing the original host runtime bundle

## Goal

Add a new runtime fetch flow with these behaviors:

1. `runtime install <name>` also saves the selected runtime manifest onto the module filesystem
2. `runtime fetch` reads that module-stored manifest first
3. `runtime fetch` uses the manifest to determine which module paths must be downloaded into the local frozen runtime directory
4. `runtime fetch` shows a progress bar while downloading

## Scope Boundary

### In scope

1. saving a runtime manifest file onto the module during install and upgrade
2. fetching a full frozen runtime copy into `~/.config/vsn1-cli/runtime`
3. reusing the manifest's existing `owned_slots` and `owned_files` inventory as the fetch source of truth
4. progress reporting for fetch
5. unit coverage for manifest round-tripping, fetch reconstruction, and command plumbing

### Out of scope

1. public arbitrary filesystem porcelain
2. repopulating discovered runtime roots such as `~/.local/share/vsn1-cli/runtimes`
3. binary-safe arbitrary file transport beyond the current text-oriented Lua file path
4. changing `screen` command behavior
5. broad daemon streaming/progress infrastructure unless it is explicitly chosen as follow-up work

## Current Relevant Baseline

1. `src/runtime.rs` already owns runtime install, upgrade, verify, repair, remove, frozen-runtime persistence, and the current module-file progress bar.
2. `src/module_files.rs` already supports chunked module-file reads and writes, but only through owned-asset wrappers.
3. `src/runtime_bundle.rs` already models the manifest inventory needed for fetch:
   - `owned_slots`
   - `owned_files`
   - `layers`
   - `fields`
4. `screen.rs` already loads layer and field metadata from `~/.config/vsn1-cli/runtime`, so a successful fetch only needs to recreate a valid local runtime bundle directory.
5. Runtime commands are currently daemon-eligible, but the daemon protocol is request/response only and cannot stream progress updates back to the CLI.

## Recommended Product Shape

### New command

Add:

1. `vsn1-cli runtime fetch`

Recommended behavior:

1. resolve the target device the same way as other runtime commands
2. read the module-stored manifest file
3. reconstruct a valid runtime bundle directory in a staging directory
4. atomically replace `~/.config/vsn1-cli/runtime` with the fetched bundle
5. leave `~/.config/vsn1-cli/pre-install` untouched

This command should not require a runtime name because the source of truth is the module itself.

### Manifest storage path on the module

Store the manifest at one fixed reserved root-level module path.

Recommended first path:

1. `/vsn1-cli-runtime-manifest.toml`

Why this shape:

1. it avoids introducing directory-creation support just to land the feature
2. it stays outside the runtime-owned event-file paths such as `/00/0d/00.cfg`
3. it is easy to explain and diagnose manually during hardware validation

## Core Design Decisions

### 1. Save the exact runtime `manifest.toml` on the module

Do not synthesize a second manifest format.

Recommended install behavior:

1. after runtime-owned assets are written successfully, write the exact bundle `manifest.toml` text to `/vsn1-cli-runtime-manifest.toml`
2. then perform the existing activation and verification flow
3. if manifest write fails, fail the install instead of silently completing a runtime that cannot later be fetched

This keeps fetch aligned with the same manifest schema already used by:

1. runtime bundle loading
2. screen field registry loading
3. installed-runtime verification

### 2. Reuse the existing manifest inventory as the fetch plan

The fetched manifest already tells the host what to download:

1. for `owned_slots`, derive the module file path from `page/element/event` using the existing `/00/0d/08.cfg` rule
2. for `owned_files`, use the declared absolute module file path

No new manifest section is required for the first implementation.

### 3. Treat fetched slot-backed files as reconstructed source assets, not byte-for-byte source recovery

Important constraint:

1. slot-backed runtime files on the module are stored in the compact wrapped form used by `module-files` verification
2. writing those bytes directly into local asset files would produce an invalid local bundle because `RuntimeBundle::load_from_dir()` would wrap them again

Recommended fetch behavior:

1. when fetching an `owned_slot`, reverse the stored slot-file representation back into a normalized local source form before writing the asset file under `~/.config/vsn1-cli/runtime`
2. when fetching an `owned_file`, write the fetched content as-is

For v1, it is acceptable that fetched slot assets preserve semantics rather than original formatting.

### 4. Keep fetch local-only in the first implementation

Recommended first command-routing choice:

1. treat `runtime fetch` as local-only, not daemon-eligible

Reason:

1. the current daemon protocol only returns a final `output: String`
2. the fetch requirement explicitly asks for a progress bar
3. local-only routing is the minimal way to guarantee visible progress without widening the daemon protocol

If streaming progress through the daemon becomes important later, that should be a separate scoped follow-up.

## Internal Architecture Changes

### 1. Generalize the module-file helpers away from owned assets

Current issue:

1. `src/module_files.rs` helpers are centered on `RuntimeOwnedAsset`
2. the manifest file to read and write is not a runtime-owned asset

Recommended refactor:

1. add generic helpers such as:
   - `read_module_file_with_progress(path, ...)`
   - `write_module_file_atomic_with_progress(path, ...)`
   - `clear_module_file(path, ...)`
2. keep the current owned-slot wrappers as thin adapters that derive a path and call the generic helpers

This is the smallest refactor that supports both:

1. the reserved manifest file
2. any future non-slot runtime helper files

### 2. Add runtime fetch reconstruction logic in `runtime.rs`

Recommended new responsibilities:

1. read the reserved module manifest file
2. parse it as `RuntimeBundleManifest`
3. validate it before downloading the referenced assets
4. fetch each referenced asset into a staging runtime directory
5. write the fetched manifest to `staging/manifest.toml`
6. validate the staged directory with `RuntimeBundle::load_from_dir()`
7. atomically replace `~/.config/vsn1-cli/runtime`

Recommended new public entrypoint:

1. `fetch_runtime_from_module(requested_target, reader) -> Result<RuntimeFetchReport>`

### 3. Add a small reverse transform for fetched slot-owned assets

Recommended helper behavior:

1. accept fetched slot-file content such as `<?lua --[[@cb]] ... ?>`
2. strip the outer `<?lua ... ?>` wrapper
3. strip the leading callback prefix when present
4. normalize the result into the plain local asset body expected by bundle loading

This logic should live close to the existing runtime asset normalization helpers in `runtime.rs` or `runtime_bundle.rs`.

### 4. Reuse the existing staging and replacement flow

The fetch path should reuse the same local filesystem safety model already used for:

1. frozen-runtime replacement
2. pre-install backup replacement

Recommended reuse:

1. stage under `~/.config/vsn1-cli/runtime.tmp`
2. only replace `~/.config/vsn1-cli/runtime` after the fetched bundle loads successfully

## Progress Bar Plan

### Requirements

Fetch should show progress at least when:

1. stderr is a TTY
2. the command is running on the cold path

### Recommended approach

Extend the existing `RuntimeProvisioningProgressBar` rather than adding a second progress implementation.

Recommended additions:

1. a `for_fetch_manifest_and_bundle(...)` constructor
2. phases such as:
   - `reading module manifest`
   - `downloading runtime files`
   - `validating fetched runtime`
   - `saving local runtime`
3. step counts based on:
   - manifest read chunk estimate
   - per-asset read chunk estimate
   - one final validation/save step

Recommended rule:

1. fetch progress should be enabled for any manifest-driven module-file read, regardless of whether the fetched runtime's original provisioning backend was `config-slots` or `module-files`

The reason is that fetch itself is always reading through the module filesystem path.

## CLI And Command Plumbing

### `src/lib.rs`

Add:

1. `RuntimeCommand::Fetch { target }`
2. help text describing that it rebuilds `~/.config/vsn1-cli/runtime` from the module-stored manifest
3. `execute_runtime_fetch(...)`
4. `CommandSuccess::RuntimeFetch { ... }`
5. success rendering that reports the fetched manifest path and the assets written locally

### `src/command_model.rs`

Add:

1. `RuntimeRequest::Fetch { target }`
2. `debug_name()` support
3. `routing()` classification as local-only for the first pass

### `src/daemon_*`

Minimal first-pass recommendation:

1. do not add daemon request handling for fetch yet
2. let the existing local-only guard reject daemon routing automatically

## Validation And Safety Rules

Recommended fetch-time checks:

1. fail if `/vsn1-cli-runtime-manifest.toml` is missing
2. fail if the fetched manifest cannot be parsed or validated
3. fail if any referenced module path is missing
4. fail if the staged local runtime directory does not load as a valid `RuntimeBundle`
5. do not touch the existing local runtime directory until the staged fetch is complete

Recommended install-time checks:

1. fail if the manifest cannot be written to the module after asset upload
2. continue to verify the installed runtime against the local frozen copy after install/upgrade as today

## Recommended Implementation Phases

### Phase 1: Generalize module-file helpers

1. add generic read/write/clear helpers by raw module path
2. make the existing owned-asset helpers call through them
3. add tests for raw path reads and writes with progress callbacks

### Phase 2: Persist manifest on install and upgrade

1. define the reserved module manifest path constant
2. write the exact bundle manifest text to the module during install/upgrade
3. add tests covering the extra write on successful installs

### Phase 3: Implement fetch reconstruction

1. add fetch report types and runtime entrypoint
2. read and validate the module manifest
3. download all referenced assets into a staging directory
4. reverse-transform slot-owned fetched files before writing local asset files
5. validate the staged runtime bundle and replace `~/.config/vsn1-cli/runtime`

### Phase 4: CLI integration and progress

1. add `runtime fetch` CLI parsing and rendering
2. route it as local-only
3. extend the current progress bar for fetch phases and chunk counts

### Phase 5: Hardware validation

1. install a runtime on computer A
2. remove or isolate `~/.config/vsn1-cli/runtime` on computer B
3. run `vsn1-cli runtime fetch`
4. confirm `runtime verify` succeeds immediately afterward
5. confirm `screen set` works using the fetched runtime metadata
6. validate both:
   - a runtime with only `owned_slots`
   - a runtime with `owned_files` plus `owned_slots`, such as `media`

## Checklist

### Step 1: Generic module-file helper refactor

- [ ] Add raw-path read/write/clear helpers in `src/module_files.rs`.
- [ ] Make owned-asset helpers thin wrappers over the raw-path helpers.
- [ ] Add regression tests for raw manifest-file reads and writes.

### Step 2: Save manifest to the module during install

- [ ] Add a reserved module manifest path constant.
- [ ] Write the exact bundle `manifest.toml` text to that path during install and upgrade.
- [ ] Fail install or upgrade if the manifest write fails.
- [ ] Add tests for manifest persistence ordering and failure propagation.

### Step 3: Implement runtime fetch in `runtime.rs`

- [ ] Add a fetch report type and public fetch entrypoint.
- [ ] Read and validate the module-stored manifest first.
- [ ] Fetch all referenced slot-backed and file-backed assets into a staging runtime directory.
- [ ] Reverse-transform fetched slot-owned files into local bundle asset form.
- [ ] Validate the staged directory with `RuntimeBundle::load_from_dir()`.
- [ ] Atomically replace `~/.config/vsn1-cli/runtime`.

### Step 4: Add CLI plumbing and progress reporting

- [ ] Add `runtime fetch` to `src/lib.rs` and `src/command_model.rs`.
- [ ] Mark it local-only in the first pass so progress stays visible.
- [ ] Extend `RuntimeProvisioningProgressBar` for fetch phases and step counts.
- [ ] Add rendering tests for success and error output.

### Step 5: Validate end to end on hardware

- [ ] Install a runtime from one host.
- [ ] Fetch it from a second host with no local runtime copy.
- [ ] Confirm `runtime verify` succeeds after fetch.
- [ ] Confirm curated `screen` commands work after fetch.
- [ ] Confirm runtimes with `owned_files` fetch correctly.

## Risks And Constraints

1. **Slot-file reverse transform risk**
   - Fetch must not store the wrapped module-file bytes as local source assets.
   - This is the most important correctness detail in the feature.

2. **Daemon progress mismatch**
   - The current daemon protocol cannot stream progress.
   - Keeping fetch local-only avoids widening scope.

3. **Manifest authority risk**
   - Fetch can only rebuild files that are represented by the stored manifest.
   - Any runtime file that must be recoverable from the module must be declared in `owned_slots` or `owned_files`.

4. **Text-only limitation**
   - Current module-file helpers are string-based and assume text content.
   - If future runtimes need binary assets, that will require a separate transport/storage design.

## Success Criteria

Treat this follow-up as successful when all of the following are true:

1. `runtime install <name>` saves a fetchable manifest onto the module
2. `runtime fetch` can rebuild `~/.config/vsn1-cli/runtime` from module state alone
3. `runtime verify` succeeds immediately after a successful fetch
4. curated `screen` commands work after fetch without any original runtime bundle on the host
5. fetch shows visible progress while reading the manifest and owned assets
