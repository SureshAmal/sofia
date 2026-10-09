use rmcp::transport::{
    AuthClient, AuthorizationManager, AuthorizationRequest, CredentialStore,
    InMemoryCredentialStore, StoredCredentials,
    auth::{OAuthClientConfig, OAuthState, OAuthTokenResponse},
};
use sofia_config::{McpOAuth, McpServerConfig, McpTransport};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub const REDIRECT_URI: &str = "http://127.0.0.1:8765/oauth/callback";

pub async fn authenticate(config: &McpServerConfig) -> Result<McpOAuth, String> {
    let (url, oauth) = oauth_config(config)?;
    let listener = TcpListener::bind("127.0.0.1:8765")
        .await
        .map_err(|_| "OAuth callback port 8765 is already in use")?;
    let mut state = OAuthState::new(url, None)
        .await
        .map_err(|error| format!("OAuth discovery failed: {error}"))?;
    let mut request = AuthorizationRequest::new(REDIRECT_URI)
        .with_preregistered_client(oauth.client_id.clone())
        .with_client_secret(oauth.client_secret.clone())
        .with_application_type("web");
    if !oauth.scopes.is_empty() {
        request = request.with_scopes(oauth.scopes.clone());
    }
    state
        .start_authorization(request)
        .await
        .map_err(|error| format!("OAuth authorization could not start: {error}"))?;
    let authorization_url = state
        .get_authorization_url()
        .await
        .map_err(|error| format!("OAuth URL could not be created: {error}"))?;
    open::that(&authorization_url).map_err(|error| format!("Browser could not open: {error}"))?;

    let (mut socket, target) = wait_for_callback(&listener).await?;
    let callback_url = format!("http://127.0.0.1:8765{target}");
    let result = async {
        state
            .handle_callback_url(&callback_url)
            .await
            .map_err(|error| format!("OAuth authorization failed: {error}"))?;
        let (_, token) = state
            .get_credentials()
            .await
            .map_err(|error| format!("OAuth credentials could not be read: {error}"))?;
        let token = token.ok_or("OAuth provider returned no token")?;
        let mut updated = oauth.clone();
        updated.token =
            Some(serde_json::to_value(token).map_err(|_| "OAuth token could not be stored")?);
        updated.token_received_at = Some(now());
        Ok::<McpOAuth, String>(updated)
    }
    .await;
    let (status, body) = if result.is_ok() {
        ("200 OK", "Sofia is connected. You can close this tab.")
    } else {
        ("400 Bad Request", "Sofia could not complete authorization.")
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
    result
}

async fn wait_for_callback(
    listener: &TcpListener,
) -> Result<(tokio::net::TcpStream, String), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    loop {
        let (mut socket, _) = tokio::time::timeout_at(deadline, listener.accept())
            .await
            .map_err(|_| "OAuth sign-in timed out after 10 minutes")?
            .map_err(|error| format!("OAuth callback failed: {error}"))?;
        let mut request_bytes = vec![0_u8; 16 * 1024];
        let count =
            match tokio::time::timeout(Duration::from_secs(5), socket.read(&mut request_bytes))
                .await
            {
                Ok(Ok(count)) => count,
                _ => continue,
            };
        if count == 0 {
            continue;
        }
        let request = String::from_utf8_lossy(&request_bytes[..count]);
        let Some(target) = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
        else {
            continue;
        };
        if !target.starts_with("/oauth/callback") {
            let _ = socket
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            continue;
        }
        return Ok((socket, target.to_string()));
    }
}

pub async fn client(url: &str, oauth: &McpOAuth) -> Result<AuthClient<reqwest::Client>, String> {
    let token: OAuthTokenResponse = serde_json::from_value(oauth.token.clone().ok_or(
        "OAuth authentication is required. Open Sofia Settings and authenticate this MCP server.",
    )?)
    .map_err(|_| "Stored OAuth token is invalid; authenticate again")?;
    let store = InMemoryCredentialStore::new();
    store
        .save(StoredCredentials::new(
            oauth.client_id.clone(),
            Some(token),
            oauth.scopes.clone(),
            oauth.token_received_at,
        ))
        .await
        .map_err(|error| format!("OAuth token could not be loaded: {error}"))?;
    let mut manager = AuthorizationManager::new(url)
        .await
        .map_err(|error| format!("OAuth discovery failed: {error}"))?;
    manager.set_credential_store(store);
    if !manager
        .initialize_from_store()
        .await
        .map_err(|error| format!("OAuth token could not be initialized: {error}"))?
    {
        return Err("OAuth authentication is required; authenticate this MCP server again".into());
    }
    let mut client_config = OAuthClientConfig::new(&oauth.client_id, REDIRECT_URI)
        .with_client_secret(&oauth.client_secret)
        .with_scopes(oauth.scopes.clone())
        .with_application_type("web");
    if oauth.scopes.is_empty() {
        client_config.scopes.clear();
    }
    manager
        .configure_client(client_config)
        .map_err(|error| format!("OAuth client could not be configured: {error}"))?;
    Ok(AuthClient::new(reqwest::Client::new(), manager))
}

fn oauth_config(config: &McpServerConfig) -> Result<(&str, &McpOAuth), String> {
    match &config.transport {
        McpTransport::Http {
            url,
            oauth: Some(oauth),
            ..
        } => Ok((url, oauth)),
        _ => Err("This MCP server does not use OAuth".into()),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
