fn main() {
    // BUILD_TIMESTAMP — UTC ISO 8601
    let ts = std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into());
    println!("cargo:rustc-env=BUILD_TIMESTAMP={ts}");

    // BUILD_GIT_HASH — short hash + optional -dirty suffix
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let dirty = std::process::Command::new("git")
        .args(["diff", "--quiet"])
        .status()
        .map(|s| if s.success() { "" } else { "-dirty" })
        .unwrap_or("");
    println!("cargo:rustc-env=BUILD_GIT_HASH={hash}{dirty}");

    // Rebuild when git HEAD changes or source changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=src/");
}
