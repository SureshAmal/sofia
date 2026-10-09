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
    assert_eq!(client.list_all_tools().await.unwrap().len(), 14);
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

    // 1. Test sofia_window_protocol
    let proto_res = client
        .call_tool(
            CallToolRequestParams::new("sofia_window_protocol")
                .with_arguments(serde_json::Map::new()),
        )
        .await
        .unwrap();
    let proto_val: serde_json::Value = serde_json::from_str(
        serde_json::to_value(proto_res).unwrap()["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        proto_val["chart_types"],
        json!(["line", "bar", "area", "pie", "radar"])
    );
    assert_eq!(
        proto_val["placements"],
        json!([
            "pill",
            "center",
            "left",
            "right",
            "bottom",
            "top_left",
            "top_right",
            "bottom_left",
            "bottom_right"
        ])
    );
    assert!(proto_val["inline_editing_rules"].is_string());
    assert!(proto_val["window_tracking"].is_string());

    // 2. Test sofia_create_document with placement
    let create_chart = client
        .call_tool(
            CallToolRequestParams::new("sofia_create_document").with_arguments(
                json!({
                    "title": "My Chart",
                    "content": {
                        "kind": "chart",
                        "chart_type": "pie",
                        "points": [{"label": "Slice A", "value": 42.0}]
                    },
                    "placement": "top_right",
                    "open": true
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    let chart_doc: serde_json::Value = serde_json::from_str(
        serde_json::to_value(create_chart).unwrap()["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let chart_id = chart_doc["id"].as_str().unwrap();
    let tags = chart_doc["tags"].as_array().unwrap();
    assert!(tags.iter().any(|t| t == "pos:top_right"));
    assert_eq!(chart_doc["open"], true);

    // 3. Test sofia_open_window placement update (replaces previous pos: tag)
    let open_res = client
        .call_tool(
            CallToolRequestParams::new("sofia_open_window").with_arguments(
                json!({
                    "id": chart_id,
                    "placement": "bottom_left"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    assert_ne!(open_res.is_error, Some(true));
    let fetched_chart = client
        .call_tool(
            CallToolRequestParams::new("sofia_get_document")
                .with_arguments(json!({"id": chart_id}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    let updated_chart: serde_json::Value = serde_json::from_str(
        serde_json::to_value(fetched_chart).unwrap()["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let updated_tags = updated_chart["tags"].as_array().unwrap();
    assert!(updated_tags.iter().any(|t| t == "pos:bottom_left"));
    assert!(!updated_tags.iter().any(|t| t == "pos:top_right"));

    // 4. Test sofia_list_windows with include_closed = false
    let list_windows_open = client
        .call_tool(
            CallToolRequestParams::new("sofia_list_windows").with_arguments(
                json!({"include_closed": false})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let win_val: serde_json::Value = serde_json::from_str(
        serde_json::to_value(list_windows_open).unwrap()["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(win_val["open_count"], 1);
    assert_eq!(win_val["closed_count"], 1); // Note doc was open: false, chart was open: true
    assert_eq!(win_val["open"].as_array().unwrap().len(), 1);
    assert_eq!(win_val["closed"].as_array().unwrap().len(), 0);
    assert_eq!(win_val["open"][0]["open"], true);

    // 5. Test sofia_list_windows with include_closed = true
    let list_windows_all = client
        .call_tool(
            CallToolRequestParams::new("sofia_list_windows")
                .with_arguments(json!({"include_closed": true}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    let win_all_val: serde_json::Value = serde_json::from_str(
        serde_json::to_value(list_windows_all).unwrap()["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(win_all_val["open_count"], 1);
    assert_eq!(win_all_val["closed_count"], 1);
    assert_eq!(win_all_val["open"].as_array().unwrap().len(), 1);
    assert_eq!(win_all_val["closed"].as_array().unwrap().len(), 1);
    assert_eq!(win_all_val["closed"][0]["open"], false);

    // 6. Test sofia_list_documents contains explicit open: bool
    let list_docs = client
        .call_tool(
            CallToolRequestParams::new("sofia_list_documents")
                .with_arguments(serde_json::Map::new()),
        )
        .await
        .unwrap();
    let docs_val: serde_json::Value = serde_json::from_str(
        serde_json::to_value(list_docs).unwrap()["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    for item in docs_val.as_array().unwrap() {
        assert!(item["open"].is_boolean());
    }

    // 7. Test sofia_delete_documents
    let del_res = client
        .call_tool(
            CallToolRequestParams::new("sofia_delete_documents")
                .with_arguments(json!({"ids": [id]}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    let del_val: serde_json::Value = serde_json::from_str(
        serde_json::to_value(del_res).unwrap()["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(del_val["deleted_count"], 1);
    assert_eq!(del_val["deleted"][0], id);

    client.cancel().await.unwrap();
    server.cancel().await.unwrap();
    let _ = std::fs::remove_dir_all(directory);
}
