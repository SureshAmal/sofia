use serde_json::json;
use sofia_config::{McpServerConfig, McpTransport};
use sofia_mcp_client::{McpBridge, tool_name};
fn fixture(id: &str) -> McpServerConfig {
    McpServerConfig {
        name: String::new(),
        id: id.into(),
        enabled: true,
        transport: McpTransport::Stdio {
            command: "python3".into(),
            args: vec![format!(
                "{}/tests/fixtures/server.py",
                env!("CARGO_MANIFEST_DIR")
            )],
            env: Default::default(),
        },
        disabled_tools: vec![],
    }
}
#[tokio::test]
async fn discovers_pages_routes_servers_and_filters_tools() {
    let first = fixture("first");
    let mut second = fixture("second");
    second.disabled_tools = vec!["second".into()];
    let mut disabled = fixture("disabled");
    disabled.enabled = false;
    let (bridge, reports) = McpBridge::connect(&[first, second, disabled]).await;
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|report| report.error.is_none()));
    assert_eq!(bridge.declarations().len(), 3);
    for server in ["first", "second"] {
        let result = bridge
            .call(&tool_name(server, "echo"), json!({"text":server}))
            .await;
        assert_eq!(result["content"][0]["text"], server);
        assert_eq!(result["isError"], false);
    }
    assert_eq!(
        bridge.call(&tool_name("second", "second"), json!({})).await["isError"],
        true
    );
}
#[test]
fn tool_names_are_stable_and_distinct() {
    assert_eq!(
        tool_name("sofia", "sofia_list_documents"),
        "mcp_sofia_sofia_list_documents"
    );
    assert_ne!(tool_name("one", "a-b"), tool_name("one", "a_2db"));
    assert_ne!(tool_name("one", "echo"), tool_name("two", "echo"));
    assert!(tool_name("server", &"x".repeat(100)).len() <= 64);
}
#[tokio::test]
async fn old_declared_names_remain_routable() {
    let (bridge, _) = McpBridge::connect(&[fixture("sofia")]).await;
    assert_eq!(
        bridge
            .call("mcp_sofia__echo", json!({"text":"legacy"}))
            .await["content"][0]["text"],
        "legacy"
    );
}
#[tokio::test]
async fn failure_is_isolated() {
    let mut broken = fixture("broken");
    broken.transport = McpTransport::Stdio {
        command: "/nonexistent/sofia-mcp-fixture".into(),
        args: vec![],
        env: Default::default(),
    };
    let (bridge, reports) = McpBridge::connect(&[broken, fixture("healthy")]).await;
    assert!(reports[0].error.is_some());
    assert_eq!(bridge.declarations().len(), 2);
}

#[tokio::test]
async fn streamable_http_sends_bearer_and_calls_tool() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut process = tokio::process::Command::new("python3")
        .arg(format!(
            "{}/tests/fixtures/http_server.py",
            env!("CARGO_MANIFEST_DIR")
        ))
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut output = BufReader::new(process.stdout.take().unwrap()).lines();
    let port = output.next_line().await.unwrap().unwrap();
    let config = McpServerConfig {
        name: String::new(),
        id: "remote".into(),
        enabled: true,
        transport: McpTransport::Http {
            url: format!("http://127.0.0.1:{port}/mcp"),
            bearer_token: "fixture-token".into(),
            oauth: None,
        },
        disabled_tools: vec![],
    };
    let (bridge, reports) = McpBridge::connect(&[config]).await;
    assert!(reports[0].error.is_none(), "{:?}", reports[0].error);
    assert_eq!(
        bridge
            .call(&tool_name("remote", "echo"), json!({"text":"http works"}))
            .await["content"][0]["text"],
        "http works"
    );
}
