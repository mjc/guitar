use git2::{BranchType, Oid, Repository, Revwalk};
use im::HashSet;
use std::cell::RefCell;
use std::collections::HashSet as StdHashSet;
use std::rc::Rc;

// Own the revwalk cursor so commit history can be loaded in pages.
pub struct Batcher {
    revwalk: Revwalk<'static>,
}

impl Batcher {
    // Build the initial revwalk from all visible local and remote branch tips.
    pub fn new(repo: Rc<RefCell<Repository>>, hidden_branch_names: &HashSet<String>, extra_roots: &[Oid]) -> Result<Self, git2::Error> {
        let revwalk = Self::build(&repo.borrow(), hidden_branch_names, extra_roots)?;
        Ok(Self { revwalk })
    }

    // Recreate the cursor after branch filters, fetches, or repository state changes.
    pub fn reset(&mut self, repo: Rc<RefCell<Repository>>, hidden_branch_names: &HashSet<String>, extra_roots: &[Oid]) -> Result<(), git2::Error> {
        self.revwalk = Self::build(&repo.borrow(), hidden_branch_names, extra_roots)?;
        Ok(())
    }

    // Pull the next page, dropping commits libgit2 cannot resolve.
    pub fn next(&mut self, count: usize) -> Vec<Oid> {
        self.revwalk.by_ref().take(count).filter_map(Result::ok).collect()
    }

    fn build(repo: &Repository, hidden_branch_names: &HashSet<String>, extra_roots: &[Oid]) -> Result<Revwalk<'static>, git2::Error> {
        // The repository outlives the revwalk in App state; this keeps libgit2's lifetime usable here.
        let repo_ref: &'static Repository = unsafe { std::mem::transmute::<&Repository, &'static Repository>(repo) };

        let mut revwalk = repo_ref.revwalk()?;
        let mut pushed = StdHashSet::new();

        for branch_type in [BranchType::Local, BranchType::Remote] {
            for branch_result in repo.branches(Some(branch_type))? {
                let (branch, _) = branch_result?;

                let Some(oid) = branch.get().target() else { continue };

                let name = branch.name()?.unwrap_or("").to_string();

                // Hidden branch names are a deny-list; new branches are visible by default.
                if !hidden_branch_names.contains(&name) {
                    revwalk.push(oid)?;
                    pushed.insert(oid);
                }
            }
        }

        for oid in extra_roots {
            if pushed.insert(*oid) {
                revwalk.push(*oid)?;
            }
        }

        revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
        Ok(revwalk)
    }
}
