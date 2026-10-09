//! Sofia's MCP server for persistent user content and presentation windows.
mod apps;
mod clipboard;
mod notify;
mod screenshot;
mod tools;
mod web;
use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use serde_json::{Value, json};
use sofia_content::Store;
use std::sync::Arc;
#[derive(Clone)]
pub struct SofiaMcp {
    store: Arc<Store>,
}
impl SofiaMcp {
    pub fn new(store: Store) -> Self {
        Self {
            store: Arc::new(store),
        }
    }
    pub async fn execute(&self, name: &str, args: Value) -> Value {
        let store = self.store.clone();
        let name = name.to_string();
        let result = tokio::task::spawn_blocking(move || tools::execute(&store, &name, args)).await;
        match result {
            Ok(Ok((value, changed))) => {
                if let Some(id) = changed {
                    let _ = notify::changed(id).await;
                }
                value
            }
            Ok(Err(error)) => json!({"isError":true,"error":error}),
            Err(_) => json!({"isError":true,"error":"Content worker failed"}),
        }
    }
}
impl ServerHandler for SofiaMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tools::declarations(),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let value = self
            .execute(
                &request.name,
                Value::Object(request.arguments.unwrap_or_default()),
            )
            .await;
        let image = value
            .get("image")
            .and_then(Value::as_object)
            .and_then(|image| {
                Some(rmcp::model::ContentBlock::image(
                    image.get("data")?.as_str()?,
                    image.get("mime_type")?.as_str()?,
                ))
            });
        let content = if let Some(image) = image {
            vec![
                rmcp::model::ContentBlock::text(
                    json!({"ok":true,"image_provided":true}).to_string(),
                ),
                image,
            ]
        } else {
            vec![rmcp::model::ContentBlock::text(value.to_string())]
        };
        Ok(if value.get("isError") == Some(&Value::Bool(true)) {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        }
        .into())
    }
}
