use crate::git::{
    repository::{open, open_worktree_owner},
    test_support::{TestDir, linked_worktree_fixture},
};

#[test]
fn open_repository_can_open_a_linked_worktree_path() {
    let dir = TestDir::new("repository-open");
    let fixture = linked_worktree_fixture(&dir, "feature");

    let linked_repo = open(&fixture.linked_path).unwrap();

    assert!(linked_repo.is_worktree());
    assert_eq!(linked_repo.commondir(), fixture.repo.commondir());
}

#[test]
fn open_worktree_owner_returns_the_shared_owner_repo() {
    let dir = TestDir::new("repository-owner");
    let fixture = linked_worktree_fixture(&dir, "feature");
    let linked_repo = open(&fixture.linked_path).unwrap();

    let owner = open_worktree_owner(&linked_repo).unwrap();

    assert_eq!(owner.workdir(), fixture.repo.workdir());
    assert!(owner.find_worktree("feature").is_ok());
}
