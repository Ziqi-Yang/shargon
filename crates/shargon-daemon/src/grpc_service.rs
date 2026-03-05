use shargon_protocol::vm_service::{
    CreateVmRequest, CreateVmResponse, DeleteVmRequest, DeleteVmResponse, ListVmsRequest,
    ListVmsResponse, PingRequest, PingResponse, StartVmRequest, StartVmResponse, StopVmRequest,
    StopVmResponse, vm_service_server::VmService,
};
use tonic::{Request, Response, Status};

#[derive(Debug, Default)]
pub struct VmServiceImpl;

#[tonic::async_trait]
impl VmService for VmServiceImpl {
    async fn ping(&self, _request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse {
            version: shargon_version::current_version_line!(),
        }))
    }

    async fn create_vm(
        &self,
        _request: Request<CreateVmRequest>,
    ) -> Result<Response<CreateVmResponse>, Status> {
        Ok(Response::new(CreateVmResponse {
            vm_id: String::new(),
        }))
    }

    async fn start_vm(
        &self,
        _request: Request<StartVmRequest>,
    ) -> Result<Response<StartVmResponse>, Status> {
        Ok(Response::new(StartVmResponse {}))
    }

    async fn stop_vm(
        &self,
        _request: Request<StopVmRequest>,
    ) -> Result<Response<StopVmResponse>, Status> {
        Ok(Response::new(StopVmResponse {}))
    }

    async fn delete_vm(
        &self,
        _request: Request<DeleteVmRequest>,
    ) -> Result<Response<DeleteVmResponse>, Status> {
        Ok(Response::new(DeleteVmResponse {}))
    }

    async fn list_vms(
        &self,
        _request: Request<ListVmsRequest>,
    ) -> Result<Response<ListVmsResponse>, Status> {
        Ok(Response::new(ListVmsResponse { vms: vec![] }))
    }
}
