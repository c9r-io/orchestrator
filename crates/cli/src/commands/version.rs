use anyhow::Result;

pub async fn run(control_plane_config: Option<&str>) -> Result<()> {
    // Try to get version from daemon first
    match crate::client::connect(control_plane_config).await {
        Ok(mut client) => match client.ping(orchestrator_proto::PingRequest {}).await {
            Ok(resp) => {
                let r = resp.into_inner();
                println!("Client:  {}", env!("CARGO_PKG_VERSION"));
                println!("Daemon:  {} ({})", r.version, r.git_hash);
            }
            Err(_) => {
                println!("Client:  {}", env!("CARGO_PKG_VERSION"));
                println!("Daemon:  not connected");
            }
        },
        Err(_) => {
            println!("Client:  {}", env!("CARGO_PKG_VERSION"));
            println!("Daemon:  not running");
        }
    }
    Ok(())
}
