use shargon_protocol::vm_service::{GetTaskRequest, StartTaskRequest, TaskSpec};
use shargon_settings::ShargonSettings;
use tokio::time::sleep;

use super::{CliCommand, is_terminal_task_state, print_diagnostics, task_state_label};
use crate::daemon::connect_to_running_vm_service;

pub struct CliRunCommand {
    argv: Vec<String>,
}

impl CliRunCommand {
    pub fn new(argv: Vec<String>) -> Self {
        Self { argv }
    }
}

impl CliCommand for CliRunCommand {
    fn execute(&self) -> anyhow::Result<()> {
        let argv = self.argv.clone();
        let retry_interval = ShargonSettings::load()?.daemon.retry_interval;
        let rt = tokio::runtime::Runtime::new()?;

        rt.block_on(async move {
            let mut client = connect_to_running_vm_service().await?;
            let mut task = client
                .start_task(StartTaskRequest {
                    spec: Some(TaskSpec {
                        argv,
                        env: Vec::new(),
                        working_directory: None,
                    }),
                })
                .await?
                .into_inner()
                .task
                .ok_or_else(|| anyhow::anyhow!("daemon returned an empty task status"))?;

            loop {
                if is_terminal_task_state(task.state) {
                    println!(
                        "{}\t{}\t{}",
                        task.id,
                        task_state_label(task.state),
                        task.machine_id.clone().unwrap_or_else(|| "-".to_string())
                    );
                    print_diagnostics(&task.diagnostics);

                    if task_state_label(task.state) == "succeeded" {
                        return Ok(());
                    }

                    let exit_code = task
                        .exit_code
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    return Err(anyhow::anyhow!(
                        "task {} finished with state {} (exit code: {exit_code})",
                        task.id,
                        task_state_label(task.state),
                    ));
                }

                sleep(retry_interval).await;
                task = client
                    .get_task(GetTaskRequest {
                        task_id: task.id.clone(),
                    })
                    .await?
                    .into_inner()
                    .task
                    .ok_or_else(|| anyhow::anyhow!("daemon returned an empty task status"))?;
            }
        })
    }
}
