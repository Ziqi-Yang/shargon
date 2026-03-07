use std::{
    io::ErrorKind,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use shargon_protocol::{SOCKET_PATH, vm_service::vm_service_server::VmServiceServer};
use tokio::{
    net::{UnixListener, UnixStream},
    time::sleep,
};
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;

use crate::rpc::VmServiceImpl;

use super::CliCommand;

pub struct CliRunCommand {}

impl CliRunCommand {
    pub fn new() -> Self {
        Self {}
    }
}

impl CliCommand for CliRunCommand {
    fn execute(&self) -> anyhow::Result<()> {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            let listener = create_daemon_listener(Path::new(SOCKET_PATH)).await?;
            let incoming = UnixListenerStream::new(listener);

            tracing::info!("listening on {}", SOCKET_PATH);

            Server::builder()
                .add_service(VmServiceServer::new(VmServiceImpl {}))
                .serve_with_incoming(incoming)
                .await?;

            Ok(())
        })
    }
}

async fn create_daemon_listener(socket_path: &Path) -> anyhow::Result<UnixListener> {
    prepare_socket_path(socket_path).await?;
    bind_socket_path(socket_path).await
}

async fn prepare_socket_path(socket_path: &Path) -> anyhow::Result<()> {
    if !socket_path.exists() {
        return Ok(());
    }

    match probe_socket_path(socket_path).await? {
        SocketProbe::Live => {
            bail!("daemon already running on {}", socket_path.display());
        }
        SocketProbe::Stale => {
            std::fs::remove_file(socket_path).with_context(|| {
                format!("failed to remove stale socket {}", socket_path.display())
            })?;
        }
        SocketProbe::Missing => {}
    }

    Ok(())
}

async fn bind_socket_path(socket_path: &Path) -> anyhow::Result<UnixListener> {
    match UnixListener::bind(socket_path) {
        Ok(listener) => Ok(listener),
        Err(err) if err.kind() == ErrorKind::AddrInUse => {
            if wait_for_live_socket(
                socket_path,
                Duration::from_millis(500),
                Duration::from_millis(25),
            )
            .await?
            {
                bail!("daemon already running on {}", socket_path.display());
            }

            Err(err)
                .with_context(|| format!("socket path already in use: {}", socket_path.display()))
        }
        Err(err) => Err(err).with_context(|| format!("failed to bind {}", socket_path.display())),
    }
}

async fn wait_for_live_socket(
    socket_path: &Path,
    timeout: Duration,
    retry_interval: Duration,
) -> anyhow::Result<bool> {
    let deadline = Instant::now() + timeout;

    loop {
        match probe_socket_path(socket_path).await? {
            SocketProbe::Live => return Ok(true),
            SocketProbe::Missing | SocketProbe::Stale => {}
        }

        if Instant::now() >= deadline {
            return Ok(false);
        }

        sleep(retry_interval).await;
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SocketProbe {
    Missing,
    Live,
    Stale,
}

async fn probe_socket_path(socket_path: &Path) -> anyhow::Result<SocketProbe> {
    if !socket_path.exists() {
        return Ok(SocketProbe::Missing);
    }

    match UnixStream::connect(socket_path).await {
        Ok(_) => Ok(SocketProbe::Live),
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::ConnectionRefused | ErrorKind::NotFound
            ) =>
        {
            Ok(SocketProbe::Stale)
        }
        Err(err) => Err(err)
            .with_context(|| format!("failed to probe existing socket {}", socket_path.display())),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn create_daemon_listener_removes_stale_socket() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;

        runtime.block_on(async {
            let temp_dir = tempdir()?;
            let socket_path = temp_dir.path().join("daemon.sock");

            let stale_listener = UnixListener::bind(&socket_path)?;
            drop(stale_listener);

            let listener = create_daemon_listener(&socket_path).await?;
            assert!(socket_path.exists());

            drop(listener);
            std::fs::remove_file(&socket_path)?;
            Ok(())
        })
    }

    #[test]
    fn create_daemon_listener_preserves_live_socket() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;

        runtime.block_on(async {
            let temp_dir = tempdir()?;
            let socket_path = temp_dir.path().join("daemon.sock");
            let live_listener = UnixListener::bind(&socket_path)?;

            let err = create_daemon_listener(&socket_path).await.unwrap_err();
            assert!(format!("{err:#}").contains("already running"));

            drop(live_listener);
            std::fs::remove_file(&socket_path)?;
            Ok(())
        })
    }

    #[test]
    fn bind_socket_path_reports_already_running_after_race() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;

        runtime.block_on(async {
            let temp_dir = tempdir()?;
            let socket_path = temp_dir.path().join("daemon.sock");
            let live_listener = UnixListener::bind(&socket_path)?;

            let err = bind_socket_path(&socket_path).await.unwrap_err();
            assert!(format!("{err:#}").contains("already running"));

            drop(live_listener);
            std::fs::remove_file(&socket_path)?;
            Ok(())
        })
    }
}
