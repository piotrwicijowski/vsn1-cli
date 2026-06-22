use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use crate::daemon_protocol::{decode_request, encode_response, DaemonRequest, DaemonResponse};

pub const EXECUTE_NOT_IMPLEMENTED_MESSAGE: &str =
    "daemon command execution path is not implemented yet";

pub trait DaemonRequestHandler: Send + Sync + 'static {
    fn handle(&self, request: DaemonRequest) -> DaemonResponse;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PlaceholderDaemonRequestHandler;

pub struct DaemonServer<H = PlaceholderDaemonRequestHandler> {
    listener: UnixListener,
    socket_path: PathBuf,
    handler: Arc<H>,
}

impl<H> fmt::Debug for DaemonServer<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DaemonServer")
            .field("socket_path", &self.socket_path)
            .finish_non_exhaustive()
    }
}

impl DaemonRequestHandler for PlaceholderDaemonRequestHandler {
    fn handle(&self, request: DaemonRequest) -> DaemonResponse {
        match request {
            DaemonRequest::Ping => DaemonResponse::Pong,
            DaemonRequest::Execute(_) => DaemonResponse::Error {
                message: EXECUTE_NOT_IMPLEMENTED_MESSAGE.to_string(),
            },
        }
    }
}

impl DaemonServer<PlaceholderDaemonRequestHandler> {
    pub fn bind(socket_path: &Path) -> io::Result<Self> {
        Self::bind_with_handler(socket_path, PlaceholderDaemonRequestHandler)
    }
}

impl<H> DaemonServer<H>
where
    H: DaemonRequestHandler,
{
    pub fn bind_with_handler(socket_path: &Path, handler: H) -> io::Result<Self> {
        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent)?;
        }

        match fs::metadata(socket_path) {
            Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(socket_path)?,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "refusing to replace non-socket path {}",
                        socket_path.display()
                    ),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        let listener = UnixListener::bind(socket_path)?;

        Ok(Self {
            listener,
            socket_path: socket_path.to_path_buf(),
            handler: Arc::new(handler),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn serve_forever(&self) -> io::Result<()> {
        loop {
            let (stream, _) = self.listener.accept()?;
            let handler = Arc::clone(&self.handler);

            thread::spawn(move || {
                let _ = Self::serve_stream_with_handler(handler, stream);
            });
        }
    }

    pub fn serve_one(&self) -> io::Result<()> {
        let (mut stream, _) = self.listener.accept()?;
        self.serve_stream(&mut stream)
    }

    fn serve_stream(&self, stream: &mut UnixStream) -> io::Result<()> {
        Self::serve_stream_with_handler(Arc::clone(&self.handler), stream.try_clone()?)
    }

    #[cfg(test)]
    pub fn serve_count(&self, count: usize) -> io::Result<()> {
        let mut threads = Vec::with_capacity(count);

        for _ in 0..count {
            let (stream, _) = self.listener.accept()?;
            let handler = Arc::clone(&self.handler);
            threads.push(thread::spawn(move || {
                Self::serve_stream_with_handler(handler, stream)
            }));
        }

        for thread in threads {
            thread
                .join()
                .map_err(|_| io::Error::other("daemon connection worker panicked"))??;
        }

        Ok(())
    }

    fn serve_stream_with_handler(handler: Arc<H>, mut stream: UnixStream) -> io::Result<()> {
        let mut request_bytes = Vec::new();
        stream.read_to_end(&mut request_bytes)?;

        let response = match decode_request(&request_bytes) {
            Ok(request) => handler.handle(request),
            Err(error) => DaemonResponse::Error {
                message: error.to_string(),
            },
        };
        let response_bytes = encode_response(&response)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;

        stream.write_all(&response_bytes)?;
        stream.flush()?;
        Ok(())
    }
}

impl<H> Drop for DaemonServer<H> {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Shutdown;
    use std::thread;

    use tempfile::tempdir;

    use crate::command_model::{CommandRequest, RuntimeRequest};
    use crate::daemon_protocol::{decode_response, encode_request};
    use crate::TargetArgs;

    #[test]
    fn bind_creates_parent_directories_and_responds_to_ping() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("runtime/vsn1-cli/daemon.sock");
        let server = DaemonServer::bind(&socket_path).unwrap();

        assert!(socket_path.parent().unwrap().is_dir());
        assert_eq!(server.socket_path(), socket_path.as_path());

        let server_thread = thread::spawn(move || server.serve_one());

        let mut client = UnixStream::connect(&socket_path).unwrap();
        let request = encode_request(&DaemonRequest::Ping).unwrap();
        client.write_all(&request).unwrap();
        client.shutdown(Shutdown::Write).unwrap();

        let mut response_bytes = Vec::new();
        client.read_to_end(&mut response_bytes).unwrap();

        server_thread.join().unwrap().unwrap();

        let response = decode_response(&response_bytes).unwrap();
        assert_eq!(response, DaemonResponse::Pong);
    }

    #[test]
    fn execute_requests_receive_a_placeholder_error_response() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        let server = DaemonServer::bind(&socket_path).unwrap();

        let server_thread = thread::spawn(move || server.serve_one());

        let mut client = UnixStream::connect(&socket_path).unwrap();
        let request = encode_request(
            &DaemonRequest::for_command(CommandRequest::Runtime(RuntimeRequest::Verify {
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
            DaemonResponse::Error {
                message: EXECUTE_NOT_IMPLEMENTED_MESSAGE.to_string(),
            }
        );
    }

    #[test]
    fn bind_rejects_existing_non_socket_paths() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("daemon.sock");
        fs::write(&socket_path, b"occupied").unwrap();

        let error = DaemonServer::bind(&socket_path).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(error
            .to_string()
            .contains("refusing to replace non-socket path"));
    }
}
