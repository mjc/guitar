use super::*;
use crate::git::{
    queries::commits::get_stashed_commits,
    test_support::{TestDir, commit_file, init_repo_at, write_workdir_file},
};
use std::fs;

#[test]
fn stash_message_uses_head_short_sha_and_summary_and_includes_untracked_files() {
    let dir = TestDir::new("stash-message");
    let mut repo = init_repo_at(&dir.join("repo"));
    let oid = commit_file(&repo, "file.txt", "content\n", "base summary");
    write_workdir_file(&repo, "extra.txt", "untracked\n");
    write_workdir_file(&repo, "file.txt", "dirty\n");

    let stash_oid = stash(&mut repo).unwrap();
    let short_id = oid.to_string()[..7].to_string();
    let summary = {
        let stash_commit = repo.find_commit(stash_oid).unwrap();
        stash_commit.summary().unwrap().to_string()
    };

    assert!(summary.contains(&short_id), "stash summary should include the HEAD short SHA");
    assert!(summary.contains("base summary"), "stash summary should include the commit summary");
    assert_eq!(fs::read_to_string(repo.workdir().unwrap().join("file.txt")).unwrap(), "content\n");
    assert!(!repo.workdir().unwrap().join("extra.txt").exists());
    let mut oids = crate::core::oids::Oids::default();
    let gix_repo = gix::open(repo.workdir().unwrap_or(repo.path())).unwrap();
    assert_eq!(get_stashed_commits(&gix_repo, &mut oids).len(), 1);
}

#[test]
fn pop_without_applying_drops_stash_without_restoring_changes() {
    let dir = TestDir::new("stash-drop-only");
    let mut repo = init_repo_at(&dir.join("repo"));
    commit_file(&repo, "file.txt", "content\n", "base summary");
    write_workdir_file(&repo, "file.txt", "dirty\n");
    write_workdir_file(&repo, "extra.txt", "untracked\n");

    let stash_oid = stash(&mut repo).unwrap();
    pop(&mut repo, &stash_oid, false).unwrap();

    let mut oids = crate::core::oids::Oids::default();
    let gix_repo = gix::open(repo.workdir().unwrap_or(repo.path())).unwrap();
    assert!(get_stashed_commits(&gix_repo, &mut oids).is_empty());
    assert_eq!(fs::read_to_string(repo.workdir().unwrap().join("file.txt")).unwrap(), "content\n");
    assert!(!repo.workdir().unwrap().join("extra.txt").exists());
}

#[test]
fn pop_with_apply_leaves_conflicts_and_drops_the_stash() {
    let dir = TestDir::new("stash-conflict");
    let mut repo = init_repo_at(&dir.join("repo"));
    commit_file(&repo, "file.txt", "content\n", "base summary");
    write_workdir_file(&repo, "file.txt", "ours\n");

    let stash_oid = stash(&mut repo).unwrap();
    commit_file(&repo, "file.txt", "theirs\n", "conflict base");

    assert!(pop(&mut repo, &stash_oid, true).is_ok());
    assert!(repo.index().unwrap().has_conflicts());
    let mut oids = crate::core::oids::Oids::default();
    let gix_repo = gix::open(repo.workdir().unwrap_or(repo.path())).unwrap();
    assert!(get_stashed_commits(&gix_repo, &mut oids).is_empty());
}
