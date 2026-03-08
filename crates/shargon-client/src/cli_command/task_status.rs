use shargon_protocol::vm_service::GetTaskRequest;

use super::{CliCommand, print_diagnostics, task_state_label};
use crate::daemon::connect_to_running_vm_service;

pub struct CliTaskStatusCommand {
    task_id: String,
}

impl CliTaskStatusCommand {
    pub fn new(task_id: String) -> Self {
        Self { task_id }
    }
}

impl CliCommand for CliTaskStatusCommand {
    fn execute(&self) -> anyhow::Result<()> {
        let task_id = self.task_id.clone();
        let rt = tokio::runtime::Runtime::new()?;

        rt.block_on(async move {
            let mut client = connect_to_running_vm_service().await?;
            let task = client
                .get_task(GetTaskRequest { task_id })
                .await?
                .into_inner()
                .task
                .ok_or_else(|| anyhow::anyhow!("daemon returned an empty task status"))?;

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
            Ok::<(), anyhow::Error>(())
        })
    }
}
