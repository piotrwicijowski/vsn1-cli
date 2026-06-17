pub mod device;
mod error;
pub mod protocol;
pub mod raw;
pub mod runtime_bundle;
pub mod targeting;
pub mod transport;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};

use crate::device::{
    discover_supported_devices, select_single_device, DeviceDiscovery, SystemDeviceDiscovery,
};
use crate::raw::send_screen_raw;
use crate::targeting::resolve_target;
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
    Install,
    Verify,
    Upgrade,
    Repair,
    Remove,
    Status,
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
            RuntimeCommand::Install => "runtime install",
            RuntimeCommand::Verify => "runtime verify",
            RuntimeCommand::Upgrade => "runtime upgrade",
            RuntimeCommand::Repair => "runtime repair",
            RuntimeCommand::Remove => "runtime remove",
            RuntimeCommand::Status => "runtime status",
        }
    }
}

impl ScreenCommand {
    fn name(&self) -> &'static str {
        match self {
            ScreenCommand::Set { .. } => "screen set",
            ScreenCommand::Clear { .. } => "screen clear",
            ScreenCommand::Raw { .. } => "screen raw",
            ScreenCommand::Activate { .. } => "screen activate",
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
        TopLevelCommand::Runtime(args) => Err(Error::unimplemented(args.command.name())),
        TopLevelCommand::Screen(args) => match args.command {
            ScreenCommand::Raw { lua, target } => {
                execute_screen_raw(discovery, transport_factory, &target, &lua)
            }
            command => Err(Error::unimplemented(command.name())),
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
                    command: RuntimeCommand::Verify,
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
                    command: RuntimeCommand::Verify,
                }),
            },
            &StaticDiscovery {
                devices: Vec::new(),
                error: None,
            },
            &mut FakeTransportFactory::default(),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "runtime verify is not implemented yet");
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
