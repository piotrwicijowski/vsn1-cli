use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::transport::{SerialTransport, SerialTransportFactory, TransportError};

pub const DEVICE_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

type WorkerResult<T> = std::result::Result<T, TransportError>;

trait DeviceWorkerOperation<T>: Send {
    fn run(self: Box<Self>, transport: &mut T) -> WorkerResult<()>;
}

impl<T, F> DeviceWorkerOperation<T> for F
where
    F: FnOnce(&mut T) -> WorkerResult<()> + Send + 'static,
{
    fn run(self: Box<Self>, transport: &mut T) -> WorkerResult<()> {
        (*self)(transport)
    }
}

pub struct DeviceSessionRegistry<F>
where
    F: SerialTransportFactory + Send + 'static,
    F::Transport: Send + 'static,
{
    inner: Arc<DeviceSessionRegistryInner<F>>,
}

impl<F> Clone for DeviceSessionRegistry<F>
where
    F: SerialTransportFactory + Send + 'static,
    F::Transport: Send + 'static,
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct DeviceSessionRegistryInner<F>
where
    F: SerialTransportFactory + Send + 'static,
    F::Transport: Send + 'static,
{
    transport_factory: Arc<Mutex<F>>,
    idle_timeout: Duration,
    workers: Mutex<HashMap<String, DeviceWorkerHandle>>,
}

#[derive(Clone)]
struct DeviceWorkerHandle {
    sender: mpsc::Sender<DeviceWorkerCommand>,
}

enum DeviceWorkerCommand {
    EnsureOpen {
        reply: mpsc::Sender<WorkerResult<()>>,
    },
    WriteImmediate {
        packet: Vec<u8>,
        reply: mpsc::Sender<WorkerResult<()>>,
    },
    RunOperation {
        operation: Box<dyn DeviceWorkerOperation<Box<dyn SerialTransport + Send>>>,
        reply: mpsc::Sender<WorkerResult<()>>,
    },
}

impl<F> DeviceSessionRegistry<F>
where
    F: SerialTransportFactory + Send + 'static,
    F::Transport: Send + 'static,
{
    pub fn new(transport_factory: F, idle_timeout: Duration) -> Self {
        Self {
            inner: Arc::new(DeviceSessionRegistryInner {
                transport_factory: Arc::new(Mutex::new(transport_factory)),
                idle_timeout,
                workers: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn ensure_open(&self, port_name: &str, baud_rate: u32) -> WorkerResult<()> {
        let (reply_sender, reply_receiver) = mpsc::channel();
        self.worker_handle(port_name, baud_rate)
            .sender
            .send(DeviceWorkerCommand::EnsureOpen {
                reply: reply_sender,
            })
            .map_err(worker_disconnected_error)?;

        reply_receiver
            .recv()
            .map_err(|_| worker_disconnected_error(()))?
    }

    pub fn write_immediate(
        &self,
        port_name: &str,
        baud_rate: u32,
        packet: Vec<u8>,
    ) -> WorkerResult<()> {
        let (reply_sender, reply_receiver) = mpsc::channel();
        self.worker_handle(port_name, baud_rate)
            .sender
            .send(DeviceWorkerCommand::WriteImmediate {
                packet,
                reply: reply_sender,
            })
            .map_err(worker_disconnected_error)?;

        reply_receiver
            .recv()
            .map_err(|_| worker_disconnected_error(()))?
    }

    pub fn with_transport<R, E, O>(
        &self,
        port_name: &str,
        baud_rate: u32,
        operation: O,
    ) -> std::result::Result<R, E>
    where
        R: Send + 'static,
        E: From<TransportError> + Send + 'static,
        O: FnOnce(&mut dyn SerialTransport) -> std::result::Result<R, E> + Send + 'static,
    {
        let (result_sender, result_receiver) = mpsc::channel();
        let (reply_sender, reply_receiver) = mpsc::channel();

        let wrapped_operation = move |transport: &mut Box<dyn SerialTransport + Send>| {
            let result = operation(transport.as_mut());
            let _ = result_sender.send(result);
            Ok(())
        };

        self.worker_handle(port_name, baud_rate)
            .sender
            .send(DeviceWorkerCommand::RunOperation {
                operation: Box::new(wrapped_operation),
                reply: reply_sender,
            })
            .map_err(worker_disconnected_error)?;

        reply_receiver
            .recv()
            .map_err(|_| E::from(worker_disconnected_error(())))?
            .map_err(E::from)?;

        result_receiver
            .recv()
            .map_err(|_| E::from(worker_disconnected_error(())))?
    }

    fn worker_handle(&self, port_name: &str, baud_rate: u32) -> DeviceWorkerHandle {
        let mut workers = self.inner.workers.lock().unwrap();
        workers
            .entry(port_name.to_string())
            .or_insert_with(|| {
                spawn_device_worker(
                    Arc::clone(&self.inner.transport_factory),
                    port_name.to_string(),
                    baud_rate,
                    self.inner.idle_timeout,
                )
            })
            .clone()
    }
}

fn spawn_device_worker<F>(
    transport_factory: Arc<Mutex<F>>,
    port_name: String,
    baud_rate: u32,
    idle_timeout: Duration,
) -> DeviceWorkerHandle
where
    F: SerialTransportFactory + Send + 'static,
    F::Transport: Send + 'static,
{
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let mut transport: Option<Box<dyn SerialTransport + Send>> = None;

        loop {
            let command = if transport.is_some() {
                match receiver.recv_timeout(idle_timeout) {
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        transport = None;
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            } else {
                match receiver.recv() {
                    Ok(command) => command,
                    Err(_) => break,
                }
            };

            let reply = command_reply(&command).clone();

            let result =
                match ensure_transport(&mut transport, &transport_factory, &port_name, baud_rate) {
                    Ok(()) => run_command(command, transport.as_mut().unwrap()),
                    Err(error) => Err(error),
                };

            send_result(&reply, result);
        }
    });

    DeviceWorkerHandle { sender }
}

fn ensure_transport<F>(
    transport: &mut Option<Box<dyn SerialTransport + Send>>,
    transport_factory: &Arc<Mutex<F>>,
    port_name: &str,
    baud_rate: u32,
) -> WorkerResult<()>
where
    F: SerialTransportFactory,
    F::Transport: Send + 'static,
{
    if transport.is_none() {
        let mut transport_factory = transport_factory.lock().unwrap();
        *transport = Some(Box::new(transport_factory.open(port_name, baud_rate)?));
    }

    Ok(())
}

fn run_command(
    command: DeviceWorkerCommand,
    transport: &mut Box<dyn SerialTransport + Send>,
) -> WorkerResult<()> {
    match command {
        DeviceWorkerCommand::EnsureOpen { .. } => Ok(()),
        DeviceWorkerCommand::WriteImmediate { packet, .. } => transport.write_immediate(&packet),
        DeviceWorkerCommand::RunOperation { operation, .. } => operation.run(transport),
    }
}

fn command_reply(command: &DeviceWorkerCommand) -> &mpsc::Sender<WorkerResult<()>> {
    match command {
        DeviceWorkerCommand::EnsureOpen { reply } => reply,
        DeviceWorkerCommand::WriteImmediate { reply, .. } => reply,
        DeviceWorkerCommand::RunOperation { reply, .. } => reply,
    }
}

fn send_result(reply: &mpsc::Sender<WorkerResult<()>>, result: WorkerResult<()>) {
    let _ = reply.send(result);
}

fn worker_disconnected_error<T>(_value: T) -> TransportError {
    TransportError::open("daemon device worker disconnected")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Condvar, Mutex as StdMutex};
    use std::time::{Duration, Instant};

    struct BlockingTransport {
        port_name: String,
        state: Arc<TestTransportState>,
    }

    #[derive(Default)]
    struct BlockingTransportFactory {
        state: Arc<TestTransportState>,
    }

    #[derive(Default)]
    struct TestTransportState {
        open_counts: StdMutex<HashMap<String, usize>>,
        active_handles: StdMutex<HashMap<String, usize>>,
        blockers: StdMutex<HashMap<String, Arc<WriteBlocker>>>,
        start_sender: StdMutex<Option<mpsc::Sender<String>>>,
    }

    struct WriteBlocker {
        released: AtomicBool,
        mutex: StdMutex<()>,
        condvar: Condvar,
    }

    impl WriteBlocker {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                released: AtomicBool::new(false),
                mutex: StdMutex::new(()),
                condvar: Condvar::new(),
            })
        }

        fn release(&self) {
            self.released.store(true, Ordering::SeqCst);
            self.condvar.notify_all();
        }

        fn wait(&self) {
            let mut guard = self.mutex.lock().unwrap();
            while !self.released.load(Ordering::SeqCst) {
                guard = self.condvar.wait(guard).unwrap();
            }
        }
    }

    impl TestTransportState {
        fn open_count(&self, port_name: &str) -> usize {
            *self
                .open_counts
                .lock()
                .unwrap()
                .get(port_name)
                .unwrap_or(&0)
        }

        fn active_handles(&self, port_name: &str) -> usize {
            *self
                .active_handles
                .lock()
                .unwrap()
                .get(port_name)
                .unwrap_or(&0)
        }

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
            *self
                .state
                .open_counts
                .lock()
                .unwrap()
                .entry(port_name.to_string())
                .or_default() += 1;
            *self
                .state
                .active_handles
                .lock()
                .unwrap()
                .entry(port_name.to_string())
                .or_default() += 1;

            Ok(BlockingTransport {
                port_name: port_name.to_string(),
                state: Arc::clone(&self.state),
            })
        }
    }

    impl Drop for BlockingTransport {
        fn drop(&mut self) {
            let mut active_handles = self.state.active_handles.lock().unwrap();
            let entry = active_handles.entry(self.port_name.clone()).or_default();
            *entry -= 1;
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
    fn reuses_the_same_open_transport_within_the_idle_window() {
        let factory = BlockingTransportFactory::default();
        let state = Arc::clone(&factory.state);
        let registry = DeviceSessionRegistry::new(factory, Duration::from_millis(100));

        registry.ensure_open("/dev/ttyACM0", 2_000_000).unwrap();
        registry.ensure_open("/dev/ttyACM0", 2_000_000).unwrap();

        assert_eq!(state.open_count("/dev/ttyACM0"), 1);
        assert_eq!(state.active_handles("/dev/ttyACM0"), 1);
    }

    #[test]
    fn closes_after_idle_timeout_and_reopens_on_the_next_request() {
        let factory = BlockingTransportFactory::default();
        let state = Arc::clone(&factory.state);
        let registry = DeviceSessionRegistry::new(factory, Duration::from_millis(40));

        registry.ensure_open("/dev/ttyACM0", 2_000_000).unwrap();
        wait_until(Duration::from_millis(200), || {
            state.active_handles("/dev/ttyACM0") == 0
        });
        registry.ensure_open("/dev/ttyACM0", 2_000_000).unwrap();

        assert_eq!(state.open_count("/dev/ttyACM0"), 2);
        assert_eq!(state.active_handles("/dev/ttyACM0"), 1);
    }

    #[test]
    fn serializes_same_device_requests() {
        let factory = BlockingTransportFactory::default();
        let state = Arc::clone(&factory.state);
        let registry = DeviceSessionRegistry::new(factory, Duration::from_secs(1));
        let blocker = state.install_blocker("/dev/ttyACM0");
        let (start_sender, start_receiver) = mpsc::channel();
        state.set_start_sender(start_sender);

        let first_registry = registry.clone();
        let first = thread::spawn(move || {
            first_registry
                .write_immediate("/dev/ttyACM0", 2_000_000, vec![1])
                .unwrap();
        });

        assert_eq!(
            start_receiver
                .recv_timeout(Duration::from_millis(100))
                .unwrap(),
            "/dev/ttyACM0"
        );

        let second_registry = registry.clone();
        let second = thread::spawn(move || {
            second_registry
                .write_immediate("/dev/ttyACM0", 2_000_000, vec![2])
                .unwrap();
        });

        assert!(start_receiver
            .recv_timeout(Duration::from_millis(50))
            .is_err());

        blocker.release();
        assert_eq!(
            start_receiver
                .recv_timeout(Duration::from_millis(100))
                .unwrap(),
            "/dev/ttyACM0"
        );

        first.join().unwrap();
        second.join().unwrap();
    }

    #[test]
    fn allows_different_devices_to_progress_independently() {
        let factory = BlockingTransportFactory::default();
        let state = Arc::clone(&factory.state);
        let registry = DeviceSessionRegistry::new(factory, Duration::from_secs(1));
        let blocker_a = state.install_blocker("/dev/ttyACM0");
        let blocker_b = state.install_blocker("/dev/ttyACM1");
        let (start_sender, start_receiver) = mpsc::channel();
        state.set_start_sender(start_sender);

        let first_registry = registry.clone();
        let first = thread::spawn(move || {
            first_registry
                .write_immediate("/dev/ttyACM0", 2_000_000, vec![1])
                .unwrap();
        });

        assert_eq!(
            start_receiver
                .recv_timeout(Duration::from_millis(100))
                .unwrap(),
            "/dev/ttyACM0"
        );

        let second_registry = registry.clone();
        let second = thread::spawn(move || {
            second_registry
                .write_immediate("/dev/ttyACM1", 2_000_000, vec![2])
                .unwrap();
        });

        let second_start = start_receiver
            .recv_timeout(Duration::from_millis(100))
            .unwrap();
        assert_eq!(second_start, "/dev/ttyACM1");

        blocker_a.release();
        blocker_b.release();

        first.join().unwrap();
        second.join().unwrap();
    }

    fn wait_until(timeout: Duration, predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if predicate() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }

        assert!(predicate(), "predicate did not become true before timeout");
    }
}
