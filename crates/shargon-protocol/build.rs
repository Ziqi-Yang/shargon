fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fds = protox::compile(["proto/shargon/v1/vm_service.proto"], ["proto"])?;
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_fds(fds)?;
    Ok(())
}
