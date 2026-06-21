# Validation Matrix

## Host Build Matrix

| Host target | Status | Notes |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | validated in this workspace on 2026-06-21 | `cargo fmt --check`, `cargo test`, and `cargo check` passed |
| `x86_64-apple-darwin` | cross-target validated in this workspace on 2026-06-21 | `cargo check --target x86_64-apple-darwin` passed with the Rust target installed locally |
| `aarch64-apple-darwin` | cross-target validated in this workspace on 2026-06-21 | `cargo check --target aarch64-apple-darwin` passed with the Rust target installed locally |

## Hardware Validation Matrix

| Date | Host | Device target | Coverage | Result |
| --- | --- | --- | --- | --- |
| 2026-06-17 | Linux | `/dev/ttyACM0`, `dx=0 dy=0` | device discovery, `screen raw`, runtime install/verify/status, layered `screen set` / `screen clear` / `screen activate`, runtime upgrade/repair/remove | pass |
| 2026-06-20 | Linux | `/dev/ttyACM0`, `dx=0 dy=0` | frozen-runtime `runtime status` / `runtime verify` exact-match, drift, and missing-local-copy behavior; named `runtime install default`; `runtime remove` restore-from-backup; `runtime remove` fallback-to-clear | pass |
| 2026-06-21 | Linux | unavailable in this workspace | attempted Step 21 manifest-defined layer hardware validation for activation, timeout, priority, fallback, and timer restart behavior | blocked: `cargo run -- device list` reported `No supported VSN1/Grid USB serial devices found.` |
| 2026-06-21 | Linux | `/dev/ttyACM0`, `dx=0 dy=0` | manifest-defined layer runtime install/verify/status with `default`; persistent-layer activation acceptance; `slow`/`fast` activation; temporary priority and fallback; timeout expiry and timer restart; lower-layer non-preemption; `screen clear slow` / `screen clear fast` fallback | pass |

## Known Limits

- Reliable visible screen updates are currently budgeted at about `5-10` updates per second depending on payload shape.
- The fast live path requires framed `IMMEDIATE` Lua payloads in the form `<?lua --[[@cb]] ... ?>`.
- The current validated runtime contract owns only the LCD init and LCD draw slots identified in the POC.
- Curated screen commands now take the same fast immediate-send path as `screen raw`; `screen set`, `screen clear`, and `screen activate` require the frozen installed runtime copy under `~/.config/vsn1-cli/runtime`, and `screen set` / `screen clear` load their field metadata from that copy.
- Layer names are runtime-defined by the installed manifest; the shipped `default` runtime currently declares `persistent`, `slow`, and `fast`.
- The shipped `default` runtime exposes only one persistent layer, so explicit hardware validation of persistent-to-persistent base switching still requires a runtime that declares multiple persistent layers.

## Follow-Up Recording Rules

- Add a new row whenever host build validation runs on a new platform or target triple.
- Add a new row whenever a major runtime or screen behavior change is validated on hardware.
- Record failures here too when they uncover an environment-specific limit or regression.
