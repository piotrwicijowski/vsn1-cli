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

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};

use crate::device::{
    discover_supported_devices, select_single_device, DeviceDiscovery, SystemDeviceDiscovery,
};
use crate::raw::send_screen_raw;
use crate::runtime::{
    inspect_bundled_runtime, install_bundled_runtime, verify_bundled_runtime,
    RuntimeInspectionReport, RuntimeInstallReport, RuntimeSlotStatus, TransportRuntimeSlotReader,
};
use crate::screen::{
    compile_activate_lua, compile_clear_lua, compile_set_lua, ScreenFieldRegistry,
    ScreenLayer as RegistryScreenLayer,
};
use crate::targeting::{resolve_target, ResolvedTarget};
use crate::transport::{SerialTransportFactory, SystemTransportFactory};

pub use error::{Error, Result};

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "vsn1-cli",
    version,
    about = "Standalone CLI for controlling the VSN1 display",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: TopLevelCommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum TopLevelCommand {
    Device(DeviceArgs),
    Runtime(RuntimeArgs),
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
    List,
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
    Install {
        #[command(flatten)]
        target: TargetArgs,
    },
    Verify {
        #[command(flatten)]
        target: TargetArgs,
    },
    Upgrade {
        #[command(flatten)]
        target: TargetArgs,
    },
    Repair {
        #[command(flatten)]
        target: TargetArgs,
    },
    Remove {
        #[command(flatten)]
        target: TargetArgs,
    },
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
    Set {
        #[arg(value_name = "FIELD=VALUE", required = true, num_args = 1..)]
        assignments: Vec<String>,
        #[arg(long, value_enum)]
        activate: Option<ActivationLayer>,
        #[command(flatten)]
        target: TargetArgs,
    },
    Clear {
        #[arg(value_enum)]
        layer: Layer,
        #[command(flatten)]
        target: TargetArgs,
    },
    Raw {
        #[arg(value_name = "LUA")]
        lua: String,
        #[command(flatten)]
        target: TargetArgs,
    },
    Activate {
        #[arg(value_enum)]
        layer: ActivationLayer,
        #[command(flatten)]
        target: TargetArgs,
    },
}

#[derive(Debug, Args, Clone, Default, PartialEq, Eq)]
pub struct TargetArgs {
    #[arg(long)]
    pub dx: Option<u16>,
    #[arg(long)]
    pub dy: Option<u16>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum Layer {
    Persistent,
    Slow,
    Fast,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum ActivationLayer {
    Slow,
    Fast,
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

impl RuntimeCommand {
    fn name(&self) -> &'static str {
        match self {
            RuntimeCommand::Install { .. } => "runtime install",
            RuntimeCommand::Verify { .. } => "runtime verify",
            RuntimeCommand::Upgrade { .. } => "runtime upgrade",
            RuntimeCommand::Repair { .. } => "runtime repair",
            RuntimeCommand::Remove { .. } => "runtime remove",
            RuntimeCommand::Status { .. } => "runtime status",
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
            RuntimeCommand::Install { target } => {
                execute_runtime_install(discovery, transport_factory, &target)
            }
            RuntimeCommand::Verify { target } => {
                execute_runtime_verify(discovery, transport_factory, &target)
            }
            RuntimeCommand::Status { target } => {
                execute_runtime_status(discovery, transport_factory, &target)
            }
            command => Err(Error::unimplemented(command.name())),
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
                execute_screen_clear(discovery, transport_factory, &target, layer)
            }
            ScreenCommand::Raw { lua, target } => {
                execute_screen_raw(discovery, transport_factory, &target, &lua)
            }
            ScreenCommand::Activate { layer, target } => {
                execute_screen_activate(discovery, transport_factory, &target, layer)
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
    activate: Option<ActivationLayer>,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let registry = ScreenFieldRegistry::bundled()?;
    let parsed_assignments = registry.parse_assignments(assignments)?;
    let lua = compile_set_lua(
        &parsed_assignments,
        activate.map(screen_layer_from_activation_layer),
    )?;

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
    layer: Layer,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let registry = ScreenFieldRegistry::bundled()?;
    let lua = compile_clear_lua(&registry, screen_layer_from_layer(layer))?;

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
    layer: ActivationLayer,
) -> Result<String>
where
    D: DeviceDiscovery,
    F: SerialTransportFactory,
{
    let lua = compile_activate_lua(screen_layer_from_activation_layer(layer))?;

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

    let report = {
        let verify_transport =
            transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
        let mut reader = TransportRuntimeSlotReader::new(verify_transport)?;
        verify_bundled_runtime(target, &mut reader)?
    };

    let mut transport = transport_factory.open(&device.port_name, protocol::GRID_BAUD_RATE)?;
    send_screen_raw(&mut transport, target.grid_target(), lua)?;

    Ok(format!(
        "Selected USB device: {device}\nTransport: opened successfully at {} baud\nModule target: {target}\nBundled runtime version: {}\nRuntime verification: exact bundled runtime match confirmed.\nSent {action} over the immediate path.\n",
        protocol::GRID_BAUD_RATE,
        report.bundle_version(),
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
    let report = verify_bundled_runtime(target, &mut reader)?;

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
    let report = install_bundled_runtime(target, &mut reader)?;

    Ok(render_runtime_install_output(
        &device.to_string(),
        target,
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
    let report = inspect_bundled_runtime(target, &mut reader)?;

    Ok(render_runtime_output(
        &device.to_string(),
        target,
        &report,
        false,
    ))
}

fn render_runtime_output(
    device: &str,
    requested_target: ResolvedTarget,
    report: &RuntimeInspectionReport,
    verified: bool,
) -> String {
    let mut output = format!(
        "Selected USB device: {device}\nTransport: opened successfully at {} baud\nModule target: {requested_target}\nBundled runtime version: {}\nStatus: {}\n",
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
            RuntimeSlotStatus::Drifted {
                actual_sha256,
                source_target,
            } => format!(
                "hash mismatch on dx={} dy={} (expected {}, got {})",
                source_target.dx,
                source_target.dy,
                inspection.slot.normalized_sha256,
                actual_sha256
            ),
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
        output.push_str("Verification: exact bundled runtime match confirmed.\n");
    }

    output
}

fn screen_layer_from_layer(layer: Layer) -> RegistryScreenLayer {
    match layer {
        Layer::Persistent => RegistryScreenLayer::Persistent,
        Layer::Slow => RegistryScreenLayer::Slow,
        Layer::Fast => RegistryScreenLayer::Fast,
    }
}

fn screen_layer_from_activation_layer(layer: ActivationLayer) -> RegistryScreenLayer {
    match layer {
        ActivationLayer::Slow => RegistryScreenLayer::Slow,
        ActivationLayer::Fast => RegistryScreenLayer::Fast,
    }
}

fn render_runtime_install_output(
    device: &str,
    requested_target: ResolvedTarget,
    report: &RuntimeInstallReport,
) -> String {
    let mut output =
        render_runtime_output(device, requested_target, report.verification_report(), true);

    output.push_str("Installed owned slots in manifest order:\n");
    for slot in report.installed_slots() {
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
    use crate::protocol::GridTarget;
    use crate::runtime_bundle::RuntimeBundle;
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
                        activate: Some(ActivationLayer::Slow),
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
                        layer: Layer::Fast,
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
            "multiple supported VSN1/Grid USB serial devices found ([\"/dev/ttyACM0\", \"/dev/ttyACM1\"]); `device info` needs exactly one visible device for now"
        );
    }

    #[test]
    fn run_returns_stub_error_for_unimplemented_command() {
        let error = execute_cli(
            Cli {
                command: TopLevelCommand::Runtime(RuntimeArgs {
                    command: RuntimeCommand::Upgrade {
                        target: TargetArgs::default(),
                    },
                }),
            },
            &StaticDiscovery {
                devices: Vec::new(),
                error: None,
            },
            &mut FakeTransportFactory::default(),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "runtime upgrade is not implemented yet");
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
        assert_eq!(&packet[32..packet.len() - 5], b"<?lua return 1 ?>");
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
            "both --dx and --dy must be provided together"
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
    fn screen_set_requires_an_exact_runtime_match_before_sending() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let mut transport_factory = FakeTransportFactory::default();

        let error = execute_cli(
            Cli {
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Set {
                        assignments: vec!["persistent.title=Hello".to_string()],
                        activate: None,
                        target: TargetArgs::default(),
                    },
                }),
            },
            &discovery,
            &mut transport_factory,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("runtime verification failed: bundled runtime"));
        assert_eq!(transport_factory.open_calls().len(), 1);
    }

    #[test]
    fn screen_set_surfaces_mixed_layer_activation_validation() {
        let error = execute_cli(
            Cli {
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Set {
                        assignments: vec![
                            "persistent.title=Hello".to_string(),
                            "slow.message=World".to_string(),
                        ],
                        activate: Some(ActivationLayer::Slow),
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
            "screen set --activate slow only supports slow-layer assignments, but `persistent.title` belongs to the persistent layer"
        );
    }

    #[test]
    fn screen_set_drops_verification_transport_before_reopening_for_send() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
            error: None,
        };
        let bundle = RuntimeBundle::bundled().unwrap();
        let fetch_responses = bundle
            .assets()
            .iter()
            .map(|asset| {
                build_config_report_frame(
                    GridTarget::new(0, 0),
                    asset.slot.page,
                    asset.slot.element,
                    asset.slot.event,
                    &asset.stored_content,
                )
            })
            .collect::<Vec<_>>();
        let mut transport_factory = SingleOpenTransportFactory::new(fetch_responses);

        let output = execute_cli(
            Cli {
                command: TopLevelCommand::Screen(ScreenArgs {
                    command: ScreenCommand::Set {
                        assignments: vec!["persistent.title=Hello".to_string()],
                        activate: None,
                        target: TargetArgs {
                            dx: Some(0),
                            dy: Some(0),
                        },
                    },
                }),
            },
            &discovery,
            &mut transport_factory,
        )
        .unwrap();

        assert!(output.contains("Runtime verification: exact bundled runtime match confirmed."));
        assert_eq!(transport_factory.open_calls.len(), 2);
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

    fn build_config_report_frame(
        source_target: GridTarget,
        page: u8,
        element: u8,
        event: u8,
        content: &str,
    ) -> Vec<u8> {
        let mut class = vec![0x02];
        class.extend_from_slice(b"060");
        class.extend_from_slice(b"d");
        class.extend_from_slice(b"010501");
        write_ascii_hex(&mut class, 2, page as usize);
        write_ascii_hex(&mut class, 2, element as usize);
        write_ascii_hex(&mut class, 2, event as usize);
        write_ascii_hex(&mut class, 4, content.len());
        class.extend_from_slice(content.as_bytes());
        class.push(0x03);

        let mut frame = vec![0x01, 0x0f];
        frame.extend_from_slice(b"0000");
        frame.extend_from_slice(b"0101");
        write_ascii_hex(&mut frame, 2, (source_target.dx + 127) as usize);
        write_ascii_hex(&mut frame, 2, (source_target.dy + 127) as usize);
        frame.extend_from_slice(b"00000000");
        frame.push(0x17);
        frame.extend_from_slice(&class);
        frame.push(0x04);

        let frame_length = frame.len();
        overwrite_ascii_hex(&mut frame, 2, 4, frame_length);
        let checksum = frame.iter().fold(0u8, |acc, byte| acc ^ byte);
        write_ascii_hex(&mut frame, 2, checksum as usize);
        frame.push(0x0a);
        frame
    }

    fn write_ascii_hex(buffer: &mut Vec<u8>, width: usize, value: usize) {
        buffer.extend_from_slice(format!("{value:0width$x}").as_bytes());
    }

    fn overwrite_ascii_hex(buffer: &mut [u8], offset: usize, width: usize, value: usize) {
        buffer[offset..offset + width].copy_from_slice(format!("{value:0width$x}").as_bytes());
    }
}
