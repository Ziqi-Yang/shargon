use std::sync::Arc;

use shargon_backend::{
    BackendKind, Diagnostic, DiagnosticSeverity, EnvironmentVariable, MachineState, MachineStatus,
    TaskSpec, TaskState, TaskStatus, VmBackend, classify_error,
};
use shargon_protocol::vm_service::{self, vm_service_server::{VmService, VmServiceServer}, CancelTaskRequest, CancelTaskResponse, GetTaskRequest, GetTaskResponse, ListMachinesRequest, ListMachinesResponse, ListTasksRequest, ListTasksResponse, PingRequest, PingResponse, StartTaskRequest, StartTaskResponse};
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct VmServiceImpl {
    backend: Arc<dyn VmBackend>,
}

impl VmServiceImpl {
    pub fn new(backend: Arc<dyn VmBackend>) -> Self {
        Self { backend }
    }

    pub fn into_server(self) -> VmServiceServer<Self> {
        VmServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl VmService for VmServiceImpl {
    async fn ping(&self, _request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse {
            msg: "pong".to_string(),
        }))
    }

    async fn start_task(
        &self,
        request: Request<StartTaskRequest>,
    ) -> Result<Response<StartTaskResponse>, Status> {
        let request = request.into_inner();
        let spec = request
            .spec
            .ok_or_else(|| Status::invalid_argument("start_task requires a task spec"))?;
        let task = self
            .backend
            .start_task(from_proto_task_spec(spec)?)
            .await
            .map_err(map_backend_error)?;

        Ok(Response::new(StartTaskResponse {
            task: Some(to_proto_task_status(task)),
        }))
    }

    async fn get_task(
        &self,
        request: Request<GetTaskRequest>,
    ) -> Result<Response<GetTaskResponse>, Status> {
        let task = self
            .backend
            .get_task(request.into_inner().task_id.as_str())
            .await
            .map_err(map_backend_error)?;

        Ok(Response::new(GetTaskResponse {
            task: Some(to_proto_task_status(task)),
        }))
    }

    async fn list_tasks(
        &self,
        _request: Request<ListTasksRequest>,
    ) -> Result<Response<ListTasksResponse>, Status> {
        let tasks = self
            .backend
            .list_tasks()
            .await
            .map_err(map_backend_error)?
            .into_iter()
            .map(to_proto_task_status)
            .collect();

        Ok(Response::new(ListTasksResponse { tasks }))
    }

    async fn cancel_task(
        &self,
        request: Request<CancelTaskRequest>,
    ) -> Result<Response<CancelTaskResponse>, Status> {
        let task = self
            .backend
            .cancel_task(request.into_inner().task_id.as_str())
            .await
            .map_err(map_backend_error)?;

        Ok(Response::new(CancelTaskResponse {
            task: Some(to_proto_task_status(task)),
        }))
    }

    async fn list_machines(
        &self,
        _request: Request<ListMachinesRequest>,
    ) -> Result<Response<ListMachinesResponse>, Status> {
        let machines = self
            .backend
            .list_machines()
            .await
            .map_err(map_backend_error)?
            .into_iter()
            .map(to_proto_machine_status)
            .collect();

        Ok(Response::new(ListMachinesResponse { machines }))
    }
}

fn from_proto_task_spec(spec: vm_service::TaskSpec) -> Result<TaskSpec, Status> {
    let task = TaskSpec {
        argv: spec.argv,
        env: spec
            .env
            .into_iter()
            .map(|env| EnvironmentVariable {
                name: env.name,
                value: env.value,
            })
            .collect(),
        working_directory: spec.working_directory,
    };

    task.validate()
        .map_err(|err| Status::invalid_argument(format!("{err:#}")))?;

    Ok(task)
}

fn to_proto_task_status(status: TaskStatus) -> vm_service::TaskStatus {
    vm_service::TaskStatus {
        id: status.id,
        backend: to_proto_backend_kind(status.backend) as i32,
        state: to_proto_task_state(status.state) as i32,
        machine_id: status.machine_id,
        exit_code: status.exit_code,
        diagnostics: status
            .diagnostics
            .into_iter()
            .map(to_proto_diagnostic)
            .collect(),
    }
}

fn to_proto_machine_status(status: MachineStatus) -> vm_service::MachineStatus {
    vm_service::MachineStatus {
        id: status.id,
        backend: to_proto_backend_kind(status.backend) as i32,
        name: status.name,
        state: to_proto_machine_state(status.state) as i32,
        current_task_id: status.current_task_id,
        diagnostics: status
            .diagnostics
            .into_iter()
            .map(to_proto_diagnostic)
            .collect(),
    }
}

fn to_proto_diagnostic(diagnostic: Diagnostic) -> vm_service::Diagnostic {
    vm_service::Diagnostic {
        code: diagnostic.code,
        severity: to_proto_diagnostic_severity(diagnostic.severity) as i32,
        message: diagnostic.message,
    }
}

fn to_proto_backend_kind(kind: BackendKind) -> vm_service::BackendKind {
    match kind {
        BackendKind::Nspawn => vm_service::BackendKind::BackendNspawn,
        BackendKind::Qemu => vm_service::BackendKind::BackendQemu,
    }
}

fn to_proto_diagnostic_severity(
    severity: DiagnosticSeverity,
) -> vm_service::DiagnosticSeverity {
    match severity {
        DiagnosticSeverity::Info => vm_service::DiagnosticSeverity::DiagnosticInfo,
        DiagnosticSeverity::Warning => vm_service::DiagnosticSeverity::DiagnosticWarning,
        DiagnosticSeverity::Error => vm_service::DiagnosticSeverity::DiagnosticError,
    }
}

fn to_proto_machine_state(state: MachineState) -> vm_service::MachineState {
    match state {
        MachineState::Provisioning => vm_service::MachineState::MachineProvisioning,
        MachineState::Idle => vm_service::MachineState::MachineIdle,
        MachineState::Busy => vm_service::MachineState::MachineBusy,
        MachineState::Failed => vm_service::MachineState::MachineFailed,
        MachineState::Stopping => vm_service::MachineState::MachineStopping,
        MachineState::Stopped => vm_service::MachineState::MachineStopped,
    }
}

fn to_proto_task_state(state: TaskState) -> vm_service::TaskState {
    match state {
        TaskState::Queued => vm_service::TaskState::TaskQueued,
        TaskState::Running => vm_service::TaskState::TaskRunning,
        TaskState::Succeeded => vm_service::TaskState::TaskSucceeded,
        TaskState::Failed => vm_service::TaskState::TaskFailed,
        TaskState::Cancelled => vm_service::TaskState::TaskCancelled,
    }
}

fn map_backend_error(err: anyhow::Error) -> Status {
    match classify_error(&err) {
        shargon_backend::BackendErrorKind::InvalidInput => {
            Status::invalid_argument(format!("{err:#}"))
        }
        shargon_backend::BackendErrorKind::NotFound => Status::not_found(format!("{err:#}")),
        shargon_backend::BackendErrorKind::FailedPrecondition => {
            Status::failed_precondition(format!("{err:#}"))
        }
        shargon_backend::BackendErrorKind::Unimplemented => {
            Status::unimplemented(format!("{err:#}"))
        }
        shargon_backend::BackendErrorKind::Internal => Status::internal(format!("{err:#}")),
    }
}
