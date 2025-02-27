use serde::{Deserialize, Serialize};
use std::time::Instant;

// Since `Instant` is opaque and cannot be serialized
// We will only store the refresh_token when serializing OAuth Credential
// Forcing refreshing of tokens for new sessions
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GitLabCred {
    #[serde(skip)]
    access_token: Option<String>,
    #[serde(skip)]
    time_to_refresh: Option<Instant>,
    refresh_token: String,
}

impl GitLabCred {
    pub fn new(
        access_token: Option<&str>,
        time_to_refresh: Option<Instant>,
        refresh_token: &str,
    ) -> Self {
        Self {
            access_token: access_token.map(|s| s.to_string()),
            time_to_refresh,
            refresh_token: refresh_token.to_string(),
        }
    }

    pub fn access_token(&self) -> Option<&str> {
        self.access_token.as_deref()
    }
    pub fn time_to_refresh(&self) -> Option<Instant> {
        self.time_to_refresh
    }
    pub fn refresh_token(&self) -> &str {
        self.refresh_token.as_str()
    }
}
