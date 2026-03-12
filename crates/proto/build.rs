fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Rebuild when PROTOC env var changes
    println!("cargo:rerun-if-env-changed=PROTOC");
    println!("cargo:rerun-if-env-changed=PROTOC_INCLUDE");

    if std::env::var_os("PROTOC")
        .filter(|p| std::path::Path::new(p).exists())
        .is_none()
    {
        let protoc = protoc_bin_vendored::protoc_bin_path()?;
        std::env::set_var("PROTOC", &protoc);
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
        std::env::set_var("PROTOC_INCLUDE", &include);
        println!(
            "cargo:warning=Using vendored protobuf include at {}",
            include.display()
        );
    }

    tonic_prost_build::compile_protos("../../proto/orchestrator.proto")?;
    Ok(())
}
