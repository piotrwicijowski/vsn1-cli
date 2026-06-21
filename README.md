# vsn1-cli

Standalone Rust CLI for controlling the `VSN1` screen directly over USB.

This project is intentionally screen-first and one-shot only. The CLI provisions a named runtime onto the device, freezes a local copy under `~/.config/vsn1-cli/runtime`, and then uses fast framed `IMMEDIATE` Lua updates for live screen control.

## Current Scope

- Linux is the primary validated host today.
- macOS support is a target, but host-side validation is still in progress.
- Curated public commands are grouped under `device`, `runtime`, and `screen`.
- Curated screen mutations load their field metadata from the frozen installed runtime copy under `~/.config/vsn1-cli/runtime`.

## Install And Build

```bash
cargo build
```

Show help:

```bash
cargo run -- --help
```

## Device Commands

List supported USB serial devices:

```bash
cargo run -- device list
```

Inspect the currently selected device and confirm transport open at `2000000` baud:

```bash
cargo run -- device info
cargo run -- device info --dx 0 --dy 0
```

Targeting rules:

- Omitting both `--dx` and `--dy` uses broadcast targeting.
- Supplying one coordinate without the other is rejected.
- Current single-device flows require exactly one discovered supported USB serial device.

## Runtime Commands

List discovered runtimes and the source copy that won precedence:

```bash
cargo run -- runtime list
```

Install a discovered runtime into the manifest-owned slots:

```bash
cargo run -- runtime install default
```

Verify exact frozen installed-runtime content match:

```bash
cargo run -- runtime verify
```

Inspect status relative to the frozen installed runtime copy:

```bash
cargo run -- runtime status
```

Repair drifted owned slots from the frozen installed runtime copy:

```bash
cargo run -- runtime repair
```

Overwrite the device from a discovered runtime without refreshing the pre-install backup:

```bash
cargo run -- runtime upgrade default
```

Restore the pre-install backup or clear the frozen runtime's owned slots:

```bash
cargo run -- runtime remove
cargo run -- runtime uninstall
```

Runtime discovery roots:

- `/usr/share/vsn1-cli/runtimes`
- `~/.local/share/vsn1-cli/runtimes`
- `assets/runtimes` when running from a dev checkout

Name collisions resolve by source precedence: `dev` > `user` > `system`.

Runtime lifecycle persistence:

- `runtime install <name>` refreshes `~/.config/vsn1-cli/pre-install` and freezes the selected runtime under `~/.config/vsn1-cli/runtime`.
- `runtime upgrade <name>` refreshes `~/.config/vsn1-cli/runtime` but does not refresh `~/.config/vsn1-cli/pre-install`.
- `runtime remove` / `runtime uninstall` restore from `~/.config/vsn1-cli/pre-install` when available, otherwise they clear the frozen runtime's owned slots with a warning.

## Screen Commands

Set curated persistent fields:

```bash
cargo run -- screen set persistent.title=Tempo persistent.value=64
```

Set and activate the `slow` overlay:

```bash
cargo run -- screen set slow.message='Disk almost full' --activate slow
```

Set and activate the `fast` overlay on an explicit target:

```bash
cargo run -- screen set fast.action=Tap --activate fast --dx 0 --dy 0
```

Clear a specific layer:

```bash
cargo run -- screen clear persistent
cargo run -- screen clear slow
```

Activate a temporary layer without changing its stored values:

```bash
cargo run -- screen activate slow
cargo run -- screen activate fast
```

`screen activate` now requires the frozen installed runtime copy under `~/.config/vsn1-cli/runtime`, just like `screen set` and `screen clear`.

Send expert-facing raw Lua:

```bash
cargo run -- screen raw "lcd:ldrr(0,0,128,64); lcd:ldsw()"
```

Current default-runtime curated fields:

- `persistent.title`
- `persistent.bottom`
- `persistent.value`
- `persistent.min`
- `persistent.max`
- `persistent.default`
- `persistent.step`
- `persistent.info`
- `persistent.clamp_min`
- `persistent.clamp_max`
- `persistent.bank`
- `slow.message`
- `fast.action`

## Runtime Compatibility Rules

- Curated `screen set`, `screen clear`, and `screen activate` commands send immediate runtime-helper Lua without a preflight exact-match verification step.
- `screen set` and `screen clear` load their curated field metadata from the frozen installed runtime copy under `~/.config/vsn1-cli/runtime`.
- `runtime install <name>` is the supported way to provision the runtime that curated helpers target.
- `screen raw` bypasses curated field validation and runtime-shape compilation, but still uses the same transport and packet framing path.
- Runtime lifecycle commands only touch the manifest-owned slots.

## Known Limits

- The practical visible update budget is currently about `5-10` updates per second depending on payload shape.
- The fast live path depends on framed `IMMEDIATE` Lua in the form `<?lua --[[@cb]] ... ?>`.
- Major behavior changes still require hardware-in-loop validation on a real device.

## Validation Notes

See `docs/validation-matrix.md` for the current host/hardware validation record and known constraints.
