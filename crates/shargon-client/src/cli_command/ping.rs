use hyper_util::rt::TokioIo;
use shargon_protocol::{
    SOCKET_PATH,
    vm_service::{PingRequest, vm_service_client::VmServiceClient},
};
use tokio::net::UnixStream;
use tonic::transport::{Endpoint, Uri};
use tower::service_fn;

use super::CliCommand;

pub struct CliPingCommand {}

impl CliPingCommand {
    pub fn new() -> Self {
        Self {}
    }
}

impl CliCommand for CliPingCommand {
    fn execute(&self) -> anyhow::Result<()> {
        let rt = tokio::runtime::Runtime::new()?;

        rt.block_on(async {
            let channel = Endpoint::try_from("http://[::]:50051")?
                .connect_with_connector(service_fn(|_: Uri| async {
                    Ok::<_, std::io::Error>(TokioIo::new(UnixStream::connect(SOCKET_PATH).await?))
                }))
                .await;

            match channel {
                Ok(channel) => {
                    let mut client = VmServiceClient::new(channel);
                    let response = client.ping(PingRequest {}).await?;
                    println!("{}", response.into_inner().msg);
                }
                Err(_) => {
                    eprintln!("daemon not running — start it with: shargon-daemon run");
                }
            }

            Ok::<(), anyhow::Error>(())
        })
    }
}
