use shargon_protocol::vm_service::ListMachinesRequest;

use super::{CliCommand, machine_state_label, print_diagnostics};
use crate::daemon::connect_to_running_vm_service;

pub struct CliMachinesCommand;

impl CliMachinesCommand {
    pub fn new() -> Self {
        Self
    }
}

impl CliCommand for CliMachinesCommand {
    fn execute(&self) -> anyhow::Result<()> {
        let rt = tokio::runtime::Runtime::new()?;

        rt.block_on(async {
            let mut client = connect_to_running_vm_service().await?;
            let response = client
                .list_machines(ListMachinesRequest {})
                .await?
                .into_inner();

            for machine in response.machines {
                println!(
                    "{}\t{}\t{}\t{}",
                    machine.id,
                    machine_state_label(machine.state),
                    machine.name,
                    machine
                        .current_task_id
                        .clone()
                        .unwrap_or_else(|| "-".to_string())
                );
                print_diagnostics(&machine.diagnostics);
            }

            Ok::<(), anyhow::Error>(())
        })
    }
}
