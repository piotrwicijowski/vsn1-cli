# vsn1-cli

Standalone Rust CLI for controlling the `VSN1` screen directly over USB.

This project is intentionally screen-first and one-shot only. The CLI provisions a named runtime onto the device, freezes a local copy under `~/.config/vsn1-cli/runtime`, and then uses fast framed `IMMEDIATE` Lua updates for live screen control.

## Current Scope

- Linux is the primary validated host today.
- macOS support is a target, but host-side validation is still in progress.
- Curated public commands are grouped under `device`, `runtime`, and `screen`.
- Curated screen mutations load their field metadata from the frozen installed runtime copy under `~/.config/vsn1-cli/runtime`.
- Curated `screen` commands now use manifest-defined layer inventory from the frozen installed runtime copy. The shipped `default` runtime currently declares `persistent`, `slow`, and `fast`, but other runtimes may declare different layer names and activation behavior.

## Install And Build

Build from the checkout:

```bash
cargo build
```

Install the CLI system-wide with the checked-in runtimes:

```bash
make install
sudo make install
```

Default install locations:

- binary: `/usr/local/bin/vsn1-cli`
- runtimes: `/usr/share/vsn1-cli/runtimes`

Override paths for packaging or staged installs with `DESTDIR`, `BINDIR`, or `RUNTIME_ROOT`.

Quick verification after install:

```bash
vsn1-cli --help
vsn1-cli runtime list
```

Remove the system-wide install:

```bash
sudo make uninstall
```

Show help from the dev checkout without installing:

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

Examples below use the shipped `default` runtime, which declares `persistent`, `slow`, and `fast`.

Set curated fields on the current default runtime's persistent base layer:

```bash
cargo run -- screen set persistent.title=Tempo persistent.value=64
```

Set and activate the current default runtime's `slow` temporary layer:

```bash
cargo run -- screen set slow.message='Disk almost full' --activate slow
```

Set and activate the current default runtime's `fast` temporary layer on an explicit target:

```bash
cargo run -- screen set fast.action=Tap --activate fast --dx 0 --dy 0
```

Clear a specific manifest-declared layer:

```bash
cargo run -- screen clear persistent
cargo run -- screen clear slow
```

Activate a manifest-declared layer without changing its stored values:

```bash
cargo run -- screen activate slow
cargo run -- screen activate fast
```

`screen activate` now requires the frozen installed runtime copy under `~/.config/vsn1-cli/runtime`, just like `screen set` and `screen clear`. Activating a persistent layer switches the active base layer; activating a temporary layer starts or restarts that layer's timeout.

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

Curated `screen` commands compile to the generic runtime helper contract used by the shipped default runtime: `set_field(layer_name, runtime_key, value)` and `activate_layer(layer_name)`.

## Runtime Compatibility Rules

- Curated `screen set`, `screen clear`, and `screen activate` commands send immediate runtime-helper Lua without a preflight exact-match verification step.
- `screen set` and `screen clear` load their curated field metadata from the frozen installed runtime copy under `~/.config/vsn1-cli/runtime`.
- Curated `screen` compilation is layer-inventory-driven and targets the generic `set_field(...)` / `activate_layer(...)` helper contract used by the shipped default runtime.
- `runtime install <name>` is the supported way to provision the runtime that curated helpers target.
- `screen raw` bypasses curated field validation and runtime-shape compilation, but still uses the same transport and packet framing path.
- Runtime lifecycle commands only touch the manifest-owned slots.

## Known Limits

- The practical visible update budget is currently about `5-10` updates per second depending on payload shape.
- The fast live path depends on framed `IMMEDIATE` Lua in the form `<?lua --[[@cb]] ... ?>`.
- Major behavior changes still require hardware-in-loop validation on a real device.

## Validation Notes

See `docs/validation-matrix.md` for the current host/hardware validation record and known constraints.
