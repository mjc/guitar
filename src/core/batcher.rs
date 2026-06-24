use crate::{
    core::{
        chunk::NONE,
        oids::{IntoGixOid, Oids},
    },
    git::gix::{commit_graph_if_available, for_each_branch_tip, gix_error},
};
use gix::traverse::commit::{Either, find};
use im::HashSet;
use rustc_hash::FxHashSet;
use std::{cmp::Ordering, collections::BinaryHeap};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkCommit {
    pub oid: gix::ObjectId,
    pub alias: u32,
    parent_aliases: [u32; 2],
    parent_len: u8,
    pub commit_time: Option<i64>,
}

impl WalkCommit {
    fn new(oid: gix::ObjectId, alias: u32, parent_aliases: [u32; 2], parent_len: u8, commit_time: Option<i64>) -> Self {
        Self { oid, alias, parent_aliases, parent_len, commit_time }
    }

    pub fn is_parentless(&self) -> bool {
        self.parent_len == 0
    }

    pub fn first_parent_alias(&self) -> u32 {
        (self.parent_len > 0).then_some(self.parent_aliases[0]).unwrap_or(NONE)
    }

    pub fn second_parent_alias(&self) -> u32 {
        (self.parent_len > 1).then_some(self.parent_aliases[1]).unwrap_or(NONE)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QueueEntry {
    commit_time: i64,
    oid: gix::ObjectId,
    alias: u32,
    graph_pos: Option<gix::commitgraph::Position>,
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.commit_time.cmp(&other.commit_time)
    }
}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct CommitWalk {
    queue: BinaryHeap<QueueEntry>,
    seen: SeenCommits,
    objects: gix::OdbHandle,
    commit_graph: Option<gix::commitgraph::Graph>,
    commit_buf: Vec<u8>,
    time_buf: Vec<u8>,
}

#[derive(Default)]
struct SeenCommits {
    graph_positions: Vec<u64>,
    loose_oids: FxHashSet<gix::ObjectId>,
}

impl SeenCommits {
    fn new(commit_graph: Option<&gix::commitgraph::Graph>, loose_capacity: usize) -> Self {
        let graph_positions = commit_graph.map_or_else(Vec::new, |graph| vec![0; bit_words(graph.num_commits() as usize)]);
        Self { graph_positions, loose_oids: FxHashSet::with_capacity_and_hasher(loose_capacity, Default::default()) }
    }

    fn insert_graph_pos(&mut self, pos: gix::commitgraph::Position) -> bool {
        let pos = pos.0 as usize;
        let word = pos / u64::BITS as usize;
        let mask = 1u64 << (pos % u64::BITS as usize);
        let Some(bits) = self.graph_positions.get_mut(word) else {
            return false;
        };
        if *bits & mask != 0 {
            return false;
        }
        *bits |= mask;
        true
    }

    fn insert_loose_oid(&mut self, oid: gix::ObjectId) -> bool {
        self.loose_oids.insert(oid)
    }
}

fn bit_words(bits: usize) -> usize {
    bits.div_ceil(u64::BITS as usize)
}

impl CommitWalk {
    fn new(repo: &gix::Repository, tips: Vec<(gix::ObjectId, u32)>) -> Result<Self, git2::Error> {
        let commit_graph = commit_graph_if_available(repo);
        let mut walk = Self {
            queue: BinaryHeap::with_capacity(tips.len()),
            seen: SeenCommits::new(commit_graph.as_ref(), tips.len()),
            objects: repo.objects.clone(),
            commit_graph,
            commit_buf: Vec::new(),
            time_buf: Vec::new(),
        };

        for (tip, alias) in tips {
            walk.enqueue(tip, alias)?;
        }

        Ok(walk)
    }

    fn enqueue(&mut self, oid: gix::ObjectId, alias: u32) -> Result<(), git2::Error> {
        enqueue_with(&mut self.queue, &mut self.seen, &self.objects, self.commit_graph.as_ref(), &mut self.time_buf, oid, alias)
    }

    fn next_commit(&mut self) -> Option<WalkCommit> {
        self.next_commit_with_aliases(None)
    }

    fn next_commit_aliased(&mut self, oids: &mut Oids) -> Option<WalkCommit> {
        self.next_commit_with_aliases(Some(oids))
    }

    fn next_commit_with_aliases(&mut self, mut oids: Option<&mut Oids>) -> Option<WalkCommit> {
        while let Some(QueueEntry { commit_time, oid, alias, graph_pos }) = self.queue.pop() {
            if let (Some(cache), Some(pos)) = (self.commit_graph.as_ref(), graph_pos) {
                let commit = cache.commit_at(pos);
                let mut parent_aliases = [NONE; 2];
                let mut parent_len = 0u8;
                let mut had_parent_error = false;
                let queue = &mut self.queue;
                let seen = &mut self.seen;
                for parent_pos in commit.iter_parents() {
                    let Ok(parent_pos) = parent_pos else {
                        had_parent_error = true;
                        break;
                    };
                    let parent = cache.commit_at(parent_pos);
                    let parent_id = parent.id().to_owned();
                    let parent_alias = oids.as_mut().map_or(NONE, |oids| oids.get_alias_by_oid(parent_id));
                    if parent_len < 2 {
                        parent_aliases[parent_len as usize] = parent_alias;
                        parent_len += 1;
                    }
                    let _ = enqueue_graph_position(queue, seen, cache, parent_pos, parent_id, parent_alias);
                }
                if had_parent_error {
                    continue;
                }
                return Some(WalkCommit::new(oid, alias, parent_aliases, parent_len, Some(commit_time)));
            }

            let commit = match find(self.commit_graph.as_ref(), &self.objects, oid.as_ref(), &mut self.commit_buf).map_err(gix_error) {
                Ok(Either::CachedCommit(commit)) => {
                    let mut parent_aliases = [NONE; 2];
                    let mut parent_len = 0u8;
                    let mut had_parent_error = false;
                    let cache = self.commit_graph.as_ref().expect("cached commits are backed by a commit graph");
                    let queue = &mut self.queue;
                    let seen = &mut self.seen;
                    for parent_pos in commit.iter_parents() {
                        let Ok(parent_pos) = parent_pos else {
                            had_parent_error = true;
                            break;
                        };
                        let parent = cache.commit_at(parent_pos);
                        let parent_id = parent.id().to_owned();
                        let parent_alias = oids.as_mut().map_or(NONE, |oids| oids.get_alias_by_oid(parent_id));
                        if parent_len < 2 {
                            parent_aliases[parent_len as usize] = parent_alias;
                            parent_len += 1;
                        }
                        let _ = enqueue_graph_position(queue, seen, cache, parent_pos, parent_id, parent_alias);
                    }
                    if had_parent_error {
                        continue;
                    }
                    WalkCommit::new(oid, alias, parent_aliases, parent_len, Some(commit_time))
                },
                Ok(Either::CommitRefIter(iter)) => {
                    let mut parent_aliases = [NONE; 2];
                    let mut parent_len = 0u8;
                    let queue = &mut self.queue;
                    let seen = &mut self.seen;
                    let objects = &self.objects;
                    let commit_graph = self.commit_graph.as_ref();
                    let time_buf = &mut self.time_buf;
                    for parent_id in iter.parent_ids() {
                        let parent_alias = oids.as_mut().map_or(NONE, |oids| oids.get_alias_by_oid(parent_id));
                        if parent_len < 2 {
                            parent_aliases[parent_len as usize] = parent_alias;
                            parent_len += 1;
                        }
                        let _ = enqueue_with(queue, seen, objects, commit_graph, time_buf, parent_id, parent_alias);
                    }
                    WalkCommit::new(oid, alias, parent_aliases, parent_len, Some(commit_time))
                },
                Err(_) => continue,
            };

            return Some(commit);
        }

        None
    }
}

fn enqueue_with(
    queue: &mut BinaryHeap<QueueEntry>, seen: &mut SeenCommits, objects: &gix::OdbHandle, commit_graph: Option<&gix::commitgraph::Graph>, time_buf: &mut Vec<u8>, oid: gix::ObjectId, alias: u32,
) -> Result<(), git2::Error> {
    if let Some(graph) = commit_graph
        && let Some(pos) = graph.lookup(oid.as_ref())
    {
        return enqueue_graph_position(queue, seen, graph, pos, oid, alias);
    }

    if !seen.insert_loose_oid(oid) {
        return Ok(());
    }
    let commit_time = match find(commit_graph, objects, oid.as_ref(), time_buf).map_err(gix_error)? {
        Either::CachedCommit(commit) => commit.committer_timestamp() as i64,
        Either::CommitRefIter(iter) => iter.committer().map_err(gix_error)?.seconds(),
    };

    queue.push(QueueEntry { commit_time, oid, alias, graph_pos: None });
    Ok(())
}

fn enqueue_graph_position(
    queue: &mut BinaryHeap<QueueEntry>, seen: &mut SeenCommits, graph: &gix::commitgraph::Graph, pos: gix::commitgraph::Position, oid: gix::ObjectId, alias: u32,
) -> Result<(), git2::Error> {
    if !seen.insert_graph_pos(pos) {
        return Ok(());
    }
    let commit_time = graph.commit_at(pos).committer_timestamp() as i64;
    queue.push(QueueEntry { commit_time, oid, alias, graph_pos: Some(pos) });
    Ok(())
}

// Own a lazy commit cursor so history pages don't precompute the entire graph.
pub struct Batcher {
    walk: Option<CommitWalk>,
}

impl Batcher {
    pub fn from_tips(repo: &gix::Repository, tips: Vec<gix::ObjectId>) -> Result<Self, git2::Error> {
        Ok(Self { walk: Some(Self::walk_from_tips(repo, tips)?) })
    }

    pub fn from_tips_with_oids(repo: &gix::Repository, tips: Vec<gix::ObjectId>, oids: &mut Oids) -> Result<Self, git2::Error> {
        oids.reserve_aliases(tips.len());
        Ok(Self { walk: Some(Self::walk_from_aliased_tips(repo, tips.into_iter().map(|oid| (oid, oids.get_alias_by_oid(oid))).collect())?) })
    }

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
            let Some(info) = walk.next_commit() else {
                self.walk = None;
                break;
            };
            out.push(info);
        }
        out.len() - before
    }

    pub fn next_aliased_into(&mut self, count: usize, out: &mut Vec<WalkCommit>, oids: &mut Oids) -> usize {
        let before = out.len();
        let Some(walk) = self.walk.as_mut() else {
            return 0;
        };

        oids.reserve_aliases(count.saturating_mul(2));
        while out.len() - before < count {
            let Some(info) = walk.next_commit_aliased(oids) else {
                self.walk = None;
                break;
            };
            out.push(info);
        }
        out.len() - before
    }

    fn build<I: IntoIterator<Item = O>, O: IntoGixOid>(repo: &gix::Repository, hidden_branch_names: &HashSet<String>, extra_roots: I) -> Result<CommitWalk, git2::Error> {
        let mut pushed: FxHashSet<gix::ObjectId> = FxHashSet::default();
        let mut tips: Vec<gix::ObjectId> = Vec::new();

        for_each_branch_tip(repo, |_, name, oid| {
            // Hidden branch names are a deny-list; new branches are visible by default.
            if !hidden_branch_names.contains(name) && pushed.insert(oid) {
                tips.push(oid);
            }
        })?;

        tips.extend(extra_roots.into_iter().map(IntoGixOid::into_gix_oid).filter(|oid| pushed.insert(*oid)));

        Self::walk_from_tips(repo, tips)
    }

    fn walk_from_tips(repo: &gix::Repository, tips: Vec<gix::ObjectId>) -> Result<CommitWalk, git2::Error> {
        Self::walk_from_aliased_tips(repo, tips.into_iter().map(|oid| (oid, NONE)).collect())
    }

    fn walk_from_aliased_tips(repo: &gix::Repository, tips: Vec<(gix::ObjectId, u32)>) -> Result<CommitWalk, git2::Error> {
        CommitWalk::new(repo, tips)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::oids::{Oids, git2_to_gix_oid};
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
        let sentinel = WalkCommit::new(git2_to_gix_oid(Oid::zero()), NONE, [NONE; 2], 0, None);
        let gix_repo = gix::open(&path).unwrap();
        let mut batcher = Batcher::new(&gix_repo, &HashSet::new(), [third]).unwrap();
        let mut out = vec![sentinel.clone()];

        assert_eq!(batcher.next_into(2, &mut out), 2);
        assert_eq!(batcher.next_into(2, &mut out), 1);
        assert_eq!(batcher.next_into(2, &mut out), 0);

        let oids = out.iter().map(|commit| commit.oid).collect::<Vec<_>>();
        assert_eq!(
            oids,
            gix_oids([third, second, first]).into_iter().fold(vec![git2_to_gix_oid(Oid::zero())], |mut all, oid| {
                all.push(oid);
                all
            })
        );
        assert!(out[3].is_parentless());
    }

    #[test]
    fn merge_commit_keeps_two_parent_aliases_without_spilling_parent_storage() {
        let (path, repo) = temp_repo("merge-parents");
        let base = commit(&repo, "base.txt", "base");
        repo.branch("side", &repo.find_commit(base).unwrap(), false).unwrap();
        let main_ref = head_refname(&repo);
        let main = commit(&repo, "main.txt", "main");
        repo.set_head("refs/heads/side").unwrap();
        repo.checkout_head(None).unwrap();
        let side = commit(&repo, "side.txt", "side");
        repo.set_head(&main_ref).unwrap();
        repo.checkout_head(None).unwrap();
        let main_commit = repo.find_commit(main).unwrap();
        let side_commit = repo.find_commit(side).unwrap();
        let merge = commit_with_parents(&repo, "merge.txt", "merge", &[&main_commit, &side_commit]);
        let gix_repo = gix::open(&path).unwrap();
        let mut oids = Oids::default();
        let mut batcher = Batcher::from_tips_with_oids(&gix_repo, vec![git2_to_gix_oid(merge)], &mut oids).unwrap();
        let mut page = Vec::new();

        assert_eq!(batcher.next_aliased_into(1, &mut page, &mut oids), 1);
        let merge_commit = &page[0];

        assert_eq!(merge_commit.oid, git2_to_gix_oid(merge));
        assert_eq!(merge_commit.first_parent_alias(), oids.get_existing_alias(main).unwrap());
        assert_eq!(merge_commit.second_parent_alias(), oids.get_existing_alias(side).unwrap());
    }

    #[test]
    fn aliased_page_carries_commit_and_parent_aliases() {
        let (path, repo) = temp_repo("aliased-merge-parents");
        let base = commit(&repo, "base.txt", "base");
        repo.branch("side", &repo.find_commit(base).unwrap(), false).unwrap();
        let main_ref = head_refname(&repo);
        let main = commit(&repo, "main.txt", "main");
        repo.set_head("refs/heads/side").unwrap();
        repo.checkout_head(None).unwrap();
        let side = commit(&repo, "side.txt", "side");
        repo.set_head(&main_ref).unwrap();
        repo.checkout_head(None).unwrap();
        let main_commit = repo.find_commit(main).unwrap();
        let side_commit = repo.find_commit(side).unwrap();
        let merge = commit_with_parents(&repo, "merge.txt", "merge", &[&main_commit, &side_commit]);
        let gix_repo = gix::open(&path).unwrap();
        let mut oids = Oids::default();
        let mut batcher = Batcher::from_tips_with_oids(&gix_repo, vec![git2_to_gix_oid(merge)], &mut oids).unwrap();
        let mut page = Vec::new();

        assert_eq!(batcher.next_aliased_into(1, &mut page, &mut oids), 1);
        let merge_commit = &page[0];

        assert_eq!(merge_commit.alias, oids.get_existing_alias(merge).unwrap());
        assert_eq!(merge_commit.first_parent_alias(), oids.get_existing_alias(main).unwrap());
        assert_eq!(merge_commit.second_parent_alias(), oids.get_existing_alias(side).unwrap());
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
    fn bit_words_rounds_up_to_u64_storage() {
        assert_eq!(bit_words(0), 0);
        assert_eq!(bit_words(1), 1);
        assert_eq!(bit_words(64), 1);
        assert_eq!(bit_words(65), 2);
    }

    #[test]
    fn seen_commits_tracks_graph_positions_without_oid_hashing() {
        let mut seen = SeenCommits::default();
        seen.graph_positions.resize(2, 0);

        assert!(seen.insert_graph_pos(gix::commitgraph::Position(64)));
        assert!(!seen.insert_graph_pos(gix::commitgraph::Position(64)));
        assert!(seen.loose_oids.is_empty());
    }

    #[test]
    fn seen_commits_tracks_loose_oids_separately() {
        let oid = git2_to_gix_oid(Oid::zero());
        let mut seen = SeenCommits::default();

        assert!(seen.insert_loose_oid(oid));
        assert!(!seen.insert_loose_oid(oid));
    }
}
