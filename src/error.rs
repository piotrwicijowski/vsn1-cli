use std::error::Error as StdError;
use std::fmt;

use crate::device::DeviceError;
use crate::protocol::ProtocolError;
use crate::runtime::RuntimeError;
use crate::runtime_bundle::RuntimeBundleError;
use crate::screen::ScreenError;
use crate::targeting::TargetingError;
use crate::transport::TransportError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Unimplemented { command: &'static str },
    Device(DeviceError),
    Protocol(ProtocolError),
    Runtime(RuntimeError),
    RuntimeBundle(RuntimeBundleError),
    Screen(ScreenError),
    Targeting(TargetingError),
    Transport(TransportError),
}

impl Error {
    pub fn unimplemented(command: &'static str) -> Self {
        Self::Unimplemented { command }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unimplemented { command } => write!(f, "{command} is not implemented yet"),
            Self::Device(error) => error.fmt(f),
            Self::Protocol(error) => error.fmt(f),
            Self::Runtime(error) => error.fmt(f),
            Self::RuntimeBundle(error) => error.fmt(f),
            Self::Screen(error) => error.fmt(f),
            Self::Targeting(error) => error.fmt(f),
            Self::Transport(error) => error.fmt(f),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Unimplemented { .. } => None,
            Self::Device(error) => Some(error),
            Self::Protocol(error) => Some(error),
            Self::Runtime(error) => Some(error),
            Self::RuntimeBundle(error) => Some(error),
            Self::Screen(error) => Some(error),
            Self::Targeting(error) => Some(error),
            Self::Transport(error) => Some(error),
        }
    }
}

impl From<DeviceError> for Error {
    fn from(value: DeviceError) -> Self {
        Self::Device(value)
    }
}

impl From<ProtocolError> for Error {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<TargetingError> for Error {
    fn from(value: TargetingError) -> Self {
        Self::Targeting(value)
    }
}

impl From<RuntimeError> for Error {
    fn from(value: RuntimeError) -> Self {
        Self::Runtime(value)
    }
}

impl From<RuntimeBundleError> for Error {
    fn from(value: RuntimeBundleError) -> Self {
        Self::RuntimeBundle(value)
    }
}

impl From<ScreenError> for Error {
    fn from(value: ScreenError) -> Self {
        Self::Screen(value)
    }
}

impl From<TransportError> for Error {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}
