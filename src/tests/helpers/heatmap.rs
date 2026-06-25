use super::*;
use crate::core::oids::git2_to_gix_oid as gix_oid;
use git2::{IndexAddOption, Oid, Repository, Signature, Time};
use std::fs;

fn commit_at(repo: &Repository, name: &str, seconds: i64) -> Oid {
    fs::write(repo.workdir().unwrap().join(name), name).unwrap();

    let mut index = repo.index().unwrap();
    index.add_all(["."], IndexAddOption::DEFAULT, None).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = Signature::new("Test User", "test@example.com", &Time::new(seconds, 0)).unwrap();
    let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();

    repo.commit(Some("HEAD"), &sig, &sig, name, &tree, &parents).unwrap()
}

struct HeatmapFixture {
    dir: tempfile::TempDir,
    today: i64,
    yesterday: i64,
    outside_grid: i64,
    first_today: Oid,
    second_today: Oid,
    yesterday_oid: Oid,
    old: Oid,
}

fn heatmap_fixture(name: &str) -> HeatmapFixture {
    let dir = tempfile::Builder::new().prefix(&format!("guitar-heatmap-{name}-")).tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let today = Utc::now().timestamp();
    let yesterday = today - 24 * 60 * 60;
    let outside_grid = today - ((TOTAL_DAYS as i64) + 10) * 24 * 60 * 60;

    HeatmapFixture {
        first_today: commit_at(&repo, "first-today.txt", today),
        second_today: commit_at(&repo, "second-today.txt", today),
        yesterday_oid: commit_at(&repo, "yesterday.txt", yesterday),
        old: commit_at(&repo, "old.txt", outside_grid),
        dir,
        today,
        yesterday,
        outside_grid,
    }
}

#[test]
fn repo_heatmap_counts_recent_commits_and_stops_at_old_boundary() {
    let fixture = heatmap_fixture("repo");
    let gix_repo = gix::open(fixture.dir.path()).unwrap();
    let weekday_today = Utc::now().weekday().num_days_from_monday() as usize;
    let counts = commits_per_day(&gix_repo, [fixture.first_today, fixture.second_today, fixture.old].map(gix_oid));
    let stopped = commits_per_day(&gix_repo, [fixture.first_today, fixture.old, fixture.second_today].map(gix_oid));
    let grid = build_heatmap(&gix_repo, [gix_oid(fixture.first_today)]);

    assert_eq!(counts[0], 2);
    assert_eq!(counts.iter().sum::<usize>(), 2);
    assert_eq!(stopped[0], 1);
    assert_eq!(stopped.iter().sum::<usize>(), 1);
    assert_eq!(grid[weekday_today][WEEKS - 1], 1);
}

#[test]
fn streamed_heatmap_matches_scanned_counts_and_filters_out_of_grid_dates() {
    let fixture = heatmap_fixture("streamed-counts");
    let gix_repo = gix::open(fixture.dir.path()).unwrap();
    let scanned = build_heatmap(&gix_repo, [fixture.first_today, fixture.yesterday_oid].map(gix_oid));
    let mut streamed = HeatmapCounts::default();
    streamed.add_commit_seconds(fixture.today);
    streamed.add_commit_seconds(fixture.yesterday);

    let mut filtered = HeatmapCounts::default();
    filtered.add_commit_seconds(fixture.today);
    filtered.add_commit_seconds(fixture.outside_grid);
    filtered.add_commit_seconds(fixture.today + 24 * 60 * 60);

    assert_eq!(streamed.build(), scanned);
    assert_eq!(filtered.build().iter().flatten().sum::<usize>(), 1);
}
