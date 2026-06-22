use std::env;
use std::error::Error as StdError;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

pub const DAEMON_SOCKET_OVERRIDE_ENV: &str = "VSN1_DAEMON_SOCKET";
pub const DAEMON_SOCKET_DIR_NAME: &str = "vsn1-cli";
pub const DAEMON_SOCKET_FILE_NAME: &str = "daemon.sock";

const TMPDIR_ENV: &str = "TMPDIR";
const XDG_RUNTIME_DIR_ENV: &str = "XDG_RUNTIME_DIR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonSocketPathError {
    MissingEnvironment { var_name: &'static str },
    UnsupportedPlatform { os: &'static str },
}

pub type Result<T> = std::result::Result<T, DaemonSocketPathError>;

impl fmt::Display for DaemonSocketPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvironment { var_name } => write!(
                f,
                "could not resolve daemon socket path because environment variable {var_name} is not set"
            ),
            Self::UnsupportedPlatform { os } => {
                write!(f, "daemon socket paths are not supported on target OS `{os}`")
            }
        }
    }
}

impl StdError for DaemonSocketPathError {}

pub fn resolve_daemon_socket_path() -> Result<PathBuf> {
    resolve_daemon_socket_path_from_env(
        env::var_os(DAEMON_SOCKET_OVERRIDE_ENV).as_deref(),
        env::var_os(XDG_RUNTIME_DIR_ENV).as_deref(),
        env::var_os(TMPDIR_ENV).as_deref(),
    )
}

pub fn resolve_daemon_socket_path_from_env(
    override_path: Option<&OsStr>,
    xdg_runtime_dir_value: Option<&OsStr>,
    tmpdir_value: Option<&OsStr>,
) -> Result<PathBuf> {
    if let Some(override_path) = override_path {
        return Ok(PathBuf::from(override_path));
    }

    #[cfg(target_os = "linux")]
    {
        let _ = tmpdir_value;
        let runtime_dir =
            xdg_runtime_dir_value.ok_or(DaemonSocketPathError::MissingEnvironment {
                var_name: XDG_RUNTIME_DIR_ENV,
            })?;
        return Ok(build_socket_path(Path::new(runtime_dir)));
    }

    #[cfg(target_os = "macos")]
    {
        let _ = xdg_runtime_dir_value;
        let tmpdir = tmpdir_value.ok_or(DaemonSocketPathError::MissingEnvironment {
            var_name: TMPDIR_ENV,
        })?;
        return Ok(build_socket_path(Path::new(tmpdir)));
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = xdg_runtime_dir_value;
        let _ = tmpdir_value;
        Err(DaemonSocketPathError::UnsupportedPlatform {
            os: env::consts::OS,
        })
    }
}

fn build_socket_path(root: &Path) -> PathBuf {
    root.join(DAEMON_SOCKET_DIR_NAME)
        .join(DAEMON_SOCKET_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_path_wins_over_platform_defaults() {
        let path = resolve_daemon_socket_path_from_env(
            Some(OsStr::new("/tmp/custom daemon.sock")),
            Some(OsStr::new("/run/user/1000")),
            Some(OsStr::new("/var/folders/example/T")),
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("/tmp/custom daemon.sock"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_resolution_uses_xdg_runtime_dir() {
        let path = resolve_daemon_socket_path_from_env(
            None,
            Some(OsStr::new("/run/user/1000")),
            Some(OsStr::new("/ignored")),
        )
        .unwrap();

        assert_eq!(
            path,
            PathBuf::from("/run/user/1000")
                .join(DAEMON_SOCKET_DIR_NAME)
                .join(DAEMON_SOCKET_FILE_NAME)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_resolution_requires_xdg_runtime_dir() {
        let error = resolve_daemon_socket_path_from_env(None, None, None).unwrap_err();

        assert_eq!(
            error,
            DaemonSocketPathError::MissingEnvironment {
                var_name: XDG_RUNTIME_DIR_ENV,
            }
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_resolution_uses_tmpdir() {
        let path = resolve_daemon_socket_path_from_env(
            None,
            Some(OsStr::new("/ignored")),
            Some(OsStr::new("/var/folders/example/T/")),
        )
        .unwrap();

        assert_eq!(
            path,
            PathBuf::from("/var/folders/example/T/")
                .join(DAEMON_SOCKET_DIR_NAME)
                .join(DAEMON_SOCKET_FILE_NAME)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_resolution_requires_tmpdir() {
        let error = resolve_daemon_socket_path_from_env(None, None, None).unwrap_err();

        assert_eq!(
            error,
            DaemonSocketPathError::MissingEnvironment {
                var_name: TMPDIR_ENV,
            }
        );
    }
}
