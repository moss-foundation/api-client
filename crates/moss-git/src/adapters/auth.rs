pub mod oauth;
pub mod ssh;

#[cfg(test)]
mod gitlab_tests {
    use anyhow::Result;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use iota_stronghold::{KeyProvider, SnapshotPath};
    use parking_lot::Mutex;
    use zeroize::Zeroizing;

    use crate::adapters::auth::{oauth::GitLabAgent, ssh::SSHAgent};
    use crate::repo::RepoHandle;
    use crate::TestStorage;

    #[test]
    fn cloning_with_https() {
        dotenv::dotenv().ok();
        let repo_url = &dotenv::var("GITLAB_TEST_REPO_HTTPS").unwrap();
        let repo_path = Path::new("test-repo-lab");

        let auth_agent =
            GitLabAgent::read_from_file().unwrap_or_else(|_| Arc::new(GitLabAgent::new()));

        let repo = RepoHandle::clone(repo_url, repo_path, auth_agent).unwrap();
    }

    #[test]
    fn cloning_with_ssh() {
        dotenv::dotenv().ok();
        let repo_url = &dotenv::var("GITLAB_TEST_REPO_SSH").unwrap();
        let repo_path = Path::new("test-repo-lab");

        let private = PathBuf::from(dotenv::var("GITLAB_SSH_PRIVATE").unwrap());
        let public = PathBuf::from(dotenv::var("GITLAB_SSH_PUBLIC").unwrap());
        let password = dotenv::var("GITLAB_SSH_PASSWORD").unwrap();

        let auth_agent = Arc::new(SSHAgent::new(Some(public), private, Some(password.into())));
        let repo = RepoHandle::clone(repo_url, repo_path, auth_agent).unwrap();
    }

    #[derive(Default)]
    struct StrongholdCollection(Arc<Mutex<HashMap<PathBuf, Stronghold>>>);

    pub struct Stronghold {
        inner: iota_stronghold::Stronghold,
        path: SnapshotPath,
        keyprovider: KeyProvider,
    }

    impl Stronghold {
        pub fn new<P: AsRef<Path>>(path: P, password: Vec<u8>) -> Result<Self> {
            let path = SnapshotPath::from_path(path);
            let stronghold = iota_stronghold::Stronghold::default();
            let keyprovider = KeyProvider::try_from(Zeroizing::new(password))?;
            if path.exists() {
                stronghold.load_snapshot(&keyprovider, &path)?;
            }
            Ok(Self {
                inner: stronghold,
                path,
                keyprovider,
            })
        }

        pub fn save(&self) -> Result<()> {
            self.inner
                .commit_with_keyprovider(&self.path, &self.keyprovider)?;
            Ok(())
        }

        pub fn inner(&self) -> &iota_stronghold::Stronghold {
            &self.inner
        }
    }

    #[test]
    fn stronghold_test() {
        use zeroize::{Zeroize, Zeroizing};

        let hash_function = |password: &str| {
            // Hash the password here with e.g. argon2, blake2b or any other secure algorithm
            // Here is an example implementation using the `rust-argon2` crate for hashing the password

            use argon2::{hash_raw, Config, Variant, Version};

            let config = Config {
                lanes: 4,
                mem_cost: 10_000,
                time_cost: 10,
                variant: Variant::Argon2id,
                version: Version::Version13,
                ..Default::default()
            };

            let salt = "your-salt".as_bytes();

            let key = hash_raw(password.as_ref(), salt, &config).expect("failed to hash password");

            key.to_vec()
        };

        let snapshot_path = PathBuf::from("vault.hold");
        let mut password = "mypass1234".to_string();
        let hash = hash_function(&password);
        let collection = StrongholdCollection::default();
        let stronghold = Stronghold::new(snapshot_path.clone(), hash).unwrap();
        let client = stronghold.inner.create_client("name your client").unwrap();

        // client.vault(vault_path)

        stronghold.save().unwrap();

        // collection.0.lock().insert(snapshot_path, stronghold);

        // password.zeroize();

        // let path = SnapshotPath::from_path("vault.hold");
        // let stronghold = iota_stronghold::Stronghold::default();
        // let pass = "password".as_bytes().to_vec();
        // let keyprovider = KeyProvider::try_from(Zeroizing::new(pass)).unwrap();
        // let client = stronghold.create_client("vault.hold").unwrap();
    }
}
