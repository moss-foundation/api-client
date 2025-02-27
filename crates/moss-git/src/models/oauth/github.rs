use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GitHubCred {
    access_token: String,
}

impl GitHubCred {
    pub fn new(access_token: &str) -> Self {
        Self {
            access_token: access_token.to_string(),
        }
    }

    pub fn access_token(&self) -> &str {
        &self.access_token
    }
}
