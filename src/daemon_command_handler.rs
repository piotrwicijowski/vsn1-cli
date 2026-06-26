use crate::command_model::{CommandRequest, DeviceRequest, RuntimeRequest, ScreenRequest};
use crate::daemon_protocol::{DaemonRequest, DaemonResponse};
use crate::daemon_server::DaemonRequestHandler;
use crate::daemon_session::{DeviceSessionRegistry, DEVICE_IDLE_TIMEOUT};
use crate::debug;
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
        debug::log("daemon-handler", "executing screen command");
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
        debug::log("daemon-handler", "executing device command");
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
            DeviceRequest::PageStore { target } => {
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
                            reader.send_page_store(resolved_target)?;

                            Ok(crate::render_command_success(
                                &CommandSuccess::DeviceAction {
                                    device: device_display.clone(),
                                    target: resolved_target,
                                    action: "PAGESTORE command over the config path",
                                },
                            ))
                        },
                    )
                    .map_err(crate::Error::from)
            }
            DeviceRequest::PageDiscard { target } => {
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
                            reader.send_page_discard(resolved_target)?;

                            Ok(crate::render_command_success(
                                &CommandSuccess::DeviceAction {
                                    device: device_display.clone(),
                                    target: resolved_target,
                                    action: "PAGEDISCARD command over the config path",
                                },
                            ))
                        },
                    )
                    .map_err(crate::Error::from)
            }
        }
    }

    fn execute_runtime_request(&self, command: RuntimeRequest) -> crate::Result<String> {
        debug::log("daemon-handler", "executing runtime command");
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

    fn write_evaluate(
        &mut self,
        packet: &[u8],
    ) -> std::result::Result<(), crate::transport::TransportError> {
        self.0.write_evaluate(packet)
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
    use crate::transport::{
        FakeTransportFactory, SerialTransport, SerialTransportFactory, TransportError,
    };
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Condvar, Mutex};
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

    #[derive(Default)]
    struct BlockingTransportFactory {
        state: Arc<TestTransportState>,
    }

    struct BlockingTransport {
        port_name: String,
        state: Arc<TestTransportState>,
    }

    #[derive(Default)]
    struct TestTransportState {
        blockers: Mutex<std::collections::HashMap<String, Arc<WriteBlocker>>>,
        start_sender: Mutex<Option<mpsc::Sender<String>>>,
    }

    struct WriteBlocker {
        released: AtomicBool,
        mutex: Mutex<()>,
        condvar: Condvar,
    }

    impl WriteBlocker {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                released: AtomicBool::new(false),
                mutex: Mutex::new(()),
                condvar: Condvar::new(),
            })
        }

        fn wait(&self) {
            let mut guard = self.mutex.lock().unwrap();
            while !self.released.load(Ordering::SeqCst) {
                guard = self.condvar.wait(guard).unwrap();
            }
        }

        fn release(&self) {
            self.released.store(true, Ordering::SeqCst);
            self.condvar.notify_all();
        }
    }

    impl TestTransportState {
        fn install_blocker(&self, port_name: &str) -> Arc<WriteBlocker> {
            let blocker = WriteBlocker::new();
            self.blockers
                .lock()
                .unwrap()
                .insert(port_name.to_string(), Arc::clone(&blocker));
            blocker
        }

        fn set_start_sender(&self, sender: mpsc::Sender<String>) {
            *self.start_sender.lock().unwrap() = Some(sender);
        }
    }

    impl SerialTransportFactory for BlockingTransportFactory {
        type Transport = BlockingTransport;

        fn open(
            &mut self,
            port_name: &str,
            _baud_rate: u32,
        ) -> std::result::Result<Self::Transport, TransportError> {
            Ok(BlockingTransport {
                port_name: port_name.to_string(),
                state: Arc::clone(&self.state),
            })
        }
    }

    impl SerialTransport for BlockingTransport {
        fn write_immediate(&mut self, _packet: &[u8]) -> std::result::Result<(), TransportError> {
            if let Some(sender) = self.state.start_sender.lock().unwrap().as_ref() {
                sender.send(self.port_name.clone()).unwrap();
            }

            if let Some(blocker) = self
                .state
                .blockers
                .lock()
                .unwrap()
                .get(&self.port_name)
                .cloned()
            {
                blocker.wait();
            }

            Ok(())
        }

        fn write_evaluate(&mut self, packet: &[u8]) -> std::result::Result<(), TransportError> {
            self.write_immediate(packet)
        }

        fn write_config(&mut self, _packet: &[u8]) -> std::result::Result<(), TransportError> {
            Ok(())
        }

        fn bytes_to_read(&self) -> std::result::Result<u32, TransportError> {
            Ok(0)
        }

        fn read(&mut self, _buffer: &mut [u8]) -> std::result::Result<usize, TransportError> {
            Ok(0)
        }

        fn clear_input(&mut self) -> std::result::Result<(), TransportError> {
            Ok(())
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

    #[test]
    fn same_device_requests_queue_safely_through_the_live_daemon() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        let transport_factory = BlockingTransportFactory::default();
        let state = Arc::clone(&transport_factory.state);
        let blocker = state.install_blocker("/dev/ttyACM0");
        let (start_sender, start_receiver) = mpsc::channel();
        state.set_start_sender(start_sender);

        let server = DaemonServer::bind_with_handler(
            &socket_path,
            ScreenDaemonRequestHandler::with_idle_timeout(
                StaticDiscovery {
                    devices: vec![test_device("/dev/ttyACM0")],
                    error: None,
                },
                transport_factory,
                std::time::Duration::from_secs(60),
            ),
        )
        .unwrap();
        let server_thread = thread::spawn(move || server.serve_count(2));

        let first_client = spawn_screen_raw_client(&socket_path, "/dev/ttyACM0");
        assert_eq!(
            start_receiver
                .recv_timeout(std::time::Duration::from_millis(100))
                .unwrap(),
            "/dev/ttyACM0"
        );

        let second_client = spawn_screen_raw_client(&socket_path, "/dev/ttyACM0");
        assert!(start_receiver
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err());

        blocker.release();
        assert_eq!(
            start_receiver
                .recv_timeout(std::time::Duration::from_millis(100))
                .unwrap(),
            "/dev/ttyACM0"
        );

        let first_response = first_client.join().unwrap();
        let second_response = second_client.join().unwrap();
        server_thread.join().unwrap().unwrap();

        assert!(matches!(first_response, DaemonResponse::Success { .. }));
        assert!(matches!(second_response, DaemonResponse::Success { .. }));
    }

    #[test]
    fn different_devices_progress_independently_through_the_live_daemon() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        let transport_factory = BlockingTransportFactory::default();
        let state = Arc::clone(&transport_factory.state);
        let blocker_a = state.install_blocker("/dev/ttyACM0");
        let blocker_b = state.install_blocker("/dev/ttyACM1");
        let (start_sender, start_receiver) = mpsc::channel();
        state.set_start_sender(start_sender);

        let server = DaemonServer::bind_with_handler(
            &socket_path,
            ScreenDaemonRequestHandler::with_idle_timeout(
                StaticDiscovery {
                    devices: vec![test_device("/dev/ttyACM0"), test_device("/dev/ttyACM1")],
                    error: None,
                },
                transport_factory,
                std::time::Duration::from_secs(60),
            ),
        )
        .unwrap();
        let server_thread = thread::spawn(move || server.serve_count(2));

        let first_client = spawn_screen_raw_client(&socket_path, "/dev/ttyACM0");
        let second_client = spawn_screen_raw_client(&socket_path, "/dev/ttyACM1");

        let first_start = start_receiver
            .recv_timeout(std::time::Duration::from_millis(100))
            .unwrap();
        let second_start = start_receiver
            .recv_timeout(std::time::Duration::from_millis(100))
            .unwrap();
        let mut starts = vec![first_start, second_start];
        starts.sort();

        assert_eq!(
            starts,
            vec!["/dev/ttyACM0".to_string(), "/dev/ttyACM1".to_string()]
        );

        blocker_a.release();
        blocker_b.release();

        let first_response = first_client.join().unwrap();
        let second_response = second_client.join().unwrap();
        server_thread.join().unwrap().unwrap();

        assert!(matches!(first_response, DaemonResponse::Success { .. }));
        assert!(matches!(second_response, DaemonResponse::Success { .. }));
    }

    fn spawn_screen_raw_client(
        socket_path: &std::path::Path,
        device_path: &str,
    ) -> thread::JoinHandle<DaemonResponse> {
        let socket_path = socket_path.to_path_buf();
        let device_path = device_path.to_string();

        thread::spawn(move || {
            let mut client = UnixStream::connect(&socket_path).unwrap();
            let request = encode_request(
                &DaemonRequest::for_command(CommandRequest::Screen(ScreenRequest::Raw {
                    lua: "return 1".to_string(),
                    target: TargetArgs {
                        device: Some(device_path),
                        dx: None,
                        dy: None,
                    },
                }))
                .unwrap(),
            )
            .unwrap();
            client.write_all(&request).unwrap();
            client.shutdown(Shutdown::Write).unwrap();

            let mut response_bytes = Vec::new();
            client.read_to_end(&mut response_bytes).unwrap();
            decode_response(&response_bytes).unwrap()
        })
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
