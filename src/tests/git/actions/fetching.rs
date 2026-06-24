use super::*;
use crate::git::{
    actions::{tagging::tag, worktrees::create_worktree},
    auth::{AuthSession, NetworkResult},
    test_support::{TestDir, add_remote_path, commit_file, create_branch, init_bare_repo_at, init_repo_at},
};
use git2::Repository;
use std::fs;

fn seed_remote(repo: &Repository, remote_name: &str, refspecs: &[&str]) {
    let mut remote = repo.find_remote(remote_name).unwrap();
    remote.push(refspecs, None).unwrap();
}

#[test]
fn fetch_populates_remote_tracking_refs_and_tags() {
    let dir = TestDir::new("fetch");
    let source = init_repo_at(&dir.join("source"));
    let commit = commit_file(&source, "file.txt", "source\n", "source");
    create_branch(&source, "feature", commit);
    tag(&source, commit, "v1.0.0").unwrap();

    let remote_path = dir.join("remote.git");
    init_bare_repo_at(&remote_path);
    add_remote_path(&source, "origin", &remote_path);
    seed_remote(&source, "origin", &["refs/heads/feature:refs/heads/feature", "refs/tags/v1.0.0:refs/tags/v1.0.0"]);

    let consumer = init_repo_at(&dir.join("consumer"));
    add_remote_path(&consumer, "origin", &remote_path);

    let handle = fetch_remote(consumer.workdir().unwrap().to_str().unwrap(), "origin", AuthSession::default());
    assert!(matches!(handle.join().unwrap(), NetworkResult::Success));

    assert_eq!(consumer.find_reference("refs/remotes/origin/feature").unwrap().target(), Some(commit));
    assert!(consumer.find_reference("refs/tags/v1.0.0").is_ok());
}

#[test]
fn fetch_reports_missing_or_unreachable_remotes() {
    let dir = TestDir::new("fetch-failures");
    let consumer = init_repo_at(&dir.join("consumer"));

    let handle = fetch_remote(consumer.workdir().unwrap().to_str().unwrap(), "origin", AuthSession::default());
    assert!(matches!(handle.join().unwrap(), NetworkResult::Failure(_)));

    let source = init_repo_at(&dir.join("source"));
    let commit = commit_file(&source, "file.txt", "source\n", "source");
    create_branch(&source, "feature", commit);

    let remote_path = dir.join("remote.git");
    init_bare_repo_at(&remote_path);
    add_remote_path(&source, "origin", &remote_path);
    seed_remote(&source, "origin", &["refs/heads/feature:refs/heads/feature"]);

    let configured = init_repo_at(&dir.join("configured"));
    add_remote_path(&configured, "origin", &remote_path);
    fs::remove_dir_all(&remote_path).unwrap();

    let handle = fetch_remote(configured.workdir().unwrap().to_str().unwrap(), "origin", AuthSession::default());
    assert!(matches!(handle.join().unwrap(), NetworkResult::Failure(_)));
}

#[test]
fn fetch_supports_linked_worktree_paths() {
    let dir = TestDir::new("fetch-linked-worktree");
    let source = init_repo_at(&dir.join("source"));
    let commit = commit_file(&source, "file.txt", "source\n", "source");
    create_branch(&source, "feature", commit);

    let remote_path = dir.join("remote.git");
    init_bare_repo_at(&remote_path);
    add_remote_path(&source, "origin", &remote_path);
    seed_remote(&source, "origin", &["refs/heads/feature:refs/heads/feature"]);

    let consumer = init_repo_at(&dir.join("consumer"));
    let consumer_commit = commit_file(&consumer, "consumer.txt", "consumer\n", "consumer");
    add_remote_path(&consumer, "origin", &remote_path);
    let linked_path = dir.join("linked");
    create_worktree(&consumer, "linked", &linked_path, consumer_commit).unwrap();

    let handle = fetch_remote(linked_path.to_str().unwrap(), "origin", AuthSession::default());
    assert!(matches!(handle.join().unwrap(), NetworkResult::Success));

    let linked_repo = Repository::open(&linked_path).unwrap();
    assert_eq!(linked_repo.find_reference("refs/remotes/origin/feature").unwrap().target(), Some(commit));
}
