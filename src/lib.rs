pub mod device;
mod error;
pub mod protocol;
pub mod raw;
pub mod runtime;
pub mod runtime_bundle;
pub mod screen;
pub mod targeting;
pub mod transport;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand};

use crate::device::{
    discover_supported_devices, select_single_device, DeviceDiscovery, SystemDeviceDiscovery,
};
use crate::raw::send_screen_raw;
use crate::runtime::{
    inspect_installed_runtime, install_runtime_with_bundle_dir, remove_installed_runtime,
    repair_installed_runtime, upgrade_runtime_with_bundle_dir, verify_installed_runtime,
    RuntimeInspectionReport, RuntimeInstallReport, RuntimeRemoveReport, RuntimeSlotStatus,
    RuntimeUpgradeReport, TransportRuntimeSlotReader,
};
use crate::runtime_bundle::{discover_runtimes, resolve_runtime, DiscoveredRuntime};
use crate::screen::{
    compile_activate_lua, compile_clear_lua, compile_set_lua, ScreenFieldRegistry,
};
use crate::targeting::{resolve_target, ResolvedTarget};
use crate::transport::{SerialTransportFactory, SystemTransportFactory};

pub use error::{Error, Result};

const TOP_LEVEL_LONG_ABOUT: &str = "Standalone CLI for controlling the VSN1 display over USB.\n\nUse `runtime install <name>` to provision a discovered runtime that the curated runtime-defined layered `screen` helpers expect.";
const DEVICE_INFO_AFTER_HELP: &str =
    "Examples:\n  vsn1-cli device info\n  vsn1-cli device info --dx 0 --dy 0";
const RUNTIME_LIST_AFTER_HELP: &str = "Lists discovered runtime names and the source copy that won resolution. Discovery precedence is dev > user > system on directory-name collisions.";
const RUNTIME_INSTALL_AFTER_HELP: &str = "Installs the selected discovered runtime into the manifest-owned slots, captures a pre-install backup under ~/.config/vsn1-cli/pre-install, freezes the runtime under ~/.config/vsn1-cli/runtime, and verifies an exact installed-runtime match.";
const RUNTIME_VERIFY_AFTER_HELP: &str = "Fails unless every owned runtime slot matches the frozen installed runtime copy under ~/.config/vsn1-cli/runtime exactly.";
const RUNTIME_UPGRADE_AFTER_HELP: &str = "Overwrites the device from the selected discovered runtime, refreshes the frozen runtime copy under ~/.config/vsn1-cli/runtime, and does not refresh the pre-install backup.";
const RUNTIME_REPAIR_AFTER_HELP: &str =
    "Reapplies the frozen installed runtime copy when the owned slots are drifted or incomplete.";
const RUNTIME_REMOVE_AFTER_HELP: &str =
    "Restores the pre-install backup when available, otherwise clears the frozen runtime's owned slots with a warning, then removes ~/.config/vsn1-cli/runtime.";
const RUNTIME_STATUS_AFTER_HELP: &str = "Shows the owned-slot inspection result relative to the frozen installed runtime copy when one is present locally.";
const SCREEN_SET_AFTER_HELP: &str = "Examples:\n  vsn1-cli screen set persistent.title=Tempo persistent.value=64\n  vsn1-cli screen set slow.message='Disk almost full' --activate slow\n  vsn1-cli screen set fast.action=Tap --activate fast --dx 0 --dy 0\n\nExamples use the shipped `default` runtime. Curated screen fields and layer names are loaded from the frozen installed runtime copy under ~/.config/vsn1-cli/runtime, so other runtimes may declare different names.";
const SCREEN_CLEAR_AFTER_HELP: &str =
    "Examples:\n  vsn1-cli screen clear persistent\n  vsn1-cli screen clear slow --dx 0 --dy 0\n\nExamples use the shipped `default` runtime. Layer names are validated against the frozen installed runtime copy under ~/.config/vsn1-cli/runtime.";
const SCREEN_RAW_AFTER_HELP: &str = "Examples:\n  vsn1-cli screen raw \"set_field('persistent','t','Hello')\"\n  vsn1-cli screen raw \"lcd:ldrr(0,0,128,64); lcd:ldsw()\" --dx 0 --dy 0\n\n`screen raw` bypasses the curated field registry and runtime-shape validation and can call whatever helper surface the installed runtime exposes.";
const SCREEN_ACTIVATE_AFTER_HELP: &str =
    "Examples:\n  vsn1-cli screen activate persistent\n  vsn1-cli screen activate slow\n  vsn1-cli screen activate fast --dx 0 --dy 0\n\nExamples use the shipped `default` runtime. `screen activate` validates layer names against the frozen installed runtime copy under ~/.config/vsn1-cli/runtime. Persistent-layer activation switches the active base layer; temporary-layer activation starts or restarts that layer's timeout.";

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "vsn1-cli",
    version,
    about = "Standalone CLI for controlling the VSN1 display over USB",
    long_about = TOP_LEVEL_LONG_ABOUT,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: TopLevelCommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum TopLevelCommand {
    #[command(about = "Discover attached VSN1/Grid USB serial devices")]
    Device(DeviceArgs),
    #[command(about = "Install, verify, inspect, repair, upgrade, and remove named runtimes")]
    Runtime(RuntimeArgs),
    #[command(about = "Send curated or raw screen updates")]
    Screen(ScreenArgs),
}

#[derive(Debug, Args, PartialEq, Eq)]
#[command(arg_required_else_help = true)]
pub struct DeviceArgs {
    #[command(subcommand)]
    pub command: DeviceCommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum DeviceCommand {
    #[command(about = "List supported VSN1/Grid USB serial devices visible on this host")]
    List,
    #[command(
        about = "Open one discovered device and show the resolved module target",
        after_help = DEVICE_INFO_AFTER_HELP
    )]
    Info {
        #[command(flatten)]
        target: TargetArgs,
    },
}

#[derive(Debug, Args, PartialEq, Eq)]
#[command(arg_required_else_help = true)]
pub struct RuntimeArgs {
    #[command(subcommand)]
    pub command: RuntimeCommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum RuntimeCommand {
    #[command(
        about = "List discovered runtimes and their winning source copies",
        after_help = RUNTIME_LIST_AFTER_HELP
    )]
    List,
    #[command(
        about = "Install a discovered runtime into the owned device slots",
        after_help = RUNTIME_INSTALL_AFTER_HELP
    )]
    Install {
        #[arg(value_name = "NAME", help = "Discovered runtime name to install")]
        name: String,
        #[command(flatten)]
        target: TargetArgs,
    },
    #[command(
        about = "Verify that the owned slots exactly match the frozen installed runtime copy",
        after_help = RUNTIME_VERIFY_AFTER_HELP
    )]
    Verify {
        #[command(flatten)]
        target: TargetArgs,
    },
    #[command(
        about = "Overwrite the device from a discovered runtime without refreshing the pre-install backup",
        after_help = RUNTIME_UPGRADE_AFTER_HELP
    )]
    Upgrade {
        #[arg(
            value_name = "NAME",
            help = "Discovered runtime name to install as the upgrade target"
        )]
        name: String,
        #[command(flatten)]
        target: TargetArgs,
    },
    #[command(
        about = "Repair drifted or incomplete owned runtime slots",
        after_help = RUNTIME_REPAIR_AFTER_HELP
    )]
    Repair {
        #[command(flatten)]
        target: TargetArgs,
    },
    #[command(
        about = "Restore the pre-install backup or clear the frozen runtime's owned slots",
        visible_alias = "uninstall",
        after_help = RUNTIME_REMOVE_AFTER_HELP
    )]
    Remove {
        #[command(flatten)]
        target: TargetArgs,
    },
    #[command(
        about = "Inspect the owned runtime slots relative to the frozen installed runtime copy",
        after_help = RUNTIME_STATUS_AFTER_HELP
    )]
    Status {
        #[command(flatten)]
        target: TargetArgs,
    },
}

#[derive(Debug, Args, PartialEq, Eq)]
#[command(arg_required_else_help = true)]
pub struct ScreenArgs {
    #[command(subcommand)]
    pub command: ScreenCommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum ScreenCommand {
    #[command(
        about = "Update one or more curated screen fields",
        after_help = SCREEN_SET_AFTER_HELP
    )]
    Set {
        #[arg(
            value_name = "FIELD=VALUE",
            required = true,
            num_args = 1..,
            help = "One or more curated screen field assignments"
        )]
        assignments: Vec<String>,
        #[arg(
            long,
            value_name = "LAYER",
            help = "Activate a manifest-defined layer after updating it"
        )]
        activate: Option<String>,
        #[command(flatten)]
        target: TargetArgs,
    },
    #[command(
        about = "Clear one curated screen layer back to its runtime defaults",
        after_help = SCREEN_CLEAR_AFTER_HELP
    )]
    Clear {
        #[arg(value_name = "LAYER", help = "Manifest-defined layer to clear")]
        layer: String,
        #[command(flatten)]
        target: TargetArgs,
    },
    #[command(
        about = "Send expert-facing raw Lua over the immediate path",
        after_help = SCREEN_RAW_AFTER_HELP
    )]
    Raw {
        #[arg(
            value_name = "LUA",
            help = "Raw Lua snippet to frame and send over the immediate path"
        )]
        lua: String,
        #[command(flatten)]
        target: TargetArgs,
    },
    #[command(
        about = "Activate a manifest-defined layer",
        after_help = SCREEN_ACTIVATE_AFTER_HELP
    )]
    Activate {
        #[arg(value_name = "LAYER", help = "Manifest-defined layer to activate")]
        layer: String,
        #[command(flatten)]
        target: TargetArgs,
    },
}

#[derive(Debug, Args, Clone, Default, PartialEq, Eq)]
pub struct TargetArgs {
    #[arg(
        long,
        help = "Explicit module x coordinate; omit both --dx and --dy to use broadcast targeting",
        help_heading = "Targeting"
    )]
    pub dx: Option<u16>,
    #[arg(
        long,
        help = "Explicit module y coordinate; omit both --dx and --dy to use broadcast targeting",
        help_heading = "Targeting"
    )]
    pub dy: Option<u16>,
}

pub fn command() -> clap::Command {
    Cli::command()
}

pub fn try_parse_from<I, T>(args: I) -> std::result::Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    Cli::try_parse_from(args)
}

pub fn run(cli: Cli) -> Result<()> {
    let discovery = SystemDeviceDiscovery;
    let mut transport_factory = SystemTransportFactory;
    let output = execute_cli(cli, &discovery, &mut transport_factory)?;
    print!("{output}");
    Ok(())
}

pub fn main() -> ExitCode {
    match try_parse_from(std::env::args_os()) {
        Ok(cli) => match run(cli) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            let exit_code = error.exit_code();
            let _ = error.print();

            u8::try_from(exit_code)
                .map(ExitCode::from)
                .unwrap_or(ExitCode::FAILURE)
        }
    }
}

fn execute_cli<D, F>(cli: Cli, discovery: &D, transport_factory: &mut F) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    match cli.command {
        TopLevelCommand::Device(args) => match args.command {
            DeviceCommand::List => render_device_list(discovery),
            DeviceCommand::Info { target } => {
                render_device_info(discovery, transport_factory, &target)
            }
        },
        TopLevelCommand::Runtime(args) => match args.command {
            RuntimeCommand::List => execute_runtime_list(),
            RuntimeCommand::Install { name, target } => {
                execute_runtime_install(discovery, transport_factory, &name, &target)
            }
            RuntimeCommand::Verify { target } => {
                execute_runtime_verify(discovery, transport_factory, &target)
            }
            RuntimeCommand::Upgrade { name, target } => {
                execute_runtime_upgrade(discovery, transport_factory, &name, &target)
            }
            RuntimeCommand::Repair { target } => {
                execute_runtime_repair(discovery, transport_factory, &target)
            }
            RuntimeCommand::Remove { target } => {
                execute_runtime_remove(discovery, transport_factory, &target)
            }
            RuntimeCommand::Status { target } => {
                execute_runtime_status(discovery, transport_factory, &target)
            }
        },
        TopLevelCommand::Screen(args) => match args.command {
            ScreenCommand::Set {
                assignments,
                activate,
                target,
            } => execute_screen_set(
                discovery,
                transport_factory,
                &target,
                &assignments,
                activate,
            ),
            ScreenCommand::Clear { layer, target } => {
                execute_screen_clear(discovery, transport_factory, &target, &layer)
            }
            ScreenCommand::Raw { lua, target } => {
                execute_screen_raw(discovery, transport_factory, &target, &lua)
            }
            ScreenCommand::Activate { layer, target } => {
                execute_screen_activate(discovery, transport_factory, &target, &layer)
            }
        },
    }
}

fn render_device_list(discovery: &impl DeviceDiscovery) -> Result<String> {
    let devices = discover_supported_devices(discovery)?;

    if devices.is_empty() {
        return Ok("No supported VSN1/Grid USB serial devices found.\n".to_string());
    }

    let mut output = String::from("Discovered supported VSN1/Grid USB serial devices:\n");

    for device in devices {
        output.push_str("- ");
        output.push_str(&device.to_string());
        output.push('\n');
    }

    Ok(output)
}

fn execute_runtime_list() -> Result<String> {
    let runtimes = discover_runtimes()?;

    if runtimes.is_empty() {
        return Ok("No runtimes found in system, user, or dev runtime roots.\n".to_string());
    }

    Ok(render_runtime_list(&runtimes))
}

fn render_runtime_list(runtimes: &[DiscoveredRuntime]) -> String {
    let mut output = String::from("Discovered runtimes:\n");

    for runtime in runtimes {
        output.push_str(&format!(
            "- {} ({}) {}\n",
            runtime.name,
            runtime.source.as_str(),
            runtime.path.display()
        ));
    }

    output
}

fn render_device_info<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let devices = discover_supported_devices(discovery)?;
    let device = select_single_device(&devices)?;
    let _transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;

    Ok(format!(
        "Selected USB device: {device}\nTransport: opened successfully at {} baud\nModule target: {target}\n",
        protocol::GRID_BAUD_RATE
    ))
}

fn execute_screen_raw<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
    lua: &str,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let devices = discover_supported_devices(discovery)?;
    let device = select_single_device(&devices)?;
    let mut transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;

    send_screen_raw(&mut transport, target.grid_target(), lua)?;

    Ok(format!(
        "Selected USB device: {device}\nTransport: opened successfully at {} baud\nModule target: {target}\nSent raw screen update over the immediate path.\n",
        protocol::GRID_BAUD_RATE
    ))
}

fn execute_screen_set<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
    assignments: &[String],
    activate: Option<String>,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let registry = ScreenFieldRegistry::installed()?;
    execute_screen_set_with_registry(
        discovery,
        transport_factory,
        target_args,
        assignments,
        activate,
        &registry,
    )
}

fn execute_screen_set_with_registry<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
    assignments: &[String],
    activate: Option<String>,
    registry: &ScreenFieldRegistry,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let parsed_assignments = registry.parse_assignments(assignments)?;
    let activate_layer = activate
        .as_deref()
        .map(|layer| registry.layer(layer).map(|layer| layer.name().clone()))
        .transpose()?;
    let lua = compile_set_lua(&parsed_assignments, activate_layer.as_ref())?;

    execute_curated_screen_lua(
        discovery,
        transport_factory,
        target_args,
        &lua,
        "curated screen update",
    )
}

fn execute_screen_clear<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
    layer: &str,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let registry = ScreenFieldRegistry::installed()?;
    execute_screen_clear_with_registry(discovery, transport_factory, target_args, layer, &registry)
}

fn execute_screen_clear_with_registry<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
    layer: &str,
    registry: &ScreenFieldRegistry,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let layer = registry.layer(layer)?.name().clone();
    let lua = compile_clear_lua(registry, &layer)?;

    execute_curated_screen_lua(
        discovery,
        transport_factory,
        target_args,
        &lua,
        "screen clear command",
    )
}

fn execute_screen_activate<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
    layer: &str,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let registry = ScreenFieldRegistry::installed()?;
    execute_screen_activate_with_registry(
        discovery,
        transport_factory,
        target_args,
        layer,
        &registry,
    )
}

fn execute_screen_activate_with_registry<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
    layer: &str,
    registry: &ScreenFieldRegistry,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let layer = registry.layer(layer)?.name().clone();
    let lua = compile_activate_lua(&layer)?;

    execute_curated_screen_lua(
        discovery,
        transport_factory,
        target_args,
        &lua,
        "screen activation command",
    )
}

fn execute_curated_screen_lua<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
    lua: &str,
    action: &str,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let devices = discover_supported_devices(discovery)?;
    let device = select_single_device(&devices)?;
    let mut transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    send_screen_raw(&mut transport, target.grid_target(), lua)?;

    Ok(format!(
        "Selected USB device: {device}\nTransport: opened successfully at {} baud\nModule target: {target}\nSent {action} over the immediate path.\n",
        protocol::GRID_BAUD_RATE,
    ))
}

fn execute_runtime_verify<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let devices = discover_supported_devices(discovery)?;
    let device = select_single_device(&devices)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = verify_installed_runtime(target, &mut reader)?;

    Ok(render_runtime_output(
        &device.to_string(),
        target,
        &report,
        true,
    ))
}

fn execute_runtime_install<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    runtime_name: &str,
    target_args: &TargetArgs,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let runtime = resolve_runtime(runtime_name)?;
    let target = resolve_target(target_args)?;
    let devices = discover_supported_devices(discovery)?;
    let device = select_single_device(&devices)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = install_runtime_with_bundle_dir(&runtime.path, target, &mut reader)?;

    Ok(render_runtime_install_output(
        &device.to_string(),
        target,
        Some(&runtime),
        &report,
    ))
}

fn execute_runtime_status<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let devices = discover_supported_devices(discovery)?;
    let device = select_single_device(&devices)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = inspect_installed_runtime(target, &mut reader)?;

    Ok(match report {
        Some(report) => render_runtime_output(&device.to_string(), target, &report, false),
        None => render_runtime_status_without_local_copy_output(&device.to_string(), target),
    })
}

fn execute_runtime_upgrade<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    runtime_name: &str,
    target_args: &TargetArgs,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let runtime = resolve_runtime(runtime_name)?;
    let target = resolve_target(target_args)?;
    let devices = discover_supported_devices(discovery)?;
    let device = select_single_device(&devices)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = upgrade_runtime_with_bundle_dir(&runtime.path, target, &mut reader)?;

    Ok(render_runtime_upgrade_output(
        &device.to_string(),
        target,
        &runtime,
        &report,
    ))
}

fn execute_runtime_repair<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let devices = discover_supported_devices(discovery)?;
    let device = select_single_device(&devices)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = repair_installed_runtime(target, &mut reader)?;

    Ok(render_runtime_repair_output(
        &device.to_string(),
        target,
        &report,
    ))
}

fn execute_runtime_remove<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let devices = discover_supported_devices(discovery)?;
    let device = select_single_device(&devices)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = remove_installed_runtime(target, &mut reader)?;

    Ok(render_runtime_remove_output(
        &device.to_string(),
        target,
        &report,
    ))
}

fn render_runtime_output(
    device: &str,
    requested_target: ResolvedTarget,
    report: &RuntimeInspectionReport,
    verified: bool,
) -> String {
    let mut output = format!(
        "Selected USB device: {device}\nTransport: opened successfully at {} baud\nModule target: {requested_target}\nInstalled runtime version: {}\nStatus: {}\n",
        protocol::GRID_BAUD_RATE,
        report.bundle_version(),
        report.status_label(),
    );

    match report.observed_targets() {
        [] => {}
        [target] => {
            output.push_str(&format!(
                "Observed runtime target: dx={} dy={}\n",
                target.dx, target.dy
            ));
        }
        targets => {
            let summary = targets
                .iter()
                .map(|target| format!("dx={} dy={}", target.dx, target.dy))
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str(&format!("Observed runtime targets: {summary}\n"));
        }
    }

    for inspection in report.slot_inspections() {
        let detail = match &inspection.status {
            RuntimeSlotStatus::Match { source_target } => {
                format!("match on dx={} dy={}", source_target.dx, source_target.dy)
            }
            RuntimeSlotStatus::Missing => "missing or blank".to_string(),
            RuntimeSlotStatus::Drifted { source_target } => {
                format!(
                    "content mismatch on dx={} dy={}",
                    source_target.dx, source_target.dy
                )
            }
            RuntimeSlotStatus::WrongTarget { actual_target } => format!(
                "responded from dx={} dy={} instead of the requested target",
                actual_target.dx, actual_target.dy
            ),
        };

        output.push_str(&format!(
            "- {} ({}): {}\n",
            inspection.slot.name,
            inspection.slot.location_display(),
            detail
        ));
    }

    if verified {
        output.push_str("Verification: exact installed runtime match confirmed.\n");
    }

    output
}

fn render_runtime_status_without_local_copy_output(
    device: &str,
    requested_target: ResolvedTarget,
) -> String {
    format!(
        "Selected USB device: {device}\nTransport: opened successfully at {} baud\nModule target: {requested_target}\nInstalled runtime: none\nStatus: no frozen installed runtime was found under ~/.config/vsn1-cli/runtime\n",
        protocol::GRID_BAUD_RATE,
    )
}

fn render_runtime_install_output(
    device: &str,
    requested_target: ResolvedTarget,
    runtime: Option<&DiscoveredRuntime>,
    report: &RuntimeInstallReport,
) -> String {
    let mut output =
        render_runtime_output(device, requested_target, report.verification_report(), true);

    if let Some(runtime) = runtime {
        output.push_str(&format!(
            "Resolved runtime: {} ({})\n",
            runtime.name,
            runtime.source.as_str()
        ));
    }

    output.push_str("Installed owned slots in manifest order:\n");
    for slot in report.installed_slots() {
        output.push_str(&format!("- {} ({})\n", slot.name, slot.location_display()));
    }

    output
}

fn render_runtime_upgrade_output(
    device: &str,
    requested_target: ResolvedTarget,
    runtime: &DiscoveredRuntime,
    report: &RuntimeUpgradeReport,
) -> String {
    let mut output = render_runtime_install_output(
        device,
        requested_target,
        Some(runtime),
        report.install_report(),
    );
    output.push_str("Upgrade: refreshed the device and frozen runtime copy without replacing the pre-install backup.\n");
    output
}

fn render_runtime_repair_output(
    device: &str,
    requested_target: ResolvedTarget,
    report: &RuntimeInstallReport,
) -> String {
    let mut output = render_runtime_install_output(device, requested_target, None, report);
    output.push_str("Repair: reapplied the frozen installed runtime to the owned slots.\n");
    output
}

fn render_runtime_remove_output(
    device: &str,
    requested_target: ResolvedTarget,
    report: &RuntimeRemoveReport,
) -> String {
    let mut output = format!(
        "Selected USB device: {device}\nTransport: opened successfully at {} baud\nModule target: {requested_target}\nRuntime removal: {}\n",
        protocol::GRID_BAUD_RATE,
        if report.restored_from_backup() {
            "restored pre-install owned slots and removed the frozen runtime copy"
        } else {
            "cleared owned slots from the frozen runtime copy and removed the frozen runtime copy"
        },
    );

    if let Some(warning) = report.warning() {
        output.push_str(&format!("Warning: {warning}\n"));
    }

    output.push_str("Affected owned slots in manifest order:\n");
    for slot in report.removed_slots() {
        output.push_str(&format!("- {} ({})\n", slot.name, slot.location_display()));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::device::{DeviceError, DiscoveredDevice};
    use crate::transport::{
        FakeTransportFactory, OpenCall, SerialTransport, SerialTransportFactory, TransportError,
    };

    #[derive(Debug, Default)]
    struct RecordingTransport {
        immediate_writes: Rc<RefCell<Vec<Vec<u8>>>>,
    }

    impl SerialTransport for RecordingTransport {
        fn write_immediate(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError> {
            self.immediate_writes.borrow_mut().push(packet.to_vec());
            Ok(())
        }

        fn write_config(&mut self, _packet: &[u8]) -> std::result::Result<(), TransportError> {
            panic!("screen raw should not use config writes")
        }

        fn bytes_to_read(&self) -> std::result::Result<u32, TransportError> {
            Ok(0)
        }

        fn read(&mut self, _buffer: &mut [u8]) -> std::result::Result<usize, TransportError> {
            Ok(0)
        }

        fn clear_input(&mut self) -> std::result::Result<(), TransportError> {
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct RecordingTransportFactory {
        open_calls: Vec<OpenCall>,
        immediate_writes: Rc<RefCell<Vec<Vec<u8>>>>,
    }

    impl RecordingTransportFactory {
        fn immediate_writes(&self) -> Vec<Vec<u8>> {
            self.immediate_writes.borrow().clone()
        }
    }

    impl SerialTransportFactory for RecordingTransportFactory {
        type Transport = RecordingTransport;

        fn open(
            &mut self,
            port_name: &str,
            baud_rate: u32,
        ) -> std::result::Result<Self::Transport, TransportError> {
            self.open_calls.push(OpenCall {
                port_name: port_name.to_string(),
                baud_rate,
            });

            Ok(RecordingTransport {
                immediate_writes: Rc::clone(&self.immediate_writes),
            })
        }
    }

    struct StaticDiscovery {
        devices: Vec<DiscoveredDevice>,
        error: Option<DeviceError>,
    }

    impl DeviceDiscovery for StaticDiscovery {
        fn discover(&self) -> std::result::Result<Vec<DiscoveredDevice>, DeviceError> {
            match &self.error {
                Some(error) => Err(error.clone()),
                None => Ok(self.devices.clone()),
            }
        }
    }

    #[derive(Debug)]
    struct SingleOpenTransport {
        active_handles: Rc<RefCell<usize>>,
        queued_reads: Vec<Vec<u8>>,
        read_cursor: usize,
        fetch_responses: Vec<Vec<u8>>,
        fetch_index: usize,
        immediate_writes: Rc<RefCell<Vec<Vec<u8>>>>,
    }

    impl Drop for SingleOpenTransport {
        fn drop(&mut self) {
            let mut active_handles = self.active_handles.borrow_mut();
            *active_handles -= 1;
        }
    }

    impl SerialTransport for SingleOpenTransport {
        fn write_immediate(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError> {
            self.immediate_writes.borrow_mut().push(packet.to_vec());
            Ok(())
        }

        fn write_config(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError> {
            if packet.get(24..28) == Some(b"060f") {
                let response = self
                    .fetch_responses
                    .get(self.fetch_index)
                    .cloned()
                    .ok_or_else(|| TransportError::config("missing queued fetch response"))?;
                self.fetch_index += 1;
                self.queued_reads.push(response);
            }

            Ok(())
        }

        fn bytes_to_read(&self) -> std::result::Result<u32, TransportError> {
            let pending = self
                .queued_reads
                .iter()
                .skip(self.read_cursor)
                .map(Vec::len)
                .sum::<usize>();
            Ok(pending as u32)
        }

        fn read(&mut self, buffer: &mut [u8]) -> std::result::Result<usize, TransportError> {
            let Some(next) = self.queued_reads.get(self.read_cursor) else {
                return Ok(0);
            };

            let read_len = next.len().min(buffer.len());
            buffer[..read_len].copy_from_slice(&next[..read_len]);
            self.read_cursor += 1;
            Ok(read_len)
        }

        fn clear_input(&mut self) -> std::result::Result<(), TransportError> {
            self.queued_reads.clear();
            self.read_cursor = 0;
            Ok(())
        }
    }

    #[derive(Debug)]
    struct SingleOpenTransportFactory {
        active_handles: Rc<RefCell<usize>>,
        fetch_responses: Vec<Vec<u8>>,
        immediate_writes: Rc<RefCell<Vec<Vec<u8>>>>,
        open_calls: Vec<OpenCall>,
    }

    impl SingleOpenTransportFactory {
        fn new(fetch_responses: Vec<Vec<u8>>) -> Self {
            Self {
                active_handles: Rc::new(RefCell::new(0)),
                fetch_responses,
                immediate_writes: Rc::new(RefCell::new(Vec::new())),
                open_calls: Vec::new(),
            }
        }

        fn immediate_writes(&self) -> Vec<Vec<u8>> {
            self.immediate_writes.borrow().clone()
        }
    }

    impl SerialTransportFactory for SingleOpenTransportFactory {
        type Transport = SingleOpenTransport;

        fn open(
            &mut self,
            port_name: &str,
            baud_rate: u32,
        ) -> std::result::Result<Self::Transport, TransportError> {
            if *self.active_handles.borrow() != 0 {
                return Err(TransportError::open("Device or resource busy"));
            }

            *self.active_handles.borrow_mut() += 1;
            self.open_calls.push(OpenCall {
                port_name: port_name.to_string(),
                baud_rate,
            });

            Ok(SingleOpenTransport {
                active_handles: Rc::clone(&self.active_handles),
                queued_reads: Vec::new(),
                read_cursor: 0,
                fetch_responses: self.fetch_responses.clone(),
                fetch_index: 0,
                immediate_writes: Rc::clone(&self.immediate_writes),
            })
        }
    }

    #[test]
    fn command_surface_includes_top_level_groups() {
        let names = command()
            .get_subcommands()
            .map(|subcommand| subcommand.get_name().to_string())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["device", "runtime", "screen"]);
    }

    #[test]
    fn top_level_help_includes_command_descriptions() {
        let mut cli_command = command();
        let help = cli_command.render_help().to_string();

        assert!(help.contains("Discover attached VSN1/Grid USB serial devices"));
        assert!(
            help.contains("Install, verify, inspect, repair, upgrade, and remove named runtimes")
        );
        assert!(help.contains("Send curated or raw screen updates"));
    }

    #[test]
    fn runtime_list_renders_discovered_names_and_sources() {
        let output = render_runtime_list(&[
            DiscoveredRuntime {
                name: "default".to_string(),
                source: crate::runtime_bundle::RuntimeSource::Dev,
                path: "/repo/assets/runtimes/default".into(),
            },
            DiscoveredRuntime {
                name: "legacy".to_string(),
                source: crate::runtime_bundle::RuntimeSource::System,
                path: "/usr/share/vsn1-cli/runtimes/legacy".into(),
            },
        ]);

        assert!(output.contains("Discovered runtimes:"));
        assert!(output.contains("- default (dev) /repo/assets/runtimes/default"));
        assert!(output.contains("- legacy (system) /usr/share/vsn1-cli/runtimes/legacy"));
    }

    #[test]
    fn screen_set_help_mentions_curated_assignments_and_activation() {
        let mut screen_command = command().find_subcommand("screen").unwrap().clone();
        let screen_help = screen_command.render_help().to_string();
        assert!(screen_help.contains("Send curated or raw screen updates"));

        let mut set_command = command()
            .find_subcommand("screen")
            .unwrap()
            .find_subcommand("set")
            .unwrap()
            .clone();
        let set_help = set_command.render_help().to_string();

        assert!(set_help.contains("One or more curated screen field assignments"));
        assert!(set_help.contains("Activate a manifest-defined layer after updating it"));
        assert!(set_help.contains("frozen installed runtime copy under ~/.config/vsn1-cli/runtime"));
    }

    #[test]
    fn parses_device_list() {
        let cli = try_parse_from(["vsn1-cli", "device", "list"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Device(DeviceArgs {
                    command: DeviceCommand::List,
                }),
            }
        );
    }

    #[test]
    fn parses_device_info_with_explicit_target() {
        let cli = try_parse_from(["vsn1-cli", "device", "info", "--dx", "1", "--dy", "2"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Device(DeviceArgs {
                    command: DeviceCommand::Info {
                        target: TargetArgs {
                            dx: Some(1),
                            dy: Some(2),
                        },
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_runtime_verify() {
        let cli = try_parse_from(["vsn1-cli", "runtime", "verify"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Runtime(RuntimeArgs {
                    command: RuntimeCommand::Verify {
                        target: TargetArgs::default(),
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_runtime_list() {
        let cli = try_parse_from(["vsn1-cli", "runtime", "list"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Runtime(RuntimeArgs {
                    command: RuntimeCommand::List,
                }),
            }
        );
    }

    #[test]
    fn parses_runtime_install_with_name() {
        let cli = try_parse_from(["vsn1-cli", "runtime", "install", "default"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Runtime(RuntimeArgs {
                    command: RuntimeCommand::Install {
                        name: "default".to_string(),
                        target: TargetArgs::default(),
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_runtime_upgrade_with_name_and_target() {
        let cli = try_parse_from([
            "vsn1-cli", "runtime", "upgrade", "default", "--dx", "0", "--dy", "0",
        ])
        .unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Runtime(RuntimeArgs {
                    command: RuntimeCommand::Upgrade {
                        name: "default".to_string(),
                        target: TargetArgs {
                            dx: Some(0),
                            dy: Some(0),
                        },
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_screen_set_with_activation_and_target() {
        let cli = try_parse_from([
            "vsn1-cli",
            "screen",
            "set",
            "persistent.title=Hello",
            "slow.message=World",
            "--activate",
            "slow",
            "--dx",
            "1",
            "--dy",
            "2",
        ])
        .unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Set {
                        assignments: vec![
                            "persistent.title=Hello".to_string(),
                            "slow.message=World".to_string(),
                        ],
                        activate: Some("slow".to_string()),
                        target: TargetArgs {
                            dx: Some(1),
                            dy: Some(2),
                        },
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_screen_clear_layer() {
        let cli = try_parse_from(["vsn1-cli", "screen", "clear", "fast"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Clear {
                        layer: "fast".to_string(),
                        target: TargetArgs::default(),
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_screen_activate_layer() {
        let cli = try_parse_from(["vsn1-cli", "screen", "activate", "persistent"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Activate {
                        layer: "persistent".to_string(),
                        target: TargetArgs::default(),
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_screen_raw() {
        let cli = try_parse_from([
            "vsn1-cli",
            "screen",
            "raw",
            "return update_param('persistent.title', 'Hello')",
        ])
        .unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Raw {
                        lua: "return update_param('persistent.title', 'Hello')".to_string(),
                        target: TargetArgs::default(),
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_screen_set_assignment_with_embedded_equals() {
        let cli =
            try_parse_from(["vsn1-cli", "screen", "set", "persistent.title=left=right"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Set {
                        assignments: vec!["persistent.title=left=right".to_string()],
                        activate: None,
                        target: TargetArgs::default(),
                    },
                }),
            }
        );
    }

    #[test]
    fn device_info_defaults_to_broadcast_and_opens_the_selected_transport() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut transport_factory = FakeTransportFactory::default();

        let output = execute_cli(
            Cli {
                command: TopLevelCommand::Device(DeviceArgs {
                    command: DeviceCommand::Info {
                        target: TargetArgs::default(),
                    },
                }),
            },
            &discovery,
            &mut transport_factory,
        )
        .unwrap();

        assert!(output.contains("Module target: broadcast"));
        assert_eq!(
            transport_factory.open_calls(),
            &[OpenCall {
                port_name: "/dev/ttyACM0".to_string(),
                baud_rate: protocol::GRID_BAUD_RATE,
            }]
        );
    }

    #[test]
    fn device_info_fails_when_multiple_supported_devices_are_visible() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0"), test_device("/dev/ttyACM1")],
            error: None,
        };
        let mut transport_factory = FakeTransportFactory::default();

        let error = execute_cli(
            Cli {
                command: TopLevelCommand::Device(DeviceArgs {
                    command: DeviceCommand::Info {
                        target: TargetArgs::default(),
                    },
                }),
            },
            &discovery,
            &mut transport_factory,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "multiple supported VSN1/Grid USB serial devices found (/dev/ttyACM0, /dev/ttyACM1); `device info` currently requires exactly one visible device"
        );
    }

    #[test]
    fn parses_runtime_remove_with_explicit_target() {
        let cli =
            try_parse_from(["vsn1-cli", "runtime", "remove", "--dx", "0", "--dy", "0"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Runtime(RuntimeArgs {
                    command: RuntimeCommand::Remove {
                        target: TargetArgs {
                            dx: Some(0),
                            dy: Some(0),
                        },
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_runtime_uninstall_alias_with_explicit_target() {
        let cli =
            try_parse_from(["vsn1-cli", "runtime", "uninstall", "--dx", "0", "--dy", "0"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                command: TopLevelCommand::Runtime(RuntimeArgs {
                    command: RuntimeCommand::Remove {
                        target: TargetArgs {
                            dx: Some(0),
                            dy: Some(0),
                        },
                    },
                }),
            }
        );
    }

    #[test]
    fn screen_raw_uses_targeting_and_sends_one_immediate_packet() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut transport_factory = RecordingTransportFactory::default();

        let output = execute_cli(
            Cli {
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Raw {
                        lua: "return 1".to_string(),
                        target: TargetArgs {
                            dx: Some(1),
                            dy: Some(2),
                        },
                    },
                }),
            },
            &discovery,
            &mut transport_factory,
        )
        .unwrap();

        assert!(output.contains("Module target: dx=1 dy=2"));
        assert_eq!(
            transport_factory.open_calls,
            vec![OpenCall {
                port_name: "/dev/ttyACM0".to_string(),
                baud_rate: protocol::GRID_BAUD_RATE,
            }]
        );

        let writes = transport_factory.immediate_writes();
        let packet = &writes[0];
        assert_eq!(&packet[14..18], b"8081");
        assert_eq!(
            &packet[32..packet.len() - 5],
            b"<?lua --[[@cb]] return 1 ?>"
        );
    }

    #[test]
    fn screen_raw_surfaces_targeting_errors() {
        let error = execute_cli(
            Cli {
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Raw {
                        lua: "return 1".to_string(),
                        target: TargetArgs {
                            dx: Some(1),
                            dy: None,
                        },
                    },
                }),
            },
            &StaticDiscovery {
                devices: vec![test_device("/dev/ttyACM0")],
                error: None,
            },
            &mut FakeTransportFactory::default(),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "both --dx and --dy must be provided together; omit both flags to use broadcast targeting"
        );
    }

    #[test]
    fn screen_raw_surfaces_protocol_errors() {
        let error = execute_cli(
            Cli {
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Raw {
                        lua: "snowman = '☃'".to_string(),
                        target: TargetArgs::default(),
                    },
                }),
            },
            &StaticDiscovery {
                devices: vec![test_device("/dev/ttyACM0")],
                error: None,
            },
            &mut FakeTransportFactory::default(),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "the current Grid packet encoder supports ASCII Lua only"
        );
    }

    #[test]
    fn screen_set_sends_without_runtime_verification() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut transport_factory = FakeTransportFactory::default();
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let output = execute_screen_set_with_registry(
            &discovery,
            &mut transport_factory,
            &TargetArgs::default(),
            &["persistent.title=Hello".to_string()],
            None,
            &registry,
        )
        .unwrap();

        assert!(output.contains("Sent curated screen update over the immediate path."));
        assert_eq!(transport_factory.open_calls().len(), 1);
    }

    #[test]
    fn screen_set_surfaces_mixed_layer_activation_validation() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let error = execute_screen_set_with_registry(
            &StaticDiscovery {
                devices: vec![test_device("/dev/ttyACM0")],
                error: None,
            },
            &mut FakeTransportFactory::default(),
            &TargetArgs::default(),
            &[
                "persistent.title=Hello".to_string(),
                "slow.message=World".to_string(),
            ],
            Some("slow".to_string()),
            &registry,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "screen set --activate slow only supports slow-layer assignments, but `persistent.title` belongs to the persistent layer"
        );
    }

    #[test]
    fn screen_activate_validates_layer_names_against_the_registry() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let error = execute_screen_activate_with_registry(
            &StaticDiscovery {
                devices: vec![test_device("/dev/ttyACM0")],
                error: None,
            },
            &mut FakeTransportFactory::default(),
            &TargetArgs::default(),
            "notice",
            &registry,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "unknown screen layer `notice` (supported layers: persistent, slow, fast); run `vsn1-cli screen --help` for examples"
        );
    }

    #[test]
    fn screen_activate_persistent_sends_the_generic_activation_helper() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut transport_factory = FakeTransportFactory::default();
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let output = execute_screen_activate_with_registry(
            &discovery,
            &mut transport_factory,
            &TargetArgs::default(),
            "persistent",
            &registry,
        )
        .unwrap();

        assert!(output.contains("Sent screen activation command over the immediate path."));
        assert_eq!(transport_factory.open_calls().len(), 1);
    }

    #[test]
    fn screen_set_opens_transport_once_and_sends_immediate_packet() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut transport_factory = SingleOpenTransportFactory::new(Vec::new());
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let output = execute_screen_set_with_registry(
            &discovery,
            &mut transport_factory,
            &TargetArgs {
                dx: Some(0),
                dy: Some(0),
            },
            &["persistent.title=Hello".to_string()],
            None,
            &registry,
        )
        .unwrap();

        assert!(output.contains("Sent curated screen update over the immediate path."));
        assert_eq!(transport_factory.open_calls.len(), 1);
        assert_eq!(transport_factory.immediate_writes().len(), 1);
    }

    fn test_device(port_name: &str) -> DiscoveredDevice {
        DiscoveredDevice {
            port_name: port_name.to_string(),
            vendor_id: 0x03eb,
            product_id: 0xecac,
            serial_number: Some("ABC123".to_string()),
            manufacturer: Some("Intech".to_string()),
            product: Some("VSN1".to_string()),
            known_label: "Grid / VSN1",
        }
    }
}
