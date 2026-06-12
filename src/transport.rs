use std::error::Error as StdError;
use std::fmt;
use std::io;

use crate::protocol::{self, ConfigWrite, ImmediateWrite};
use crate::Result;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TransportOperation {
    Immediate,
    Config,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError {
    operation: TransportOperation,
    message: String,
}

pub trait SerialTransport {
    fn write_immediate(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError>;
    fn write_config(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError>;
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FakeTransport {
    immediate_writes: Vec<Vec<u8>>,
    config_writes: Vec<Vec<u8>>,
    next_immediate_error: Option<TransportError>,
    next_config_error: Option<TransportError>,
}

impl TransportError {
    pub fn immediate(message: impl Into<String>) -> Self {
        Self {
            operation: TransportOperation::Immediate,
            message: message.into(),
        }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self {
            operation: TransportOperation::Config,
            message: message.into(),
        }
    }

    pub fn from_io(operation: TransportOperation, error: io::Error) -> Self {
        Self {
            operation,
            message: error.to_string(),
        }
    }

    pub fn operation(&self) -> TransportOperation {
        self.operation
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operation = match self.operation {
            TransportOperation::Immediate => "immediate",
            TransportOperation::Config => "config",
        };

        write!(f, "{operation} transport write failed: {}", self.message)
    }
}

impl StdError for TransportError {}

impl FakeTransport {
    pub fn fail_next_immediate(&mut self, error: TransportError) {
        self.next_immediate_error = Some(error);
    }

    pub fn fail_next_config(&mut self, error: TransportError) {
        self.next_config_error = Some(error);
    }

    pub fn immediate_writes(&self) -> &[Vec<u8>] {
        &self.immediate_writes
    }

    pub fn config_writes(&self) -> &[Vec<u8>] {
        &self.config_writes
    }
}

impl SerialTransport for FakeTransport {
    fn write_immediate(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError> {
        if let Some(error) = self.next_immediate_error.take() {
            return Err(error);
        }

        self.immediate_writes.push(packet.to_vec());
        Ok(())
    }

    fn write_config(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError> {
        if let Some(error) = self.next_config_error.take() {
            return Err(error);
        }

        self.config_writes.push(packet.to_vec());
        Ok(())
    }
}

pub fn send_immediate(
    transport: &mut impl SerialTransport,
    write: &ImmediateWrite<'_>,
) -> Result<()> {
    let packet = protocol::encode_immediate_packet(write)?;
    transport.write_immediate(&packet)?;
    Ok(())
}

pub fn send_config(transport: &mut impl SerialTransport, write: &ConfigWrite<'_>) -> Result<()> {
    let packet = protocol::encode_config_packet(write)?;
    transport.write_config(&packet)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ConfigLocation, GridTarget, PacketIdentity};

    #[test]
    fn fake_transport_keeps_immediate_and_config_paths_separate() {
        let mut transport = FakeTransport::default();

        send_immediate(
            &mut transport,
            &ImmediateWrite {
                target: GridTarget::BROADCAST,
                lua: "return 1",
                identity: PacketIdentity::new(0, 1),
            },
        )
        .unwrap();

        send_config(
            &mut transport,
            &ConfigWrite {
                target: GridTarget::new(0, 0),
                location: ConfigLocation::new(0xff, 13, 8),
                lua: "return 2",
                identity: PacketIdentity::new(0, 2),
            },
        )
        .unwrap();

        assert_eq!(transport.immediate_writes().len(), 1);
        assert_eq!(transport.config_writes().len(), 1);
        assert_eq!(&transport.immediate_writes()[0][24..27], b"085");
        assert_eq!(&transport.config_writes()[0][24..27], b"060");
    }

    #[test]
    fn send_immediate_maps_transport_failures() {
        let mut transport = FakeTransport::default();
        transport.fail_next_immediate(TransportError::from_io(
            TransportOperation::Immediate,
            io::Error::new(io::ErrorKind::BrokenPipe, "write failed"),
        ));

        let error = send_immediate(
            &mut transport,
            &ImmediateWrite {
                target: GridTarget::BROADCAST,
                lua: "return 1",
                identity: PacketIdentity::new(0, 1),
            },
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "immediate transport write failed: write failed"
        );
    }

    #[test]
    fn send_config_maps_transport_failures() {
        let mut transport = FakeTransport::default();
        transport.fail_next_config(TransportError::config("permission denied"));

        let error = send_config(
            &mut transport,
            &ConfigWrite {
                target: GridTarget::new(0, 0),
                location: ConfigLocation::new(0xff, 13, 8),
                lua: "return 2",
                identity: PacketIdentity::new(0, 2),
            },
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "config transport write failed: permission denied"
        );
    }
}
