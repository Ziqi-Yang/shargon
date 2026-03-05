use std::path::Path;

use shargon_protocol::{SOCKET_PATH, vm_service::vm_service_server::VmServiceServer};
use tokio::net::UnixListener;
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
            if Path::new(SOCKET_PATH).exists() {
                std::fs::remove_file(SOCKET_PATH)?;
            }

            let listener = UnixListener::bind(SOCKET_PATH)?;
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
