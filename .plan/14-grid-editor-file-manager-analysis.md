# Grid Editor File Manager Analysis

## Purpose

This note captures a code-grounded analysis of how the newer `grid-editor` implements the firmware File Manager capability, with an emphasis on what matters for a future standalone `vsn1-cli` implementation.

The key question was whether `grid-editor` uses a dedicated file-transfer protocol or whether it manages files indirectly through the existing Lua execution path.

## Sources Reviewed

- `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte`
- `grid-editor/src/renderer/main/panels/DebugMonitor/SendEvaluate.svelte`
- `grid-editor/src/renderer/serialport/evaluate-parser.ts`
- `grid-editor/src/renderer/runtime/engine.store.ts`
- `grid-editor/src/renderer/serialport/serialport.ts`
- `grid-editor/src/renderer/serialport/instructions.ts`
- `grid-editor/node_modules/@intechstudio/grid-protocol/dist/index.js`
- `grid-editor/src/electron/main.ts`
- https://docs.intech.studio/wiki/more/file-manager/

## High-Level Conclusion

`grid-editor` does not appear to use a dedicated host-side file-management protocol.

Instead, the current File Manager is implemented on top of raw module-side Lua evaluation:

1. the editor sends a custom raw `EVALUATE` packet to one selected module
2. the module executes filesystem-related Lua such as `io.open`, `os.rename`, `os.remove`, `dirent.list`, and `dirent.mkdir`
3. the editor parses the returned Lua values from the `EVALUATE` response

Practical consequence:

1. the new firmware capability does let code longer than the old `~900` character slot limit live on the module filesystem
2. but the current editor implementation is still fundamentally remote-Lua-driven, not a separate robust binary file-transfer layer

## Relevant Vendor Guidance

The public File Manager docs add a few practical constraints that are important for future `vsn1-cli` design even though they are not enforced by the checked-in UI code.

Documented guidance:

1. Grid Editor `v1.6.6` is the first release with File Manager support
2. files under roughly `15000` bytes were a reported development sweet spot
3. on-module memory usage after initialization should ideally stay at or below roughly `135 KB`
4. custom code may need explicit `collectgarbage("collect")`
5. if a configuration becomes unrecoverable, a user can perform an NVM erase shortly after USB connect

Practical consequence:

1. the new capability removes the old slot-length ceiling as the main blocker
2. but memory pressure and runtime initialization cost remain real device constraints

## Confirmed Protocol Shape

### Regular slot/immediate limits still exist

The checked-in protocol metadata still reports `GRID_PARAMETER_ACTIONSTRING_maxlength = "909"` in `grid-editor/node_modules/@intechstudio/grid-protocol/dist/index.js:69`.

Normal editor-side immediate execution still enforces that limit in `grid-editor/src/renderer/serialport/instructions.ts:179-220`.

### File Manager bypasses the normal immediate helper

The File Manager does not use `SendConfigImmediate`.

Instead, `sendLua(...)` in `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:58-110` manually constructs a raw packet using:

```text
\x02086e0001 + 04 + <4 hex chars of script size> + <script> + \x03
```

It then:

1. reuses the first `23` bytes of a normal encoded packet as a BRC header
2. overwrites bytes `2..5` with the total packet length as ASCII hex
3. computes an XOR checksum over the whole message
4. appends the checksum as two ASCII hex chars
5. appends a final linefeed byte `0x0a`

The same raw `EVALUATE` construction is duplicated in `grid-editor/src/renderer/main/panels/DebugMonitor/SendEvaluate.svelte:56-119`.

### Incoming response framing

Incoming packets are considered complete when the transport sees `LF` with `EOT` three bytes earlier in `grid-editor/src/renderer/serialport/serialport.ts:156-178`.

That matters for a future Rust implementation because the raw file-manager path is still using the normal Grid framing rules, not a separate stream.

### EVALUATE response parsing

`grid-editor/src/renderer/serialport/evaluate-parser.ts` parses returned Lua values from the raw response payload.

Confirmed details:

1. `raw[6..7]` is treated as the returned element count
2. `raw[8+]` contains the encoded Lua values
3. supported Lua types are nil `0`, boolean `1`, number `3`, string `4`, and table `5`
4. tables are parsed as recursive key-value pairs
5. strings decode firmware-emitted `\xNN` escape sequences

This parser is enough for the current UI because the file manager only needs simple return shapes like strings, booleans, numbers, and tables.

## How Files Are Managed On The Module

### Module targeting

The File Manager is explicitly single-module, not broadcast-first.

`grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:21-55` builds a module picker from connected runtime modules and then uses that selected module's `dx` and `dy` for every operation.

This is a good default for filesystem work and should likely stay true in `vsn1-cli`.

### Directory listing

Directory listing uses:

```lua
return dirent.list(path)
```

Source: `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:539-563`.

The UI expects each returned row to be a Lua table where:

1. `row[1]` is the entry name
2. `row[2] === 2` means directory
3. anything else is treated as a file

The UI navigates from root `/` and assumes POSIX-style absolute paths. The breadcrumb code includes the example `"/00/foo/"`, which lines up with the docs' explanation that the module stores configuration content in a folder tree rooted at `/`.

### File reads

File reads are two-stage and chunked.

First, the editor computes the file size by opening the file and repeatedly reading `256` bytes until EOF:

```lua
local f=io.open(path,"r")
if not f then return nil end
local n=0
local c=f:read(256)
while c do n=n+#c c=f:read(256) end
f:close()
return n
```

Then it rereads the file in `50` byte chunks using repeated reopen + `seek` + `read(50)` calls.

Source: `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:302-359`.

Important implementation detail:

1. `READ_CHUNK_SIZE` is currently hard-coded to `50`
2. the file is reopened for every chunk
3. `collectgarbage("collect")` is called after each chunk read

This is clearly conservative and likely slow, but it keeps each evaluate request small.

### File writes

File writes are also chunked and use a temp-file rename commit pattern.

Source: `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:361-439`.

Flow:

1. choose final path `path`
2. choose temp path `path + ".tmp"`
3. if the selected language is `lua`, run `GridScript.compressScript(fileContent)` before upload
4. split the content into `50` character chunks
5. for the first chunk, open temp file in `"w"`; for later chunks, open in `"a"`
6. write escaped chunk content with `f:write(...)`
7. rename temp file into place with `os.rename(tmpPath, path)`
8. reopen and rescan the saved file to confirm only the byte count

The actual chunk write snippet is:

```lua
local f=io.open(tmpPath,mode)
if not f then return false end
f:write(chunk)
f:close()
collectgarbage("collect")
return true
```

Important details:

1. `CHUNK_SIZE` is also hard-coded to `50`
2. chunk-write evaluate requests are sent with `compress = false` so the wrapper Lua is not passed through `GridScript.compressScript`
3. save verification checks size only, not full content equality
4. a failed upload can plausibly leave `*.tmp` behind

### Lua-file transform behavior

Lua files are not treated as byte-preserving text files.

Source:

- save path: `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:370-378`
- read path: `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:343-350`
- formatter/minifier behavior: `grid-editor/node_modules/@intechstudio/grid-protocol/dist/index.js:4203-4216`

Confirmed behavior:

1. saving a `.lua` file runs `GridScript.compressScript(...)`
2. that performs `shortify(...)` plus Lua minification
3. reading a `.lua` file runs `GridScript.expandScript(...)`
4. that performs `humanize(...)` plus Lua beautification

Practical consequence:

1. the file manager is not round-tripping Lua source byte-for-byte
2. it is round-tripping the editor's compressed/humanized representation

For `vsn1-cli`, this is a major design decision point. A standalone CLI should decide explicitly whether it wants:

1. exact byte preservation
2. editor-compatible compressed Lua files
3. both, but only when requested

### Create, rename, copy, and delete

Sources:

- create/copy/rename: `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:468-524`
- delete: `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:526-537`

Confirmed operations:

1. new file: `io.open(path, "w")` then close
2. new folder: `dirent.mkdir(path)`
3. rename: `os.rename(oldPath, newPath)`
4. copy: open source with `io.open(..., "r")`, open destination with `io.open(..., "w")`, then copy by repeated `read(256)` and `write(...)`
5. delete: `os.remove(path)`

The editor therefore depends on the module firmware exposing at least:

1. the Lua `io` library
2. `os.rename`
3. `os.remove`
4. a `dirent` helper namespace with `list` and `mkdir`

### Lua module cache invalidation attempt

After saving a `.lua` file, the editor tries to invalidate the module cache with:

```lua
package.loaded[moduleName] = nil
```

Source: `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:425-432`.

Important caveat:

1. the cache key is only the selected file basename without `.lua`
2. it does not include the directory path
3. the docs' example uses `require("/test")`, not `require("test")`

So the current invalidation logic does not obviously match absolute-path or nested-file `require(...)` usage.

## What This Says About On-Module Layout

The docs say the module filesystem exposes stored configuration content as a nested folder tree where top-level folders correspond to pages and deeper folders correspond to elements and events.

The current editor code does not add any extra abstraction on top of that.

Instead, it exposes the raw filesystem rooted at `/` and lets the user browse whatever is there.

That means a future `vsn1-cli` implementation must treat the module filesystem as shared state with at least two categories:

1. firmware/editor-owned configuration tree already used by Grid itself
2. user/runtime-owned files uploaded intentionally for custom logic such as large Lua helpers

The safest future design is likely to reserve a dedicated namespace for `vsn1-cli` files rather than writing into unknown editor-owned paths.

## Additional Findings And Risks

### No dedicated file-transfer abstraction was found

I found no second implementation path elsewhere in `grid-editor` that uses lower-level filesystem packets for reads, writes, rename, or delete.

Everything meaningful routes through `FileManager.svelte` plus the shared raw `EVALUATE` parser.

### Protocol metadata hints at filesystem helper names

`grid-editor/node_modules/@intechstudio/grid-protocol/dist/index.js:277-282` defines protocol metadata entries named:

1. `readdir` / short name `gfls`
2. `readfile` / short name `gfcat`

But the checked-in editor code does not appear to call those helpers anywhere.

So there may be deeper firmware capability here, but the current editor implementation is not using it in a way that gave me a standalone host contract yet.

### Weak response correlation

`sendLua(...)` waits for a response using filter:

```ts
filter: {
  class_name: "EVALUATE",
  brc_parameters: {},
  class_parameters: {},
}
```

Source: `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:96-109`.

`grid-editor/src/renderer/runtime/engine.store.ts:375-431` then validates only:

1. matching `class_name`
2. any explicitly provided BRC parameters
3. any explicitly provided class parameters except `LASTHEADER`

Because the File Manager passes empty `brc_parameters` and empty `class_parameters`, any inbound `EVALUATE` message can satisfy the waiter.

This is acceptable only because the editor currently serializes this work and effectively assumes no competing `EVALUATE` traffic.

For `vsn1-cli`, response matching should be stronger.

### Single global waiter and timeout retry behavior

`grid-editor/src/renderer/runtime/engine.store.ts:171-172` keeps a single global `waiter`.

`grid-editor/src/renderer/runtime/engine.store.ts:333-336` also retries recursively on timeout.

Practical consequence:

1. concurrent response-required raw evaluate operations are fragile
2. a failed module response can lead to repeated retries instead of a crisp fail-fast error

That is not a good model to copy into `vsn1-cli`.

### Current UI is text-oriented, not a robust arbitrary-binary file tool

The docs market File Manager broadly, but the checked-in UI is still text-centric.

Examples:

1. the editor surface is Monaco text editing
2. string escaping is custom and Lua-string-based in `grid-editor/src/renderer/main/panels/FileManager/FileManager.svelte:183-191`
3. `.lua` files are transformed through formatter/minifier steps
4. the evaluate parser blindly decodes `\xNN` escape sequences in returned strings

So while the firmware may allow arbitrary file contents, the current editor implementation is not the right reference for binary-safe host behavior.

### Directory operations are incomplete

The UI enables copy and delete for whatever is selected except `.` and `..`.

But:

1. copy uses `io.open(src, "r")`, so it only truly supports files
2. delete uses `os.remove(path)`, with no recursive directory handling
3. there is no separate recursive remove or recursive copy implementation

### No path sanitization

New file, new folder, copy, and rename simply concatenate `currentPath + opValue.trim()`.

The UI does not block slashes, `..`, or other path escapes.

A future CLI should have a stronger contract and validation story.

### No tests found for File Manager

I did not find File Manager coverage under `grid-editor/playwright-tests/`.

So the checked-in editor is useful as a behavior reference, but not as a high-confidence correctness oracle.

### Doc/code drift already exists

The public docs still say to enable File Manager from Package Manager.

But `grid-editor/src/electron/main.ts:355-361` now force-enables `file-manager` as an always-enabled built-in package.

That is minor, but it is a reminder not to treat docs as the only source of truth when implementing this feature.

## Implications For `vsn1-cli`

### Main product opportunity

This capability is relevant because it gives `vsn1-cli` a plausible path to store runtime helpers, assets, or larger Lua code on the module itself and avoid relying on a single configuration slot's `~900` character ceiling.

### Most likely first implementation strategy

The smallest realistic standalone implementation path appears to be:

1. implement raw module `EVALUATE` send/receive in Rust
2. implement Lua-value response decoding for nil, boolean, number, string, and table
3. build filesystem operations by executing small Lua snippets on the module
4. reserve a dedicated `vsn1-cli` namespace on the module filesystem
5. keep regular slot content small and use it only as a loader into uploaded files when needed

This would mirror the editor's real behavior without requiring `grid-editor`.

### Things `vsn1-cli` should improve instead of copying directly

If this is implemented later, do not copy the editor's rough edges unchanged.

Recommended differences:

1. use explicit module targeting, not broadcast-first, for filesystem mutation commands
2. use stronger response correlation than the editor currently uses
3. fail once on timeout with clear diagnostics instead of recursive retries
4. separate byte-preserving upload mode from editor-compatible Lua transform mode
5. verify saved file content with content comparison or hashing, not size only
6. choose chunk sizes based on hardware validation rather than inheriting the editor's provisional `50` byte constants
7. provide explicit namespace and path validation to avoid clobbering Grid-owned configuration files

### Hardware-in-loop questions to answer before implementing

Before productizing this in `vsn1-cli`, hardware validation should confirm:

1. the exact maximum safe evaluate payload size
2. whether chunk sizes significantly larger than `50` are reliable
3. the real persistence and wear behavior of filesystem writes
4. the exact on-device directory layout for page, element, and event-backed configuration files
5. whether there is a better firmware-exposed helper path than raw `io.open` snippets
6. whether file-based Lua loading works cleanly for the VSN1 runtime shape we need

### Scope caution for the current project

This is promising follow-up scope, but it should still be treated as a deliberate extension, not an assumption baked into current `vsn1-cli` architecture.

Reasons:

1. the current implementation evidence is UI-driven and somewhat ad hoc
2. the protocol details are partly magic-number-based
3. the firmware filesystem capability clearly exists, but the host contract is not yet polished

## Recommended Future Follow-Up

If this becomes active implementation work later, the clean next investigation order is:

1. capture raw USB traffic for one list, one read, one write, and one rename operation against a real module
2. confirm the exact `EVALUATE` class structure independently from the Svelte implementation
3. prototype a Rust-side `evaluate()` helper plus Lua-value decoder
4. validate a dedicated `vsn1-cli` on-module directory layout
5. only then decide whether runtime installation should remain slot-first, file-first, or hybrid
