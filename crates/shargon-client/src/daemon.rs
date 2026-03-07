use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use hyper_util::rt::TokioIo;
use shargon_protocol::{
    SOCKET_PATH,
    vm_service::{PingRequest, vm_service_client::VmServiceClient},
};
use tokio::{net::UnixStream, time::sleep};
use tonic::{
    Code, Status,
    transport::{Channel, Endpoint, Uri},
};
use tower::service_fn;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const CONNECTOR_ENDPOINT: &str = "http://[::]:50051";

#[derive(Clone, Debug)]
struct DaemonBootstrapConfig {
    socket_path: PathBuf,
    readiness_timeout: Duration,
    retry_interval: Duration,
}

impl Default for DaemonBootstrapConfig {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from(SOCKET_PATH),
            readiness_timeout: Duration::from_secs(3),
            retry_interval: Duration::from_millis(50),
        }
    }
}

enum ProbeDaemonError {
    Unavailable(anyhow::Error),
    Other(anyhow::Error),
}

pub async fn connect_or_start_vm_service() -> anyhow::Result<VmServiceClient<Channel>> {
    let daemon_path = resolve_daemon_executable()?;
    let config = DaemonBootstrapConfig::default();

    connect_or_start_vm_service_with_launcher(&config, daemon_path.as_path(), || {
        spawn_daemon_process(&daemon_path)
    })
    .await
}

async fn connect_or_start_vm_service_with_launcher<L>(
    config: &DaemonBootstrapConfig,
    daemon_path: &Path,
    mut spawn_daemon: L,
) -> anyhow::Result<VmServiceClient<Channel>>
where
    L: FnMut() -> anyhow::Result<()>,
{
    match try_connect_vm_service(&config.socket_path).await {
        Ok(client) => return Ok(client),
        Err(ProbeDaemonError::Unavailable(_)) => {}
        Err(ProbeDaemonError::Other(err)) => return Err(err),
    }

    spawn_daemon()
        .with_context(|| format!("failed to spawn daemon from {}", daemon_path.display()))?;

    wait_for_daemon_ready(config, daemon_path).await
}

async fn wait_for_daemon_ready(
    config: &DaemonBootstrapConfig,
    daemon_path: &Path,
) -> anyhow::Result<VmServiceClient<Channel>> {
    let deadline = Instant::now() + config.readiness_timeout;

    loop {
        let unavailable_err = match try_connect_vm_service(&config.socket_path).await {
            Ok(client) => return Ok(client),
            Err(ProbeDaemonError::Unavailable(err)) => err,
            Err(ProbeDaemonError::Other(err)) => return Err(err),
        };

        if Instant::now() >= deadline {
            bail!(
                "timed out waiting for daemon to become ready from {} after {:?}. Start it manually with: shargon-daemon run.{}",
                daemon_path.display(),
                config.readiness_timeout,
                format!(" Last error: {unavailable_err:#}"),
            );
        }

        sleep(config.retry_interval).await;
    }
}

async fn try_connect_vm_service(
    socket_path: &Path,
) -> Result<VmServiceClient<Channel>, ProbeDaemonError> {
    match connect_vm_service(socket_path).await {
        Ok(client) => Ok(client),
        Err(err) if is_daemon_unavailable(&err) => Err(ProbeDaemonError::Unavailable(err)),
        Err(err) => Err(ProbeDaemonError::Other(err)),
    }
}

async fn connect_vm_service(socket_path: &Path) -> anyhow::Result<VmServiceClient<Channel>> {
    let socket_path = socket_path.to_path_buf();
    let channel = Endpoint::try_from(CONNECTOR_ENDPOINT)?
        .connect_with_connector(service_fn(move |_: Uri| {
            let socket_path = socket_path.clone();
            async move {
                Ok::<_, std::io::Error>(TokioIo::new(UnixStream::connect(&socket_path).await?))
            }
        }))
        .await?;

    let mut client = VmServiceClient::new(channel);
    client
        .ping(PingRequest {})
        .await
        .context("daemon ping failed")?;

    Ok(client)
}

fn is_daemon_unavailable(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_error| {
                matches!(
                    io_error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                )
            })
    }) || err.chain().any(|cause| {
        cause
            .downcast_ref::<Status>()
            .is_some_and(|status| status.code() == Code::Unavailable)
    })
}

fn resolve_daemon_executable() -> anyhow::Result<PathBuf> {
    resolve_daemon_executable_from(
        std::env::current_exe().context("failed to locate current executable")?,
        std::env::var_os("PATH").as_deref(),
    )
}

fn resolve_daemon_executable_from(
    current_exe: impl AsRef<Path>,
    path_env: Option<&OsStr>,
) -> anyhow::Result<PathBuf> {
    let current_exe = current_exe.as_ref();

    if let Some(parent_dir) = current_exe.parent() {
        let sibling_path = parent_dir.join("shargon-daemon");
        if sibling_path.is_file() {
            return Ok(sibling_path);
        }
    }

    if let Some(path_env) = path_env {
        for path_dir in std::env::split_paths(path_env) {
            let candidate = path_dir.join("shargon-daemon");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    bail!(
        "unable to locate shargon-daemon next to {} or in PATH",
        current_exe.display()
    )
}

fn spawn_daemon_process(daemon_path: &Path) -> anyhow::Result<()> {
    let mut command = Command::new(daemon_path);
    command
        .arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }

            Ok(())
        });
    }

    let _child = command.spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use shargon_protocol::vm_service::{
        PingRequest, PingResponse,
        vm_service_server::{VmService, VmServiceServer},
    };
    use tempfile::tempdir;
    use tokio::{runtime::Handle, sync::oneshot};
    use tokio_stream::wrappers::UnixListenerStream;
    use tonic::{Request, Response, transport::Server};

    use super::*;

    #[derive(Default)]
    struct TestVmService;

    #[tonic::async_trait]
    impl VmService for TestVmService {
        async fn ping(
            &self,
            _request: Request<PingRequest>,
        ) -> Result<Response<PingResponse>, tonic::Status> {
            Ok(Response::new(PingResponse {
                msg: "pong".to_string(),
            }))
        }
    }

    #[test]
    fn resolves_sibling_daemon_before_path() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let sibling_dir = temp_dir.path().join("bin");
        let path_dir = temp_dir.path().join("path-bin");
        fs::create_dir_all(&sibling_dir)?;
        fs::create_dir_all(&path_dir)?;

        let current_exe = sibling_dir.join("shargon-client");
        let sibling_daemon = sibling_dir.join("shargon-daemon");
        let path_daemon = path_dir.join("shargon-daemon");

        touch_executable(&current_exe)?;
        touch_executable(&sibling_daemon)?;
        touch_executable(&path_daemon)?;

        let resolved = resolve_daemon_executable_from(
            &current_exe,
            Some(OsStr::new(path_dir.to_str().context("non-utf8 temp path")?)),
        )?;

        assert_eq!(resolved, sibling_daemon);
        Ok(())
    }

    #[test]
    fn resolves_daemon_from_path_when_sibling_missing() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let current_dir = temp_dir.path().join("bin");
        let path_dir = temp_dir.path().join("path-bin");
        fs::create_dir_all(&current_dir)?;
        fs::create_dir_all(&path_dir)?;

        let current_exe = current_dir.join("shargon-client");
        let path_daemon = path_dir.join("shargon-daemon");

        touch_executable(&current_exe)?;
        touch_executable(&path_daemon)?;

        let resolved = resolve_daemon_executable_from(
            &current_exe,
            Some(OsStr::new(path_dir.to_str().context("non-utf8 temp path")?)),
        )?;

        assert_eq!(resolved, path_daemon);
        Ok(())
    }

    #[test]
    fn errors_when_daemon_cannot_be_resolved() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let current_dir = temp_dir.path().join("bin");
        fs::create_dir_all(&current_dir)?;
        let current_exe = current_dir.join("shargon-client");
        touch_executable(&current_exe)?;

        let err = resolve_daemon_executable_from(&current_exe, None).unwrap_err();
        assert!(format!("{err:#}").contains("unable to locate shargon-daemon"));
        Ok(())
    }

    #[test]
    fn connect_or_start_reuses_running_daemon() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;

        runtime.block_on(async {
            let temp_dir = tempdir()?;
            let socket_path = temp_dir.path().join("daemon.sock");
            let config = test_config(&socket_path);

            let (shutdown_tx, shutdown_rx) = oneshot::channel();
            let server_handle = tokio::spawn(run_test_server(socket_path.clone(), shutdown_rx));
            wait_for_test_server(&socket_path).await?;

            let spawn_count = AtomicUsize::new(0);
            let _client = connect_or_start_vm_service_with_launcher(
                &config,
                Path::new("/test/shargon-daemon"),
                || {
                    spawn_count.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await?;

            assert_eq!(spawn_count.load(Ordering::SeqCst), 0);

            let _ = shutdown_tx.send(());
            server_handle.await??;
            Ok(())
        })
    }

    #[test]
    fn connect_or_start_spawns_once_and_waits_for_readiness() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;

        runtime.block_on(async {
            let temp_dir = tempdir()?;
            let socket_path = temp_dir.path().join("daemon.sock");
            let config = test_config(&socket_path);

            let spawn_count = Arc::new(AtomicUsize::new(0));
            let server_shutdown = Arc::new(Mutex::new(None));
            let server_handle = Arc::new(Mutex::new(None));
            let handle = Handle::current();

            let _client = connect_or_start_vm_service_with_launcher(
                &config,
                Path::new("/test/shargon-daemon"),
                {
                    let socket_path = socket_path.clone();
                    let spawn_count = Arc::clone(&spawn_count);
                    let server_shutdown = Arc::clone(&server_shutdown);
                    let server_handle = Arc::clone(&server_handle);

                    move || {
                        spawn_count.fetch_add(1, Ordering::SeqCst);

                        let (shutdown_tx, shutdown_rx) = oneshot::channel();
                        let join_handle =
                            handle.spawn(run_test_server(socket_path.clone(), shutdown_rx));

                        *server_shutdown.lock().expect("shutdown mutex poisoned") =
                            Some(shutdown_tx);
                        *server_handle.lock().expect("join mutex poisoned") = Some(join_handle);
                        Ok(())
                    }
                },
            )
            .await?;

            assert_eq!(spawn_count.load(Ordering::SeqCst), 1);

            let shutdown_tx = server_shutdown
                .lock()
                .expect("shutdown mutex poisoned")
                .take()
                .expect("server shutdown sender missing");
            let join_handle = server_handle
                .lock()
                .expect("join mutex poisoned")
                .take()
                .expect("server join handle missing");

            let _ = shutdown_tx.send(());
            join_handle.await??;
            Ok(())
        })
    }

    #[test]
    fn connect_or_start_times_out_with_actionable_error() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;

        runtime.block_on(async {
            let temp_dir = tempdir()?;
            let socket_path = temp_dir.path().join("daemon.sock");
            let config = DaemonBootstrapConfig {
                socket_path,
                readiness_timeout: Duration::from_millis(100),
                retry_interval: Duration::from_millis(10),
            };
            let spawn_count = AtomicUsize::new(0);

            let err = connect_or_start_vm_service_with_launcher(
                &config,
                Path::new("/test/shargon-daemon"),
                || {
                    spawn_count.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await
            .unwrap_err();

            assert_eq!(spawn_count.load(Ordering::SeqCst), 1);
            assert!(format!("{err:#}").contains("shargon-daemon run"));
            Ok(())
        })
    }

    fn touch_executable(path: &Path) -> anyhow::Result<()> {
        fs::write(path, [])?;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
        Ok(())
    }

    fn test_config(socket_path: &Path) -> DaemonBootstrapConfig {
        DaemonBootstrapConfig {
            socket_path: socket_path.to_path_buf(),
            readiness_timeout: Duration::from_secs(1),
            retry_interval: Duration::from_millis(25),
        }
    }

    async fn wait_for_test_server(socket_path: &Path) -> anyhow::Result<()> {
        let config = test_config(socket_path);
        let _client = wait_for_daemon_ready(&config, Path::new("/test/shargon-daemon")).await?;
        Ok(())
    }

    async fn run_test_server(
        socket_path: PathBuf,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> anyhow::Result<()> {
        let listener = tokio::net::UnixListener::bind(&socket_path)?;
        let incoming = UnixListenerStream::new(listener);

        Server::builder()
            .add_service(VmServiceServer::new(TestVmService))
            .serve_with_incoming_shutdown(incoming, async {
                let _ = shutdown_rx.await;
            })
            .await?;

        Ok(())
    }
}
