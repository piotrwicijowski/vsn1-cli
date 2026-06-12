use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::io::Write;
use std::time::Duration;

use serialport::SerialPort;

use crate::protocol::{self, ConfigWrite, ImmediateWrite};
use crate::Result;

const SERIAL_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TransportOperation {
    Open,
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

pub trait SerialTransportFactory {
    type Transport: SerialTransport;

    fn open(
        &mut self,
        port_name: &str,
        baud_rate: u32,
    ) -> std::result::Result<Self::Transport, TransportError>;
}

pub struct SystemTransportFactory;

pub struct SystemSerialTransport {
    port: Box<dyn SerialPort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCall {
    pub port_name: String,
    pub baud_rate: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FakeTransport {
    immediate_writes: Vec<Vec<u8>>,
    config_writes: Vec<Vec<u8>>,
    next_immediate_error: Option<TransportError>,
    next_config_error: Option<TransportError>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FakeTransportFactory {
    open_calls: Vec<OpenCall>,
    next_open_error: Option<TransportError>,
}

impl TransportError {
    pub fn open(message: impl Into<String>) -> Self {
        Self {
            operation: TransportOperation::Open,
            message: message.into(),
        }
    }

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
            TransportOperation::Open => "transport open",
            TransportOperation::Immediate => "immediate",
            TransportOperation::Config => "config",
        };

        match self.operation {
            TransportOperation::Open => write!(f, "{operation} failed: {}", self.message),
            _ => write!(f, "{operation} transport write failed: {}", self.message),
        }
    }
}

impl StdError for TransportError {}

impl SerialTransportFactory for SystemTransportFactory {
    type Transport = SystemSerialTransport;

    fn open(
        &mut self,
        port_name: &str,
        baud_rate: u32,
    ) -> std::result::Result<Self::Transport, TransportError> {
        let port = serialport::new(port_name, baud_rate)
            .timeout(SERIAL_TIMEOUT)
            .open()
            .map_err(|error| TransportError::open(error.to_string()))?;

        Ok(SystemSerialTransport { port })
    }
}

impl SerialTransport for SystemSerialTransport {
    fn write_immediate(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError> {
        self.port
            .write_all(packet)
            .map_err(|error| TransportError::from_io(TransportOperation::Immediate, error))?;
        self.port
            .flush()
            .map_err(|error| TransportError::from_io(TransportOperation::Immediate, error))?;

        Ok(())
    }

    fn write_config(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError> {
        self.port
            .write_all(packet)
            .map_err(|error| TransportError::from_io(TransportOperation::Config, error))?;
        self.port
            .flush()
            .map_err(|error| TransportError::from_io(TransportOperation::Config, error))?;

        Ok(())
    }
}

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

impl FakeTransportFactory {
    pub fn fail_next_open(&mut self, error: TransportError) {
        self.next_open_error = Some(error);
    }

    pub fn open_calls(&self) -> &[OpenCall] {
        &self.open_calls
    }
}

impl SerialTransportFactory for FakeTransportFactory {
    type Transport = FakeTransport;

    fn open(
        &mut self,
        port_name: &str,
        baud_rate: u32,
    ) -> std::result::Result<Self::Transport, TransportError> {
        if let Some(error) = self.next_open_error.take() {
            return Err(error);
        }

        self.open_calls.push(OpenCall {
            port_name: port_name.to_string(),
            baud_rate,
        });

        Ok(FakeTransport::default())
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

    #[test]
    fn fake_transport_factory_records_open_calls() {
        let mut factory = FakeTransportFactory::default();

        let _transport = factory.open("/dev/ttyACM0", 2_000_000).unwrap();

        assert_eq!(
            factory.open_calls(),
            &[OpenCall {
                port_name: "/dev/ttyACM0".to_string(),
                baud_rate: 2_000_000,
            }]
        );
    }
}
