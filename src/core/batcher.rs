use crate::{
    core::oids::IntoGixOid,
    git::gix::{commit_graph_if_available, for_each_branch_tip, gix_error},
};
use gix::traverse::commit::ParentIds;
use im::HashSet;
use std::collections::HashSet as StdHashSet;

type CommitWalk = gix::traverse::commit::Simple<gix::OdbHandle, fn(&gix::oid) -> bool>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkCommit {
    pub oid: gix::ObjectId,
    pub parent_ids: ParentIds,
    pub commit_time: Option<i64>,
}

// Own a lazy commit cursor so history pages don't precompute the entire graph.
pub struct Batcher {
    walk: Option<CommitWalk>,
}

impl Batcher {
    // Build the initial commit cursor from all visible local and remote branch tips.
    pub fn new<I: IntoIterator<Item = O>, O: IntoGixOid>(repo: &gix::Repository, hidden_branch_names: &HashSet<String>, extra_roots: I) -> Result<Self, git2::Error> {
        let walk = Self::build(repo, hidden_branch_names, extra_roots)?;
        Ok(Self { walk: Some(walk) })
    }

    // Recreate the cursor after branch filters, fetches, or repository state changes.
    pub fn reset<I: IntoIterator<Item = O>, O: IntoGixOid>(&mut self, repo: &gix::Repository, hidden_branch_names: &HashSet<String>, extra_roots: I) -> Result<(), git2::Error> {
        self.walk = Some(Self::build(repo, hidden_branch_names, extra_roots)?);
        Ok(())
    }

    // Pull the next page, dropping commits the object database cannot resolve.
    pub fn next(&mut self, count: usize) -> Vec<WalkCommit> {
        let mut page = Vec::with_capacity(count);
        self.next_into(count, &mut page);
        page
    }

    // Pull the next page into an existing output buffer to avoid a temporary page allocation.
    pub fn next_into(&mut self, count: usize, out: &mut Vec<WalkCommit>) -> usize {
        let before = out.len();
        let Some(walk) = self.walk.as_mut() else {
            return 0;
        };

        while out.len() - before < count {
            let Some(result) = walk.next() else {
                self.walk = None;
                break;
            };

            let Ok(info) = result else { continue };
            out.push(WalkCommit { oid: info.id, parent_ids: info.parent_ids, commit_time: info.commit_time });
        }
        out.len() - before
    }

    fn build<I: IntoIterator<Item = O>, O: IntoGixOid>(repo: &gix::Repository, hidden_branch_names: &HashSet<String>, extra_roots: I) -> Result<CommitWalk, git2::Error> {
        let mut pushed: StdHashSet<gix::ObjectId> = StdHashSet::new();
        let mut tips: Vec<gix::ObjectId> = Vec::new();

        for_each_branch_tip(repo, |_, name, oid| {
            // Hidden branch names are a deny-list; new branches are visible by default.
            if !hidden_branch_names.contains(name) && pushed.insert(oid) {
                tips.push(oid);
            }
        })?;

        tips.extend(extra_roots.into_iter().map(IntoGixOid::into_gix_oid).filter(|oid| pushed.insert(*oid)));

        let commit_graph = commit_graph_if_available(repo);
        let walk = gix::traverse::commit::Simple::new(tips, repo.objects.clone())
            .sorting(gix::traverse::commit::simple::Sorting::ByCommitTime(gix::traverse::commit::simple::CommitTimeOrder::NewestFirst))
            .map(|walk| walk.commit_graph(commit_graph))
            .map_err(gix_error)?;
        Ok(walk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::oids::git2_to_gix_oid;
    use git2::{BranchType, Commit, Oid, Repository, Signature};
    use std::{
        collections::HashSet as StdHashSet,
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_repo(name: &str) -> (PathBuf, Repository) {
        let id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("guitar-batcher-{name}-{id}"));
        fs::create_dir_all(&path).unwrap();
        let repo = Repository::init(&path).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test User").unwrap();
            config.set_str("user.email", "test@example.com").unwrap();
        }
        (path, repo)
    }

    fn commit(repo: &Repository, file: &str, message: &str) -> Oid {
        let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
        let parents: Vec<&Commit<'_>> = parent.iter().collect();
        commit_with_parents(repo, file, message, &parents)
    }

    fn commit_with_parents(repo: &Repository, file: &str, message: &str, parents: &[&Commit<'_>]) -> Oid {
        let workdir = repo.workdir().unwrap().to_path_buf();
        fs::write(workdir.join(file), message).unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new(file)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("Test User", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, parents).unwrap()
    }

    fn branch_tip(repo: &Repository, name: &str) -> Oid {
        repo.find_branch(name, BranchType::Local).unwrap().get().target().unwrap()
    }

    fn head_refname(repo: &Repository) -> String {
        repo.head().unwrap().name().unwrap().to_string()
    }

    fn gix_oids(oids: impl IntoIterator<Item = Oid>) -> Vec<gix::ObjectId> {
        oids.into_iter().map(git2_to_gix_oid).collect()
    }

    #[test]
    fn next_into_appends_pages_without_replacing_existing_output() {
        let (path, repo) = temp_repo("next-into");
        let first = commit(&repo, "first.txt", "first");
        let second = commit(&repo, "second.txt", "second");
        let third = commit(&repo, "third.txt", "third");
        let sentinel = WalkCommit { oid: git2_to_gix_oid(Oid::zero()), parent_ids: ParentIds::new(), commit_time: None };
        let gix_repo = gix::open(&path).unwrap();
        let mut batcher = Batcher::new(&gix_repo, &HashSet::new(), [third]).unwrap();
        let mut out = vec![sentinel.clone()];

        assert_eq!(batcher.next_into(2, &mut out), 2);
        assert_eq!(batcher.next_into(2, &mut out), 1);
        assert_eq!(batcher.next_into(2, &mut out), 0);

        let oids = out.iter().map(|commit| commit.oid).collect::<Vec<_>>();
        assert_eq!(oids, gix_oids([Oid::zero(), third, second, first]));
        assert_eq!(out[1].parent_ids.iter().map(|id| Oid::from_bytes(id.as_slice()).unwrap()).collect::<Vec<_>>(), vec![second]);
        assert_eq!(out[2].parent_ids.iter().map(|id| Oid::from_bytes(id.as_slice()).unwrap()).collect::<Vec<_>>(), vec![first]);
        assert!(out[3].parent_ids.is_empty());
    }

    #[test]
    fn duplicate_branch_tips_are_returned_once() {
        let (path, repo) = temp_repo("duplicate-tips");
        let first = commit(&repo, "first.txt", "first");
        let second = commit(&repo, "second.txt", "second");
        repo.branch("duplicate", &repo.find_commit(second).unwrap(), false).unwrap();
        let gix_repo = gix::open(&path).unwrap();
        let mut batcher = Batcher::new(&gix_repo, &HashSet::new(), [second]).unwrap();

        assert_eq!(batcher.next(10).iter().map(|commit| commit.oid).collect::<Vec<_>>(), gix_oids([second, first]));
    }

    #[test]
    fn hidden_branch_tip_is_not_used_as_a_walk_root() {
        let (path, repo) = temp_repo("hidden-tip");
        let main = commit(&repo, "main.txt", "main");
        let main_ref = head_refname(&repo);
        repo.branch("hidden", &repo.find_commit(main).unwrap(), false).unwrap();
        repo.set_head("refs/heads/hidden").unwrap();
        repo.checkout_head(None).unwrap();
        let hidden = commit(&repo, "hidden.txt", "hidden");
        repo.set_head(&main_ref).unwrap();
        repo.checkout_head(None).unwrap();

        let mut hidden_names = HashSet::new();
        hidden_names.insert("hidden".to_string());
        let gix_repo = gix::open(&path).unwrap();
        let mut batcher = Batcher::new(&gix_repo, &hidden_names, std::iter::empty::<gix::ObjectId>()).unwrap();

        assert_eq!(branch_tip(&repo, "hidden"), hidden);
        assert_eq!(batcher.next(10).iter().map(|commit| commit.oid).collect::<Vec<_>>(), gix_oids([main]));
    }

    #[test]
    fn remote_branch_tip_is_used_and_can_be_hidden() {
        let (path, repo) = temp_repo("remote-tip");
        let base = commit(&repo, "base.txt", "base");
        let main_ref = head_refname(&repo);
        repo.branch("side", &repo.find_commit(base).unwrap(), false).unwrap();
        repo.set_head("refs/heads/side").unwrap();
        repo.checkout_head(None).unwrap();
        let side = commit(&repo, "side.txt", "side");
        repo.set_head(&main_ref).unwrap();
        repo.checkout_head(None).unwrap();
        repo.find_branch("side", BranchType::Local).unwrap().delete().unwrap();
        repo.reference("refs/remotes/origin/side", side, true, "test").unwrap();

        let gix_repo = gix::open(&path).unwrap();
        let mut batcher = Batcher::new(&gix_repo, &HashSet::new(), std::iter::empty::<gix::ObjectId>()).unwrap();
        assert_eq!(batcher.next(10).iter().map(|commit| commit.oid).collect::<StdHashSet<_>>(), StdHashSet::from([git2_to_gix_oid(side), git2_to_gix_oid(base)]));

        let mut hidden_names = HashSet::new();
        hidden_names.insert("origin/side".to_string());
        let mut batcher = Batcher::new(&gix_repo, &hidden_names, std::iter::empty::<gix::ObjectId>()).unwrap();
        assert_eq!(batcher.next(10).iter().map(|commit| commit.oid).collect::<Vec<_>>(), gix_oids([base]));
    }

    #[test]
    fn merge_commit_preserves_first_two_parent_ids() {
        let (path, repo) = temp_repo("merge-parents");
        let first = commit(&repo, "first.txt", "first");
        let main_ref = head_refname(&repo);
        repo.branch("side", &repo.find_commit(first).unwrap(), false).unwrap();
        let main = commit(&repo, "main.txt", "main");
        repo.set_head("refs/heads/side").unwrap();
        repo.checkout_head(None).unwrap();
        let side = commit(&repo, "side.txt", "side");
        repo.set_head(&main_ref).unwrap();
        repo.checkout_head(None).unwrap();
        let merge = {
            let main_commit = repo.find_commit(main).unwrap();
            let side_commit = repo.find_commit(side).unwrap();
            commit_with_parents(&repo, "merge.txt", "merge", &[&main_commit, &side_commit])
        };
        let gix_repo = gix::open(&path).unwrap();
        let mut batcher = Batcher::new(&gix_repo, &HashSet::new(), [merge]).unwrap();

        let page = batcher.next(10);
        let merge_commit = page.iter().find(|commit| commit.oid == git2_to_gix_oid(merge)).expect("merge commit is returned");

        assert_eq!(merge_commit.parent_ids.iter().map(|id| Oid::from_bytes(id.as_slice()).unwrap()).collect::<Vec<_>>(), vec![main, side]);
    }

    #[test]
    fn exhausted_batcher_stays_empty_until_reset() {
        let (path, repo) = temp_repo("exhausted");
        let first = commit(&repo, "first.txt", "first");
        let second = commit(&repo, "second.txt", "second");
        let gix_repo = gix::open(&path).unwrap();
        let mut batcher = Batcher::new(&gix_repo, &HashSet::new(), [second]).unwrap();

        assert_eq!(batcher.next(10).iter().map(|commit| commit.oid).collect::<Vec<_>>(), gix_oids([second, first]));
        assert!(batcher.next(10).is_empty());
        assert!(batcher.next(10).is_empty());

        let third = commit(&repo, "third.txt", "third");
        let gix_repo = gix::open(&path).unwrap();
        batcher.reset(&gix_repo, &HashSet::new(), [third]).unwrap();

        assert_eq!(batcher.next(10).iter().map(|commit| commit.oid).collect::<Vec<_>>(), gix_oids([third, second, first]));
    }
}
