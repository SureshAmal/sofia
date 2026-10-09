use base64::{Engine as _, engine::general_purpose::STANDARD};
use gemini_live::types::FunctionDeclaration;
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, RoleClient,
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
    service: RunningService<RoleClient, rmcp::model::InitializeRequestParams>,
}

#[derive(Debug)]
pub struct ToolImage {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[derive(Debug)]
pub struct ToolOutput {
    pub response: Value,
    pub images: Vec<ToolImage>,
}

impl ConnectedServer {
    pub async fn connect(config: &McpServerConfig) -> Result<Self, String> {
        config.validate()?;
        tokio::time::timeout(Duration::from_secs(15), async {
            let mut client_config = rmcp::model::InitializeRequestParams::new(
                rmcp::model::ClientCapabilities::default(),
                rmcp::model::Implementation::new("sofia", env!("CARGO_PKG_VERSION")),
            );
            client_config.protocol_version = rmcp::model::ProtocolVersion::V_2024_11_05;
            let service = match &config.transport {
                McpTransport::Stdio { command, args, env } => {
                    let mut cmd = tokio::process::Command::new(command);
                    cmd.args(args).envs(env);
                    client_config
                        .clone()
                        .serve_with_lifecycle(
                            TokioChildProcess::new(cmd).map_err(|_| "MCP process could not start")?,
                            ClientLifecycleMode::Initialize,
                        )
                        .await
                }
                McpTransport::Http {
                    url,
                    oauth: Some(oauth),
                    ..
                } => {
                    let client = crate::oauth::client(url, oauth).await?;
                    let mut transport_config = StreamableHttpClientTransportConfig::default();
                    transport_config.uri = url.clone().into();
                    let transport =
                        StreamableHttpClientTransport::with_client(client, transport_config);
                    client_config
                        .clone()
                        .serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
                        .await
                }
                McpTransport::Http {
                    url,
                    bearer_token,
                    oauth: None,
                } => {
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
                    client_config
                        .serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
                        .await
                }
            }
            .map_err(|error| format!("MCP handshake failed: {error}"))?;
            let tools = service
                .list_all_tools()
                .await
                .map_err(|_| "MCP tools could not be listed")?;
            Ok(Self { service, tools })
        })
        .await
        .map_err(|_| "MCP connection/discovery timed out after 15 seconds".to_string())?
    }
    pub async fn call(&self, name: String, args: Value) -> Result<ToolOutput, String> {
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
        let images = result
            .content
            .iter()
            .filter_map(|content| content.as_image())
            .map(|image| {
                let bytes = STANDARD
                    .decode(&image.data)
                    .map_err(|_| "MCP returned invalid base64 image data")?;
                if bytes.len() > 20 * 1024 * 1024 {
                    return Err("MCP image exceeds the 20 MiB model input limit".into());
                }
                Ok(ToolImage {
                    bytes,
                    mime_type: image.mime_type.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut response =
            serde_json::to_value(result).map_err(|_| "MCP result could not be encoded")?;
        if !images.is_empty()
            && let Some(content) = response.get_mut("content").and_then(Value::as_array_mut)
        {
            content.retain(|block| block.get("type").and_then(Value::as_str) != Some("image"));
        }
        if serde_json::to_vec(&response)
            .map_err(|_| "Invalid MCP result")?
            .len()
            > 128 * 1024
        {
            return Err(
                "Tool completed, but its result exceeds the 128 KiB model response limit".into(),
            );
        }
        Ok(ToolOutput { response, images })
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
    aliases: HashMap<String, String>,
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
                            name.clone(),
                            Binding {
                                server: server.clone(),
                                original: tool.name.to_string(),
                            },
                        );
                        bridge
                            .aliases
                            .insert(legacy_tool_name(&config.id, &tool.name), name);
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
        self.call_with_media(name, args).await.response
    }

    pub async fn call_with_media(&self, name: &str, args: Value) -> ToolOutput {
        let binding = self.bindings.get(name).or_else(|| {
            self.aliases
                .get(name)
                .and_then(|canonical| self.bindings.get(canonical))
        });
        let result = match binding {
            Some(binding) => binding.server.call(binding.original.clone(), args).await,
            None => Err("Unknown or disabled MCP tool".into()),
        };
        result.unwrap_or_else(|error| ToolOutput {
            response: json!({"isError":true,"error":error}),
            images: Vec::new(),
        })
    }
}

pub fn tool_name(server: &str, tool: &str) -> String {
    let safe: String = tool
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                byte as char
            } else {
                '_'
            }
        })
        .collect();
    let name = format!("mcp_{server}_{safe}");
    if name.len() <= 64 && safe == tool {
        name
    } else {
        let hash = tool.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
        let suffix = format!("_h{hash:016x}");
        let available = 64 - suffix.len();
        name[..name.len().min(available)].to_string() + &suffix
    }
}

fn legacy_tool_name(server: &str, tool: &str) -> String {
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
