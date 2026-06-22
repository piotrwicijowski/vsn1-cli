use crate::command_model::{CommandRequest, DeviceRequest, RuntimeRequest, ScreenRequest};
use crate::daemon_protocol::{DaemonRequest, DaemonResponse};
use crate::daemon_server::DaemonRequestHandler;
use crate::daemon_session::{DeviceSessionRegistry, DEVICE_IDLE_TIMEOUT};
use crate::device::{discover_supported_devices, select_device, DeviceDiscovery};
use crate::protocol::{self, ImmediateWrite, PacketIdentity};
use crate::runtime::{
    inspect_installed_runtime, install_runtime_with_bundle_dir, remove_installed_runtime,
    repair_installed_runtime, upgrade_runtime_with_bundle_dir, verify_installed_runtime,
    TransportRuntimeSlotReader,
};
use crate::runtime_bundle::resolve_runtime;
use crate::screen::{
    compile_activate_lua, compile_clear_lua, compile_set_lua, ScreenFieldRegistry,
};
use crate::targeting::resolve_target;
use crate::transport::{SerialTransport, SerialTransportFactory};
use crate::{CommandSuccess, TargetArgs};

const DAEMON_SCREEN_PACKET_IDENTITY: PacketIdentity = PacketIdentity::new(0, 1);

pub struct ScreenDaemonRequestHandler<D, F>
where
    D: DeviceDiscovery + Send + Sync + 'static,
    F: SerialTransportFactory + Send + 'static,
    F::Transport: Send + 'static,
{
    discovery: D,
    sessions: DeviceSessionRegistry<F>,
}

impl<D, F> ScreenDaemonRequestHandler<D, F>
where
    D: DeviceDiscovery + Send + Sync + 'static,
    F: SerialTransportFactory + Send + 'static,
    F::Transport: Send + 'static,
{
    pub fn new(discovery: D, transport_factory: F) -> Self {
        Self::with_idle_timeout(discovery, transport_factory, DEVICE_IDLE_TIMEOUT)
    }

    pub fn with_idle_timeout(
        discovery: D,
        transport_factory: F,
        idle_timeout: std::time::Duration,
    ) -> Self {
        Self {
            discovery,
            sessions: DeviceSessionRegistry::new(transport_factory, idle_timeout),
        }
    }

    fn execute_screen_request(&self, command: ScreenRequest) -> crate::Result<String> {
        match command {
            ScreenRequest::Raw { lua, target } => {
                self.execute_screen_lua(&target, &lua, "raw screen update")
            }
            ScreenRequest::Set {
                assignments,
                activate,
                target,
            } => {
                let registry = ScreenFieldRegistry::installed()?;
                let parsed_assignments = registry.parse_assignments(&assignments)?;
                let activate_layer = activate
                    .as_deref()
                    .map(|layer| registry.layer(layer).map(|layer| layer.name().clone()))
                    .transpose()?;
                let lua = compile_set_lua(&parsed_assignments, activate_layer.as_ref())?;

                self.execute_screen_lua(&target, &lua, "curated screen update")
            }
            ScreenRequest::Clear { layer, target } => {
                let registry = ScreenFieldRegistry::installed()?;
                let layer = registry.layer(&layer)?.name().clone();
                let lua = compile_clear_lua(&registry, &layer)?;

                self.execute_screen_lua(&target, &lua, "screen clear command")
            }
            ScreenRequest::Activate { layer, target } => {
                let registry = ScreenFieldRegistry::installed()?;
                let layer = registry.layer(&layer)?.name().clone();
                let lua = compile_activate_lua(&layer)?;

                self.execute_screen_lua(&target, &lua, "screen activation command")
            }
        }
    }

    fn execute_device_request(&self, command: DeviceRequest) -> crate::Result<String> {
        match command {
            DeviceRequest::List => Err(crate::Error::from(
                crate::daemon_client::DaemonClientError::Protocol(
                    crate::daemon_protocol::DaemonProtocolError::LocalOnlyCommand,
                ),
            )),
            DeviceRequest::Info { target } => {
                let resolved_target = resolve_target(&target)?;
                let device = resolve_usb_device(&self.discovery, &target)?;
                self.sessions
                    .ensure_open(&device.port_name, protocol::GRID_BAUD_RATE)?;

                Ok(crate::render_command_success(&CommandSuccess::DeviceInfo {
                    device: device.to_string(),
                    target: resolved_target,
                }))
            }
        }
    }

    fn execute_runtime_request(&self, command: RuntimeRequest) -> crate::Result<String> {
        match command {
            RuntimeRequest::List => Err(crate::Error::from(
                crate::daemon_client::DaemonClientError::Protocol(
                    crate::daemon_protocol::DaemonProtocolError::LocalOnlyCommand,
                ),
            )),
            RuntimeRequest::Verify { target } => {
                let resolved_target = resolve_target(&target)?;
                let device = resolve_usb_device(&self.discovery, &target)?;
                let device_display = device.to_string();
                let port_name = device.port_name.clone();
                self.sessions
                    .with_transport::<String, crate::Error, _>(
                        &port_name,
                        protocol::GRID_BAUD_RATE,
                        move |transport| {
                            let mut reader = TransportRuntimeSlotReader::new(
                                BorrowedSerialTransport(transport),
                            )?;
                            let report = verify_installed_runtime(resolved_target, &mut reader)?;

                            Ok(crate::render_command_success(
                                &CommandSuccess::RuntimeStatus {
                                    device: device_display.clone(),
                                    target: resolved_target,
                                    report: Some(report),
                                    verified: true,
                                },
                            ))
                        },
                    )
                    .map_err(crate::Error::from)
            }
            RuntimeRequest::Install { name, target } => {
                let runtime = resolve_runtime(&name)?;
                let resolved_target = resolve_target(&target)?;
                let device = resolve_usb_device(&self.discovery, &target)?;
                let device_display = device.to_string();
                let port_name = device.port_name.clone();
                self.sessions
                    .with_transport::<String, crate::Error, _>(
                        &port_name,
                        protocol::GRID_BAUD_RATE,
                        move |transport| {
                            let mut reader = TransportRuntimeSlotReader::new(
                                BorrowedSerialTransport(transport),
                            )?;
                            let report = install_runtime_with_bundle_dir(
                                &runtime.path,
                                resolved_target,
                                &mut reader,
                            )?;

                            Ok(crate::render_command_success(
                                &CommandSuccess::RuntimeInstall {
                                    device: device_display.clone(),
                                    target: resolved_target,
                                    runtime: Some(runtime.clone()),
                                    report,
                                },
                            ))
                        },
                    )
                    .map_err(crate::Error::from)
            }
            RuntimeRequest::Upgrade { name, target } => {
                let runtime = resolve_runtime(&name)?;
                let resolved_target = resolve_target(&target)?;
                let device = resolve_usb_device(&self.discovery, &target)?;
                let device_display = device.to_string();
                let port_name = device.port_name.clone();
                self.sessions
                    .with_transport::<String, crate::Error, _>(
                        &port_name,
                        protocol::GRID_BAUD_RATE,
                        move |transport| {
                            let mut reader = TransportRuntimeSlotReader::new(
                                BorrowedSerialTransport(transport),
                            )?;
                            let report = upgrade_runtime_with_bundle_dir(
                                &runtime.path,
                                resolved_target,
                                &mut reader,
                            )?;

                            Ok(crate::render_command_success(
                                &CommandSuccess::RuntimeUpgrade {
                                    device: device_display.clone(),
                                    target: resolved_target,
                                    runtime: runtime.clone(),
                                    report,
                                },
                            ))
                        },
                    )
                    .map_err(crate::Error::from)
            }
            RuntimeRequest::Repair { target } => {
                let resolved_target = resolve_target(&target)?;
                let device = resolve_usb_device(&self.discovery, &target)?;
                let device_display = device.to_string();
                let port_name = device.port_name.clone();
                self.sessions
                    .with_transport::<String, crate::Error, _>(
                        &port_name,
                        protocol::GRID_BAUD_RATE,
                        move |transport| {
                            let mut reader = TransportRuntimeSlotReader::new(
                                BorrowedSerialTransport(transport),
                            )?;
                            let report = repair_installed_runtime(resolved_target, &mut reader)?;

                            Ok(crate::render_command_success(
                                &CommandSuccess::RuntimeRepair {
                                    device: device_display.clone(),
                                    target: resolved_target,
                                    report,
                                },
                            ))
                        },
                    )
                    .map_err(crate::Error::from)
            }
            RuntimeRequest::Remove { target } => {
                let resolved_target = resolve_target(&target)?;
                let device = resolve_usb_device(&self.discovery, &target)?;
                let device_display = device.to_string();
                let port_name = device.port_name.clone();
                self.sessions
                    .with_transport::<String, crate::Error, _>(
                        &port_name,
                        protocol::GRID_BAUD_RATE,
                        move |transport| {
                            let mut reader = TransportRuntimeSlotReader::new(
                                BorrowedSerialTransport(transport),
                            )?;
                            let report = remove_installed_runtime(resolved_target, &mut reader)?;

                            Ok(crate::render_command_success(
                                &CommandSuccess::RuntimeRemove {
                                    device: device_display.clone(),
                                    target: resolved_target,
                                    report,
                                },
                            ))
                        },
                    )
                    .map_err(crate::Error::from)
            }
            RuntimeRequest::Status { target } => {
                let resolved_target = resolve_target(&target)?;
                let device = resolve_usb_device(&self.discovery, &target)?;
                let device_display = device.to_string();
                let port_name = device.port_name.clone();
                self.sessions
                    .with_transport::<String, crate::Error, _>(
                        &port_name,
                        protocol::GRID_BAUD_RATE,
                        move |transport| {
                            let mut reader = TransportRuntimeSlotReader::new(
                                BorrowedSerialTransport(transport),
                            )?;
                            let report = inspect_installed_runtime(resolved_target, &mut reader)?;

                            Ok(crate::render_command_success(
                                &CommandSuccess::RuntimeStatus {
                                    device: device_display.clone(),
                                    target: resolved_target,
                                    report,
                                    verified: false,
                                },
                            ))
                        },
                    )
                    .map_err(crate::Error::from)
            }
        }
    }

    fn execute_screen_lua(
        &self,
        target_args: &TargetArgs,
        lua: &str,
        action: &'static str,
    ) -> crate::Result<String> {
        let target = resolve_target(target_args)?;
        let device = resolve_usb_device(&self.discovery, target_args)?;
        let packet = protocol::encode_immediate_packet(&ImmediateWrite {
            target: target.grid_target(),
            lua,
            identity: DAEMON_SCREEN_PACKET_IDENTITY,
        })?;
        self.sessions
            .write_immediate(&device.port_name, protocol::GRID_BAUD_RATE, packet)?;

        Ok(crate::render_command_success(
            &CommandSuccess::ScreenAction {
                device: device.to_string(),
                target,
                action,
            },
        ))
    }
}

impl<D, F> DaemonRequestHandler for ScreenDaemonRequestHandler<D, F>
where
    D: DeviceDiscovery + Send + Sync + 'static,
    F: SerialTransportFactory + Send + 'static,
    F::Transport: Send + 'static,
{
    fn handle(&self, request: DaemonRequest) -> DaemonResponse {
        match request {
            DaemonRequest::Ping => DaemonResponse::Pong,
            DaemonRequest::Execute(CommandRequest::Device(command)) => {
                match self.execute_device_request(command) {
                    Ok(output) => DaemonResponse::Success { output },
                    Err(error) => DaemonResponse::Error {
                        message: error.to_string(),
                    },
                }
            }
            DaemonRequest::Execute(CommandRequest::Runtime(command)) => {
                match self.execute_runtime_request(command) {
                    Ok(output) => DaemonResponse::Success { output },
                    Err(error) => DaemonResponse::Error {
                        message: error.to_string(),
                    },
                }
            }
            DaemonRequest::Execute(CommandRequest::Screen(command)) => {
                match self.execute_screen_request(command) {
                    Ok(output) => DaemonResponse::Success { output },
                    Err(error) => DaemonResponse::Error {
                        message: error.to_string(),
                    },
                }
            }
        }
    }
}

struct BorrowedSerialTransport<'a>(&'a mut dyn SerialTransport);

impl SerialTransport for BorrowedSerialTransport<'_> {
    fn write_immediate(
        &mut self,
        packet: &[u8],
    ) -> std::result::Result<(), crate::transport::TransportError> {
        self.0.write_immediate(packet)
    }

    fn write_config(
        &mut self,
        packet: &[u8],
    ) -> std::result::Result<(), crate::transport::TransportError> {
        self.0.write_config(packet)
    }

    fn bytes_to_read(&self) -> std::result::Result<u32, crate::transport::TransportError> {
        self.0.bytes_to_read()
    }

    fn read(
        &mut self,
        buffer: &mut [u8],
    ) -> std::result::Result<usize, crate::transport::TransportError> {
        self.0.read(buffer)
    }

    fn clear_input(&mut self) -> std::result::Result<(), crate::transport::TransportError> {
        self.0.clear_input()
    }
}

fn resolve_usb_device(
    discovery: &impl DeviceDiscovery,
    target_args: &TargetArgs,
) -> crate::Result<crate::device::DiscoveredDevice> {
    let devices = discover_supported_devices(discovery)?;
    Ok(select_device(&devices, target_args.device.as_deref())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon_protocol::{decode_response, encode_request};
    use crate::daemon_server::DaemonServer;
    use crate::device::{DeviceError, DiscoveredDevice};
    use crate::transport::FakeTransportFactory;
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;
    use std::thread;
    use tempfile::tempdir;

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
    fn screen_raw_round_trips_over_the_daemon_socket() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        let server = DaemonServer::bind_with_handler(
            &socket_path,
            ScreenDaemonRequestHandler::with_idle_timeout(
                StaticDiscovery {
                    devices: vec![test_device("/dev/ttyACM0")],
                    error: None,
                },
                FakeTransportFactory::default(),
                std::time::Duration::from_secs(60),
            ),
        )
        .unwrap();
        let server_thread = thread::spawn(move || server.serve_one());

        let mut client = UnixStream::connect(&socket_path).unwrap();
        let request = encode_request(
            &DaemonRequest::for_command(CommandRequest::Screen(ScreenRequest::Raw {
                lua: "return 1".to_string(),
                target: TargetArgs::default(),
            }))
            .unwrap(),
        )
        .unwrap();
        client.write_all(&request).unwrap();
        client.shutdown(Shutdown::Write).unwrap();

        let mut response_bytes = Vec::new();
        client.read_to_end(&mut response_bytes).unwrap();
        server_thread.join().unwrap().unwrap();

        let response = decode_response(&response_bytes).unwrap();
        assert_eq!(
            response,
            DaemonResponse::Success {
                output: "Selected USB device: /dev/ttyACM0 [Grid / VSN1] VID:PID 03eb:ecac product=VSN1 manufacturer=Intech serial=ABC123\nTransport: opened successfully at 2000000 baud\nModule target: broadcast\nSent raw screen update over the immediate path.\n".to_string()
            }
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
