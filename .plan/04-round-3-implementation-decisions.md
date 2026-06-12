# VSN1 CLI Planning: Round 3 Implementation Decisions

## Source

User answers collected interactively on 2026-06-11.

## Confirmed decisions

1. **Initial `screen` mutation surface:** `set` and `clear` first.
   - v1 should prioritize a small, stable screen-writing API.
   - More specialized screen verbs can be layered later once the parameter-slot model is proven.

2. **Parameter naming policy:** curated stable names.
   - The CLI should own and document the supported slot/field names.
   - Unknown names should be rejected rather than passed through silently.

3. **Runtime compatibility policy:** exact version match required.
   - Screen commands that depend on the bundled runtime must fail unless the installed runtime matches exactly.
   - This reinforces the bundled-runtime strategy and reduces compatibility ambiguity.

4. **Runtime lifecycle scope for the first milestone:** full lifecycle.
   - Initial implementation planning should cover install, verify, upgrade, repair, and remove.
   - These can still be phased, but the architecture should support the full set from the start.

5. **Low-level escape hatch:** public raw command.
   - v1 should expose an expert-facing raw operation in the CLI.
   - This command should remain clearly separate from the curated high-level parameter-slot surface.

## Immediate implications

1. The screen command family now has a likely core shape:
   - `screen set ...`
   - `screen clear ...`
   - `screen raw ...`

2. The runtime subsystem needs a durable version identity and validation strategy, likely based on:
   - bundled manifest metadata
   - known target script slots
   - exact content hashes or another exact-match mechanism

3. Since parameter names are curated, the runtime bundle and the host-side screen API must be versioned together.

4. Since a raw command is public, the CLI will need a clear safety boundary between:
   - supported stable commands
   - unsafe or expert-only direct operations

## Remaining implementation details to define in the architecture plan

1. Exact command syntax and argument conventions.
2. Internal module boundaries in the Rust library.
3. How bundled runtime assets are represented and verified.
4. Which script/config slots the CLI will own during provisioning.
5. How `remove` behaves without damaging unrelated user/device state.
