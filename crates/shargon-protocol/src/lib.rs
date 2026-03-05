pub mod vm_service {
    tonic::include_proto!("shargon.v1");
}

/// Shared Unix socket path for daemon ↔ client communication.
pub const SOCKET_PATH: &str = "/tmp/shargon-daemon.sock";
