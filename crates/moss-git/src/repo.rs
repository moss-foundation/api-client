use anyhow::Result;
use git2::build::RepoBuilder;
use git2::{IndexAddOption, IntoCString, PushOptions, RemoteCallbacks, Repository, Signature};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ports::AuthAgent;

pub struct RepoHandle {
    // FIXME: Is it necessary to store the url of the repo?
    url: Option<String>,
    path: PathBuf,
    auth_agent: Arc<dyn AuthAgent>,
    // public for easier testing
    pub repo: Repository,
}

// https://stackoverflow.com/questions/27672722/libgit2-commit-example
// https://github.com/rust-lang/git2-rs/tree/master/examples

// TODO: Use callback to return/display progress
impl RepoHandle {
    pub fn clone(url: &str, path: &Path, auth_agent: Arc<dyn AuthAgent>) -> Result<RepoHandle> {
        let mut callbacks = RemoteCallbacks::new();
        auth_agent.generate_callback(&mut callbacks)?;

        let mut fetch_opts = git2::FetchOptions::new();
        fetch_opts.remote_callbacks(callbacks);
        let mut builder = RepoBuilder::new();
        builder.fetch_options(fetch_opts);

        let repo = builder.clone(url, &path)?;

        Ok(RepoHandle {
            url: Some(url.to_string()),
            path: path.to_owned(),
            auth_agent: auth_agent.clone(),
            repo,
        })
    }

    pub fn open(path: &Path, auth_agent: Arc<dyn AuthAgent>) -> Result<RepoHandle> {
        let repo = Repository::open(path)?;
        // FIXME: This assumes that the remote's name is `origin`
        // Is there a better way to get the url of a local repo?
        let remote = repo.find_remote("origin");

        let url = remote
            .map(|r| r.pushurl().map(|s| s.to_string()))
            .unwrap_or(None);
        Ok(RepoHandle {
            url,
            path: path.to_owned(),
            auth_agent: auth_agent.clone(),
            repo,
        })
    }

    pub fn add(
        &self,
        paths: impl IntoIterator<Item = impl IntoCString>,
        opts: IndexAddOption,
    ) -> Result<()> {
        let mut index = self.repo.index()?;
        index.add_all(paths, opts, None)?;
        index.write()?;
        Ok(())
    }

    pub fn remove(&self, paths: impl IntoIterator<Item = impl IntoCString>) -> Result<()> {
        let mut index = self.repo.index()?;
        index.remove_all(paths, None)?;
        index.write()?;
        Ok(())
    }

    pub fn commit(&self, message: &str, sig: Signature) -> Result<()> {
        let mut index = self.repo.index()?;
        let tree = self.repo.find_tree(index.write_tree()?)?;
        let last_commit = self.repo.head()?.peel_to_commit();

        if let Ok(parent) = last_commit {
            self.repo
                .commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&parent])?;
        } else {
            self.repo
                .commit(Some("HEAD"), &sig, &sig, &message, &tree, &[])?;
        }
        Ok(())
    }

    pub fn push(
        &self,
        remote_name: Option<&str>,
        local_branch_name: Option<&str>,
        remote_branch_name: Option<&str>,
    ) -> Result<()> {
        let remote_name = remote_name.unwrap_or("origin");
        let local_branch_name = local_branch_name.unwrap_or("main");
        let remote_branch_name = remote_branch_name.unwrap_or("main");
        let mut remote = self.repo.find_remote(remote_name)?;
        let mut callbacks = RemoteCallbacks::new();
        self.auth_agent.generate_callback(&mut callbacks)?;
        remote.push(
            &[&format!(
                "refs/heads/{}:refs/heads/{}",
                local_branch_name, remote_branch_name
            )],
            Some(&mut PushOptions::new().remote_callbacks(callbacks)),
        )?;
        Ok(())
    }

    pub fn fetch(&self, remote_name: Option<&str>) -> Result<git2::AnnotatedCommit> {
        let remote_name = remote_name.unwrap_or("origin");
        let mut remote = self.repo.find_remote(remote_name)?;
        let refspec = format!("+refs/heads/*:refs/remotes/{}/*", remote_name);

        let mut fetch_opts = git2::FetchOptions::new();
        let mut callbacks = RemoteCallbacks::new();
        self.auth_agent.generate_callback(&mut callbacks)?;
        fetch_opts.remote_callbacks(callbacks);
        fetch_opts.prune(git2::FetchPrune::On);

        remote.fetch(&[&refspec], Some(&mut fetch_opts), None)?;

        let fetch_head = self.repo.find_reference("FETCH_HEAD")?;
        Ok(self.repo.reference_to_annotated_commit(&fetch_head)?)
    }

    pub fn merge(&self, branch_name: &str) -> Result<()> {
        let incoming_reference = self
            .repo
            .find_branch(branch_name, git2::BranchType::Local)?
            .into_reference();
        self.merge_helper(
            self.repo
                .reference_to_annotated_commit(&incoming_reference)?,
        )?;

        Ok(())
    }

    pub fn pull(&self, remote_name: Option<&str>) -> Result<()> {
        // Pull = Fetch + Merge FETCH_HEAD
        let mut remote = self.repo.find_remote(remote_name.unwrap_or("origin"))?;
        let mut callbacks = RemoteCallbacks::new();
        self.auth_agent.generate_callback(&mut callbacks)?;

        let fetch_commit = self.fetch(remote.name())?;
        self.merge_helper(fetch_commit)?;

        Ok(())
    }
}
impl RepoHandle {
    fn fast_forward(
        &self,
        our_reference: &mut git2::Reference,
        incoming_commit: &git2::AnnotatedCommit,
    ) -> Result<(), git2::Error> {
        let name = match our_reference.name() {
            Some(s) => s.to_string(),
            None => String::from_utf8_lossy(our_reference.name_bytes()).to_string(),
        };
        let msg = format!(
            "Fast-Forward: Setting {} to id: {}",
            name,
            incoming_commit.id()
        );
        println!("{}", msg);
        our_reference.set_target(incoming_commit.id(), &msg)?;
        self.repo.set_head(&name)?;
        self.repo.checkout_head(Some(
            git2::build::CheckoutBuilder::default()
                // TODO: Handle dirty workspace state (stashing?)
                .force(),
        ))?;
        Ok(())
    }

    fn normal_merge(
        &self,
        our_commit: &git2::AnnotatedCommit,
        incoming_commit: &git2::AnnotatedCommit,
    ) -> Result<(), git2::Error> {
        let our_tree = self.repo.find_commit(our_commit.id())?.tree()?;
        let incoming_tree = self.repo.find_commit(incoming_commit.id())?.tree()?;
        let ancestor = self
            .repo
            .find_commit(
                self.repo
                    .merge_base(our_commit.id(), incoming_commit.id())?,
            )?
            .tree()?;
        let mut idx = self
            .repo
            .merge_trees(&ancestor, &our_tree, &incoming_tree, None)?;

        if idx.has_conflicts() {
            println!("Merge conflicts detected...");
            self.repo.checkout_index(Some(&mut idx), None)?;
            return Ok(());
        }
        let result_tree = self.repo.find_tree(idx.write_tree_to(&self.repo)?)?;
        // now create the merge commit
        let msg = format!("Merge: {} into {}", incoming_commit.id(), our_commit.id());
        let sig = self.repo.signature()?;
        let local_commit = self.repo.find_commit(our_commit.id())?;
        let remote_commit = self.repo.find_commit(incoming_commit.id())?;
        // Do our merge commit and set current branch head to that commit.
        let _merge_commit = self.repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &msg,
            &result_tree,
            &[&local_commit, &remote_commit],
        )?;
        // Set working tree to match head.
        self.repo.checkout_head(None)?;
        Ok(())
    }

    fn merge_helper(&self, incoming_commit: git2::AnnotatedCommit) -> Result<(), git2::Error> {
        let head = self.repo.head()?;
        let our_branch = head.name().unwrap_or("main");

        // 1. do a merge analysis
        let analysis = self.repo.merge_analysis(&[&incoming_commit])?;

        // 2. Do the appropriate merge
        if analysis.0.is_fast_forward() {
            println!("Doing a fast forward");
            // do a fast forward
            let refname = format!("refs/heads/{}", our_branch);
            match self.repo.find_reference(&refname) {
                Ok(mut r) => {
                    self.fast_forward(&mut r, &incoming_commit)?;
                }
                Err(_) => {
                    // The branch doesn't exist so just set the reference to the
                    // commit directly. Usually this is because you are pulling
                    // into an empty repository.
                    self.repo.reference(
                        &refname,
                        incoming_commit.id(),
                        true,
                        &format!("Setting {} to {}", our_branch, incoming_commit.id()),
                    )?;
                    self.repo.set_head(&refname)?;
                    self.repo.checkout_head(Some(
                        git2::build::CheckoutBuilder::default()
                            .allow_conflicts(true)
                            .conflict_style_merge(true)
                            .force(),
                    ))?;
                }
            };
        } else if analysis.0.is_normal() {
            // do a normal merge
            let head_commit = self
                .repo
                .reference_to_annotated_commit(&self.repo.head()?)?;
            self.normal_merge(&head_commit, &incoming_commit)?;
        } else {
            println!("Nothing to do...");
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use git2::{IndexAddOption, Signature};
    use std::path::Path;
    use std::sync::Arc;
    use std::time::SystemTime;
    use crate::adapters::auth::oauth::GitHubAgent;
    use crate::repo::RepoHandle;
    use crate::TestStorage;

    // cargo test test_clone_add_commit_push -- --nocapture
    #[test]
    fn test_clone_add_commit_push() {
        // TODO: Support verified signed commits using `gpg`
        // From example: https://github.com/ramosbugs/oauth2-rs/blob/main/examples/github.rs
        // https://users.rust-lang.org/t/how-to-use-git2-push-correctly/97202/6
        let repo_url = dotenv::var("GITHUB_TEST_REPO_HTTPS").unwrap();
        let repo_path = Path::new("test-repo");

        let mut auth_agent =
            GitHubAgent::read_from_file().unwrap_or_else(|_| Arc::new(GitHubAgent::new()));

        let repo = RepoHandle::clone(&repo_url, &repo_path, auth_agent).unwrap();

        let time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        std::fs::write(repo_path.join("TEST.txt"), time.to_string()).unwrap();
        let author = Signature::now("Hongyu Yang", "brutusyhy@gmail.com").unwrap();

        // Git Add
        repo.add(vec![Path::new("TEST.txt")], IndexAddOption::DEFAULT)
            .expect("Failed to add");

        // Git Commit
        repo.commit(&format!("Test Commit {time}"), author)
            .expect("Failed to commit");

        // Git Push
        repo.push(Some("origin"), Some("main"), Some("main"))
            .expect("Failed to push");
    }

    #[test]
    fn test_open_fetch_pull() {
        let repo_url = dotenv::var("GITHUB_TEST_REPO_HTTPS").unwrap();
        let repo_path = Path::new("test-repo");

        let mut auth_agent =
            GitHubAgent::read_from_file().unwrap_or_else(|_| Arc::new(GitHubAgent::new()));

        let repo = RepoHandle::open(repo_path, auth_agent).unwrap();

        println!(
            "HEAD before pulling: {}",
            repo.repo
                .find_reference("HEAD")
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .message()
                .unwrap()
        );

        let _ = repo.fetch(Some("origin"));

        println!(
            "Current FETCH_HEAD: {}",
            repo.repo
                .find_reference("FETCH_HEAD")
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .message()
                .unwrap()
        );

        repo.pull(Some("origin")).unwrap();

        println!(
            "Current HEAD: {}",
            repo.repo
                .find_reference("HEAD")
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .message()
                .unwrap()
        );
    }
}
