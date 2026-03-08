use std::{error::Error as StdError, fmt};

use anyhow::anyhow;
use async_trait::async_trait;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendKind {
    Nspawn,
    Qemu,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nspawn => "nspawn",
            Self::Qemu => "qemu",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

impl Diagnostic {
    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: DiagnosticSeverity::Warning,
            message: message.into(),
        }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: DiagnosticSeverity::Error,
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineState {
    Provisioning,
    Idle,
    Busy,
    Failed,
    Stopping,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MachineStatus {
    pub id: String,
    pub backend: BackendKind,
    pub name: String,
    pub state: MachineState,
    pub current_task_id: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskSpec {
    pub argv: Vec<String>,
    pub env: Vec<EnvironmentVariable>,
    pub working_directory: Option<String>,
}

impl TaskSpec {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.argv.is_empty() {
            return Err(invalid_input("task argv must not be empty"));
        }

        if let Some(working_directory) = &self.working_directory {
            if !working_directory.starts_with('/') {
                return Err(invalid_input(format!(
                    "task working_directory must be absolute, got {working_directory}"
                )));
            }
        }

        for env in &self.env {
            if env.name.is_empty() {
                return Err(invalid_input("environment variable name must not be empty"));
            }

            if env.name.contains('=') {
                return Err(invalid_input(format!(
                    "environment variable name must not contain '=': {}",
                    env.name
                )));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskStatus {
    pub id: String,
    pub backend: BackendKind,
    pub state: TaskState,
    pub machine_id: Option<String>,
    pub exit_code: Option<i32>,
    pub diagnostics: Vec<Diagnostic>,
}

#[async_trait]
pub trait VmBackend: Send + Sync {
    fn kind(&self) -> BackendKind;

    async fn reconcile_pool(&self, target_size: usize) -> anyhow::Result<()>;

    async fn start_task(&self, spec: TaskSpec) -> anyhow::Result<TaskStatus>;

    async fn get_task(&self, task_id: &str) -> anyhow::Result<TaskStatus>;

    async fn list_tasks(&self) -> anyhow::Result<Vec<TaskStatus>>;

    async fn cancel_task(&self, task_id: &str) -> anyhow::Result<TaskStatus>;

    async fn list_machines(&self) -> anyhow::Result<Vec<MachineStatus>>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendErrorKind {
    InvalidInput,
    NotFound,
    FailedPrecondition,
    Unimplemented,
    Internal,
}

#[derive(Debug)]
pub struct BackendError {
    kind: BackendErrorKind,
    message: String,
}

impl BackendError {
    pub fn new(kind: BackendErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> BackendErrorKind {
        self.kind
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl StdError for BackendError {}

pub fn invalid_input(message: impl Into<String>) -> anyhow::Error {
    anyhow!(BackendError::new(BackendErrorKind::InvalidInput, message))
}

pub fn not_found(message: impl Into<String>) -> anyhow::Error {
    anyhow!(BackendError::new(BackendErrorKind::NotFound, message))
}

pub fn failed_precondition(message: impl Into<String>) -> anyhow::Error {
    anyhow!(BackendError::new(
        BackendErrorKind::FailedPrecondition,
        message,
    ))
}

pub fn unimplemented(message: impl Into<String>) -> anyhow::Error {
    anyhow!(BackendError::new(BackendErrorKind::Unimplemented, message))
}

pub fn internal(message: impl Into<String>) -> anyhow::Error {
    anyhow!(BackendError::new(BackendErrorKind::Internal, message))
}

pub fn classify_error(err: &anyhow::Error) -> BackendErrorKind {
    err.chain()
        .find_map(|cause| cause.downcast_ref::<BackendError>().map(BackendError::kind))
        .unwrap_or(BackendErrorKind::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_spec_requires_non_empty_argv() {
        let err = TaskSpec {
            argv: Vec::new(),
            env: Vec::new(),
            working_directory: None,
        }
        .validate()
        .unwrap_err();

        assert_eq!(classify_error(&err), BackendErrorKind::InvalidInput);
        assert!(format!("{err:#}").contains("argv"));
    }

    #[test]
    fn task_spec_requires_absolute_working_directory() {
        let err = TaskSpec {
            argv: vec!["cargo".to_string(), "test".to_string()],
            env: Vec::new(),
            working_directory: Some("relative".to_string()),
        }
        .validate()
        .unwrap_err();

        assert_eq!(classify_error(&err), BackendErrorKind::InvalidInput);
        assert!(format!("{err:#}").contains("absolute"));
    }

    #[test]
    fn task_spec_rejects_invalid_environment_variable_names() {
        let err = TaskSpec {
            argv: vec!["cargo".to_string()],
            env: vec![EnvironmentVariable {
                name: "BAD=NAME".to_string(),
                value: "1".to_string(),
            }],
            working_directory: None,
        }
        .validate()
        .unwrap_err();

        assert_eq!(classify_error(&err), BackendErrorKind::InvalidInput);
        assert!(format!("{err:#}").contains("must not contain"));
    }
}
