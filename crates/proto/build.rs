fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Rebuild when PROTOC env var changes
    println!("cargo:rerun-if-env-changed=PROTOC");

    // If PROTOC is not set or does not point to a valid binary, use protobuf-src
    if std::env::var("PROTOC")
        .ok()
        .filter(|p| std::path::Path::new(p).exists())
        .is_none()
    {
        let protoc = protobuf_src::protoc();
        std::env::set_var("PROTOC", &protoc);
        println!(
            "cargo:warning=Using protobuf-src protoc at {}",
            protoc.display()
        );
    }

    tonic_prost_build::compile_protos("../../proto/orchestrator.proto")?;
    Ok(())
}
