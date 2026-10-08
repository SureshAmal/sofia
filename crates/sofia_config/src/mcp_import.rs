//! Import the commonly shared mcpServers configuration format.
use crate::{McpServerConfig, McpTransport};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    url: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}
/// Preserve argument boundaries, including spaces; never interpret shell commands.
pub fn import_mcp_servers(text: &str) -> Result<Vec<McpServerConfig>, String> {
    let root: serde_json::Value = serde_json::from_str(text)
        .map_err(|_| "Invalid JSON. Paste the complete MCP configuration.".to_string())?;
    let servers = root
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
        .ok_or("Expected an mcpServers object")?;
    if servers.is_empty() {
        return Err("The configuration contains no servers".into());
    }
    let mut ids = HashSet::new();
    let mut result = Vec::new();
    for (name, value) in servers {
        let entry: Entry = serde_json::from_value(value.clone()).map_err(|_| {
            format!("Invalid configuration for {name}: use command/args/env or url/headers")
        })?;
        let base: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .take(24)
            .collect();
        let base = base.trim_matches('_');
        let base = if base.is_empty() { "server" } else { base };
        let mut id = base.to_string();
        let mut index = 2;
        while !ids.insert(id.clone()) {
            id = format!("{base}_{index}");
            index += 1;
        }
        let transport = match (entry.command, entry.url) {
            (Some(command), None) if entry.headers.is_empty() => McpTransport::Stdio {
                command,
                args: entry.args,
                env: entry.env,
            },
            (None, Some(url)) if entry.args.is_empty() && entry.env.is_empty() => {
                let mut bearer_token = String::new();
                for (header, value) in entry.headers {
                    if !header.eq_ignore_ascii_case("authorization") {
                        return Err(format!("{name}: only Authorization headers are supported"));
                    }
                    bearer_token = value
                        .strip_prefix("Bearer ")
                        .ok_or_else(|| format!("{name}: use Authorization: Bearer <token>"))?
                        .into();
                }
                McpTransport::Http { url, bearer_token }
            }
            _ => return Err(format!("{name}: specify either a command or a URL")),
        };
        let config = McpServerConfig {
            name: name.clone(),
            id,
            enabled: true,
            transport,
            disabled_tools: vec![],
        };
        config.validate()?;
        result.push(config);
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_shared_npx_configuration_without_splitting_arguments() {
        let config=import_mcp_servers(r#"{"mcpServers":{"Parallel Search MCP":{"command":"npx","args":["-y","mcp-remote","https://search.parallel.ai/mcp","value with spaces"]}}}"#).unwrap();
        assert_eq!(config[0].id, "parallel_search_mcp");
        match &config[0].transport {
            McpTransport::Stdio { command, args, env } => {
                assert_eq!(command, "npx");
                assert_eq!(args[3], "value with spaces");
                assert!(env.is_empty());
            }
            _ => panic!("Expected stdio"),
        }
    }
    #[test]
    fn imports_http_and_rejects_unsupported_config_without_losing_fields() {
        let servers=import_mcp_servers(r#"{"mcpServers":{"Remote":{"url":"https://example.com/mcp","headers":{"Authorization":"Bearer secret"}}}}"#).unwrap();
        assert!(
            matches!(&servers[0].transport,McpTransport::Http{bearer_token,..} if bearer_token=="secret")
        );
        assert!(
            import_mcp_servers(r#"{"mcpServers":{"Bad":{"command":"test","unsupported":true}}}"#)
                .is_err()
        );
    }
}
