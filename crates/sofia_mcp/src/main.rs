use rmcp::ServiceExt;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = sofia_content::Store::open(sofia_content::Store::default_path()?)?;
    let server = sofia_mcp::SofiaMcp::new(store)
        .serve(rmcp::transport::stdio())
        .await?;
    server.waiting().await?;
    Ok(())
}
