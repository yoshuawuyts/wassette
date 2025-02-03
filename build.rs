fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .compile_protos(&["protos/service.proto"], &["protos"])?;
    Ok(())
}
