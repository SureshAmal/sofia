use serde_json::json;
use sofia_content::Store;
#[tokio::test]
async fn documents_roundtrip_through_real_mcp_client() {
    use rmcp::{ServiceExt, model::CallToolRequestParams};
    let directory = std::env::temp_dir().join(format!("sofia-mcp-test-{}", uuid::Uuid::new_v4()));
    let store = Store::open(directory.join("content.db")).unwrap();
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let server = tokio::spawn(async move {
        sofia_mcp::SofiaMcp::new(store)
            .serve(server_io)
            .await
            .unwrap()
    });
    let client = ().serve(client_io).await.unwrap();
    let server = server.await.unwrap();
    assert_eq!(client.list_all_tools().await.unwrap().len(), 8);
    let created=client.call_tool(CallToolRequestParams::new("sofia_create_document").with_arguments(json!({"title":"Test note","content":{"kind":"note","markdown":"Hello **world**"},"open":false}).as_object().unwrap().clone())).await.unwrap();
    let result = created;
    let encoded = serde_json::to_value(result).unwrap();
    let doc: serde_json::Value =
        serde_json::from_str(encoded["content"][0]["text"].as_str().unwrap()).unwrap();
    let id = doc["id"].as_str().unwrap();
    let request =
        json!({"id":id,"expected_revision":1,"content":{"kind":"note","markdown":"Updated"}});
    let first = client
        .call_tool(
            CallToolRequestParams::new("sofia_update_document")
                .with_arguments(request.as_object().unwrap().clone()),
        )
        .await
        .unwrap();

    assert_ne!(first.is_error, Some(true));
    let second = client
        .call_tool(
            CallToolRequestParams::new("sofia_update_document")
                .with_arguments(request.as_object().unwrap().clone()),
        )
        .await
        .unwrap();

    assert_eq!(second.is_error, Some(true));
    client.cancel().await.unwrap();
    server.cancel().await.unwrap();
    let _ = std::fs::remove_dir_all(directory);
}
