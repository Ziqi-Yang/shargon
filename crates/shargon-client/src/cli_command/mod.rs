mod cancel;
mod machines;
mod ping;
mod run;
mod task_status;
mod tasks;
mod version;

use shargon_protocol::vm_service::{
    Diagnostic, DiagnosticSeverity, MachineState, TaskState,
};

pub use cancel::CliCancelCommand;
pub use machines::CliMachinesCommand;
pub use ping::CliPingCommand;
pub use run::CliRunCommand;
pub use task_status::CliTaskStatusCommand;
pub use tasks::CliTasksCommand;
pub use version::CliVersionCommand;

pub trait CliCommand {
    fn execute(&self) -> anyhow::Result<()>;
}

pub(crate) fn task_state_label(value: i32) -> &'static str {
    match TaskState::try_from(value).unwrap_or(TaskState::TaskUnspecified) {
        TaskState::TaskQueued => "queued",
        TaskState::TaskRunning => "running",
        TaskState::TaskSucceeded => "succeeded",
        TaskState::TaskFailed => "failed",
        TaskState::TaskCancelled => "cancelled",
        TaskState::TaskUnspecified => "unknown",
    }
}

pub(crate) fn machine_state_label(value: i32) -> &'static str {
    match MachineState::try_from(value).unwrap_or(MachineState::MachineUnspecified) {
        MachineState::MachineProvisioning => "provisioning",
        MachineState::MachineIdle => "idle",
        MachineState::MachineBusy => "busy",
        MachineState::MachineFailed => "failed",
        MachineState::MachineStopping => "stopping",
        MachineState::MachineStopped => "stopped",
        MachineState::MachineUnspecified => "unknown",
    }
}

pub(crate) fn is_terminal_task_state(value: i32) -> bool {
    matches!(
        TaskState::try_from(value).unwrap_or(TaskState::TaskUnspecified),
        TaskState::TaskSucceeded | TaskState::TaskFailed | TaskState::TaskCancelled
    )
}

pub(crate) fn print_diagnostics(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        println!(
            "[{}] {}: {}",
            diagnostic_severity_label(diagnostic.severity),
            diagnostic.code,
            diagnostic.message
        );
    }
}

fn diagnostic_severity_label(value: i32) -> &'static str {
    match DiagnosticSeverity::try_from(value).unwrap_or(DiagnosticSeverity::DiagnosticUnspecified) {
        DiagnosticSeverity::DiagnosticInfo => "info",
        DiagnosticSeverity::DiagnosticWarning => "warning",
        DiagnosticSeverity::DiagnosticError => "error",
        DiagnosticSeverity::DiagnosticUnspecified => "unknown",
    }
}
