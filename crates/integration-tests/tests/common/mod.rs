use std::path::PathBuf;

fn manifests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/common/manifests")
}

pub fn load_manifest(name: &str) -> String {
    let path = manifests_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read manifest {}: {}", path.display(), e))
}
