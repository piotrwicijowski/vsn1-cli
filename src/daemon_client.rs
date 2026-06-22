use std::error::Error as StdError;
use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use crate::command_model::CommandRequest;
use crate::daemon_protocol::{
    decode_response, encode_request, DaemonProtocolError, DaemonRequest, DaemonResponse,
};
use crate::daemon_socket::{self, DaemonSocketPathError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonClientError {
    Protocol(DaemonProtocolError),
    Execution { message: String },
    Io { message: String },
}

pub type Result<T> = std::result::Result<T, DaemonClientError>;

pub trait DaemonCommandClient {
    fn try_execute(&mut self, request: &CommandRequest) -> Result<Option<String>>;
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SystemDaemonClient {
    socket_path_override: Option<PathBuf>,
}

impl SystemDaemonClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_socket_path(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path_override: Some(socket_path.into()),
        }
    }

    fn resolve_socket_path(&self) -> std::result::Result<Option<PathBuf>, DaemonSocketPathError> {
        match &self.socket_path_override {
            Some(path) => Ok(Some(path.clone())),
            None => daemon_socket::resolve_daemon_socket_path().map(Some),
        }
    }
}

impl fmt::Display for DaemonClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => error.fmt(f),
            Self::Execution { message } => write!(f, "daemon execution failed: {message}"),
            Self::Io { message } => write!(f, "daemon I/O failed: {message}"),
        }
    }
}

impl StdError for DaemonClientError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Protocol(error) => Some(error),
            Self::Execution { .. } | Self::Io { .. } => None,
        }
    }
}

impl From<DaemonProtocolError> for DaemonClientError {
    fn from(value: DaemonProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl DaemonCommandClient for SystemDaemonClient {
    fn try_execute(&mut self, request: &CommandRequest) -> Result<Option<String>> {
        let request = DaemonRequest::for_command(request.clone())?;

        let socket_path = match self.resolve_socket_path() {
            Ok(Some(path)) => path,
            Ok(None) => return Ok(None),
            Err(DaemonSocketPathError::MissingEnvironment { .. })
            | Err(DaemonSocketPathError::UnsupportedPlatform { .. }) => return Ok(None),
        };

        let mut stream = match UnixStream::connect(&socket_path) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Ok(None);
            }
            Err(error) => {
                return Err(DaemonClientError::Io {
                    message: error.to_string(),
                });
            }
        };

        let request_bytes = encode_request(&request)?;
        stream
            .write_all(&request_bytes)
            .map_err(|error| DaemonClientError::Io {
                message: error.to_string(),
            })?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|error| DaemonClientError::Io {
                message: error.to_string(),
            })?;

        let mut response_bytes = Vec::new();
        stream
            .read_to_end(&mut response_bytes)
            .map_err(|error| DaemonClientError::Io {
                message: error.to_string(),
            })?;

        match decode_response(&response_bytes)? {
            DaemonResponse::Success { output } => Ok(Some(output)),
            DaemonResponse::Error { message } => Err(DaemonClientError::Execution { message }),
            DaemonResponse::Pong => Err(DaemonClientError::Protocol(
                DaemonProtocolError::InvalidMessage {
                    message: "daemon returned Pong to an Execute request".to_string(),
                },
            )),
        }
    }
}
