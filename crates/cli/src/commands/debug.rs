use std::fs::File;
use std::io::Write;
use std::net::ToSocketAddrs;
use std::process::{self, Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::{DebugCommands, SandboxProbeCommands};

const PROBE_PREFIX: &str = "SANDBOX_PROBE";
const PAGE_SIZE: usize = 4096;

pub async fn run_local(command: DebugCommands) -> Result<()> {
    match command {
        DebugCommands::SandboxProbe { probe } => run_probe(probe),
        DebugCommands::ChildIdle { sleep_secs } => {
            std::thread::sleep(Duration::from_secs(sleep_secs));
            Ok(())
        }
    }
}

fn run_probe(probe: SandboxProbeCommands) -> Result<()> {
    match probe {
        SandboxProbeCommands::WriteFile { path, contents } => {
            let mut file = File::create(&path)
                .with_context(|| format!("failed to create probe file '{}'", path))?;
            file.write_all(contents.as_bytes())
                .with_context(|| format!("failed to write probe file '{}'", path))?;
            Ok(())
        }
        SandboxProbeCommands::OpenFiles { count } => open_files_probe(count),
        SandboxProbeCommands::CpuBurn => cpu_burn_probe(),
        SandboxProbeCommands::AllocMemory { chunk_mb, total_mb } => {
            alloc_memory_probe(chunk_mb, total_mb)
        }
        SandboxProbeCommands::SpawnChildren { count, sleep_secs } => {
            spawn_children_probe(count, sleep_secs)
        }
        SandboxProbeCommands::DnsResolve { host, port } => dns_resolve_probe(&host, port),
    }
}

fn open_files_probe(count: usize) -> Result<()> {
    let mut files = Vec::with_capacity(count);
    for _ in 0..count {
        match File::open("/dev/null") {
            Ok(file) => files.push(file),
            Err(err) => exit_with_probe_failure(&format!(
                "{PROBE_PREFIX} resource=open_files reason_code=open_files_limit_exceeded error={}",
                sanitize_value(&err.to_string())
            )),
        }
    }
    Ok(())
}

fn cpu_burn_probe() -> Result<()> {
    let mut value: u64 = 0;
    loop {
        value = std::hint::black_box(value.wrapping_mul(1_664_525).wrapping_add(1_013_904_223));
    }
}

fn alloc_memory_probe(chunk_mb: usize, total_mb: usize) -> Result<()> {
    let chunk_bytes = chunk_mb.saturating_mul(1024 * 1024);
    let iterations = std::cmp::max(1, total_mb / std::cmp::max(1, chunk_mb));
    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let mut block = Vec::new();
        if let Err(err) = block.try_reserve_exact(chunk_bytes) {
            exit_with_probe_failure(&format!(
                "{PROBE_PREFIX} resource=memory reason_code=memory_limit_exceeded error={}",
                sanitize_value(&err.to_string())
            ));
        }
        unsafe {
            block.set_len(chunk_bytes);
        }
        for index in (0..chunk_bytes).step_by(PAGE_SIZE) {
            block[index] = 1;
        }
        blocks.push(block);
    }
    Ok(())
}

fn spawn_children_probe(count: usize, sleep_secs: u64) -> Result<()> {
    let current_exe = std::env::current_exe().context("resolve current executable for probe")?;
    let mut children: Vec<Child> = Vec::new();
    for _ in 0..count {
        let spawn_result = Command::new(&current_exe)
            .arg("debug")
            .arg("child-idle")
            .arg("--sleep-secs")
            .arg(sleep_secs.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawn_result {
            Ok(child) => children.push(child),
            Err(err) => {
                cleanup_children(&mut children);
                exit_with_probe_failure(&format!(
                    "{PROBE_PREFIX} resource=processes reason_code=processes_limit_exceeded error={}",
                    sanitize_value(&err.to_string())
                ));
            }
        }
    }
    cleanup_children(&mut children);
    Ok(())
}

fn dns_resolve_probe(host: &str, port: u16) -> Result<()> {
    let target = format!("{host}:{port}");
    if let Err(err) = target.to_socket_addrs() {
        exit_with_probe_failure(&format!(
            "{PROBE_PREFIX} network=blocked reason_code=network_blocked target={} error={}",
            sanitize_value(host),
            sanitize_value(&err.to_string())
        ));
    }
    Ok(())
}

fn cleanup_children(children: &mut [Child]) {
    for child in children.iter_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn sanitize_value(value: &str) -> String {
    value.replace(char::is_whitespace, "_")
}

fn exit_with_probe_failure(message: &str) -> ! {
    eprintln!("{message}");
    process::exit(1);
}
