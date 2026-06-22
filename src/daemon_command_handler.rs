use crate::command_model::{CommandRequest, ScreenRequest};
use crate::daemon_protocol::{DaemonRequest, DaemonResponse};
use crate::daemon_server::{DaemonRequestHandler, EXECUTE_NOT_IMPLEMENTED_MESSAGE};
use crate::daemon_session::{DeviceSessionRegistry, DEVICE_IDLE_TIMEOUT};
use crate::device::{discover_supported_devices, select_device, DeviceDiscovery};
use crate::protocol::{self, ImmediateWrite, PacketIdentity};
use crate::screen::{
    compile_activate_lua, compile_clear_lua, compile_set_lua, ScreenFieldRegistry,
};
use crate::targeting::resolve_target;
use crate::transport::SerialTransportFactory;
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
            DaemonRequest::Execute(CommandRequest::Screen(command)) => {
                match self.execute_screen_request(command) {
                    Ok(output) => DaemonResponse::Success { output },
                    Err(error) => DaemonResponse::Error {
                        message: error.to_string(),
                    },
                }
            }
            DaemonRequest::Execute(_) => DaemonResponse::Error {
                message: EXECUTE_NOT_IMPLEMENTED_MESSAGE.to_string(),
            },
        }
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
