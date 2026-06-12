# VSN1 CLI Planning: Round 2 Product Shape Decisions

## Source

User answers collected interactively on 2026-06-11.

## Confirmed decisions

1. **Top-level v1 command groups:** `device`, `runtime`, `screen`.
   - Public v1 UX should stay centered on these three verb families.
   - Low-level debugging can exist internally or as future work, but it is not required as a first-class v1 public surface.

2. **Default targeting behavior:** broadcast first.
   - The default command behavior should target the active screen update path broadly unless a narrower target is specified.
   - This must be balanced with the fail-fast policy so ambiguous or dangerous cases are still surfaced clearly.

3. **High-level screen model:** parameter-slot model.
   - The screen API should be closest to the existing runtime-helper concepts rather than a generic graphics canvas.
   - Lower-level draw operations can still exist underneath the abstraction or as limited escape hatches.

4. **Provisioning asset strategy:** bundle a known-good runtime.
   - The CLI should ship with a vetted runtime/profile asset set.
   - Runtime install/update behavior should be versioned alongside the codebase.

5. **Failure policy:** fail fast with clear errors.
   - Commands should stop on ambiguity, mismatch, or transport/provisioning failure.
   - Diagnostics should explain what failed and what the user can do next.

## Combined implications

1. The public command model can stay small and opinionated:
   - `device ...`
   - `runtime ...`
   - `screen ...`

2. Since the screen model is parameter-slot oriented, the implementation should first optimize for:
   - setting named values
   - clearing/resetting known display state
   - verifying runtime compatibility with the bundled profile/runtime

3. Since provisioning is bundled and fail-fast, the CLI should have a strict understanding of:
   - the bundled runtime version
   - install/upgrade preconditions
   - post-install validation steps

4. Broadcast-first targeting suggests the device/runtime model likely assumes a known deployment topology, but this still needs explicit command semantics so users know when broadcast is happening.

## Remaining design questions

1. What exact `screen` subcommands belong in v1.
2. Whether parameter names should be fixed and curated by the tool or passed through from the user.
3. How strictly the CLI should enforce runtime version matching.
4. Whether provisioning should support install-only, upgrade-only, repair, and verify as separate operations.
5. Whether low-level escape hatches should exist in the library, the CLI, or neither in v1.
