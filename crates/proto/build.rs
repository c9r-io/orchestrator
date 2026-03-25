fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Rebuild when proto file or PROTOC env var changes
    println!("cargo:rerun-if-changed=orchestrator.proto");
    println!("cargo:rerun-if-env-changed=PROTOC");
    println!("cargo:rerun-if-env-changed=PROTOC_INCLUDE");

    if std::env::var_os("PROTOC")
        .filter(|p| std::path::Path::new(p).exists())
        .is_none()
    {
        let protoc = protoc_bin_vendored::protoc_bin_path()?;
        // SAFETY: build scripts run single-threaded before compilation.
        unsafe { std::env::set_var("PROTOC", &protoc) };
        println!(
            "cargo:warning=Using vendored protoc at {}",
            protoc.display()
        );
    }

    if std::env::var_os("PROTOC_INCLUDE")
        .filter(|p| std::path::Path::new(p).exists())
        .is_none()
    {
        let include = protoc_bin_vendored::include_path()?;
        // SAFETY: build scripts run single-threaded before compilation.
        unsafe { std::env::set_var("PROTOC_INCLUDE", &include) };
        println!(
            "cargo:warning=Using vendored protobuf include at {}",
            include.display()
        );
    }

    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let proto_file = manifest_dir.join("orchestrator.proto");
    tonic_prost_build::configure().compile_protos(&[&proto_file], &[&manifest_dir])?;
    Ok(())
}
