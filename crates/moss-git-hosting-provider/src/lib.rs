pub mod github;
pub mod gitlab;

use url::Url;

// TODO: Create a `Sensitive` type for storing passwords securely?
// TODO: Preserving the auth info for repos

pub trait TestStorage {
    // TODO: We will use more secure method of storing the AuthAgent info
    // For easy testing, we will use environment variables for now
    fn write_to_file(&self) -> anyhow::Result<()>;
    fn read_from_file() -> anyhow::Result<std::sync::Arc<Self>>;
}

pub trait GitHostingProvider {
    fn name(&self) -> String;
    fn base_url(&self) -> Url;
}
