# VSN1 CLI Planning: Round 1 Architecture Decisions

## Source

User answers collected interactively on 2026-06-11.

## Confirmed decisions

1. **v1 scope:** screen-first.
   - `vsn1-cli` should focus on driving the VSN1 screen well.
   - Broader device concerns should only be included when they directly support screen workflows.

2. **Execution model:** one-shot CLI only for v1.
   - Do not design the initial product around a daemon or long-lived session mode.
   - Persistent-process ideas can remain internal future options, but they are not a v1 product requirement.

3. **Public input surface:** subcommands only.
   - The first public UX should be human-invoked CLI commands and flags.
   - JSON, script, and streaming surfaces are not primary v1 requirements.

4. **Rendering model:** hybrid high-level API.
   - The user-facing model should be higher level than raw Lua.
   - Internally, the implementation may target runtime helpers and selected low-level primitives as needed.

5. **Runtime ownership:** provision runtime in v1.
   - `vsn1-cli` should own installing or updating the required runtime/profile, not just assume it already exists.

6. **Code shape:** reusable library plus binary.
   - Build a Rust library crate with the device/protocol/rendering logic.
   - Keep the CLI as a thin layer over the library.

7. **Verification standard:** hardware-in-loop required.
   - Major implementation milestones must be validated on a real device.
   - Non-hardware tests are still useful, but they do not replace device confirmation.

## Immediate implications

1. The architecture should separate:
   - transport/protocol internals
   - runtime provisioning
   - screen-oriented command handlers
   - host-side render/update planning

2. Since v1 is one-shot CLI only, the baseline command path should succeed without requiring a background service.

3. Since runtime provisioning is in scope, implementation planning must include:
   - how the runtime assets are represented in the repo
   - how installation/update is performed over USB
   - how compatibility is verified after provisioning

4. Since the public interface is subcommands only, the initial UX should be designed around a small set of stable verbs rather than a general-purpose scripting layer.

## Still unresolved after round 1

1. Which exact subcommands belong in v1.
2. What the high-level screen model should look like.
3. Whether v1 should target one module by default or support multi-target addressing from day one.
4. How runtime provisioning assets should be packaged and versioned.
5. What error semantics and recovery policy the CLI should follow.
