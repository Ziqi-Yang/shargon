fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::compile_protos("proto/shargon/v1/vm_service.proto")?;

    Ok(())
}
