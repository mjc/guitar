use super::*;
use crate::git::{
    actions::tagging::tag,
    auth::{AuthSession, NetworkResult},
    test_support::{TestDir, add_remote_path, commit_file, create_branch, init_bare_repo_at, init_repo_at},
};
use git2::{ObjectType, Repository};

fn seed_remote(repo: &Repository, remote_name: &str, refspecs: &[&str]) {
    let mut remote = repo.find_remote(remote_name).unwrap();
    remote.push(refspecs, None).unwrap();
}

#[test]
fn push_branch_updates_the_remote_branch() {
    let dir = TestDir::new("push-branch");
    let source = init_repo_at(&dir.join("source"));
    let commit = commit_file(&source, "file.txt", "source\n", "source");
    create_branch(&source, "feature", commit);
    let remote_path = dir.join("remote.git");
    init_bare_repo_at(&remote_path);
    add_remote_path(&source, "origin", &remote_path);

    let handle = push_branch(source.workdir().unwrap().to_str().unwrap(), "origin", "feature", false, AuthSession::default());
    assert!(matches!(handle.join().unwrap(), NetworkResult::Success));

    let remote = Repository::open(&remote_path).unwrap();
    assert_eq!(remote.find_reference("refs/heads/feature").unwrap().target(), Some(commit));
}

#[test]
fn push_tags_updates_remote_tags() {
    let dir = TestDir::new("push-tags");
    let source = init_repo_at(&dir.join("source"));
    let commit = commit_file(&source, "file.txt", "source\n", "source");
    tag(&source, commit, "v1.0.0").unwrap();

    let commit_obj = source.find_object(commit, Some(ObjectType::Commit)).unwrap();
    let signature = source.signature().unwrap();
    source.tag("v2.0.0", &commit_obj, &signature, "release", false).unwrap();

    let remote_path = dir.join("remote.git");
    init_bare_repo_at(&remote_path);
    add_remote_path(&source, "origin", &remote_path);

    let handle = push_tags(source.workdir().unwrap().to_str().unwrap(), "origin", AuthSession::default());
    assert!(matches!(handle.join().unwrap(), NetworkResult::Success));

    let remote = Repository::open(&remote_path).unwrap();
    assert!(remote.find_reference("refs/tags/v1.0.0").is_ok());
    assert!(remote.find_reference("refs/tags/v2.0.0").is_ok());
}

#[test]
fn delete_remote_branch_removes_the_remote_ref() {
    let dir = TestDir::new("delete-remote-branch");
    let source = init_repo_at(&dir.join("source"));
    let commit = commit_file(&source, "file.txt", "source\n", "source");
    create_branch(&source, "feature", commit);

    let remote_path = dir.join("remote.git");
    init_bare_repo_at(&remote_path);
    add_remote_path(&source, "origin", &remote_path);
    seed_remote(&source, "origin", &["refs/heads/feature:refs/heads/feature"]);

    let handle = delete_remote_branch(source.workdir().unwrap().to_str().unwrap(), "origin", "feature", AuthSession::default());
    assert!(matches!(handle.join().unwrap(), NetworkResult::Success));

    let remote = Repository::open(&remote_path).unwrap();
    assert!(remote.find_reference("refs/heads/feature").is_err());
}
