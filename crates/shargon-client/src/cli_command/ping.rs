use shargon_protocol::vm_service::PingRequest;

use super::CliCommand;
use crate::daemon::connect_or_start_vm_service;

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
            let mut client = connect_or_start_vm_service().await?;
            let response = client.ping(PingRequest {}).await?;
            println!("{}", response.into_inner().msg);

            Ok::<(), anyhow::Error>(())
        })
    }
}
