# Validation Matrix

## Host Build Matrix

| Host target | Status | Notes |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | validated in this workspace | `cargo fmt --check`, `cargo test`, and `cargo check` are the required baseline checks |
| macOS targets | pending host confirmation | cross-target validation should be recorded here once `cargo check --target <apple-target>` succeeds in a host environment with the target installed |

## Hardware Validation Matrix

| Date | Host | Device target | Coverage | Result |
| --- | --- | --- | --- | --- |
| 2026-06-17 | Linux | `/dev/ttyACM0`, `dx=0 dy=0` | device discovery, `screen raw`, runtime install/verify/status, layered `screen set` / `screen clear` / `screen activate`, runtime upgrade/repair/remove | pass |

## Known Limits

- Reliable visible screen updates are currently budgeted at about `5-10` updates per second depending on payload shape.
- The fast live path requires framed `IMMEDIATE` Lua payloads in the form `<?lua --[[@cb]] ... ?>`.
- The current validated runtime contract owns only the LCD init and LCD draw slots identified in the POC.
- Curated screen commands now take the same fast immediate-send path as `screen raw`; runtime lifecycle commands remain available to provision and verify the bundled layered runtime separately.

## Follow-Up Recording Rules

- Add a new row whenever host build validation runs on a new platform or target triple.
- Add a new row whenever a major runtime or screen behavior change is validated on hardware.
- Record failures here too when they uncover an environment-specific limit or regression.
