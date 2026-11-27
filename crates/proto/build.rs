fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure().compile_protos(&["proto/nitya/v1/devices.proto", "proto/nitya/v1/keys.proto"], &["proto"])?;
    Ok(())
}
