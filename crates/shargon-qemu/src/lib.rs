use async_trait::async_trait;
use shargon_backend::{
    BackendKind, MachineStatus, TaskSpec, TaskStatus, VmBackend, unimplemented,
};

#[derive(Debug, Default)]
pub struct QemuBackend;

impl QemuBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl VmBackend for QemuBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Qemu
    }

    async fn reconcile_pool(&self, _target_size: usize) -> anyhow::Result<()> {
        Err(unimplemented("the qemu backend is not implemented yet"))
    }

    async fn start_task(&self, _spec: TaskSpec) -> anyhow::Result<TaskStatus> {
        Err(unimplemented("the qemu backend is not implemented yet"))
    }

    async fn get_task(&self, _task_id: &str) -> anyhow::Result<TaskStatus> {
        Err(unimplemented("the qemu backend is not implemented yet"))
    }

    async fn list_tasks(&self) -> anyhow::Result<Vec<TaskStatus>> {
        Err(unimplemented("the qemu backend is not implemented yet"))
    }

    async fn cancel_task(&self, _task_id: &str) -> anyhow::Result<TaskStatus> {
        Err(unimplemented("the qemu backend is not implemented yet"))
    }

    async fn list_machines(&self) -> anyhow::Result<Vec<MachineStatus>> {
        Err(unimplemented("the qemu backend is not implemented yet"))
    }
}
