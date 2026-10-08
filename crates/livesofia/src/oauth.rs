//! Google OAuth refresh-token exchange for the Vertex Live connection.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gemini_live::error::BearerTokenError;
use gemini_live::transport::BearerTokenProvider;
use serde::Deserialize;
use thiserror::Error;
use tokio::sync::Mutex;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REFRESH_MARGIN: Duration = Duration::from_secs(60);

/// The OAuth client and refresh token issued for the Google Cloud project.
/// Keep these values out of logs and store them in the OS credential store later.
pub struct GoogleOAuthCredentials {
    client_id: String,
    client_secret: String,
    refresh_token: String,
}

impl GoogleOAuthCredentials {
    /// Use values collected by the settings app or another credential source.
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        refresh_token: impl Into<String>,
    ) -> Result<Self, OAuthError> {
        let credentials = Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            refresh_token: refresh_token.into(),
        };
        for (name, value) in [
            ("CLIENT_ID", credentials.client_id.as_str()),
            ("CLIENT_SECRET", credentials.client_secret.as_str()),
            ("REFRESH_TOKEN", credentials.refresh_token.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(OAuthError::MissingSetting(name));
            }
        }
        Ok(credentials)
    }

    pub fn from_env() -> Result<Self, OAuthError> {
        Self::new(
            required_env("CLIENT_ID")?,
            required_env("CLIENT_SECRET")?,
            required_env("REFRESH_TOKEN")?,
        )
    }
}

fn required_env(name: &'static str) -> Result<String, OAuthError> {
    let value = std::env::var(name).map_err(|_| OAuthError::MissingSetting(name))?;
    if value.trim().is_empty() {
        return Err(OAuthError::MissingSetting(name));
    }
    Ok(value)
}

#[derive(Clone)]
pub struct GoogleTokenSource {
    inner: Arc<TokenSourceInner>,
}

struct TokenSourceInner {
    credentials: GoogleOAuthCredentials,
    client: reqwest::Client,
    cached: Mutex<Option<CachedToken>>,
}

struct CachedToken {
    value: String,
    expires_at: Instant,
}

impl GoogleTokenSource {
    pub fn new(credentials: GoogleOAuthCredentials) -> Result<Self, OAuthError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            inner: Arc::new(TokenSourceInner {
                credentials,
                client,
                cached: Mutex::new(None),
            }),
        })
    }

    /// Return a cached token or refresh it before its expiry.
    pub async fn access_token(&self) -> Result<String, OAuthError> {
        let mut cached = self.inner.cached.lock().await;
        if let Some(token) = cached.as_ref()
            && token.expires_at > Instant::now() + REFRESH_MARGIN
        {
            return Ok(token.value.clone());
        }

        let credentials = &self.inner.credentials;
        let response = self
            .inner
            .client
            .post(TOKEN_URL)
            .form(&[
                ("client_id", credentials.client_id.as_str()),
                ("client_secret", credentials.client_secret.as_str()),
                ("refresh_token", credentials.refresh_token.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error = response.json::<TokenErrorResponse>().await.ok();
            return Err(OAuthError::Rejected {
                status,
                code: error
                    .map(|value| value.error)
                    .unwrap_or_else(|| "unknown".into()),
            });
        }

        let token: TokenResponse = response.json().await?;
        if token.token_type != "Bearer" || token.access_token.is_empty() || token.expires_in == 0 {
            return Err(OAuthError::InvalidResponse);
        }
        let expires_at = Instant::now() + Duration::from_secs(token.expires_in);
        let value = token.access_token;
        *cached = Some(CachedToken {
            value: value.clone(),
            expires_at,
        });
        Ok(value)
    }

    /// `gemini-live` calls this on every WebSocket connection or reconnect.
    pub fn bearer_provider(&self) -> BearerTokenProvider {
        let source = self.clone();
        BearerTokenProvider::from_fn("google-oauth-refresh", move || {
            let source = source.clone();
            async move {
                source.access_token().await.map_err(|error| {
                    BearerTokenError::with_source("Google token refresh failed", error)
                })
            }
        })
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    token_type: String,
}

#[derive(Deserialize)]
struct TokenErrorResponse {
    error: String,
}

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("missing OAuth setting: {0}")]
    MissingSetting(&'static str),
    #[error("Google token request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Google rejected the refresh token (HTTP {status}, {code})")]
    Rejected { status: u16, code: String },
    #[error("Google returned an invalid access-token response")]
    InvalidResponse,
}
