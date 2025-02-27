use std::sync::Arc;

use moss_git::ports::AuthAgent;
use url::Url;

use crate::GitHostingProvider;

pub struct GitHubClient {
    auth_agent: Arc<dyn AuthAgent>,
}

impl GitHubClient {
    pub fn new(auth_agent: Arc<dyn AuthAgent>) -> Self {
        Self { auth_agent }
    }
}

impl GitHostingProvider for GitHubClient {
    fn name(&self) -> String {
        "GitHub".to_string()
    }

    fn base_url(&self) -> Url {
        Url::parse("https://github.com").unwrap()
    }
}
