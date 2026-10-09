#[tokio::main]
async fn main() {
    let settings = sofia_config::load().expect("settings");
    let config = settings
        .mcp_servers
        .iter()
        .find(|server| server.id == "gmail")
        .expect("gmail config");
    match sofia_mcp_client::ConnectedServer::connect(config).await {
        Ok(server) => println!("connected: {} tools", server.tools.len()),
        Err(error) => println!("error: {error}"),
    }
}
