use crate::helpers::branch_visibility::branch_name_from_ref;

pub const HISTORY_OBJECT_CACHE_BYTES: usize = 64 * 1024 * 1024;

pub fn enable_history_object_cache(repo: &mut gix::Repository) {
    repo.object_cache_size_if_unset(HISTORY_OBJECT_CACHE_BYTES);
}

pub fn commit_graph_if_available(repo: &gix::Repository) -> Option<gix::commitgraph::Graph> {
    repo.commit_graph_if_enabled().ok().flatten()
}

pub fn history_commit_count_hint(repo: &gix::Repository) -> Option<usize> {
    commit_graph_if_available(repo).map(|graph| graph.num_commits() as usize)
}

pub fn for_each_branch_tip(repo: &gix::Repository, mut visit: impl FnMut(bool, &str, gix::ObjectId)) -> Result<(), git2::Error> {
    let references = repo.references().map_err(gix_error)?;
    for (is_local, references) in [(true, references.local_branches()), (false, references.remote_branches())] {
        for reference in references.map_err(gix_error)?.flatten() {
            let Some(name) = branch_name_from_ref(reference.name().as_bstr()) else {
                continue;
            };
            let Some(oid) = reference.try_id().map(|id| id.detach()) else {
                continue;
            };
            visit(is_local, name, oid);
        }
    }
    Ok(())
}

pub(crate) fn gix_error(error: impl std::fmt::Display) -> git2::Error {
    git2::Error::from_str(&error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_support::{commit_file, init_repo_at};
    use std::process::Command;

    #[test]
    fn commit_graph_helper_uses_written_commit_graph() {
        let temp = tempfile::Builder::new().prefix("guitar-gix-commit-graph-").tempdir().unwrap();
        let repo = init_repo_at(temp.path());
        commit_file(&repo, "one.txt", "one\n", "one");
        commit_file(&repo, "two.txt", "two\n", "two");

        let status = Command::new("git").arg("-C").arg(temp.path()).args(["commit-graph", "write", "--reachable"]).status().unwrap();
        assert!(status.success());

        let gix_repo = gix::open(temp.path()).unwrap();
        assert!(commit_graph_if_available(&gix_repo).is_some());
    }
}
