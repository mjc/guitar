use super::*;
use crate::core::graph_service::{GraphCommand, GraphEvent, GraphFileHistoryRow, GraphLookupKind, GraphLookupResult, GraphRow};
use crate::git::queries::helpers::{FileStatus, UncommittedChanges};
use crate::git::test_support::{TestDir, commit_file, parent_with_submodule, stage_path, temp_repo, write_workdir_file};
use git2::Repository;
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use std::{
    rc::Rc,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn app_with_repo(repo: Rc<Repository>) -> App {
    App { repo: Some(crate::app::app::RepoHandle::from_repo(repo)), viewport: Viewport::Graph, focus: Focus::Viewport, ..Default::default() }
}

fn wait_until(app: &mut App, repo: &Rc<Repository>, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !done(app) && Instant::now() < deadline {
        app.sync(repo);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn graph_row(index: usize, alias: u32, oid: git2::Oid) -> GraphRow {
    GraphRow {
        index,
        alias,
        oid,
        summary: "commit".to_string(),
        committer_date: String::new(),
        committer_name: String::new(),
        is_merge: false,
        has_any_branch: false,
        branches: Default::default(),
        tags: Default::default(),
        is_stash: false,
        stash_lane: None,
        worktrees: Default::default(),
        has_current_worktree: false,
        reflog: None,
    }
}

fn history_row(index: usize, oid: git2::Oid) -> GraphFileHistoryRow {
    GraphFileHistoryRow { graph_index: index, oid, summary: "history".to_string(), status: FileStatus::Modified }
}

fn stop_graph_service(app: &mut App) {
    if let Some(tx) = app.graph_tx.take() {
        let _ = tx.send(GraphCommand::Shutdown);
    }
    if let Some(cancel) = app.walker_cancel.take() {
        cancel.store(true, Ordering::SeqCst);
    }
    if let Some(handle) = app.walker_handle.take() {
        let _ = handle.join();
    }
}

#[test]
fn splash_draws_recent_repository_actions() {
    let backend = TestBackend::new(140, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App { viewport: Viewport::Splash, focus: Focus::Viewport, recent: vec!["/repo/a".into(), "/repo/b".into()], ..Default::default() };
    app.layout.app = Rect::new(0, 0, 140, 24);
    app.layout.graph = Rect::new(0, 0, 140, 24);

    terminal.draw(|frame| app.draw_splash(frame)).unwrap();

    let rendered = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(rendered.contains("recent repositories:"));
    assert!(rendered.contains("actions: remove (d) | move up (Shift + K) | move down (Shift + J)"));
    assert!(rendered.contains("/repo/a"));
    assert!(rendered.contains("/repo/b"));
}

#[test]
fn reload_captures_selected_commit_oid_and_visual_offset_for_restore() {
    let (dir, repo) = temp_repo("restore-capture");
    let oid = commit_file(&repo, "selected.txt", "selected", "selected");
    let path_string = dir.join("repo").display().to_string();
    let repo = Rc::new(repo);
    let mut app = app_with_repo(repo.clone());
    app.path = Some(path_string.clone());
    app.recent = vec![path_string];
    app.graph_selected = 4;
    app.graph_scroll.set(2);
    app.graph.graph_window = Some(GraphWindowCache { version: 1, start: 4, end: 5, head_alias: 9, rows: vec![graph_row(4, 9, oid)], history: Default::default() });

    app.reload(None);

    assert_eq!(app.graph.pending_selection_restore, Some(GraphSelectionRestore { oid, selected_offset: 2 }));
    stop_graph_service(&mut app);
}

#[test]
fn pending_restore_requests_oid_lookup_on_progress() {
    let (_dir, repo) = temp_repo("restore-progress");
    let oid = commit_file(&repo, "selected.txt", "selected", "selected");
    let repo = Rc::new(repo);
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let mut app = app_with_repo(repo.clone());
    app.graph_tx = Some(cmd_tx);
    app.graph_rx = Some(event_rx);
    app.graph.generation = 7;
    app.graph.pending_selection_restore = Some(GraphSelectionRestore { oid, selected_offset: 2 });

    event_tx.send(GraphEvent::Progress { generation: 7, version: 1, total: 2, is_first: false, is_complete: false }).unwrap();
    app.sync(&repo);

    match cmd_rx.try_recv().unwrap() {
        GraphCommand::Lookup { generation, request_id, kind: GraphLookupKind::Oid { oid: actual_oid } } => {
            assert_eq!(generation, 7);
            assert_eq!(request_id, 1);
            assert_eq!(actual_oid, oid);
        },
        other => panic!("expected oid restore lookup, got {other:?}"),
    }

    let (pending_id, pending_action) = app.graph.pending_lookup.unwrap();
    assert_eq!(pending_id, 1);
    assert!(matches!(pending_action, PendingGraphLookup::RestoreSelection));
}

#[test]
fn first_graph_progress_with_dirty_submodule_status_stays_in_graph_view() {
    let dir = TestDir::new("dirty-submodule-progress");
    let (parent, _child_path) = parent_with_submodule(&dir);
    write_workdir_file(&parent, "deps/child/file.txt", "dirty\n");
    let repo = Rc::new(parent);
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let mut app = app_with_repo(repo.clone());
    app.viewport = Viewport::Splash;
    app.graph_rx = Some(event_rx);
    app.graph.generation = 9;

    event_tx.send(GraphEvent::Progress { generation: 9, version: 1, total: 1, is_first: true, is_complete: false }).unwrap();
    app.sync(&repo);

    assert_eq!(app.viewport, Viewport::Graph);
    assert_eq!(app.focus, Focus::Viewport);
    assert!(!app.is_uncommitted_loaded);

    let clean = UncommittedChanges { is_clean: true, ..Default::default() };
    event_tx.send(GraphEvent::Uncommitted { generation: 9, result: Ok(clean) }).unwrap();
    app.sync(&repo);

    assert!(app.is_uncommitted_loaded);
    assert!(app.uncommitted.is_clean);
}

#[test]
fn uncommitted_metadata_waits_for_complete_progress_then_selection_loads_details() {
    let (dir, repo) = temp_repo("deferred-uncommitted");
    commit_file(&repo, "tracked.txt", "tracked", "tracked");
    write_workdir_file(&repo, "staged.txt", "staged\n");
    stage_path(&repo, "staged.txt");
    write_workdir_file(&repo, "new.txt", "new\n");
    let repo = Rc::new(repo);
    let (cmd_tx, _cmd_rx) = std::sync::mpsc::channel();
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let mut app = app_with_repo(repo.clone());
    app.path = Some(dir.join("repo").display().to_string());
    app.graph_tx = Some(cmd_tx);
    app.graph_event_tx = Some(event_tx.clone());
    app.graph_rx = Some(event_rx);
    app.graph.generation = 11;

    event_tx.send(GraphEvent::Progress { generation: 11, version: 1, total: 1, is_first: false, is_complete: false }).unwrap();
    app.sync(&repo);
    assert!(!app.is_uncommitted_loaded);

    event_tx.send(GraphEvent::Progress { generation: 11, version: 2, total: 1, is_first: false, is_complete: true }).unwrap();
    wait_until(&mut app, &repo, |app| app.is_uncommitted_loaded);

    assert!(app.is_uncommitted_loaded);
    assert!(!app.is_uncommitted_detail_loaded);
    assert_eq!(app.uncommitted.staged.added, vec!["staged.txt".to_string()]);
    assert!(app.uncommitted.unstaged.added.is_empty());
    app.graph.total = 2;

    app.select_graph_index(0);
    assert!(app.is_uncommitted_detail_loading);
    wait_until(&mut app, &repo, |app| app.is_uncommitted_detail_loaded);

    assert!(app.is_uncommitted_detail_loaded);
    assert!(!app.is_uncommitted_detail_loading);
    assert_eq!(app.uncommitted.unstaged.added, vec!["new.txt".to_string()]);
}

fn assert_restore_lookup_case(
    name: &str, initial_selected: usize, graph_total: usize, graph_is_complete: bool, selected_offset: usize, lookup_result: GraphLookupResult, expected_selected: usize, expected_scroll: usize,
) {
    let (_dir, repo) = temp_repo(name);
    let oid = commit_file(&repo, "selected.txt", "selected", "selected");
    let repo = Rc::new(repo);
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let mut app = app_with_repo(repo.clone());
    app.graph_rx = Some(event_rx);
    app.graph_selected = initial_selected;
    app.graph.generation = 7;
    app.graph.total = graph_total;
    app.graph.is_complete = graph_is_complete;
    app.graph.pending_selection_restore = Some(GraphSelectionRestore { oid, selected_offset });
    app.graph.pending_lookup = Some((3, PendingGraphLookup::RestoreSelection));

    event_tx.send(GraphEvent::LookupResult { generation: 7, request_id: 3, result: lookup_result }).unwrap();
    app.sync(&repo);

    assert_eq!(app.graph_selected, expected_selected, "{name}");
    assert_eq!(app.graph_scroll.get(), expected_scroll, "{name}");
    assert_eq!(app.graph.pending_selection_restore, None, "{name}");
}

#[test]
fn restore_lookup_cases_cover_selection_and_scroll() {
    assert_restore_lookup_case("restore-success", 1, 10, false, 2, GraphLookupResult::Index(Some(4)), 4, 2);
    assert_restore_lookup_case("restore-top", 6, 10, false, 4, GraphLookupResult::Index(Some(1)), 1, 0);
    assert_restore_lookup_case("restore-missing", 2, 6, true, 2, GraphLookupResult::Index(None), 2, 0);
}

#[test]
fn explicit_graph_navigation_clears_pending_restore() {
    let mut app = App { viewport: Viewport::Graph, focus: Focus::Viewport, graph_selected: 1, ..Default::default() };
    app.graph.total = 5;
    app.graph.pending_selection_restore = Some(GraphSelectionRestore { oid: git2::Oid::zero(), selected_offset: 0 });

    app.on_scroll_down();

    assert_eq!(app.graph_selected, 2);
    assert_eq!(app.graph.pending_selection_restore, None);
}

#[test]
fn file_history_event_updates_only_matching_request() {
    let (_dir, repo) = temp_repo("file-history-event");
    let oid = commit_file(&repo, "target.txt", "target", "target");
    let repo = Rc::new(repo);
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let mut app = app_with_repo(repo.clone());
    app.graph_rx = Some(event_rx);
    app.focus = Focus::Search;
    app.search_path = Some("target.txt".to_string());
    app.search_request_id = Some(3);
    app.search_is_loading = true;
    app.graph.generation = 7;

    event_tx.send(GraphEvent::FileHistory { generation: 7, request_id: 2, path: "target.txt".to_string(), rows: vec![history_row(1, oid)], error: None }).unwrap();
    app.sync(&repo);

    assert!(app.search_is_loading);
    assert!(app.search_rows.is_empty());
    assert_eq!(app.search_request_id, Some(3));

    event_tx.send(GraphEvent::FileHistory { generation: 7, request_id: 3, path: "target.txt".to_string(), rows: vec![history_row(1, oid)], error: None }).unwrap();
    app.sync(&repo);

    assert!(!app.search_is_loading);
    assert_eq!(app.search_request_id, None);
    assert_eq!(app.search_rows.len(), 1);
    assert_eq!(app.search_rows[0].graph_index, 1);
}
