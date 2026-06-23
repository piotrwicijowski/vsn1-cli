use std::error::Error as StdError;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::command_model::CommandRequest;

pub const DAEMON_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DaemonRequest {
    Ping,
    Execute(CommandRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DaemonResponse {
    Pong,
    Success { output: String },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonProtocolError {
    InvalidMessage { message: String },
    VersionMismatch { expected: u32, actual: u32 },
    LocalOnlyCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VersionedDaemonRequest {
    version: u32,
    request: DaemonRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VersionedDaemonResponse {
    version: u32,
    response: DaemonResponse,
}

pub type Result<T> = std::result::Result<T, DaemonProtocolError>;

impl DaemonRequest {
    pub fn debug_name(&self) -> &'static str {
        match self {
            Self::Ping => "ping",
            Self::Execute(command) => command.debug_name(),
        }
    }

    pub fn for_command(command: CommandRequest) -> Result<Self> {
        if command.is_local_only() {
            return Err(DaemonProtocolError::LocalOnlyCommand);
        }

        Ok(Self::Execute(command))
    }
}

impl DaemonResponse {
    pub fn debug_name(&self) -> &'static str {
        match self {
            Self::Pong => "pong",
            Self::Success { .. } => "success",
            Self::Error { .. } => "error",
        }
    }
}

impl fmt::Display for DaemonProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMessage { message } => {
                write!(f, "invalid daemon protocol message: {message}")
            }
            Self::VersionMismatch { expected, actual } => write!(
                f,
                "daemon protocol version mismatch: expected {expected}, got {actual}"
            ),
            Self::LocalOnlyCommand => {
                write!(f, "local-only commands must not be sent to the daemon")
            }
        }
    }
}

impl StdError for DaemonProtocolError {}

pub fn encode_request(request: &DaemonRequest) -> Result<Vec<u8>> {
    serde_json::to_vec(&VersionedDaemonRequest {
        version: DAEMON_PROTOCOL_VERSION,
        request: request.clone(),
    })
    .map_err(|error| DaemonProtocolError::InvalidMessage {
        message: error.to_string(),
    })
}

pub fn decode_request(message: &[u8]) -> Result<DaemonRequest> {
    let envelope: VersionedDaemonRequest =
        serde_json::from_slice(message).map_err(|error| DaemonProtocolError::InvalidMessage {
            message: error.to_string(),
        })?;

    if envelope.version != DAEMON_PROTOCOL_VERSION {
        return Err(DaemonProtocolError::VersionMismatch {
            expected: DAEMON_PROTOCOL_VERSION,
            actual: envelope.version,
        });
    }

    Ok(envelope.request)
}

pub fn encode_response(response: &DaemonResponse) -> Result<Vec<u8>> {
    serde_json::to_vec(&VersionedDaemonResponse {
        version: DAEMON_PROTOCOL_VERSION,
        response: response.clone(),
    })
    .map_err(|error| DaemonProtocolError::InvalidMessage {
        message: error.to_string(),
    })
}

pub fn decode_response(message: &[u8]) -> Result<DaemonResponse> {
    let envelope: VersionedDaemonResponse =
        serde_json::from_slice(message).map_err(|error| DaemonProtocolError::InvalidMessage {
            message: error.to_string(),
        })?;

    if envelope.version != DAEMON_PROTOCOL_VERSION {
        return Err(DaemonProtocolError::VersionMismatch {
            expected: DAEMON_PROTOCOL_VERSION,
            actual: envelope.version,
        });
    }

    Ok(envelope.response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_model::{DeviceRequest, RuntimeRequest, ScreenRequest};
    use crate::TargetArgs;

    #[test]
    fn round_trips_daemon_request() {
        let request = DaemonRequest::for_command(CommandRequest::Screen(ScreenRequest::Raw {
            lua: "return 1".to_string(),
            target: TargetArgs::default(),
        }))
        .unwrap();

        let encoded = encode_request(&request).unwrap();
        let decoded = decode_request(&encoded).unwrap();

        assert_eq!(decoded, request);
    }

    #[test]
    fn round_trips_daemon_response() {
        let response = DaemonResponse::Success {
            output: "Selected USB device: /dev/ttyACM0\n".to_string(),
        };

        let encoded = encode_response(&response).unwrap();
        let decoded = decode_response(&encoded).unwrap();

        assert_eq!(decoded, response);
    }

    #[test]
    fn rejects_local_only_commands_for_daemon_requests() {
        let error =
            DaemonRequest::for_command(CommandRequest::Device(DeviceRequest::List)).unwrap_err();

        assert_eq!(error, DaemonProtocolError::LocalOnlyCommand);
    }

    #[test]
    fn reports_request_version_mismatch() {
        let message = serde_json::to_vec(&serde_json::json!({
            "version": DAEMON_PROTOCOL_VERSION + 1,
            "request": {
                "Execute": {
                    "Runtime": {
                        "Verify": {
                            "target": {
                                "device": null,
                                "dx": null,
                                "dy": null
                            }
                        }
                    }
                }
            }
        }))
        .unwrap();

        let error = decode_request(&message).unwrap_err();

        assert_eq!(
            error,
            DaemonProtocolError::VersionMismatch {
                expected: DAEMON_PROTOCOL_VERSION,
                actual: DAEMON_PROTOCOL_VERSION + 1,
            }
        );
    }

    #[test]
    fn reports_response_version_mismatch() {
        let message = serde_json::to_vec(&serde_json::json!({
            "version": DAEMON_PROTOCOL_VERSION + 1,
            "response": "Pong"
        }))
        .unwrap();

        let error = decode_response(&message).unwrap_err();

        assert_eq!(
            error,
            DaemonProtocolError::VersionMismatch {
                expected: DAEMON_PROTOCOL_VERSION,
                actual: DAEMON_PROTOCOL_VERSION + 1,
            }
        );
    }

    #[test]
    fn rejects_invalid_json_messages() {
        let error = decode_request(b"not json").unwrap_err();

        assert!(matches!(error, DaemonProtocolError::InvalidMessage { .. }));
    }

    #[test]
    fn accepts_daemon_eligible_runtime_commands() {
        let request = DaemonRequest::for_command(CommandRequest::Runtime(RuntimeRequest::Verify {
            target: TargetArgs::default(),
        }))
        .unwrap();

        assert!(matches!(request, DaemonRequest::Execute(_)));
    }
}
