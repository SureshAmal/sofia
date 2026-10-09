use rmcp::ServiceExt;
use sofia_memory::{mcp::SofiaMemoryMcp, MemoryStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // Direct CLI command support
    if args.len() > 1 && args[1] == "recall" {
        let query = args.get(2).map(|s| s.as_str()).unwrap_or("");
        let store = MemoryStore::open(MemoryStore::default_path()?)?;
        let res = store.recall(query, 5)?;
        println!("{}", serde_json::to_string_pretty(&res)?);
        return Ok(());
    }

    if args.len() > 1 && args[1] == "preferences" {
        let store = MemoryStore::open(MemoryStore::default_path()?)?;
        let prefs = store.list_user_preferences()?;
        println!("{}", serde_json::to_string_pretty(&prefs)?);
        return Ok(());
    }

    // Default mode: MCP Stdio Service for Sofia / Gemini
    let store = MemoryStore::open(MemoryStore::default_path()?)?;
    let server = SofiaMemoryMcp::new(store)
        .serve(rmcp::transport::stdio())
        .await?;
    server.waiting().await?;
    Ok(())
}
