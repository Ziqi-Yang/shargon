use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use shargon_backend::{
    BackendKind, Diagnostic, EnvironmentVariable, MachineState, MachineStatus, TaskSpec,
    TaskState, TaskStatus, VmBackend, failed_precondition, internal, not_found,
};
use shargon_settings::NspawnSettings;
use tempfile::Builder;
use tokio::{
    process::Command,
    sync::{mpsc, oneshot},
    time::sleep,
};

const READINESS_RETRY_INTERVAL: Duration = Duration::from_millis(250);
const SNAPSHOT_WARNING_CODE: &str = "filesystem.slow_snapshot";
const TASK_FAILURE_CODE: &str = "task.execution_failed";

#[derive(Clone)]
pub struct NspawnBackend {
    command_tx: mpsc::Sender<BackendCommand>,
}

impl NspawnBackend {
    pub async fn new(settings: NspawnSettings) -> anyhow::Result<Self> {
        Self::new_with_system(settings, Arc::new(RealNspawnSystem))
    }

    fn new_with_system<S>(settings: NspawnSettings, system: Arc<S>) -> anyhow::Result<Self>
    where
        S: NspawnSystem,
    {
        let root_directory = settings
            .root_directory
            .clone()
            .ok_or_else(|| failed_precondition("backend.nspawn.root_directory must be configured"))?;

        let static_diagnostics = system.detect_snapshot_warning(root_directory.as_path())?;
        for diagnostic in &static_diagnostics {
            tracing::warn!("{}", diagnostic.message);
        }

        let (command_tx, command_rx) = mpsc::channel(32);
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        tokio::spawn(BackendActor {
            settings,
            root_directory,
            system,
            static_diagnostics,
            command_rx,
            event_rx,
            event_tx,
            state: BackendState::default(),
        }
        .run());

        Ok(Self { command_tx })
    }

    async fn send_command<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<anyhow::Result<T>>) -> BackendCommand,
    ) -> anyhow::Result<T> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(build(response_tx))
            .await
            .map_err(|_| internal("nspawn backend task loop is not running"))?;
        response_rx
            .await
            .map_err(|_| internal("nspawn backend task loop exited before replying"))?
    }
}

#[async_trait]
impl VmBackend for NspawnBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Nspawn
    }

    async fn reconcile_pool(&self, target_size: usize) -> anyhow::Result<()> {
        self.send_command(|response_tx| BackendCommand::ReconcilePool {
            target_size,
            response_tx,
        })
        .await
    }

    async fn start_task(&self, spec: TaskSpec) -> anyhow::Result<TaskStatus> {
        self.send_command(|response_tx| BackendCommand::StartTask { spec, response_tx })
            .await
    }

    async fn get_task(&self, task_id: &str) -> anyhow::Result<TaskStatus> {
        self.send_command(|response_tx| BackendCommand::GetTask {
            task_id: task_id.to_string(),
            response_tx,
        })
        .await
    }

    async fn list_tasks(&self) -> anyhow::Result<Vec<TaskStatus>> {
        self.send_command(|response_tx| BackendCommand::ListTasks { response_tx })
            .await
    }

    async fn cancel_task(&self, task_id: &str) -> anyhow::Result<TaskStatus> {
        self.send_command(|response_tx| BackendCommand::CancelTask {
            task_id: task_id.to_string(),
            response_tx,
        })
        .await
    }

    async fn list_machines(&self) -> anyhow::Result<Vec<MachineStatus>> {
        self.send_command(|response_tx| BackendCommand::ListMachines { response_tx })
            .await
    }
}

enum BackendCommand {
    ReconcilePool {
        target_size: usize,
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    StartTask {
        spec: TaskSpec,
        response_tx: oneshot::Sender<anyhow::Result<TaskStatus>>,
    },
    GetTask {
        task_id: String,
        response_tx: oneshot::Sender<anyhow::Result<TaskStatus>>,
    },
    ListTasks {
        response_tx: oneshot::Sender<anyhow::Result<Vec<TaskStatus>>>,
    },
    CancelTask {
        task_id: String,
        response_tx: oneshot::Sender<anyhow::Result<TaskStatus>>,
    },
    ListMachines {
        response_tx: oneshot::Sender<anyhow::Result<Vec<MachineStatus>>>,
    },
}

enum BackendEvent {
    MachineReady {
        machine_id: String,
    },
    MachineProvisionFailed {
        machine_id: String,
        error_message: String,
    },
    MachineExited {
        machine_id: String,
        error_message: String,
    },
    TaskFinished {
        task_id: String,
        exit_code: Option<i32>,
        error_message: Option<String>,
    },
}

#[derive(Default)]
struct BackendState {
    target_size: usize,
    next_machine_id: u64,
    next_task_id: u64,
    machines: BTreeMap<String, MachineRecord>,
    tasks: BTreeMap<String, TaskRecord>,
    task_queue: VecDeque<String>,
}

struct MachineRecord {
    status: MachineStatus,
}

struct TaskRecord {
    status: TaskStatus,
    spec: TaskSpec,
    unit_name: Option<String>,
    cancellation_requested: bool,
}

struct BackendActor<S>
where
    S: NspawnSystem,
{
    settings: NspawnSettings,
    root_directory: PathBuf,
    system: Arc<S>,
    static_diagnostics: Vec<Diagnostic>,
    command_rx: mpsc::Receiver<BackendCommand>,
    event_rx: mpsc::UnboundedReceiver<BackendEvent>,
    event_tx: mpsc::UnboundedSender<BackendEvent>,
    state: BackendState,
}

impl<S> BackendActor<S>
where
    S: NspawnSystem,
{
    async fn run(mut self) {
        loop {
            tokio::select! {
                Some(command) = self.command_rx.recv() => self.handle_command(command).await,
                Some(event) = self.event_rx.recv() => self.handle_event(event).await,
                else => break,
            }
        }
    }

    async fn handle_command(&mut self, command: BackendCommand) {
        match command {
            BackendCommand::ReconcilePool {
                target_size,
                response_tx,
            } => {
                let _ = response_tx.send(self.handle_reconcile_pool(target_size));
            }
            BackendCommand::StartTask { spec, response_tx } => {
                let _ = response_tx.send(self.handle_start_task(spec));
            }
            BackendCommand::GetTask {
                task_id,
                response_tx,
            } => {
                let _ = response_tx.send(self
                    .state
                    .tasks
                    .get(&task_id)
                    .map(|task| task.status.clone())
                    .ok_or_else(|| not_found(format!("task {task_id} was not found"))));
            }
            BackendCommand::ListTasks { response_tx } => {
                let mut tasks = self
                    .state
                    .tasks
                    .values()
                    .map(|task| task.status.clone())
                    .collect::<Vec<_>>();
                tasks.sort_by(|left, right| left.id.cmp(&right.id));
                let _ = response_tx.send(Ok(tasks));
            }
            BackendCommand::CancelTask {
                task_id,
                response_tx,
            } => {
                let result = self.handle_cancel_task(&task_id).await;
                let _ = response_tx.send(result);
            }
            BackendCommand::ListMachines { response_tx } => {
                let mut machines = self
                    .state
                    .machines
                    .values()
                    .map(|machine| machine.status.clone())
                    .collect::<Vec<_>>();
                machines.sort_by(|left, right| left.id.cmp(&right.id));
                let _ = response_tx.send(Ok(machines));
            }
        }
    }

    fn handle_reconcile_pool(&mut self, target_size: usize) -> anyhow::Result<()> {
        if target_size == 0 {
            return Err(failed_precondition(
                "nspawn pool size must be greater than zero",
            ));
        }

        self.state.target_size = target_size;
        self.reconcile_pool_internal()
    }

    fn handle_start_task(&mut self, spec: TaskSpec) -> anyhow::Result<TaskStatus> {
        spec.validate()?;

        let task_id = self.next_task_id();
        let mut status = TaskStatus {
            id: task_id.clone(),
            backend: BackendKind::Nspawn,
            state: TaskState::Queued,
            machine_id: None,
            exit_code: None,
            diagnostics: self.static_diagnostics.clone(),
        };

        self.state.tasks.insert(
            task_id.clone(),
            TaskRecord {
                status: status.clone(),
                spec,
                unit_name: None,
                cancellation_requested: false,
            },
        );
        self.state.task_queue.push_back(task_id.clone());
        self.schedule_queued_tasks();

        status = self
            .state
            .tasks
            .get(&task_id)
            .map(|task| task.status.clone())
            .unwrap_or(status);

        Ok(status)
    }

    async fn handle_cancel_task(&mut self, task_id: &str) -> anyhow::Result<TaskStatus> {
        let Some(task) = self.state.tasks.get_mut(task_id) else {
            return Err(not_found(format!("task {task_id} was not found")));
        };

        match task.status.state {
            TaskState::Queued => {
                self.state.task_queue.retain(|queued| queued != task_id);
                task.status.state = TaskState::Cancelled;
                task.status.exit_code = None;
                Ok(task.status.clone())
            }
            TaskState::Running => {
                task.cancellation_requested = true;
                let machine_id = task.status.machine_id.clone().ok_or_else(|| {
                    internal(format!(
                        "running task {task_id} is missing an assigned machine"
                    ))
                })?;
                let unit_name = task.unit_name.clone().ok_or_else(|| {
                    internal(format!("running task {task_id} is missing its unit name"))
                })?;
                let machine_name = self
                    .state
                    .machines
                    .get(&machine_id)
                    .map(|machine| machine.status.name.clone())
                    .ok_or_else(|| {
                        internal(format!(
                            "assigned machine {machine_id} for task {task_id} is missing"
                        ))
                    })?;

                self.system.cancel_task(&machine_name, &unit_name).await?;
                Ok(task.status.clone())
            }
            TaskState::Succeeded | TaskState::Failed | TaskState::Cancelled => {
                Ok(task.status.clone())
            }
        }
    }

    async fn handle_event(&mut self, event: BackendEvent) {
        match event {
            BackendEvent::MachineReady { machine_id } => {
                if let Some(machine) = self.state.machines.get_mut(&machine_id) {
                    machine.status.state = MachineState::Idle;
                }
                self.schedule_queued_tasks();
            }
            BackendEvent::MachineProvisionFailed {
                machine_id,
                error_message,
            } => {
                tracing::error!("failed to provision machine {machine_id}: {error_message}");
                self.state.machines.remove(&machine_id);
                self.reconcile_pool_logged();
            }
            BackendEvent::MachineExited {
                machine_id,
                error_message,
            } => {
                tracing::warn!("machine {machine_id} exited: {error_message}");
                if let Some(machine) = self.state.machines.remove(&machine_id) {
                    if let Some(task_id) = machine.status.current_task_id {
                        self.fail_task(
                            &task_id,
                            Diagnostic::error(TASK_FAILURE_CODE, error_message.clone()),
                        );
                    }
                }
                self.reconcile_pool_logged();
            }
            BackendEvent::TaskFinished {
                task_id,
                exit_code,
                error_message,
            } => {
                self.finish_task(task_id.as_str(), exit_code, error_message);
                self.schedule_queued_tasks();
                self.reconcile_pool_logged();
            }
        }
    }

    fn next_machine_id(&mut self) -> String {
        self.state.next_machine_id += 1;
        format!("machine-{:06}", self.state.next_machine_id)
    }

    fn next_task_id(&mut self) -> String {
        self.state.next_task_id += 1;
        format!("task-{:06}", self.state.next_task_id)
    }

    fn reconcile_pool_internal(&mut self) -> anyhow::Result<()> {
        while self.active_machine_count() < self.state.target_size {
            self.spawn_machine()?;
        }

        Ok(())
    }

    fn active_machine_count(&self) -> usize {
        self.state
            .machines
            .values()
            .filter(|machine| {
                matches!(
                    machine.status.state,
                    MachineState::Provisioning | MachineState::Idle | MachineState::Busy
                )
            })
            .count()
    }

    fn spawn_machine(&mut self) -> anyhow::Result<()> {
        let machine_id = self.next_machine_id();
        let machine_name = format!("{}-{}", self.settings.machine_prefix, machine_id);
        let status = MachineStatus {
            id: machine_id.clone(),
            backend: BackendKind::Nspawn,
            name: machine_name.clone(),
            state: MachineState::Provisioning,
            current_task_id: None,
            diagnostics: self.static_diagnostics.clone(),
        };

        self.state
            .machines
            .insert(machine_id.clone(), MachineRecord { status });

        self.system
            .spawn_machine(
                machine_id,
                machine_name,
                self.root_directory.clone(),
                self.settings.boot_timeout,
                self.event_tx.clone(),
            )
            .map_err(|err| {
                failed_precondition(format!(
                    "failed to start an nspawn machine. run shargon-daemon as a privileged system service: {err:#}"
                ))
            })
    }

    fn schedule_queued_tasks(&mut self) {
        loop {
            let Some(task_id) = self.state.task_queue.pop_front() else {
                break;
            };
            let Some(machine_id) = self.find_idle_machine_id() else {
                self.state.task_queue.push_front(task_id);
                break;
            };

            if let Err(err) = self.assign_task(task_id.as_str(), machine_id.as_str()) {
                tracing::error!("failed to assign task {task_id} to {machine_id}: {err:#}");
                self.fail_task(
                    task_id.as_str(),
                    Diagnostic::error(TASK_FAILURE_CODE, format!("{err:#}")),
                );
            }
        }
    }

    fn find_idle_machine_id(&self) -> Option<String> {
        self.state
            .machines
            .values()
            .find(|machine| machine.status.state == MachineState::Idle)
            .map(|machine| machine.status.id.clone())
    }

    fn assign_task(&mut self, task_id: &str, machine_id: &str) -> anyhow::Result<()> {
        let unit_name = format!("shargon-task-{task_id}");
        let machine_name = self
            .state
            .machines
            .get(machine_id)
            .map(|machine| machine.status.name.clone())
            .ok_or_else(|| internal(format!("machine {machine_id} is missing")))?;
        let spec = self
            .state
            .tasks
            .get(task_id)
            .map(|task| task.spec.clone())
            .ok_or_else(|| internal(format!("task {task_id} is missing")))?;

        self.system.run_task(
            task_id.to_string(),
            machine_name,
            unit_name.clone(),
            spec,
            self.event_tx.clone(),
        )?;

        if let Some(machine) = self.state.machines.get_mut(machine_id) {
            machine.status.state = MachineState::Busy;
            machine.status.current_task_id = Some(task_id.to_string());
        }

        if let Some(task) = self.state.tasks.get_mut(task_id) {
            task.status.state = TaskState::Running;
            task.status.machine_id = Some(machine_id.to_string());
            task.status.exit_code = None;
            task.unit_name = Some(unit_name);
        }

        Ok(())
    }

    fn fail_task(&mut self, task_id: &str, diagnostic: Diagnostic) {
        let machine_id = self
            .state
            .tasks
            .get(task_id)
            .and_then(|task| task.status.machine_id.clone());

        if let Some(task) = self.state.tasks.get_mut(task_id) {
            task.status.state = TaskState::Failed;
            task.status.exit_code = None;
            if !task.status.diagnostics.contains(&diagnostic) {
                task.status.diagnostics.push(diagnostic);
            }
        }

        if let Some(machine_id) = machine_id
            && let Some(machine) = self.state.machines.get_mut(&machine_id)
            && machine.status.current_task_id.as_deref() == Some(task_id)
        {
            machine.status.state = MachineState::Idle;
            machine.status.current_task_id = None;
        }
    }

    fn finish_task(&mut self, task_id: &str, exit_code: Option<i32>, error_message: Option<String>) {
        let mut machine_id = None;

        if let Some(task) = self.state.tasks.get_mut(task_id) {
            machine_id = task.status.machine_id.clone();
            task.status.exit_code = exit_code;

            if task.cancellation_requested {
                task.status.state = TaskState::Cancelled;
            } else if let Some(message) = error_message {
                task.status.state = TaskState::Failed;
                task.status
                    .diagnostics
                    .push(Diagnostic::error(TASK_FAILURE_CODE, message));
            } else if exit_code == Some(0) {
                task.status.state = TaskState::Succeeded;
            } else {
                task.status.state = TaskState::Failed;
            }
        }

        if let Some(machine_id) = machine_id
            && let Some(machine) = self.state.machines.get_mut(&machine_id)
            && machine.status.current_task_id.as_deref() == Some(task_id)
        {
            machine.status.state = MachineState::Idle;
            machine.status.current_task_id = None;
        }
    }

    fn reconcile_pool_logged(&mut self) {
        if let Err(err) = self.reconcile_pool_internal() {
            tracing::error!("failed to reconcile nspawn pool: {err:#}");
        }
    }
}

#[async_trait]
trait NspawnSystem: Send + Sync + 'static {
    fn detect_snapshot_warning(&self, root_directory: &Path) -> anyhow::Result<Vec<Diagnostic>>;

    fn spawn_machine(
        &self,
        machine_id: String,
        machine_name: String,
        root_directory: PathBuf,
        boot_timeout: Duration,
        event_tx: mpsc::UnboundedSender<BackendEvent>,
    ) -> anyhow::Result<()>;

    fn run_task(
        &self,
        task_id: String,
        machine_name: String,
        unit_name: String,
        spec: TaskSpec,
        event_tx: mpsc::UnboundedSender<BackendEvent>,
    ) -> anyhow::Result<()>;

    async fn cancel_task(&self, machine_name: &str, unit_name: &str) -> anyhow::Result<()>;
}

struct RealNspawnSystem;

#[async_trait]
impl NspawnSystem for RealNspawnSystem {
    fn detect_snapshot_warning(&self, root_directory: &Path) -> anyhow::Result<Vec<Diagnostic>> {
        match detect_snapshot_support(root_directory) {
            Ok(SnapshotSupport::Fast) => Ok(Vec::new()),
            Ok(SnapshotSupport::Slow(message)) => {
                Ok(vec![Diagnostic::warning(SNAPSHOT_WARNING_CODE, message)])
            }
            Err(err) => Ok(vec![Diagnostic::warning(
                SNAPSHOT_WARNING_CODE,
                format!(
                    "shargon could not verify fast snapshot support for {}: {err:#}. systemd-nspawn --ephemeral may fall back to slower copies.",
                    root_directory.display()
                ),
            )]),
        }
    }

    fn spawn_machine(
        &self,
        machine_id: String,
        machine_name: String,
        root_directory: PathBuf,
        boot_timeout: Duration,
        event_tx: mpsc::UnboundedSender<BackendEvent>,
    ) -> anyhow::Result<()> {
        let mut command = Command::new("systemd-nspawn");
        command
            .args(nspawn_spawn_args(root_directory.as_path(), &machine_name))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let mut child = command.spawn().with_context(|| {
            format!("failed to spawn systemd-nspawn for machine {machine_name}")
        })?;

        let exit_event_tx = event_tx.clone();
        let exit_machine_id = machine_id.clone();
        tokio::spawn(async move {
            let error_message = match child.wait().await {
                Ok(status) => format!("systemd-nspawn exited with {}", render_status(status.code())),
                Err(err) => format!("failed waiting for systemd-nspawn: {err}"),
            };
            let _ = exit_event_tx.send(BackendEvent::MachineExited {
                machine_id: exit_machine_id,
                error_message,
            });
        });

        let ready_event_tx = event_tx;
        tokio::spawn(async move {
            match wait_for_machine_ready(&machine_name, boot_timeout).await {
                Ok(()) => {
                    let _ = ready_event_tx.send(BackendEvent::MachineReady { machine_id });
                }
                Err(err) => {
                    let _ = ready_event_tx.send(BackendEvent::MachineProvisionFailed {
                        machine_id,
                        error_message: format!("{err:#}"),
                    });
                }
            }
        });

        Ok(())
    }

    fn run_task(
        &self,
        task_id: String,
        machine_name: String,
        unit_name: String,
        spec: TaskSpec,
        event_tx: mpsc::UnboundedSender<BackendEvent>,
    ) -> anyhow::Result<()> {
        tokio::spawn(async move {
            let mut command = Command::new("systemd-run");
            command
                .args(systemd_run_task_args(&machine_name, &unit_name, &spec))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());

            let result = command.status().await;
            let event = match result {
                Ok(status) => BackendEvent::TaskFinished {
                    task_id,
                    exit_code: status.code(),
                    error_message: None,
                },
                Err(err) => BackendEvent::TaskFinished {
                    task_id,
                    exit_code: None,
                    error_message: Some(format!("failed to execute task through systemd-run: {err}")),
                },
            };
            let _ = event_tx.send(event);
        });

        Ok(())
    }

    async fn cancel_task(&self, machine_name: &str, unit_name: &str) -> anyhow::Result<()> {
        let status = Command::new("systemctl")
            .args(systemctl_cancel_args(machine_name, unit_name))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .with_context(|| format!("failed to run systemctl stop for {unit_name}"))?;

        if !status.success() {
            return Err(anyhow!(
                "systemctl stop for {unit_name} returned {}",
                render_status(status.code())
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq)]
enum SnapshotSupport {
    Fast,
    Slow(String),
}

fn detect_snapshot_support(root_directory: &Path) -> anyhow::Result<SnapshotSupport> {
    let filesystem_type = detect_filesystem_type(root_directory)?;
    snapshot_support_for_filesystem(root_directory, filesystem_type.as_str())
}

fn snapshot_support_for_filesystem(
    root_directory: &Path,
    filesystem_type: &str,
) -> anyhow::Result<SnapshotSupport> {
    match filesystem_type.trim() {
        "btrfs" => Ok(SnapshotSupport::Fast),
        "xfs" => match probe_xfs_reflink(root_directory) {
            Ok(true) => Ok(SnapshotSupport::Fast),
            Ok(false) => Ok(SnapshotSupport::Slow(format!(
                "{} is on xfs without reflink support, so systemd-nspawn --ephemeral may fall back to slow full copies.",
                root_directory.display()
            ))),
            Err(err) => Ok(SnapshotSupport::Slow(format!(
                "{} is on xfs, but shargon could not verify reflink support ({err:#}). systemd-nspawn --ephemeral may fall back to slow full copies.",
                root_directory.display()
            ))),
        },
        other => Ok(SnapshotSupport::Slow(format!(
            "{} is on {other}, not btrfs or reflink-capable xfs, so systemd-nspawn --ephemeral may fall back to slow full copies.",
            root_directory.display()
        ))),
    }
}

fn detect_filesystem_type(root_directory: &Path) -> anyhow::Result<String> {
    let output = std::process::Command::new("findmnt")
        .args(["-no", "FSTYPE", "--target"])
        .arg(root_directory)
        .output()
        .with_context(|| format!("failed to run findmnt for {}", root_directory.display()))?;

    if !output.status.success() {
        return Err(anyhow!(
            "findmnt returned {} while inspecting {}",
            render_status(output.status.code()),
            root_directory.display()
        ));
    }

    let filesystem_type = String::from_utf8(output.stdout)
        .context("findmnt output was not valid UTF-8")?
        .trim()
        .to_string();

    if filesystem_type.is_empty() {
        return Err(anyhow!(
            "findmnt returned an empty filesystem type for {}",
            root_directory.display()
        ));
    }

    Ok(filesystem_type)
}

fn probe_xfs_reflink(root_directory: &Path) -> anyhow::Result<bool> {
    let temp_dir = Builder::new()
        .prefix(".shargon-reflink-")
        .tempdir_in(root_directory)
        .with_context(|| {
            format!(
                "failed to create a reflink probe directory under {}",
                root_directory.display()
            )
        })?;
    let source_path = temp_dir.path().join("source");
    let target_path = temp_dir.path().join("target");

    fs::write(&source_path, b"shargon reflink probe")
        .with_context(|| format!("failed to write {}", source_path.display()))?;

    let output = std::process::Command::new("cp")
        .arg("--reflink=always")
        .arg(&source_path)
        .arg(&target_path)
        .output()
        .with_context(|| "failed to execute cp --reflink=always".to_string())?;

    Ok(output.status.success())
}

async fn wait_for_machine_ready(machine_name: &str, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;

    loop {
        let status = Command::new("systemd-run")
            .args(systemd_run_probe_args(machine_name))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        let last_error = match status {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => format!("readiness probe returned {}", render_status(status.code())),
            Err(err) => format!("readiness probe failed: {err}"),
        };

        if Instant::now() >= deadline {
            return Err(anyhow!(
                "timed out waiting for machine {machine_name} to become ready. last error: {last_error}"
            ));
        }

        sleep(READINESS_RETRY_INTERVAL).await;
    }
}

fn nspawn_spawn_args(root_directory: &Path, machine_name: &str) -> Vec<String> {
    vec![
        format!("--directory={}", root_directory.display()),
        format!("--machine={machine_name}"),
        "--ephemeral".to_string(),
        "--boot".to_string(),
        "--console=read-only".to_string(),
    ]
}

fn systemd_run_probe_args(machine_name: &str) -> Vec<String> {
    vec![
        format!("--machine={machine_name}"),
        "--wait".to_string(),
        "--quiet".to_string(),
        "--collect".to_string(),
        "--service-type=exec".to_string(),
        "/usr/bin/true".to_string(),
    ]
}

fn systemd_run_task_args(machine_name: &str, unit_name: &str, spec: &TaskSpec) -> Vec<String> {
    let mut args = vec![
        format!("--machine={machine_name}"),
        format!("--unit={unit_name}"),
        "--wait".to_string(),
        "--pipe".to_string(),
        "--collect".to_string(),
        "--service-type=exec".to_string(),
    ];

    if let Some(working_directory) = &spec.working_directory {
        args.push(format!("--working-directory={working_directory}"));
    }

    for EnvironmentVariable { name, value } in &spec.env {
        args.push(format!("--setenv={name}={value}"));
    }

    args.push("--".to_string());
    args.extend(spec.argv.clone());
    args
}

fn systemctl_cancel_args(machine_name: &str, unit_name: &str) -> Vec<String> {
    vec![
        format!("--machine={machine_name}"),
        "stop".to_string(),
        format!("{unit_name}.service"),
    ]
}

fn render_status(code: Option<i32>) -> String {
    code.map(|value| format!("exit code {value}"))
        .unwrap_or_else(|| "signal termination".to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn btrfs_is_detected_as_fast() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        assert_eq!(
            snapshot_support_for_filesystem(temp_dir.path(), "btrfs")?,
            SnapshotSupport::Fast
        );
        Ok(())
    }

    #[test]
    fn unsupported_filesystems_produce_warning() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let result = snapshot_support_for_filesystem(temp_dir.path(), "ext4")?;

        match result {
            SnapshotSupport::Fast => panic!("expected slow snapshot warning"),
            SnapshotSupport::Slow(message) => {
                assert!(message.contains("ext4"));
                assert!(message.contains("slow"));
            }
        }
        Ok(())
    }

    #[test]
    fn build_nspawn_command_matches_expected_flags() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let args = nspawn_spawn_args(temp_dir.path(), "shargon-machine-1");

        assert_eq!(
            args,
            vec![
                format!("--directory={}", temp_dir.path().display()),
                "--machine=shargon-machine-1".to_string(),
                "--ephemeral".to_string(),
                "--boot".to_string(),
                "--console=read-only".to_string(),
            ]
        );
        Ok(())
    }

    #[test]
    fn build_systemd_run_task_command_includes_env_and_workdir() {
        let args = systemd_run_task_args(
            "shargon-machine-1",
            "shargon-task-task-000001",
            &TaskSpec {
                argv: vec!["cargo".to_string(), "test".to_string()],
                env: vec![EnvironmentVariable {
                    name: "RUST_LOG".to_string(),
                    value: "debug".to_string(),
                }],
                working_directory: Some("/workspace".to_string()),
            },
        );

        assert_eq!(
            args,
            vec![
                "--machine=shargon-machine-1".to_string(),
                "--unit=shargon-task-task-000001".to_string(),
                "--wait".to_string(),
                "--pipe".to_string(),
                "--collect".to_string(),
                "--service-type=exec".to_string(),
                "--working-directory=/workspace".to_string(),
                "--setenv=RUST_LOG=debug".to_string(),
                "--".to_string(),
                "cargo".to_string(),
                "test".to_string(),
            ]
        );
    }

    #[test]
    fn replaces_failed_machine_to_keep_pool_size() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;

        runtime.block_on(async {
            let temp_dir = tempfile::tempdir()?;
            let system = Arc::new(FakeNspawnSystem::new(
                VecDeque::from(vec![SpawnOutcome::Fail("boom".to_string()), SpawnOutcome::Ready]),
                VecDeque::new(),
            ));
            let backend = NspawnBackend::new_with_system(
                NspawnSettings {
                    root_directory: Some(temp_dir.path().to_path_buf()),
                    machine_prefix: "ci".to_string(),
                    boot_timeout: Duration::from_secs(1),
                },
                system.clone(),
            )?;

            backend.reconcile_pool(1).await?;
            wait_until(|| async {
                backend
                    .list_machines()
                    .await
                    .map(|machines| machines.len() == 1 && machines[0].state == MachineState::Idle)
                    .unwrap_or(false)
            })
            .await?;

            assert_eq!(system.spawn_attempts(), 2);
            Ok(())
        })
    }

    #[test]
    fn executes_tasks_in_fifo_order() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;

        runtime.block_on(async {
            let temp_dir = tempfile::tempdir()?;
            let system = Arc::new(FakeNspawnSystem::new(
                VecDeque::from(vec![SpawnOutcome::Ready]),
                VecDeque::from(vec![
                    TaskOutcome::Finish(0),
                    TaskOutcome::Finish(0),
                ]),
            ));
            let backend = NspawnBackend::new_with_system(
                NspawnSettings {
                    root_directory: Some(temp_dir.path().to_path_buf()),
                    machine_prefix: "ci".to_string(),
                    boot_timeout: Duration::from_secs(1),
                },
                system.clone(),
            )?;

            backend.reconcile_pool(1).await?;
            wait_until(|| async {
                backend
                    .list_machines()
                    .await
                    .map(|machines| machines.iter().any(|machine| machine.state == MachineState::Idle))
                    .unwrap_or(false)
            })
            .await?;

            let first = backend
                .start_task(TaskSpec {
                    argv: vec!["echo".to_string(), "first".to_string()],
                    env: Vec::new(),
                    working_directory: None,
                })
                .await?;
            let second = backend
                .start_task(TaskSpec {
                    argv: vec!["echo".to_string(), "second".to_string()],
                    env: Vec::new(),
                    working_directory: None,
                })
                .await?;

            wait_until(|| async {
                backend
                    .list_tasks()
                    .await
                    .map(|tasks| tasks.iter().all(|task| task.state == TaskState::Succeeded))
                    .unwrap_or(false)
            })
            .await?;

            let executed = system.executed_tasks();
            assert_eq!(executed, vec![first.id, second.id]);
            Ok(())
        })
    }

    async fn wait_until<F, Fut>(mut predicate: F) -> anyhow::Result<()>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if predicate().await {
                return Ok(());
            }

            if Instant::now() >= deadline {
                return Err(anyhow!("condition was not met before timeout"));
            }

            sleep(Duration::from_millis(20)).await;
        }
    }

    struct FakeNspawnSystem {
        snapshot_diagnostics: Vec<Diagnostic>,
        spawn_outcomes: Mutex<VecDeque<SpawnOutcome>>,
        task_outcomes: Mutex<VecDeque<TaskOutcome>>,
        spawn_attempts: Mutex<usize>,
        executed_tasks: Mutex<Vec<String>>,
    }

    impl FakeNspawnSystem {
        fn new(spawn_outcomes: VecDeque<SpawnOutcome>, task_outcomes: VecDeque<TaskOutcome>) -> Self {
            Self {
                snapshot_diagnostics: Vec::new(),
                spawn_outcomes: Mutex::new(spawn_outcomes),
                task_outcomes: Mutex::new(task_outcomes),
                spawn_attempts: Mutex::new(0),
                executed_tasks: Mutex::new(Vec::new()),
            }
        }

        fn spawn_attempts(&self) -> usize {
            *self.spawn_attempts.lock().expect("spawn attempts mutex poisoned")
        }

        fn executed_tasks(&self) -> Vec<String> {
            self.executed_tasks
                .lock()
                .expect("executed tasks mutex poisoned")
                .clone()
        }
    }

    #[async_trait]
    impl NspawnSystem for FakeNspawnSystem {
        fn detect_snapshot_warning(&self, _root_directory: &Path) -> anyhow::Result<Vec<Diagnostic>> {
            Ok(self.snapshot_diagnostics.clone())
        }

        fn spawn_machine(
            &self,
            machine_id: String,
            _machine_name: String,
            _root_directory: PathBuf,
            _boot_timeout: Duration,
            event_tx: mpsc::UnboundedSender<BackendEvent>,
        ) -> anyhow::Result<()> {
            *self.spawn_attempts.lock().expect("spawn attempts mutex poisoned") += 1;

            let outcome = self
                .spawn_outcomes
                .lock()
                .expect("spawn outcomes mutex poisoned")
                .pop_front()
                .unwrap_or(SpawnOutcome::Ready);

            tokio::spawn(async move {
                match outcome {
                    SpawnOutcome::Ready => {
                        let _ = event_tx.send(BackendEvent::MachineReady { machine_id });
                    }
                    SpawnOutcome::Fail(error_message) => {
                        let _ = event_tx.send(BackendEvent::MachineProvisionFailed {
                            machine_id,
                            error_message,
                        });
                    }
                }
            });

            Ok(())
        }

        fn run_task(
            &self,
            task_id: String,
            _machine_name: String,
            _unit_name: String,
            _spec: TaskSpec,
            event_tx: mpsc::UnboundedSender<BackendEvent>,
        ) -> anyhow::Result<()> {
            self.executed_tasks
                .lock()
                .expect("executed tasks mutex poisoned")
                .push(task_id.clone());

            let outcome = self
                .task_outcomes
                .lock()
                .expect("task outcomes mutex poisoned")
                .pop_front()
                .unwrap_or(TaskOutcome::Finish(0));

            tokio::spawn(async move {
                match outcome {
                    TaskOutcome::Finish(exit_code) => {
                        let _ = event_tx.send(BackendEvent::TaskFinished {
                            task_id,
                            exit_code: Some(exit_code),
                            error_message: None,
                        });
                    }
                }
            });

            Ok(())
        }

        async fn cancel_task(&self, _machine_name: &str, _unit_name: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    enum SpawnOutcome {
        Ready,
        Fail(String),
    }

    enum TaskOutcome {
        Finish(i32),
    }
}
