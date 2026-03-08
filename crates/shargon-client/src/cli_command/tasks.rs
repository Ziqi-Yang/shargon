use shargon_protocol::vm_service::ListTasksRequest;

use super::{CliCommand, print_diagnostics, task_state_label};
use crate::daemon::connect_to_running_vm_service;

pub struct CliTasksCommand;

impl CliTasksCommand {
    pub fn new() -> Self {
        Self
    }
}

impl CliCommand for CliTasksCommand {
    fn execute(&self) -> anyhow::Result<()> {
        let rt = tokio::runtime::Runtime::new()?;

        rt.block_on(async {
            let mut client = connect_to_running_vm_service().await?;
            let response = client.list_tasks(ListTasksRequest {}).await?.into_inner();

            for task in response.tasks {
                println!(
                    "{}\t{}\t{}\t{}",
                    task.id,
                    task_state_label(task.state),
                    task.machine_id.clone().unwrap_or_else(|| "-".to_string()),
                    task.exit_code
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "-".to_string())
                );
                print_diagnostics(&task.diagnostics);
            }

            Ok::<(), anyhow::Error>(())
        })
    }
}
