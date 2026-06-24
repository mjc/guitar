use super::*;
use crate::{
    core::oids::git2_to_gix_oid,
    git::{
        actions::worktrees::{create_worktree, lock_worktree, remove_worktree, unlock_worktree},
        queries::commits::get_current_branch,
        test_support::{TestDir, commit_file, init_repo_at, linked_worktree_fixture, stage_path, write_workdir_file},
    },
};
use std::fs;

#[test]
fn lists_main_and_linked_worktrees_with_stable_metadata() {
    let dir = TestDir::new("worktree-list");
    let repo_path = dir.join("repo");
    let repo = init_repo_at(&repo_path);
    let oid = commit_file(&repo, "file.txt", "hello\n", "initial");

    create_worktree(&repo, "zeta", &dir.join("repo-zeta"), oid).unwrap();
    create_worktree(&repo, "alpha", &dir.join("repo-alpha"), oid).unwrap();

    let entries = list_worktrees(&repo, Some(&repo_path)).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name, "repo");
    assert!(entries[0].is_main());
    assert!(entries[0].is_current);
    assert_eq!(entries[0].branch.as_deref(), get_current_branch(&repo).as_deref());
    assert_eq!(entries[0].head, Some(git2_to_gix_oid(oid)));

    let linked_names: Vec<_> = entries.iter().skip(1).map(|entry| entry.name.as_str()).collect();
    assert_eq!(linked_names, vec!["alpha", "zeta"]);

    for entry in entries.iter().skip(1) {
        assert!(entry.is_linked());
        assert!(entry.is_valid);
        assert!(!entry.is_current);
        assert_eq!(entry.branch.as_deref(), Some(entry.name.as_str()));
        assert_eq!(entry.head, Some(git2_to_gix_oid(oid)));
        assert!(!entry.is_dirty);
        assert!(entry.locked_reason.is_none());
        assert!(!entry.is_prunable);
    }
}

#[test]
fn marks_current_linked_worktree() {
    let dir = TestDir::new("worktree-current");
    let fixture = linked_worktree_fixture(&dir, "feature");

    let entries = list_worktrees(&fixture.linked_repo, Some(&fixture.linked_path)).unwrap();
    let linked = entries.iter().find(|entry| entry.name == "feature").unwrap();
    let main = entries.iter().find(|entry| entry.is_main()).unwrap();

    assert!(linked.is_current);
    assert_eq!(linked.branch.as_deref(), Some("feature"));
    assert_eq!(linked.head, Some(git2_to_gix_oid(fixture.base)));
    assert!(main.is_main());
    assert!(!main.is_current);
    assert_eq!(main.head, Some(git2_to_gix_oid(fixture.base)));
}

#[test]
fn marks_dirty_worktrees_when_staged_files_exist() {
    let dir = TestDir::new("worktree-staged");
    let fixture = linked_worktree_fixture(&dir, "feature");

    write_workdir_file(&fixture.linked_repo, "staged.txt", "staged\n");
    stage_path(&fixture.linked_repo, "staged.txt");

    let entries = list_worktrees(&fixture.repo, Some(&fixture.repo_path)).unwrap();
    let linked = entries.iter().find(|entry| entry.name == "feature").unwrap();

    assert!(linked.is_dirty);
    assert!(linked.is_linked());
}

#[test]
fn marks_dirty_main_worktree_when_staged_files_exist() {
    let dir = TestDir::new("worktree-dirty");
    let repo_path = dir.join("repo");
    let repo = init_repo_at(&repo_path);
    commit_file(&repo, "file.txt", "hello\n", "initial");

    write_workdir_file(&repo, "staged.txt", "staged\n");
    stage_path(&repo, "staged.txt");

    let entries = list_worktrees(&repo, Some(&repo_path)).unwrap();
    let main = entries.iter().find(|entry| entry.is_main()).unwrap();

    assert!(main.is_current);
    assert!(main.is_dirty);
}

#[test]
fn metadata_listing_skips_dirty_scan_but_keeps_identity() {
    let dir = TestDir::new("worktree-metadata");
    let repo_path = dir.join("repo");
    let repo = init_repo_at(&repo_path);
    let oid = commit_file(&repo, "file.txt", "hello\n", "initial");

    fs::write(repo_path.join("untracked.txt"), "extra\n").unwrap();

    let entries = list_worktrees_metadata(&repo, Some(&repo_path)).unwrap();
    let main = entries.iter().find(|entry| entry.is_main()).unwrap();

    assert!(main.is_current);
    assert_eq!(main.head, Some(git2_to_gix_oid(oid)));
    assert_eq!(main.branch.as_deref(), get_current_branch(&repo).as_deref());
    assert!(!main.is_dirty);
}

#[test]
fn metadata_listing_can_mark_current_worktree_from_uncommitted_state() {
    let dir = TestDir::new("worktree-current-dirty-metadata");
    let fixture = linked_worktree_fixture(&dir, "feature");

    let entries =
        list_worktrees_metadata_with_current_dirty(&fixture.linked_repo, Some(&fixture.linked_path), &crate::git::queries::helpers::UncommittedChanges { is_clean: false, ..Default::default() })
            .unwrap();
    let path_entries = list_worktrees_metadata_with_current_dirty_from_path(
        &fixture.linked_path,
        Some(&fixture.linked_path),
        &crate::git::queries::helpers::UncommittedChanges { is_clean: false, ..Default::default() },
    )
    .unwrap();
    let linked = entries.iter().find(|entry| entry.name == "feature").unwrap();
    let main = entries.iter().find(|entry| entry.is_main()).unwrap();
    let path_linked = path_entries.iter().find(|entry| entry.name == "feature").unwrap();

    assert!(linked.is_current);
    assert!(linked.is_dirty);
    assert!(path_linked.is_current);
    assert!(path_linked.is_dirty);
    assert!(!main.is_current);
    assert!(!main.is_dirty);
}

#[test]
fn reports_lock_reason_and_prunability_for_stale_worktrees() {
    let dir = TestDir::new("worktree-stale");
    let repo_path = dir.join("repo");
    let repo = init_repo_at(&repo_path);
    let oid = commit_file(&repo, "file.txt", "hello\n", "initial");

    create_worktree(&repo, "feature", &dir.join("repo-feature"), oid).unwrap();
    lock_worktree(&repo, "feature", Some("keep it")).unwrap();

    let locked_entries = list_worktrees(&repo, Some(&repo_path)).unwrap();
    let locked = locked_entries.iter().find(|entry| entry.name == "feature").unwrap();
    assert_eq!(locked.locked_reason.as_deref(), Some("keep it"));
    assert!(!locked.can_remove());

    unlock_worktree(&repo, "feature").unwrap();
    remove_worktree(&repo, "feature").unwrap();
    assert!(repo.find_worktree("feature").is_err());

    let stale_path = dir.join("repo-stale");
    create_worktree(&repo, "stale", &stale_path, oid).unwrap();
    fs::remove_dir_all(&stale_path).unwrap();

    let entries = list_worktrees(&repo, Some(&repo_path)).unwrap();
    let stale = entries.iter().find(|entry| entry.name == "stale").unwrap();
    assert!(!stale.is_valid);
    assert!(stale.is_prunable);
}
