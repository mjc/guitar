use crate::{
    git::actions::gix_support::{open_repo_path, to_git2_error},
    git::auth::{AuthAttempt, AuthSession, NetworkResult, network_result},
    helpers::localisation::network,
};
use std::{sync::atomic::AtomicBool, thread};

const TAG_FETCH_REFSPEC: &str = "refs/tags/*:refs/tags/*";

fn heads_fetch_refspec(remote_name: &str) -> String {
    format!("+refs/heads/*:refs/remotes/{remote_name}/*")
}

fn remote_url(remote: &gix::Remote<'_>) -> Result<gix::Url, git2::Error> {
    remote.url(gix::remote::Direction::Fetch).ok_or_else(|| git2::Error::from_str("Remote URL is missing")).cloned()
}

fn configure_fetch_remote<'repo>(repo: &'repo gix::Repository, remote_name: &str) -> Result<gix::Remote<'repo>, git2::Error> {
    let mut remote = repo.find_remote(remote_name).map_err(to_git2_error)?;
    remote = remote.with_fetch_tags(gix::remote::fetch::Tags::All);
    let heads_refspec = heads_fetch_refspec(remote_name);
    remote = remote.with_refspecs(Some(heads_refspec.as_str()), gix::remote::Direction::Fetch).map_err(to_git2_error)?;
    remote = remote.with_refspecs(Some(TAG_FETCH_REFSPEC), gix::remote::Direction::Fetch).map_err(to_git2_error)?;
    Ok(remote)
}

fn fetch_remote_inner(repo_path: &str, remote_name: &str, attempt: AuthAttempt) -> Result<(), git2::Error> {
    let mut repo = open_repo_path(repo_path)?;
    repo.committer_or_set_generic_fallback().map_err(to_git2_error)?;

    let remote = configure_fetch_remote(&repo, remote_name)?;
    let remote_url = remote_url(&remote)?;
    let mut connection = remote.connect(gix::remote::Direction::Fetch).map_err(to_git2_error)?;
    let mut configured_credentials = connection.configured_credentials(remote_url).map_err(to_git2_error)?;
    connection.set_credentials(move |action| attempt.gix_credentials_with(action, &mut configured_credentials));

    let mut progress = gix::progress::Discard;
    let pending_pack = connection.prepare_fetch(&mut progress, Default::default()).map_err(to_git2_error)?;
    let should_interrupt = AtomicBool::new(false);
    pending_pack.receive(&mut progress, &should_interrupt).map_err(to_git2_error).map(|_| ())
}

// Run fetch on a worker thread so auth prompts and network latency stay outside the draw loop.
pub fn fetch_remote(repo_path: &str, remote_name: &str, auth_session: AuthSession) -> thread::JoinHandle<NetworkResult> {
    // Own the inputs before crossing the worker boundary.
    let repo_path = repo_path.to_string();
    let remote_name = remote_name.to_string();

    thread::spawn(move || {
        let attempt = AuthAttempt::new(auth_session, network::FETCH());
        let result = fetch_remote_inner(&repo_path, remote_name.as_str(), attempt.clone());

        network_result(network::FETCH(), &attempt, result)
    })
}

#[cfg(test)]
#[path = "../../tests/git/actions/fetching.rs"]
mod tests;
