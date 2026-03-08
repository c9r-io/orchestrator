use anyhow::Result;

pub async fn run() -> Result<()> {
    // Try to get version from daemon first
    match crate::client::connect().await {
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
