pub mod command_model;
pub mod daemon_client;
pub mod daemon_command_handler;
pub mod daemon_protocol;
pub mod daemon_server;
pub mod daemon_session;
pub mod daemon_socket;
mod debug;
pub mod device;
mod error;
mod module_files;
pub mod protocol;
pub mod raw;
pub mod runtime;
pub mod runtime_bundle;
pub mod screen;
pub mod targeting;
pub mod transport;

use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand};
use serde::{Deserialize, Serialize};

use crate::command_model::{CommandRequest, DeviceRequest, RuntimeRequest, ScreenRequest};
use crate::daemon_client::{DaemonCommandClient, SystemDaemonClient};
use crate::device::{
    discover_supported_devices, select_device, DeviceDiscovery, DiscoveredDevice,
    SystemDeviceDiscovery,
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
    "Examples:\n  vsn1-cli device info\n  vsn1-cli device info --device /dev/cu.usbmodem101\n  vsn1-cli device info --dx 0 --dy 0";
const DEVICE_PAGE_STORE_AFTER_HELP: &str =
    "Examples:\n  vsn1-cli device page-store\n  vsn1-cli device page-store --dx 0 --dy 0\n\nThis sends a raw PAGESTORE command over the config path to validate module behavior after file-backed runtime writes.";
const DEVICE_PAGE_DISCARD_AFTER_HELP: &str =
    "Examples:\n  vsn1-cli device page-discard\n  vsn1-cli device page-discard --dx 0 --dy 0\n\nThis sends a raw PAGEDISCARD command over the config path to validate whether the firmware can reload stored page configuration without a power cycle.";
const RUNTIME_LIST_AFTER_HELP: &str = "Lists discovered runtime names and the source copy that won resolution. Discovery precedence is dev > user > system on directory-name collisions.";
const RUNTIME_INSTALL_AFTER_HELP: &str = "Installs the selected discovered runtime into the manifest-owned slots, captures a pre-install backup under ~/.config/vsn1-cli/pre-install, freezes the runtime under ~/.config/vsn1-cli/runtime, and verifies an exact installed-runtime match.";
const RUNTIME_VERIFY_AFTER_HELP: &str = "Fails unless every owned runtime slot matches the frozen installed runtime copy under ~/.config/vsn1-cli/runtime exactly.";
const RUNTIME_UPGRADE_AFTER_HELP: &str = "Overwrites the device from the selected discovered runtime, refreshes the frozen runtime copy under ~/.config/vsn1-cli/runtime, and does not refresh the pre-install backup.";
const RUNTIME_REPAIR_AFTER_HELP: &str =
    "Reapplies the frozen installed runtime copy when the owned slots are drifted or incomplete.";
const RUNTIME_REMOVE_AFTER_HELP: &str =
    "Restores the pre-install backup when available, otherwise clears the frozen runtime's owned slots with a warning, then removes ~/.config/vsn1-cli/runtime.";
const RUNTIME_STATUS_AFTER_HELP: &str = "Shows the owned-slot inspection result relative to the frozen installed runtime copy when one is present locally.";
const SCREEN_SET_AFTER_HELP: &str = "Examples:\n  vsn1-cli screen set persistent.title=Tempo persistent.value=64\n  vsn1-cli screen set persistent.title=Tempo slow.message='Disk almost full' --activate slow\n  vsn1-cli screen set persistent.title=Tempo fast.action=Tap --activate fast --dx 0 --dy 0\n\nExamples use the shipped `default` runtime. Curated screen fields and layer names are loaded from the frozen installed runtime copy under ~/.config/vsn1-cli/runtime, so other runtimes may declare different names.";
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
    #[arg(long, global = true, help = "Enable debug logging to stderr")]
    pub debug: bool,

    #[command(subcommand)]
    pub command: TopLevelCommand,
}

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "vsn1-daemon",
    version,
    about = "Host-local daemon for VSN1 CLI command forwarding"
)]
pub struct DaemonCli {
    #[arg(long, global = true, help = "Enable debug logging to stderr")]
    pub debug: bool,
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
    #[command(
        about = "Send a raw PAGESTORE command to the resolved module target",
        after_help = DEVICE_PAGE_STORE_AFTER_HELP
    )]
    PageStore {
        #[command(flatten)]
        target: TargetArgs,
    },
    #[command(
        about = "Send a raw PAGEDISCARD command to the resolved module target",
        after_help = DEVICE_PAGE_DISCARD_AFTER_HELP
    )]
    PageDiscard {
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
            help = "Raw Lua snippet to normalize and send over the immediate path"
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

#[derive(Debug, Args, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Explicit USB serial device path; omit to auto-select when exactly one supported device is visible",
        help_heading = "Device"
    )]
    pub device: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSuccess {
    DeviceList {
        devices: Vec<DiscoveredDevice>,
    },
    DeviceInfo {
        device: String,
        target: ResolvedTarget,
    },
    DeviceAction {
        device: String,
        target: ResolvedTarget,
        action: &'static str,
    },
    RuntimeList {
        runtimes: Vec<DiscoveredRuntime>,
    },
    ScreenAction {
        device: String,
        target: ResolvedTarget,
        action: &'static str,
    },
    RuntimeStatus {
        device: String,
        target: ResolvedTarget,
        report: Option<RuntimeInspectionReport>,
        verified: bool,
    },
    RuntimeInstall {
        device: String,
        target: ResolvedTarget,
        runtime: Option<DiscoveredRuntime>,
        report: RuntimeInstallReport,
    },
    RuntimeUpgrade {
        device: String,
        target: ResolvedTarget,
        runtime: DiscoveredRuntime,
        report: RuntimeUpgradeReport,
    },
    RuntimeRepair {
        device: String,
        target: ResolvedTarget,
        report: RuntimeInstallReport,
    },
    RuntimeRemove {
        device: String,
        target: ResolvedTarget,
        report: RuntimeRemoveReport,
    },
}

pub trait CommandExecutor {
    fn execute(&mut self, request: CommandRequest) -> Result<CommandSuccess>;
}

pub struct OneShotCommandExecutor<'a, D, F> {
    discovery: &'a D,
    transport_factory: &'a mut F,
}

impl<'a, D, F> OneShotCommandExecutor<'a, D, F> {
    pub fn new(discovery: &'a D, transport_factory: &'a mut F) -> Self {
        Self {
            discovery,
            transport_factory,
        }
    }
}

impl<D, F> CommandExecutor for OneShotCommandExecutor<'_, D, F>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    fn execute(&mut self, request: CommandRequest) -> Result<CommandSuccess> {
        execute_command_request_with_one_shot_transport(
            request,
            self.discovery,
            self.transport_factory,
        )
    }
}

pub fn command() -> clap::Command {
    Cli::command()
}

pub fn daemon_command() -> clap::Command {
    DaemonCli::command()
}

pub fn try_parse_from<I, T>(args: I) -> std::result::Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    Cli::try_parse_from(args)
}

pub fn try_parse_command_request_from<I, T>(
    args: I,
) -> std::result::Result<CommandRequest, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    try_parse_from(args).map(CommandRequest::from)
}

pub fn try_parse_daemon_from<I, T>(args: I) -> std::result::Result<DaemonCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    DaemonCli::try_parse_from(args)
}

pub fn run(cli: Cli) -> Result<()> {
    debug::set_debug_enabled(cli.debug);
    let discovery = SystemDeviceDiscovery;
    let mut transport_factory = SystemTransportFactory;
    let mut executor = OneShotCommandExecutor::new(&discovery, &mut transport_factory);
    let mut daemon_client = SystemDaemonClient::new();
    let output = execute_and_render_command_with_optional_daemon(
        &mut executor,
        &mut daemon_client,
        CommandRequest::from(cli),
    )?;
    let _ = io::stderr().flush();
    print!("{output}");
    let _ = io::stdout().flush();
    Ok(())
}

pub fn main() -> ExitCode {
    match try_parse_from(std::env::args_os()) {
        Ok(cli) => match run(cli) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{}", render_command_error(&error));
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

pub fn daemon_main() -> ExitCode {
    let daemon_cli = match try_parse_daemon_from(std::env::args_os()) {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = error.exit_code();
            let _ = error.print();

            return u8::try_from(exit_code)
                .map(ExitCode::from)
                .unwrap_or(ExitCode::FAILURE);
        }
    };

    debug::set_debug_enabled(daemon_cli.debug);
    let socket_path = match daemon_socket::resolve_daemon_socket_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    debug::log(
        "daemon",
        format!("starting daemon on {}", socket_path.display()),
    );

    let server = match daemon_server::DaemonServer::bind_with_handler(
        &socket_path,
        daemon_command_handler::ScreenDaemonRequestHandler::new(
            SystemDeviceDiscovery,
            SystemTransportFactory,
        ),
    ) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = server.serve_forever() {
        eprintln!("error: {error}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

pub fn execute_and_render_command(
    executor: &mut impl CommandExecutor,
    request: CommandRequest,
) -> Result<String> {
    executor
        .execute(request)
        .map(|success| render_command_success(&success))
}

pub fn execute_and_render_command_with_optional_daemon(
    executor: &mut impl CommandExecutor,
    daemon_client: &mut impl DaemonCommandClient,
    request: CommandRequest,
) -> Result<String> {
    if request.is_daemon_eligible() {
        debug::log("cli", format!("trying daemon for {}", request.debug_name()));
        if let Some(output) = daemon_client.try_execute(&request)? {
            debug::log("cli", format!("daemon handled {}", request.debug_name()));
            return Ok(output);
        }

        debug::log(
            "cli",
            format!(
                "daemon unavailable for {}; using cold path",
                request.debug_name()
            ),
        );
    } else {
        debug::log(
            "cli",
            format!("{} is local-only; bypassing daemon", request.debug_name()),
        );
    }

    debug::log(
        "cli",
        format!("executing cold path for {}", request.debug_name()),
    );
    execute_and_render_command(executor, request)
}

pub fn render_command_success(success: &CommandSuccess) -> String {
    match success {
        CommandSuccess::DeviceList { devices } => render_device_list_output(devices),
        CommandSuccess::DeviceInfo { device, target } => {
            render_transport_open_output(device, *target, None)
        }
        CommandSuccess::DeviceAction {
            device,
            target,
            action,
        } => render_transport_open_output(device, *target, Some(format!("Sent {action}.\n"))),
        CommandSuccess::RuntimeList { runtimes } => render_runtime_list(runtimes),
        CommandSuccess::ScreenAction {
            device,
            target,
            action,
        } => render_transport_open_output(
            device,
            *target,
            Some(format!("Sent {action} over the immediate path.\n")),
        ),
        CommandSuccess::RuntimeStatus {
            device,
            target,
            report,
            verified,
        } => match report {
            Some(report) => render_runtime_output(device, *target, report, *verified),
            None => render_runtime_status_without_local_copy_output(device, *target),
        },
        CommandSuccess::RuntimeInstall {
            device,
            target,
            runtime,
            report,
        } => render_runtime_install_output(device, *target, runtime.as_ref(), report),
        CommandSuccess::RuntimeUpgrade {
            device,
            target,
            runtime,
            report,
        } => render_runtime_upgrade_output(device, *target, runtime, report),
        CommandSuccess::RuntimeRepair {
            device,
            target,
            report,
        } => render_runtime_repair_output(device, *target, report),
        CommandSuccess::RuntimeRemove {
            device,
            target,
            report,
        } => render_runtime_remove_output(device, *target, report),
    }
}

pub fn render_command_error(error: &Error) -> String {
    format!("error: {error}")
}

#[cfg(test)]
fn execute_cli<D, F>(cli: Cli, discovery: &D, transport_factory: &mut F) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let mut executor = OneShotCommandExecutor::new(discovery, transport_factory);
    execute_and_render_command(&mut executor, CommandRequest::from(cli))
}

fn execute_command_request_with_one_shot_transport<D, F>(
    request: CommandRequest,
    discovery: &D,
    transport_factory: &mut F,
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    match request {
        CommandRequest::Device(command) => match command {
            DeviceRequest::List => execute_device_list(discovery),
            DeviceRequest::Info { target } => {
                execute_device_info(discovery, transport_factory, &target)
            }
            DeviceRequest::PageStore { target } => {
                execute_device_page_store(discovery, transport_factory, &target)
            }
            DeviceRequest::PageDiscard { target } => {
                execute_device_page_discard(discovery, transport_factory, &target)
            }
        },
        CommandRequest::Runtime(command) => match command {
            RuntimeRequest::List => execute_runtime_list(),
            RuntimeRequest::Install { name, target } => {
                execute_runtime_install(discovery, transport_factory, &name, &target)
            }
            RuntimeRequest::Verify { target } => {
                execute_runtime_verify(discovery, transport_factory, &target)
            }
            RuntimeRequest::Upgrade { name, target } => {
                execute_runtime_upgrade(discovery, transport_factory, &name, &target)
            }
            RuntimeRequest::Repair { target } => {
                execute_runtime_repair(discovery, transport_factory, &target)
            }
            RuntimeRequest::Remove { target } => {
                execute_runtime_remove(discovery, transport_factory, &target)
            }
            RuntimeRequest::Status { target } => {
                execute_runtime_status(discovery, transport_factory, &target)
            }
        },
        CommandRequest::Screen(command) => match command {
            ScreenRequest::Set {
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
            ScreenRequest::Clear { layer, target } => {
                execute_screen_clear(discovery, transport_factory, &target, &layer)
            }
            ScreenRequest::Raw { lua, target } => {
                execute_screen_raw(discovery, transport_factory, &target, &lua)
            }
            ScreenRequest::Activate { layer, target } => {
                execute_screen_activate(discovery, transport_factory, &target, &layer)
            }
        },
    }
}

fn execute_device_list(discovery: &impl DeviceDiscovery) -> Result<CommandSuccess> {
    let devices = discover_supported_devices(discovery)?;

    Ok(CommandSuccess::DeviceList { devices })
}

fn render_device_list_output(devices: &[DiscoveredDevice]) -> String {
    if devices.is_empty() {
        return "No supported VSN1/Grid USB serial devices found.\n".to_string();
    }

    let mut output = String::from("Discovered supported VSN1/Grid USB serial devices:\n");

    for device in devices {
        output.push_str("- ");
        output.push_str(&device.to_string());
        output.push('\n');
    }

    output
}

fn execute_runtime_list() -> Result<CommandSuccess> {
    let runtimes = discover_runtimes()?;

    Ok(CommandSuccess::RuntimeList { runtimes })
}

fn render_runtime_list(runtimes: &[DiscoveredRuntime]) -> String {
    if runtimes.is_empty() {
        return "No runtimes found in system, user, or dev runtime roots.\n".to_string();
    }

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

fn execute_device_info<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let device = resolve_usb_device(discovery, target_args)?;
    let _transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;

    Ok(CommandSuccess::DeviceInfo {
        device: device.to_string(),
        target,
    })
}

fn execute_device_page_store<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let device = resolve_usb_device(discovery, target_args)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    reader.send_page_store(target)?;

    Ok(CommandSuccess::DeviceAction {
        device: device.to_string(),
        target,
        action: "PAGESTORE command over the config path",
    })
}

fn execute_device_page_discard<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let device = resolve_usb_device(discovery, target_args)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    reader.send_page_discard(target)?;

    Ok(CommandSuccess::DeviceAction {
        device: device.to_string(),
        target,
        action: "PAGEDISCARD command over the config path",
    })
}

fn execute_screen_raw<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
    lua: &str,
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let device = resolve_usb_device(discovery, target_args)?;
    let mut transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;

    send_screen_raw(&mut transport, target.grid_target(), lua)?;

    Ok(CommandSuccess::ScreenAction {
        device: device.to_string(),
        target,
        action: "raw screen update",
    })
}

fn execute_screen_set<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
    assignments: &[String],
    activate: Option<String>,
) -> Result<CommandSuccess>
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
) -> Result<CommandSuccess>
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
) -> Result<CommandSuccess>
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
) -> Result<CommandSuccess>
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
) -> Result<CommandSuccess>
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
) -> Result<CommandSuccess>
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
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let device = resolve_usb_device(discovery, target_args)?;
    let mut transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    send_screen_raw(&mut transport, target.grid_target(), lua)?;

    let action = match action {
        "curated screen update" => "curated screen update",
        "screen clear command" => "screen clear command",
        "screen activation command" => "screen activation command",
        _ => unreachable!("screen action should use one of the known static labels"),
    };

    Ok(CommandSuccess::ScreenAction {
        device: device.to_string(),
        target,
        action,
    })
}

fn execute_runtime_verify<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let device = resolve_usb_device(discovery, target_args)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = verify_installed_runtime(target, &mut reader)?;

    Ok(CommandSuccess::RuntimeStatus {
        device: device.to_string(),
        target,
        report: Some(report),
        verified: true,
    })
}

fn execute_runtime_install<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    runtime_name: &str,
    target_args: &TargetArgs,
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let runtime = resolve_runtime(runtime_name)?;
    let target = resolve_target(target_args)?;
    let device = resolve_usb_device(discovery, target_args)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = install_runtime_with_bundle_dir(&runtime.path, target, &mut reader)?;

    Ok(CommandSuccess::RuntimeInstall {
        device: device.to_string(),
        target,
        runtime: Some(runtime),
        report,
    })
}

fn execute_runtime_status<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let device = resolve_usb_device(discovery, target_args)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = inspect_installed_runtime(target, &mut reader)?;

    Ok(CommandSuccess::RuntimeStatus {
        device: device.to_string(),
        target,
        report,
        verified: false,
    })
}

fn execute_runtime_upgrade<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    runtime_name: &str,
    target_args: &TargetArgs,
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let runtime = resolve_runtime(runtime_name)?;
    let target = resolve_target(target_args)?;
    let device = resolve_usb_device(discovery, target_args)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = upgrade_runtime_with_bundle_dir(&runtime.path, target, &mut reader)?;

    Ok(CommandSuccess::RuntimeUpgrade {
        device: device.to_string(),
        target,
        runtime,
        report,
    })
}

fn execute_runtime_repair<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let device = resolve_usb_device(discovery, target_args)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = repair_installed_runtime(target, &mut reader)?;

    Ok(CommandSuccess::RuntimeRepair {
        device: device.to_string(),
        target,
        report,
    })
}

fn execute_runtime_remove<D, F>(
    discovery: &D,
    transport_factory: &mut F,
    target_args: &TargetArgs,
) -> Result<CommandSuccess>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let target = resolve_target(target_args)?;
    let device = resolve_usb_device(discovery, target_args)?;
    let transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    let mut reader = TransportRuntimeSlotReader::new(transport)?;
    let report = remove_installed_runtime(target, &mut reader)?;

    Ok(CommandSuccess::RuntimeRemove {
        device: device.to_string(),
        target,
        report,
    })
}

fn render_transport_open_output(
    device: &str,
    target: ResolvedTarget,
    detail: Option<String>,
) -> String {
    let mut output = format!(
        "Selected USB device: {device}\nTransport: opened successfully at {} baud\nModule target: {target}\n",
        protocol::GRID_BAUD_RATE,
    );

    if let Some(detail) = detail {
        output.push_str(&detail);
    }

    output
}

fn render_runtime_output(
    device: &str,
    requested_target: ResolvedTarget,
    report: &RuntimeInspectionReport,
    verified: bool,
) -> String {
    let mut output = format!(
        "Selected USB device: {device}\nTransport: opened successfully at {} baud\nModule target: {requested_target}\nInstalled runtime: frozen copy present\nStatus: {}\n",
        protocol::GRID_BAUD_RATE,
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

fn resolve_usb_device(
    discovery: &impl DeviceDiscovery,
    target_args: &TargetArgs,
) -> Result<DiscoveredDevice> {
    let devices = discover_supported_devices(discovery)?;
    Ok(select_device(&devices, target_args.device.as_deref())?)
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
    use std::os::unix::net::UnixListener;
    use std::rc::Rc;
    use std::thread;

    use crate::daemon_client::{DaemonClientError, DaemonCommandClient, SystemDaemonClient};
    use crate::daemon_command_handler::ScreenDaemonRequestHandler;
    use crate::daemon_server::DaemonServer;
    use crate::device::{DeviceError, DiscoveredDevice};
    use crate::transport::{
        FakeTransportFactory, OpenCall, SerialTransport, SerialTransportFactory, TransportError,
    };
    use tempfile::tempdir;

    #[derive(Debug, Default)]
    struct RecordingTransport {
        immediate_writes: Rc<RefCell<Vec<Vec<u8>>>>,
    }

    impl SerialTransport for RecordingTransport {
        fn write_immediate(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError> {
            self.immediate_writes.borrow_mut().push(packet.to_vec());
            Ok(())
        }

        fn write_evaluate(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError> {
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

        fn write_evaluate(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError> {
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

    #[derive(Debug)]
    struct RecordingExecutor {
        calls: Rc<RefCell<Vec<CommandRequest>>>,
        response: Result<CommandSuccess>,
    }

    impl RecordingExecutor {
        fn new(response: Result<CommandSuccess>) -> Self {
            Self {
                calls: Rc::new(RefCell::new(Vec::new())),
                response,
            }
        }

        fn calls(&self) -> Vec<CommandRequest> {
            self.calls.borrow().clone()
        }
    }

    impl CommandExecutor for RecordingExecutor {
        fn execute(&mut self, request: CommandRequest) -> Result<CommandSuccess> {
            self.calls.borrow_mut().push(request);
            self.response.clone()
        }
    }

    #[derive(Debug)]
    struct RecordingDaemonClient {
        calls: Rc<RefCell<Vec<CommandRequest>>>,
        response: crate::daemon_client::Result<Option<String>>,
    }

    impl RecordingDaemonClient {
        fn new(response: crate::daemon_client::Result<Option<String>>) -> Self {
            Self {
                calls: Rc::new(RefCell::new(Vec::new())),
                response,
            }
        }

        fn calls(&self) -> Vec<CommandRequest> {
            self.calls.borrow().clone()
        }
    }

    impl DaemonCommandClient for RecordingDaemonClient {
        fn try_execute(
            &mut self,
            request: &CommandRequest,
        ) -> crate::daemon_client::Result<Option<String>> {
            self.calls.borrow_mut().push(request.clone());
            self.response.clone()
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
                debug: false,
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
                debug: false,
                command: TopLevelCommand::Device(DeviceArgs {
                    command: DeviceCommand::Info {
                        target: TargetArgs {
                            device: None,
                            dx: Some(1),
                            dy: Some(2),
                        },
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_device_info_with_explicit_device() {
        let cli = try_parse_from([
            "vsn1-cli",
            "device",
            "info",
            "--device",
            "/dev/cu.usbmodem101",
        ])
        .unwrap();

        assert_eq!(
            cli,
            Cli {
                debug: false,
                command: TopLevelCommand::Device(DeviceArgs {
                    command: DeviceCommand::Info {
                        target: TargetArgs {
                            device: Some("/dev/cu.usbmodem101".to_string()),
                            dx: None,
                            dy: None,
                        },
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_device_page_store_with_explicit_target() {
        let cli =
            try_parse_from(["vsn1-cli", "device", "page-store", "--dx", "0", "--dy", "0"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                debug: false,
                command: TopLevelCommand::Device(DeviceArgs {
                    command: DeviceCommand::PageStore {
                        target: TargetArgs {
                            device: None,
                            dx: Some(0),
                            dy: Some(0),
                        },
                    },
                }),
            }
        );
    }

    #[test]
    fn parses_device_page_discard_with_explicit_target() {
        let cli = try_parse_from([
            "vsn1-cli",
            "device",
            "page-discard",
            "--dx",
            "0",
            "--dy",
            "0",
        ])
        .unwrap();

        assert_eq!(
            cli,
            Cli {
                debug: false,
                command: TopLevelCommand::Device(DeviceArgs {
                    command: DeviceCommand::PageDiscard {
                        target: TargetArgs {
                            device: None,
                            dx: Some(0),
                            dy: Some(0),
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
                debug: false,
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
                debug: false,
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
                debug: false,
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
                debug: false,
                command: TopLevelCommand::Runtime(RuntimeArgs {
                    command: RuntimeCommand::Upgrade {
                        name: "default".to_string(),
                        target: TargetArgs {
                            device: None,
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
                debug: false,
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Set {
                        assignments: vec![
                            "persistent.title=Hello".to_string(),
                            "slow.message=World".to_string(),
                        ],
                        activate: Some("slow".to_string()),
                        target: TargetArgs {
                            device: None,
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
                debug: false,
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
                debug: false,
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
                debug: false,
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
                debug: false,
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
    fn parses_device_list_into_a_local_only_command_request() {
        let request = try_parse_command_request_from(["vsn1-cli", "device", "list"]).unwrap();

        assert_eq!(request, CommandRequest::Device(DeviceRequest::List));
        assert!(request.is_local_only());
    }

    #[test]
    fn parses_top_level_debug_flag() {
        let cli = try_parse_from(["vsn1-cli", "--debug", "device", "list"]).unwrap();

        assert!(cli.debug);
        assert_eq!(
            cli.command,
            TopLevelCommand::Device(DeviceArgs {
                command: DeviceCommand::List,
            })
        );
    }

    #[test]
    fn parses_daemon_debug_flag() {
        let cli = try_parse_daemon_from(["vsn1-daemon", "--debug"]).unwrap();

        assert!(cli.debug);
    }

    #[test]
    fn parses_runtime_verify_into_a_daemon_eligible_command_request() {
        let request = try_parse_command_request_from(["vsn1-cli", "runtime", "verify"]).unwrap();

        assert_eq!(
            request,
            CommandRequest::Runtime(RuntimeRequest::Verify {
                target: TargetArgs::default(),
            })
        );
        assert!(request.is_daemon_eligible());
    }

    #[test]
    fn parses_screen_set_into_a_daemon_eligible_command_request() {
        let request = try_parse_command_request_from([
            "vsn1-cli",
            "screen",
            "set",
            "persistent.title=Hello",
            "--activate",
            "persistent",
        ])
        .unwrap();

        assert_eq!(
            request,
            CommandRequest::Screen(ScreenRequest::Set {
                assignments: vec!["persistent.title=Hello".to_string()],
                activate: Some("persistent".to_string()),
                target: TargetArgs::default(),
            })
        );
        assert!(request.is_daemon_eligible());
    }

    #[test]
    fn render_command_error_reuses_the_shared_error_format() {
        let error = Error::from(TransportError::open("Device or resource busy"));

        assert_eq!(
            render_command_error(&error),
            "error: transport open failed: Device or resource busy"
        );
    }

    #[test]
    fn direct_success_rendering_matches_execute_and_render_for_device_info() {
        let request = CommandRequest::Device(DeviceRequest::Info {
            target: TargetArgs::default(),
        });
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut direct_transport_factory = FakeTransportFactory::default();
        let mut direct_executor =
            OneShotCommandExecutor::new(&discovery, &mut direct_transport_factory);
        let success = direct_executor.execute(request.clone()).unwrap();
        let direct_output = render_command_success(&success);

        let mut rendered_transport_factory = FakeTransportFactory::default();
        let mut rendered_executor =
            OneShotCommandExecutor::new(&discovery, &mut rendered_transport_factory);
        let rendered_output = execute_and_render_command(&mut rendered_executor, request).unwrap();

        assert_eq!(direct_output, rendered_output);
        assert!(direct_output.contains("Transport: opened successfully at 2000000 baud"));
    }

    #[test]
    fn local_only_requests_bypass_the_daemon_client() {
        let request = CommandRequest::Device(DeviceRequest::List);
        let mut executor = RecordingExecutor::new(Ok(CommandSuccess::DeviceList {
            devices: Vec::new(),
        }));
        let mut daemon_client = RecordingDaemonClient::new(Ok(Some("daemon output\n".to_string())));

        let output = execute_and_render_command_with_optional_daemon(
            &mut executor,
            &mut daemon_client,
            request.clone(),
        )
        .unwrap();

        assert_eq!(output, "No supported VSN1/Grid USB serial devices found.\n");
        assert_eq!(executor.calls(), vec![request]);
        assert!(daemon_client.calls().is_empty());
    }

    #[test]
    fn daemon_unavailable_falls_back_to_local_execution() {
        let request = CommandRequest::Screen(ScreenRequest::Raw {
            lua: "return 1".to_string(),
            target: TargetArgs::default(),
        });
        let mut executor = RecordingExecutor::new(Ok(CommandSuccess::ScreenAction {
            device: "/dev/ttyACM0".to_string(),
            target: ResolvedTarget::Broadcast,
            action: "raw screen update",
        }));
        let mut daemon_client = RecordingDaemonClient::new(Ok(None));

        let output = execute_and_render_command_with_optional_daemon(
            &mut executor,
            &mut daemon_client,
            request.clone(),
        )
        .unwrap();

        assert!(output.contains("Sent raw screen update over the immediate path."));
        assert_eq!(executor.calls(), vec![request.clone()]);
        assert_eq!(daemon_client.calls(), vec![request]);
    }

    #[test]
    fn daemon_errors_do_not_fall_back_to_local_execution() {
        let request = CommandRequest::Runtime(RuntimeRequest::Verify {
            target: TargetArgs::default(),
        });
        let mut executor = RecordingExecutor::new(Ok(CommandSuccess::RuntimeList {
            runtimes: Vec::new(),
        }));
        let mut daemon_client = RecordingDaemonClient::new(Err(DaemonClientError::Execution {
            message: "boom".to_string(),
        }));

        let error = execute_and_render_command_with_optional_daemon(
            &mut executor,
            &mut daemon_client,
            request.clone(),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "daemon execution failed: boom");
        assert!(executor.calls().is_empty());
        assert_eq!(daemon_client.calls(), vec![request]);
    }

    #[test]
    fn stale_socket_falls_back_as_daemon_unavailable() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        drop(listener);

        let request = CommandRequest::Runtime(RuntimeRequest::Verify {
            target: TargetArgs::default(),
        });
        let mut daemon_client = SystemDaemonClient::with_socket_path(&socket_path);

        let response = daemon_client.try_execute(&request).unwrap();

        assert_eq!(response, None);
    }

    #[test]
    fn missing_socket_falls_back_to_local_execution() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("missing.sock");
        let request = CommandRequest::Screen(ScreenRequest::Raw {
            lua: "return 1".to_string(),
            target: TargetArgs::default(),
        });
        let mut executor = RecordingExecutor::new(Ok(CommandSuccess::ScreenAction {
            device: "/dev/ttyACM0".to_string(),
            target: ResolvedTarget::Broadcast,
            action: "raw screen update",
        }));
        let mut daemon_client = SystemDaemonClient::with_socket_path(&socket_path);

        let output = execute_and_render_command_with_optional_daemon(
            &mut executor,
            &mut daemon_client,
            request.clone(),
        )
        .unwrap();

        assert!(output.contains("Sent raw screen update over the immediate path."));
        assert_eq!(executor.calls(), vec![request]);
    }

    #[test]
    fn live_daemon_execution_errors_do_not_fall_back_locally() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        let server = DaemonServer::bind(&socket_path).unwrap();
        let server_thread = thread::spawn(move || server.serve_one());

        let request = CommandRequest::Runtime(RuntimeRequest::Verify {
            target: TargetArgs::default(),
        });
        let mut executor = RecordingExecutor::new(Ok(CommandSuccess::RuntimeList {
            runtimes: Vec::new(),
        }));
        let mut daemon_client = SystemDaemonClient::with_socket_path(&socket_path);

        let error = execute_and_render_command_with_optional_daemon(
            &mut executor,
            &mut daemon_client,
            request,
        )
        .unwrap_err();

        server_thread.join().unwrap().unwrap();

        assert_eq!(
            error.to_string(),
            "daemon execution failed: daemon command execution path is not implemented yet"
        );
        assert!(executor.calls().is_empty());
    }

    #[test]
    fn live_daemon_screen_raw_uses_daemon_output_without_local_fallback() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        let server = DaemonServer::bind_with_handler(
            &socket_path,
            ScreenDaemonRequestHandler::with_idle_timeout(
                StaticDiscovery {
                    devices: vec![test_device("/dev/ttyACM0")],
                    error: None,
                },
                FakeTransportFactory::default(),
                std::time::Duration::from_secs(60),
            ),
        )
        .unwrap();
        let server_thread = thread::spawn(move || server.serve_one());

        let request = CommandRequest::Screen(ScreenRequest::Raw {
            lua: "return 1".to_string(),
            target: TargetArgs::default(),
        });
        let mut direct_transport_factory = FakeTransportFactory::default();
        let direct_discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut direct_executor =
            OneShotCommandExecutor::new(&direct_discovery, &mut direct_transport_factory);
        let expected_output =
            execute_and_render_command(&mut direct_executor, request.clone()).unwrap();

        let mut executor = RecordingExecutor::new(Ok(CommandSuccess::DeviceList {
            devices: Vec::new(),
        }));
        let mut daemon_client = SystemDaemonClient::with_socket_path(&socket_path);

        let daemon_output = execute_and_render_command_with_optional_daemon(
            &mut executor,
            &mut daemon_client,
            request,
        )
        .unwrap();

        server_thread.join().unwrap().unwrap();

        assert_eq!(daemon_output, expected_output);
        assert!(executor.calls().is_empty());
    }

    #[test]
    fn live_daemon_device_info_uses_daemon_output_without_local_fallback() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        let server = DaemonServer::bind_with_handler(
            &socket_path,
            ScreenDaemonRequestHandler::with_idle_timeout(
                StaticDiscovery {
                    devices: vec![test_device("/dev/ttyACM0")],
                    error: None,
                },
                FakeTransportFactory::default(),
                std::time::Duration::from_secs(60),
            ),
        )
        .unwrap();
        let server_thread = thread::spawn(move || server.serve_one());

        let request = CommandRequest::Device(DeviceRequest::Info {
            target: TargetArgs::default(),
        });
        let mut direct_transport_factory = FakeTransportFactory::default();
        let direct_discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut direct_executor =
            OneShotCommandExecutor::new(&direct_discovery, &mut direct_transport_factory);
        let expected_output =
            execute_and_render_command(&mut direct_executor, request.clone()).unwrap();

        let mut executor = RecordingExecutor::new(Ok(CommandSuccess::DeviceList {
            devices: Vec::new(),
        }));
        let mut daemon_client = SystemDaemonClient::with_socket_path(&socket_path);

        let daemon_output = execute_and_render_command_with_optional_daemon(
            &mut executor,
            &mut daemon_client,
            request,
        )
        .unwrap();

        server_thread.join().unwrap().unwrap();

        assert_eq!(daemon_output, expected_output);
        assert!(executor.calls().is_empty());
    }

    #[test]
    fn live_daemon_runtime_status_uses_daemon_output_without_local_fallback() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        let server = DaemonServer::bind_with_handler(
            &socket_path,
            ScreenDaemonRequestHandler::with_idle_timeout(
                StaticDiscovery {
                    devices: vec![test_device("/dev/ttyACM0")],
                    error: None,
                },
                FakeTransportFactory::default(),
                std::time::Duration::from_secs(60),
            ),
        )
        .unwrap();
        let server_thread = thread::spawn(move || server.serve_one());

        let request = CommandRequest::Runtime(RuntimeRequest::Status {
            target: TargetArgs::default(),
        });
        let mut direct_transport_factory = FakeTransportFactory::default();
        let direct_discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut direct_executor =
            OneShotCommandExecutor::new(&direct_discovery, &mut direct_transport_factory);
        let expected_output =
            execute_and_render_command(&mut direct_executor, request.clone()).unwrap();

        let mut executor = RecordingExecutor::new(Ok(CommandSuccess::DeviceList {
            devices: Vec::new(),
        }));
        let mut daemon_client = SystemDaemonClient::with_socket_path(&socket_path);

        let daemon_output = execute_and_render_command_with_optional_daemon(
            &mut executor,
            &mut daemon_client,
            request,
        )
        .unwrap();

        server_thread.join().unwrap().unwrap();

        assert_eq!(daemon_output, expected_output);
        assert!(executor.calls().is_empty());
    }

    #[test]
    fn live_daemon_busy_port_failures_do_not_fall_back_locally() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        let mut transport_factory = FakeTransportFactory::default();
        transport_factory.fail_next_open(TransportError::open("Device or resource busy"));
        let server = DaemonServer::bind_with_handler(
            &socket_path,
            ScreenDaemonRequestHandler::with_idle_timeout(
                StaticDiscovery {
                    devices: vec![test_device("/dev/ttyACM0")],
                    error: None,
                },
                transport_factory,
                std::time::Duration::from_secs(60),
            ),
        )
        .unwrap();
        let server_thread = thread::spawn(move || server.serve_one());

        let request = CommandRequest::Screen(ScreenRequest::Raw {
            lua: "return 1".to_string(),
            target: TargetArgs::default(),
        });
        let mut executor = RecordingExecutor::new(Ok(CommandSuccess::DeviceList {
            devices: Vec::new(),
        }));
        let mut daemon_client = SystemDaemonClient::with_socket_path(&socket_path);

        let error = execute_and_render_command_with_optional_daemon(
            &mut executor,
            &mut daemon_client,
            request,
        )
        .unwrap_err();

        server_thread.join().unwrap().unwrap();

        assert_eq!(
            error.to_string(),
            "daemon execution failed: transport open failed: Device or resource busy"
        );
        assert!(executor.calls().is_empty());
    }

    #[test]
    fn live_daemon_disconnect_does_not_fall_back_locally() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let server_thread = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
        });

        let request = CommandRequest::Screen(ScreenRequest::Raw {
            lua: "return 1".to_string(),
            target: TargetArgs::default(),
        });
        let mut executor = RecordingExecutor::new(Ok(CommandSuccess::DeviceList {
            devices: Vec::new(),
        }));
        let mut daemon_client = SystemDaemonClient::with_socket_path(&socket_path);

        let error = execute_and_render_command_with_optional_daemon(
            &mut executor,
            &mut daemon_client,
            request,
        )
        .unwrap_err();

        server_thread.join().unwrap();

        let message = error.to_string();
        assert!(
            message.contains("invalid daemon protocol message")
                || message.contains("daemon I/O failed")
        );
        assert!(executor.calls().is_empty());
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
                debug: false,
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
                debug: false,
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
            "multiple supported VSN1/Grid USB serial devices found (/dev/ttyACM0, /dev/ttyACM1); rerun with `--device <path>` to select one explicitly"
        );
    }

    #[test]
    fn device_info_uses_the_requested_device_when_multiple_devices_are_visible() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0"), test_device("/dev/ttyACM1")],
            error: None,
        };
        let mut transport_factory = FakeTransportFactory::default();

        let output = execute_cli(
            Cli {
                debug: false,
                command: TopLevelCommand::Device(DeviceArgs {
                    command: DeviceCommand::Info {
                        target: TargetArgs {
                            device: Some("/dev/ttyACM1".to_string()),
                            dx: None,
                            dy: None,
                        },
                    },
                }),
            },
            &discovery,
            &mut transport_factory,
        )
        .unwrap();

        assert!(output.contains("Selected USB device: /dev/ttyACM1"));
        assert_eq!(
            transport_factory.open_calls(),
            &[OpenCall {
                port_name: "/dev/ttyACM1".to_string(),
                baud_rate: protocol::GRID_BAUD_RATE,
            }]
        );
    }

    #[test]
    fn device_info_auto_selects_the_macos_callout_device_for_a_tty_cu_pair() {
        let discovery = StaticDiscovery {
            devices: vec![
                test_device("/dev/tty.usbmodem101"),
                test_device("/dev/cu.usbmodem101"),
            ],
            error: None,
        };
        let mut transport_factory = FakeTransportFactory::default();

        let output = execute_cli(
            Cli {
                debug: false,
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

        assert!(output.contains("Selected USB device: /dev/cu.usbmodem101"));
        assert_eq!(
            transport_factory.open_calls(),
            &[OpenCall {
                port_name: "/dev/cu.usbmodem101".to_string(),
                baud_rate: protocol::GRID_BAUD_RATE,
            }]
        );
    }

    #[test]
    fn parses_runtime_remove_with_explicit_target() {
        let cli =
            try_parse_from(["vsn1-cli", "runtime", "remove", "--dx", "0", "--dy", "0"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                debug: false,
                command: TopLevelCommand::Runtime(RuntimeArgs {
                    command: RuntimeCommand::Remove {
                        target: TargetArgs {
                            device: None,
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
                debug: false,
                command: TopLevelCommand::Runtime(RuntimeArgs {
                    command: RuntimeCommand::Remove {
                        target: TargetArgs {
                            device: None,
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
                debug: false,
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Raw {
                        lua: "return 1".to_string(),
                        target: TargetArgs {
                            device: None,
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
        assert_eq!(&packet[32..packet.len() - 5], b"--[[@cb]] return 1");
    }

    #[test]
    fn screen_raw_surfaces_targeting_errors() {
        let error = execute_cli(
            Cli {
                debug: false,
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Raw {
                        lua: "return 1".to_string(),
                        target: TargetArgs {
                            device: None,
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
    fn screen_raw_strips_non_ascii_before_sending() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut transport_factory = RecordingTransportFactory::default();

        let output = execute_cli(
            Cli {
                debug: false,
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Raw {
                        lua: "snowman = '☃'".to_string(),
                        target: TargetArgs::default(),
                    },
                }),
            },
            &discovery,
            &mut transport_factory,
        )
        .unwrap();

        assert!(output.contains("Module target: broadcast"));
        assert_eq!(transport_factory.open_calls.len(), 1);

        let writes = transport_factory.immediate_writes();
        let packet = &writes[0];
        assert_eq!(&packet[32..packet.len() - 5], b"--[[@cb]] snowman = ''");
    }

    #[test]
    fn screen_set_sends_without_runtime_verification() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut transport_factory = FakeTransportFactory::default();
        let registry = ScreenFieldRegistry::bundled().unwrap();

        let output = render_command_success(
            &execute_screen_set_with_registry(
                &discovery,
                &mut transport_factory,
                &TargetArgs::default(),
                &["persistent.title=Hello".to_string()],
                None,
                &registry,
            )
            .unwrap(),
        );

        assert!(output.contains("Sent curated screen update over the immediate path."));
        assert_eq!(transport_factory.open_calls().len(), 1);
    }

    #[test]
    fn runtime_install_rendering_includes_verification_summary_and_installed_slots() {
        let source_target = protocol::GridTarget::new(0, 0);
        let installed_slots = vec![
            crate::runtime_bundle::OwnedRuntimeSlot {
                name: "lcd-init".to_string(),
                page: 0,
                element: 13,
                event: 0,
                asset: "lcd-init.lua".to_string(),
                install_order: 10,
            },
            crate::runtime_bundle::OwnedRuntimeSlot {
                name: "lcd-draw".to_string(),
                page: 0,
                element: 13,
                event: 8,
                asset: "lcd-draw.lua".to_string(),
                install_order: 20,
            },
        ];
        let report = RuntimeInstallReport::new_for_tests(
            installed_slots.clone(),
            RuntimeInspectionReport::new_for_tests(
                ResolvedTarget::Explicit(source_target),
                vec![source_target],
                installed_slots
                    .iter()
                    .cloned()
                    .map(|slot| crate::runtime::RuntimeSlotInspection {
                        slot,
                        status: RuntimeSlotStatus::Match { source_target },
                    })
                    .collect(),
            ),
        );

        let output = render_command_success(&CommandSuccess::RuntimeInstall {
            device: test_device("/dev/ttyACM0").to_string(),
            target: ResolvedTarget::Explicit(source_target),
            runtime: Some(DiscoveredRuntime {
                name: "media".to_string(),
                path: std::path::PathBuf::from("/tmp/media"),
                source: crate::runtime_bundle::RuntimeSource::Dev,
            }),
            report,
        });

        assert!(output.contains("Installed runtime: frozen copy present"));
        assert!(output.contains("Status: exact-match compatible"));
        assert!(output.contains("Observed runtime target: dx=0 dy=0"));
        assert!(output.contains("- lcd-init (page=0 element=13 event=0): match on dx=0 dy=0"));
        assert!(output.contains("- lcd-draw (page=0 element=13 event=8): match on dx=0 dy=0"));
        assert!(output.contains("Verification: exact installed runtime match confirmed."));
        assert!(output.contains("Resolved runtime: media (dev)"));
        assert!(output.contains("Installed owned slots in manifest order:"));
    }

    #[test]
    fn screen_set_allows_mixed_layer_updates_with_activation() {
        let registry = ScreenFieldRegistry::bundled().unwrap();
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut transport_factory = FakeTransportFactory::default();

        let output = render_command_success(
            &execute_screen_set_with_registry(
                &discovery,
                &mut transport_factory,
                &TargetArgs::default(),
                &[
                    "persistent.title=Hello".to_string(),
                    "slow.message=World".to_string(),
                ],
                Some("slow".to_string()),
                &registry,
            )
            .unwrap(),
        );

        assert!(output.contains("Sent curated screen update over the immediate path."));
        assert_eq!(transport_factory.open_calls().len(), 1);
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

        let output = render_command_success(
            &execute_screen_activate_with_registry(
                &discovery,
                &mut transport_factory,
                &TargetArgs::default(),
                "persistent",
                &registry,
            )
            .unwrap(),
        );

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

        let output = render_command_success(
            &execute_screen_set_with_registry(
                &discovery,
                &mut transport_factory,
                &TargetArgs {
                    device: None,
                    dx: Some(0),
                    dy: Some(0),
                },
                &["persistent.title=Hello".to_string()],
                None,
                &registry,
            )
            .unwrap(),
        );

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
