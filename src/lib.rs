mod error;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};

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
    Err(Error::unimplemented(cli.command.name()))
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

impl TopLevelCommand {
    fn name(&self) -> &'static str {
        match self {
            TopLevelCommand::Device(args) => args.command.name(),
            TopLevelCommand::Runtime(args) => args.command.name(),
            TopLevelCommand::Screen(args) => args.command.name(),
        }
    }
}

impl DeviceCommand {
    fn name(&self) -> &'static str {
        match self {
            DeviceCommand::List => "device list",
            DeviceCommand::Info { .. } => "device info",
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn run_returns_stub_error_for_unimplemented_command() {
        let error = run(Cli {
            command: TopLevelCommand::Device(DeviceArgs {
                command: DeviceCommand::List,
            }),
        })
        .unwrap_err();

        assert_eq!(error.to_string(), "device list is not implemented yet");
    }
}
