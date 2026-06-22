use serde::{Deserialize, Serialize};

use crate::{
    Cli, DeviceArgs, DeviceCommand, RuntimeArgs, RuntimeCommand, ScreenArgs, ScreenCommand,
    TargetArgs, TopLevelCommand,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CommandRouting {
    LocalOnly,
    DaemonEligible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandRequest {
    Device(DeviceRequest),
    Runtime(RuntimeRequest),
    Screen(ScreenRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceRequest {
    List,
    Info { target: TargetArgs },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeRequest {
    List,
    Install { name: String, target: TargetArgs },
    Verify { target: TargetArgs },
    Upgrade { name: String, target: TargetArgs },
    Repair { target: TargetArgs },
    Remove { target: TargetArgs },
    Status { target: TargetArgs },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenRequest {
    Set {
        assignments: Vec<String>,
        activate: Option<String>,
        target: TargetArgs,
    },
    Clear {
        layer: String,
        target: TargetArgs,
    },
    Raw {
        lua: String,
        target: TargetArgs,
    },
    Activate {
        layer: String,
        target: TargetArgs,
    },
}

impl CommandRequest {
    pub fn routing(&self) -> CommandRouting {
        match self {
            Self::Device(DeviceRequest::List) | Self::Runtime(RuntimeRequest::List) => {
                CommandRouting::LocalOnly
            }
            Self::Device(DeviceRequest::Info { .. })
            | Self::Runtime(RuntimeRequest::Install { .. })
            | Self::Runtime(RuntimeRequest::Verify { .. })
            | Self::Runtime(RuntimeRequest::Upgrade { .. })
            | Self::Runtime(RuntimeRequest::Repair { .. })
            | Self::Runtime(RuntimeRequest::Remove { .. })
            | Self::Runtime(RuntimeRequest::Status { .. })
            | Self::Screen(ScreenRequest::Set { .. })
            | Self::Screen(ScreenRequest::Clear { .. })
            | Self::Screen(ScreenRequest::Raw { .. })
            | Self::Screen(ScreenRequest::Activate { .. }) => CommandRouting::DaemonEligible,
        }
    }

    pub fn is_local_only(&self) -> bool {
        self.routing() == CommandRouting::LocalOnly
    }

    pub fn is_daemon_eligible(&self) -> bool {
        self.routing() == CommandRouting::DaemonEligible
    }
}

impl From<Cli> for CommandRequest {
    fn from(value: Cli) -> Self {
        value.command.into()
    }
}

impl From<TopLevelCommand> for CommandRequest {
    fn from(value: TopLevelCommand) -> Self {
        match value {
            TopLevelCommand::Device(args) => Self::Device(args.into()),
            TopLevelCommand::Runtime(args) => Self::Runtime(args.into()),
            TopLevelCommand::Screen(args) => Self::Screen(args.into()),
        }
    }
}

impl From<DeviceArgs> for DeviceRequest {
    fn from(value: DeviceArgs) -> Self {
        match value.command {
            DeviceCommand::List => Self::List,
            DeviceCommand::Info { target } => Self::Info { target },
        }
    }
}

impl From<RuntimeArgs> for RuntimeRequest {
    fn from(value: RuntimeArgs) -> Self {
        match value.command {
            RuntimeCommand::List => Self::List,
            RuntimeCommand::Install { name, target } => Self::Install { name, target },
            RuntimeCommand::Verify { target } => Self::Verify { target },
            RuntimeCommand::Upgrade { name, target } => Self::Upgrade { name, target },
            RuntimeCommand::Repair { target } => Self::Repair { target },
            RuntimeCommand::Remove { target } => Self::Remove { target },
            RuntimeCommand::Status { target } => Self::Status { target },
        }
    }
}

impl From<ScreenArgs> for ScreenRequest {
    fn from(value: ScreenArgs) -> Self {
        match value.command {
            ScreenCommand::Set {
                assignments,
                activate,
                target,
            } => Self::Set {
                assignments,
                activate,
                target,
            },
            ScreenCommand::Clear { layer, target } => Self::Clear { layer, target },
            ScreenCommand::Raw { lua, target } => Self::Raw { lua, target },
            ScreenCommand::Activate { layer, target } => Self::Activate { layer, target },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_discovery_only_commands_as_local_only() {
        assert!(CommandRequest::Device(DeviceRequest::List).is_local_only());
        assert!(CommandRequest::Runtime(RuntimeRequest::List).is_local_only());
    }

    #[test]
    fn classifies_serial_commands_as_daemon_eligible() {
        assert!(CommandRequest::Device(DeviceRequest::Info {
            target: TargetArgs::default(),
        })
        .is_daemon_eligible());
        assert!(CommandRequest::Runtime(RuntimeRequest::Verify {
            target: TargetArgs::default(),
        })
        .is_daemon_eligible());
        assert!(CommandRequest::Screen(ScreenRequest::Raw {
            lua: "return 1".to_string(),
            target: TargetArgs::default(),
        })
        .is_daemon_eligible());
    }
}
