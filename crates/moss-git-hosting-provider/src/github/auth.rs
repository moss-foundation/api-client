use anyhow::Result;
use git2::{Cred, RemoteCallbacks};
use moss_git::ports::AuthAgent;
use oauth2::basic::BasicClient;
use oauth2::url::Url;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl,
    Scope, TokenResponse, TokenUrl,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::string::ToString;
use std::sync::Arc;

use crate::TestStorage;

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

const GITHUB_AUTH_URL: &'static str = "https://github.com/login/oauth/authorize";
const GITHUB_TOKEN_URL: &'static str = "https://github.com/login/oauth/access_token";

const GITHUB_SCOPES: [&'static str; 3] = ["repo", "read:user", "user:email"];

#[derive(Serialize, Deserialize)]
pub struct GitHubAuthAgent {
    cred: RwLock<Option<GitHubCred>>,
}

impl GitHubAuthAgent {
    pub fn new() -> Self {
        Self {
            cred: RwLock::new(None),
        }
    }
}

impl GitHubAuthAgent {
    fn client_id() -> Result<ClientId> {
        dotenv::dotenv()?;
        Ok(ClientId::new(dotenv::var("GITHUB_CLIENT_ID")?))
    }
    fn client_secret() -> Result<ClientSecret> {
        dotenv::dotenv()?;
        Ok(ClientSecret::new(dotenv::var("GITHUB_CLIENT_SECRET")?))
    }

    fn initial_auth(&self) -> Result<()> {
        println!("Initial OAuth Protocol");
        // Setting the port as 0 automatically assigns a free port
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let callback_port = listener.local_addr()?.port();

        let client = BasicClient::new(GitHubAuthAgent::client_id()?)
            .set_client_secret(GitHubAuthAgent::client_secret()?)
            .set_auth_uri(AuthUrl::new(GITHUB_AUTH_URL.to_string())?)
            .set_token_uri(TokenUrl::new(GITHUB_TOKEN_URL.to_string())?)
            .set_redirect_uri(RedirectUrl::new(format!(
                "http://127.0.0.1:{}",
                callback_port.to_string()
            ))?);

        // https://datatracker.ietf.org/doc/html/rfc7636#section-1.1
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        // Generate the authorization URL to which we'll redirect the user.
        let (authorize_url, _csrf_state) = client
            .authorize_url(CsrfToken::new_random)
            .add_scopes(GITHUB_SCOPES.into_iter().map(|s| Scope::new(s.to_string())))
            // Let the user select which account to authorize
            // https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps
            .add_extra_param("prompt", "select_account")
            .set_pkce_challenge(pkce_challenge)
            .url();

        if webbrowser::open(&authorize_url.to_string()).is_err() {
            println!("Open this URL in your browser:\n{authorize_url}\n");
        }

        let (code, _state) = {
            let Some(mut stream) = listener.incoming().flatten().next() else {
                panic!("listener terminated without accepting a connection");
            };

            let mut reader = BufReader::new(&stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line)?;

            // GET /?code=*** HTTP/1.1
            let redirect_url = request_line.split_whitespace().nth(1).unwrap();
            let url = Url::parse(&("http://127.0.0.1".to_string() + redirect_url))?;

            let code = url
                .query_pairs()
                .find(|(key, _)| key == "code")
                .map(|(_, code)| AuthorizationCode::new(code.into_owned()))
                .unwrap();

            let state = url
                .query_pairs()
                .find(|(key, _)| key == "state")
                .map(|(_, state)| CsrfToken::new(state.into_owned()))
                .unwrap();

            // TODO: Once the code is received, the focus should switch back to the main application
            let message = "Go back to your terminal :)";
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
                message.len(),
                message
            );
            stream.write_all(response.as_bytes())?;

            (code, state)
        };

        let http_client = reqwest::blocking::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        // Exchange the code + PKCE verifier with access & refresh token.
        let token_res = client
            .exchange_code(code)
            .set_pkce_verifier(pkce_verifier)
            .request(&http_client)?;

        let access_token = token_res.access_token().secret().as_str();
        let mut write = self.cred.write();
        // GitHub's access_token does not have expiration
        *write = Some(GitHubCred::new(access_token));
        Ok(())
    }
}

impl AuthAgent for GitHubAuthAgent {
    fn generate_callback<'a>(&'a self, cb: &mut RemoteCallbacks<'a>) -> Result<()> {
        if self.cred.read().is_none() {
            self.initial_auth()
                .expect("Unable to finish initial authentication");
        }
        let cred = self.cred.read().clone().unwrap();
        cb.credentials(move |_url, username_from_url, _allowed_types| {
            Cred::userpass_plaintext("oauth2", cred.access_token())
        });
        self.write_to_file()?;
        Ok(())
    }
}

impl TestStorage for GitHubAuthAgent {
    fn write_to_file(&self) -> Result<()> {
        println!("Writing to file");
        std::fs::write("github_oauth.json", serde_json::to_string(&self)?)?;
        Ok(())
    }

    fn read_from_file() -> Result<Arc<Self>> {
        dbg!("-----------");
        dbg!(&std::fs::read_to_string("gitlab_oauth.json",)?);

        Ok(Arc::new(serde_json::from_str(&std::fs::read_to_string(
            "github_oauth.json",
        )?)?))
    }
}

#[cfg(test)]
mod github_tests {
    use super::*;

    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    // use crate::adapters::auth::ssh::SSHAgent;
    // use crate::repo::RepoHandle;
    use crate::TestStorage;

    // Run cargo test cloning_with_https -- --nocapture
    // #[test]
    // fn cloning_with_https() {
    //     // From example: https://github.com/ramosbugs/oauth2-rs/blob/main/examples/github.rs
    //     dotenv::dotenv().ok();
    //     let repo_url = &dotenv::var("GITHUB_TEST_REPO_HTTPS").unwrap();
    //     let repo_path = Path::new("test-repo");

    //     let auth_agent =
    //         GitHubAgent::read_from_file().unwrap_or_else(|_| Arc::new(GitHubAgent::new()));

    //     let repo = RepoHandle::clone(repo_url, repo_path, auth_agent).unwrap();
    // }

    // #[test]
    // fn cloning_with_ssh() {
    //     dotenv::dotenv().ok();
    //     let repo_url = &dotenv::var("GITHUB_TEST_REPO_SSH").unwrap();
    //     let repo_path = Path::new("test-repo");

    //     let private = PathBuf::from(dotenv::var("GITHUB_SSH_PRIVATE").unwrap());
    //     let public = PathBuf::from(dotenv::var("GITHUB_SSH_PUBLIC").unwrap());
    //     let password = dotenv::var("GITHUB_SSH_PASSWORD").unwrap();

    //     let auth_agent = Arc::new(SSHAgent::new(Some(public), private, Some(password.into())));
    //     let repo = RepoHandle::clone(repo_url, repo_path, auth_agent).unwrap();
    // }
}
