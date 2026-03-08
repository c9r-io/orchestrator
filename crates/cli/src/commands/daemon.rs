use crate::DaemonCommands;
use anyhow::{Context, Result};

pub async fn run(cmd: DaemonCommands) -> Result<()> {
    match cmd {
        DaemonCommands::Start {
            foreground,
            bind,
            workers: _,
        } => {
            let daemon_binary = find_daemon_binary()?;

            let mut args = Vec::new();
            if foreground {
                args.push("--foreground".to_string());
            }
            if let Some(addr) = bind {
                args.push("--bind".to_string());
                args.push(addr);
            }

            if foreground {
                // Run in foreground with restart loop — if daemon exits with code 75,
                // relaunch it (self-restart after binary rebuild).
                const EXIT_RESTART: i32 = 75;
                loop {
                    let status = std::process::Command::new(&daemon_binary)
                        .args(&args)
                        .status()
                        .with_context(|| {
                            format!("failed to start daemon: {}", daemon_binary.display())
                        })?;
                    let code = status.code().unwrap_or(1);
                    if code == EXIT_RESTART {
                        eprintln!(
                            "[orchestrator] restart requested (exit {}) — re-launching daemon",
                            EXIT_RESTART
                        );
                        // Verify daemon binary still exists
                        if !daemon_binary.exists() {
                            eprintln!(
                                "[orchestrator] daemon binary missing after restart — rebuilding"
                            );
                            let rebuild = std::process::Command::new("cargo")
                                .args(["build", "--release", "-p", "orchestratord"])
                                .status();
                            if let Err(e) = rebuild {
                                eprintln!("[orchestrator] rebuild failed: {}", e);
                                std::process::exit(1);
                            }
                        }
                        continue;
                    }
                    std::process::exit(code);
                }
            } else {
                // Daemonize — spawn and detach
                let child = std::process::Command::new(&daemon_binary)
                    .args(&args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .with_context(|| {
                        format!("failed to spawn daemon: {}", daemon_binary.display())
                    })?;
                println!("Daemon started (pid: {})", child.id());
            }
            Ok(())
        }

        DaemonCommands::Stop => {
            let app_root = detect_app_root();
            let pid_path = app_root.join("data/daemon.pid");
            let socket_path = app_root.join("data/orchestrator.sock");

            if let Some(pid) = read_pid(&pid_path) {
                #[cfg(unix)]
                {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                    println!("Sent SIGTERM to daemon (pid: {})", pid);
                }
                // Wait briefly, then clean up
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let _ = std::fs::remove_file(&pid_path);
                let _ = std::fs::remove_file(&socket_path);
            } else {
                println!("No daemon PID file found. Daemon may not be running.");
            }
            Ok(())
        }

        DaemonCommands::Status => {
            let app_root = detect_app_root();
            let pid_path = app_root.join("data/daemon.pid");
            let socket_path = app_root.join("data/orchestrator.sock");

            if let Some(pid) = read_pid(&pid_path) {
                #[cfg(unix)]
                let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
                #[cfg(not(unix))]
                let alive = false;

                if alive {
                    println!("Daemon is running (pid: {})", pid);
                    println!("  Socket: {}", socket_path.display());

                    // Try to ping
                    match crate::client::connect().await {
                        Ok(mut client) => {
                            match client.ping(orchestrator_proto::PingRequest {}).await {
                                Ok(resp) => {
                                    let r = resp.into_inner();
                                    println!("  Version: {} ({})", r.version, r.git_hash);
                                }
                                Err(e) => {
                                    println!("  Ping failed: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            println!("  Connection failed: {}", e);
                        }
                    }
                } else {
                    println!("Daemon is not running (stale PID file: {})", pid);
                }
            } else {
                println!("Daemon is not running (no PID file)");
            }
            Ok(())
        }

        DaemonCommands::Restart => {
            // Stop then start
            let stop_cmd = DaemonCommands::Stop;
            Box::pin(run(stop_cmd)).await?;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let start_cmd = DaemonCommands::Start {
                foreground: false,
                bind: None,
                workers: 1,
            };
            Box::pin(run(start_cmd)).await
        }
    }
}

fn find_daemon_binary() -> Result<std::path::PathBuf> {
    // Look for orchestratord next to orchestrator binary
    if let Ok(self_path) = std::env::current_exe() {
        let dir = self_path.parent().unwrap_or(std::path::Path::new("."));
        let candidate = dir.join("orchestratord");
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    // Fall back to PATH
    Ok(std::path::PathBuf::from("orchestratord"))
}

fn detect_app_root() -> std::path::PathBuf {
    std::env::var("ORCHESTRATOR_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
}

fn read_pid(path: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}
