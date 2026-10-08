use gemini_live::types::FunctionDeclaration;
use rmcp::{
    RoleClient, ServiceExt,
    model::CallToolRequestParams,
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use sofia_config::{McpServerConfig, McpTransport};
use std::{collections::HashMap, sync::Arc, time::Duration};

pub struct ConnectedServer {
    pub tools: Vec<rmcp::model::Tool>,
    service: RunningService<RoleClient, ()>,
}
impl ConnectedServer {
    pub async fn connect(config: &McpServerConfig) -> Result<Self, String> {
        config.validate()?;
        tokio::time::timeout(Duration::from_secs(15), async {
            let service = match &config.transport {
                McpTransport::Stdio { command, args, env } => {
                    let mut cmd = tokio::process::Command::new(command);
                    cmd.args(args).envs(env);
                    ().serve(
                        TokioChildProcess::new(cmd).map_err(|_| "MCP process could not start")?,
                    )
                    .await
                }
                McpTransport::Http { url, bearer_token } => {
                    let client = reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .connect_timeout(Duration::from_secs(10))
                        .build()
                        .map_err(|_| "MCP HTTP client could not start")?;
                    let mut transport_config = StreamableHttpClientTransportConfig::default();
                    transport_config.uri = url.clone().into();
                    transport_config.auth_header =
                        (!bearer_token.trim().is_empty()).then(|| bearer_token.clone());
                    let transport =
                        StreamableHttpClientTransport::with_client(client, transport_config);
                    ().serve(transport).await
                }
            }
            .map_err(|_| "MCP handshake failed; check the server and credentials")?;
            let tools = service
                .list_all_tools()
                .await
                .map_err(|_| "MCP tools could not be listed")?;
            Ok(Self { service, tools })
        })
        .await
        .map_err(|_| "MCP connection/discovery timed out after 15 seconds".to_string())?
    }
    pub async fn call(&self, name: String, args: Value) -> Result<Value, String> {
        let args = args
            .as_object()
            .cloned()
            .ok_or("Tool arguments must be an object")?;
        let result = tokio::time::timeout(
            Duration::from_secs(60),
            self.service
                .call_tool(CallToolRequestParams::new(name).with_arguments(args)),
        )
        .await
        .map_err(|_| "MCP tool timed out after 60 seconds")?
        .map_err(|_| "MCP tool request failed; execution outcome may be unknown")?;
        let result = serde_json::to_value(result).map_err(|_| "MCP result could not be encoded")?;
        if serde_json::to_vec(&result)
            .map_err(|_| "Invalid MCP result")?
            .len()
            > 128 * 1024
        {
            return Err(
                "Tool completed, but its result exceeds the 128 KiB model response limit".into(),
            );
        }
        Ok(result)
    }
}

pub struct ServerReport {
    pub id: String,
    pub tools: usize,
    pub error: Option<String>,
}
struct Binding {
    server: Arc<ConnectedServer>,
    original: String,
}
#[derive(Default)]
pub struct McpBridge {
    declarations: Vec<FunctionDeclaration>,
    bindings: HashMap<String, Binding>,
}
impl McpBridge {
    pub async fn connect(configs: &[McpServerConfig]) -> (Self, Vec<ServerReport>) {
        let results = futures_util::future::join_all(
            configs
                .iter()
                .filter(|config| config.enabled)
                .map(|config| async move { (config, ConnectedServer::connect(config).await) }),
        )
        .await;
        let mut bridge = Self::default();
        let mut reports = Vec::new();
        for (config, result) in results {
            let mut report = ServerReport {
                id: config.id.clone(),
                tools: 0,
                error: None,
            };
            match result {
                Err(error) => report.error = Some(error),
                Ok(server) => {
                    let server = Arc::new(server);
                    for tool in &server.tools {
                        if config
                            .disabled_tools
                            .iter()
                            .any(|name| name == tool.name.as_ref())
                        {
                            continue;
                        }
                        let name = tool_name(&config.id, &tool.name);
                        if bridge.bindings.contains_key(&name) {
                            report.error = Some("Duplicate tool identity".into());
                            continue;
                        }
                        let schema = Value::Object(tool.input_schema.as_ref().clone());
                        let parameters = match crate::schema::convert(&schema) {
                            Ok(parameters) => parameters,
                            Err(_) => {
                                report.error = Some(
                                    "One or more tools have unsupported parameter schemas".into(),
                                );
                                continue;
                            }
                        };
                        bridge.declarations.push(FunctionDeclaration {
                            name: name.clone(),
                            description: format!(
                                "MCP server {}: {}",
                                config.id,
                                tool.description.as_deref().unwrap_or(&tool.name)
                            ),
                            parameters,
                            scheduling: None,
                            behavior: None,
                        });
                        bridge.bindings.insert(
                            name,
                            Binding {
                                server: server.clone(),
                                original: tool.name.to_string(),
                            },
                        );
                        report.tools += 1;
                    }
                }
            }
            reports.push(report);
        }
        (bridge, reports)
    }
    pub fn declarations(&self) -> Vec<FunctionDeclaration> {
        self.declarations.clone()
    }
    pub async fn call(&self, name: &str, args: Value) -> Value {
        let result = match self.bindings.get(name) {
            Some(binding) => binding.server.call(binding.original.clone(), args).await,
            None => Err("Unknown or disabled MCP tool".into()),
        };
        result.unwrap_or_else(|error| json!({"isError":true,"error":error}))
    }
}

pub fn tool_name(server: &str, tool: &str) -> String {
    let encoded: String = tool
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                (byte as char).to_string()
            } else {
                format!("_{byte:02x}")
            }
        })
        .collect();
    let name = format!("mcp_{server}__{encoded}");
    if name.len() <= 64 {
        name
    } else {
        let hash = tool.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
        format!("mcp_{server}__h{hash:016x}")
    }
}
