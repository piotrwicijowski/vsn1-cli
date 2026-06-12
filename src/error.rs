use std::error::Error as StdError;
use std::fmt;

use crate::protocol::ProtocolError;
use crate::transport::TransportError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Unimplemented { command: &'static str },
    Protocol(ProtocolError),
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
            Self::Protocol(error) => error.fmt(f),
            Self::Transport(error) => error.fmt(f),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Unimplemented { .. } => None,
            Self::Protocol(error) => Some(error),
            Self::Transport(error) => Some(error),
        }
    }
}

impl From<ProtocolError> for Error {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<TransportError> for Error {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}
