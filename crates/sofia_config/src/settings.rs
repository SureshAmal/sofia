use serde::{Deserialize, Serialize};

pub const DEFAULT_SYSTEM_PROMPT: &str = include_str!("system_prompt.txt");

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub assistant: AssistantSettings,
    pub vertex: VertexSettings,
    pub audio: AudioSettings,
    pub gemini: GeminiSettings,
    pub appearance: AppearanceSettings,
    pub connection: ConnectionSettings,
    pub generative: GenerativeSettings,
    pub mcp_servers: Vec<McpServerConfig>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct McpServerConfig {
    pub name: String,
    pub id: String,
    pub enabled: bool,
    pub transport: McpTransport,
    pub disabled_tools: Vec<String>,
}
impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            id: String::new(),
            enabled: true,
            transport: McpTransport::Stdio {
                command: String::new(),
                args: Vec::new(),
                env: Default::default(),
            },
            disabled_tools: Vec::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)]
        bearer_token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        oauth: Option<McpOAuth>,
    },
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct McpOAuth {
    pub client_id: String,
    pub client_secret: String,
    pub scopes: Vec<String>,
    pub token: Option<serde_json::Value>,
    pub token_received_at: Option<u64>,
}
impl McpServerConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 32
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err("MCP server ID must contain 1–32 letters, digits or underscores.".into());
        }
        if !self.enabled {
            return Ok(());
        }
        match &self.transport {
            McpTransport::Stdio { command, .. } if command.trim().is_empty() => {
                Err(format!("MCP {}: command is required.", self.id))
            }
            McpTransport::Http { url, .. }
                if !url.starts_with("http://") && !url.starts_with("https://") =>
            {
                Err(format!(
                    "MCP {}: use an http:// or https:// endpoint.",
                    self.id
                ))
            }
            McpTransport::Http {
                oauth: Some(oauth), ..
            } if oauth.client_id.trim().is_empty() || oauth.client_secret.trim().is_empty() => {
                Err(format!(
                    "MCP {}: OAuth client ID and client secret are required.",
                    self.id
                ))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectionSettings {
    /// None selects Gemini API when a key is present, otherwise Vertex.
    pub use_vertex_ai: Option<bool>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GenerativeSettings {
    pub api_key: String,
    pub model: String,
}
impl Default for GenerativeSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: "gemini-3.8-live".into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AssistantSettings {
    pub system_prompt: String,
}
impl Default for AssistantSettings {
    fn default() -> Self {
        Self {
            system_prompt: DEFAULT_SYSTEM_PROMPT.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VertexSettings {
    pub project_id: String,
    pub location: String,
    pub model: String,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}
impl Default for VertexSettings {
    fn default() -> Self {
        Self {
            project_id: String::new(),
            location: "us-central1".into(),
            model: "gemini-3.8-live".into(),
            client_id: String::new(),
            client_secret: String::new(),
            refresh_token: String::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub output_device_id: Option<String>,
    pub auto_listen: bool,
}
impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            output_device_id: None,
            auto_listen: true,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GeminiSettings {
    pub voice_name: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    pub theme: String,
    pub font_family: Option<String>,
    pub radius: Option<u8>,
}
impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            font_family: None,
            radius: None,
        }
    }
}
impl Settings {
    pub fn uses_vertex_ai(&self) -> bool {
        self.connection
            .use_vertex_ai
            .unwrap_or(self.generative.api_key.trim().is_empty())
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.assistant.system_prompt.trim().is_empty()
            || self.assistant.system_prompt.len() > 32_768
        {
            return Err("System prompt must contain 1–32768 bytes.".into());
        }
        if self.uses_vertex_ai()
            && (self.vertex.model.trim().is_empty() || self.vertex.location.trim().is_empty())
        {
            return Err("Model and Vertex location are required.".into());
        }
        if !self.uses_vertex_ai() && self.generative.model.trim().is_empty() {
            return Err("Gemini API model is required.".into());
        }
        let mut ids = std::collections::HashSet::new();
        for server in &self.mcp_servers {
            server.validate()?;
            if !ids.insert(&server.id) {
                return Err(format!("Duplicate MCP server ID: {}", server.id));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod mcp_tests {
    use super::*;
    #[test]
    fn appearance_roundtrip() {
        let mut settings = Settings::default();
        settings.appearance.font_family = Some("Noto Sans".into());
        settings.appearance.radius = Some(10);
        let restored: Settings =
            serde_json::from_value(serde_json::to_value(settings).unwrap()).unwrap();
        assert_eq!(
            restored.appearance.font_family.as_deref(),
            Some("Noto Sans")
        );
        assert_eq!(restored.appearance.radius, Some(10));
    }
    #[test]
    fn mcp_roundtrip_and_duplicate_validation() {
        let mut settings = Settings::default();
        let server = McpServerConfig {
            name: String::new(),
            id: "local".into(),
            enabled: true,
            transport: McpTransport::Stdio {
                command: "server".into(),
                args: vec!["--stdio".into()],
                env: Default::default(),
            },
            disabled_tools: vec!["delete".into()],
        };
        settings.mcp_servers.push(server.clone());
        let encoded = serde_json::to_value(&settings).unwrap();
        let decoded: Settings = serde_json::from_value(encoded).unwrap();
        assert!(decoded.validate().is_ok());
        assert_eq!(decoded.mcp_servers[0].disabled_tools, vec!["delete"]);
        settings.mcp_servers.push(server);
        assert!(settings.validate().is_err());
    }
}
