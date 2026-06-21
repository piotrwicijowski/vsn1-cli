# Global Or Shared Screen Fields Follow-Up

## Purpose

This note captures the current implementation answer to a future question: can `vsn1-cli` support curated screen fields that are not scoped to a specific manifest-defined layer?

Example motivating use case:

- a `title` value that should appear on every visible layer without repeating the value into each layer separately

## Current answer

No. The current curated screen model does not support layerless or global fields.

Every curated field is currently required to:

1. reference a declared manifest layer
2. use a public name in `<layer>.<field>` form
3. compile through the generic runtime helper `set_field(layer_name, runtime_key, value)`

That means a field such as `title=Tempo` or any other layerless public field shape is not valid in the current contract.

## Current implementation constraints

### Manifest contract

The manifest-defined layer model requires:

1. each field entry to reference an existing declared layer
2. each public field name to use `<layer>.<field>` syntax where the prefix matches the declared layer name

This is part of both the implementation and the manifest-defined layer follow-up plan.

### Host compiler contract

The host compiler currently assumes every curated field update is layer-targeted:

1. `screen set ...` compiles to one or more `set_field(layer_name, runtime_key, value)` calls
2. `screen activate <layer>` compiles to `activate_layer(layer_name)`
3. `screen clear <layer>` is also layer-based

There is no host/runtime concept today for:

1. `set_global_field(...)`
2. a manifest field with no layer
3. a public field name that omits the layer prefix

### Clear/default behavior is not fully manifest-driven yet

Even beyond the global-field question, curated clear/default behavior is still hard-coded by field name on the host side.

Practical consequence:

1. adding a new field such as `slow.title` would not be fully plug-and-play today
2. host code would also need to learn that field's clear/default behavior

So the current model is stricter than just "fields must name a layer".

## What is possible today

### Option 1: runtime-side shared rendering from an existing persistent field

This is the smallest path if the goal is only "show one title across all visible layers".

Approach:

1. keep a field such as `persistent.title`
2. teach the runtime renderer to continue drawing that title even when a temporary layer is visible

Implications:

1. no public CLI contract change is required
2. no manifest schema change is required
3. behavior is runtime-specific, not a general global-field feature

This is the most minimal future path if the need is purely visual reuse.

### Option 2: duplicate per-layer fields

Approach:

1. add fields such as `persistent.title`, `slow.title`, and `fast.title`
2. set each one explicitly when needed

Implications:

1. conceptually straightforward
2. repetitive for callers
3. still requires host work today because clear/default handling is not yet fully manifest-defined

## If true global fields are ever desired

Supporting real global fields cleanly would likely require all of the following:

1. manifest support for a field scope that is not tied to a single layer
2. a host validation model that accepts public names outside `<layer>.<field>` form, or a separate namespace such as `global.title`
3. a runtime helper contract addition such as `set_global_field(key, value)` or equivalent runtime state handling
4. clear/default behavior for globals, ideally moved into the manifest instead of remaining host-hard-coded
5. explicit rendering semantics for how globals combine with active persistent and temporary layers

That is a broader contract change and should be treated as a scoped follow-up, not a small tweak.

## Recommendation

If this topic comes back later, prefer this order:

1. first decide whether the real need is only shared visual rendering across layers
2. if yes, prefer runtime-side reuse of an existing persistent field
3. only design true global fields if there is a concrete need for global state that is independently set, cleared, validated, and rendered across runtimes
