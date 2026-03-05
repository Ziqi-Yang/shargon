use shargon_protocol::vm_service::{PingRequest, PingResponse, vm_service_server::VmService};
use tonic::{Request, Response, Status};

#[derive(Debug, Default)]
pub struct VmServiceImpl;

#[tonic::async_trait]
impl VmService for VmServiceImpl {
    async fn ping(&self, _request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse {
            msg: "pong".to_string(),
        }))
    }
}
