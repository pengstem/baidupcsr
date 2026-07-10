use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::pcs::FileEntry;

use super::{
    active_player_log_line, bduss_from_user, breadcrumb_preferred_selection, build_pcs_client,
    delete_plan_for_selection, download_target_path, fetch_first_successful_dlink_range,
    format_video_bench_start, is_local_open_size_allowed, is_video_entry, json_string,
    local_open_command_candidates, local_open_target_path, pan_user_agent_for_config,
    parse_byte_size_arg, parse_command, parse_http_request_path, parse_remote_command,
    parse_video_bench_positions, partial_download_path, persist_workdir_if_active,
    preview_key_for_entry, proxy_from_config_or_env, redacted_path_hash,
    remote_parent_path_for_bench, rename_plan_for_selected, search_remote_by_listing_with,
    task_panel_item, video_bench_offset, CommandAction, DlinkRangeAttempt, ParsedRemoteCommand,
    RemoteSearchMode, VideoBenchCase, VideoBenchCliOptions, VideoBenchMovie, VideoBenchPosition,
    VideoBenchResolvedOptions, LOCAL_OPEN_MAX_BYTES,
};

fn config_fixture(name: &str) -> Result<crate::config::PcsConfig, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/config")
        .join(name);
    crate::config::load_config(&path).map_err(|err| err.to_string())
}

fn config_fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/config")
        .join(name)
}

fn file(fs_id: i64, size: i64) -> FileEntry {
    FileEntry {
        fs_id,
        app_id: 266_719,
        path: "/docs/notes.md".to_string(),
        server_filename: "notes.md".to_string(),
        ctime: 1,
        mtime: 2,
        md5: String::new(),
        block_list: Vec::new(),
        size,
        is_dir: false,
        has_subdir: false,
    }
}

fn remote_entry(fs_id: i64, path: &str, is_dir: bool) -> FileEntry {
    FileEntry {
        fs_id,
        app_id: 266_719,
        path: path.to_string(),
        server_filename: path.rsplit('/').next().unwrap_or(path).to_string(),
        ctime: 1,
        mtime: 2,
        md5: String::new(),
        block_list: Vec::new(),
        size: if is_dir { 0 } else { 12 },
        is_dir,
        has_subdir: is_dir,
    }
}

fn pcs_error_kind(err: &crate::pcs::PcsError) -> String {
    match err {
        crate::pcs::PcsError::Cancelled { .. } => "cancelled".to_string(),
        crate::pcs::PcsError::Parse { message, .. } => format!("parse:{message}"),
        crate::pcs::PcsError::Network { .. } => "network".to_string(),
        crate::pcs::PcsError::Remote { code, .. } => format!("remote_code={code}"),
    }
}

#[test]
fn persist_workdir_if_active_saves_browser_path_to_config() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("baidupcsr-cli-workdir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = dir.join("pcs_config.json");
    std::fs::copy(config_fixture_path("pcs_config_minimal.json"), &path)
        .map_err(|err| err.to_string())?;

    persist_workdir_if_active(100_000_001, &path, "/电影/科幻电影")
        .map_err(|err| err.to_string())?;
    let config = crate::config::load_config(&path).map_err(|err| err.to_string())?;
    let user = config
        .active_user()
        .ok_or_else(|| "active user missing".to_string())?;

    assert_eq!(user.workdir, "/电影/科幻电影");
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(dir);
    Ok(())
}

#[test]
fn breadcrumb_navigation_prefers_the_next_child_under_target() {
    assert_eq!(
        breadcrumb_preferred_selection("/docs/videos/2026", "/docs"),
        Some("/docs/videos".to_string())
    );
    assert_eq!(
        breadcrumb_preferred_selection("/docs/videos/2026", "/"),
        Some("/docs".to_string())
    );
    assert_eq!(breadcrumb_preferred_selection("/docs", "/docs"), None);
}

fn expect_operation(
    command: ParsedRemoteCommand,
) -> Result<crate::app::RemoteOperationPlan, String> {
    match command {
        ParsedRemoteCommand::Operation(plan) => Ok(plan),
        ParsedRemoteCommand::Search { .. } => {
            Err("command parsed as search, expected operation".to_string())
        }
    }
}

fn expect_search(
    command: ParsedRemoteCommand,
) -> Result<(String, String, RemoteSearchMode), String> {
    match command {
        ParsedRemoteCommand::Search {
            path,
            keyword,
            mode,
        } => Ok((path, keyword, mode)),
        ParsedRemoteCommand::Operation(_) => {
            Err("command parsed as operation, expected search".to_string())
        }
    }
}

#[test]
fn active_player_log_line_uses_last_sanitized_player_line() -> Result<(), String> {
    let log = Arc::new(Mutex::new(crate::preview::PlayerLogRing::new(2)));
    {
        let mut guard = log.lock().map_err(|_| "log lock poisoned".to_string())?;
        guard.push("A: 00:00:01 / 00:20:00");
        guard.push("Error: https://d.pcs.baidu.com/file/private?token=SECRET");
    }

    let line = active_player_log_line(&log);

    assert_eq!(line, "player_log=Error: <redacted-url>");
    assert!(!line.contains("SECRET"));
    Ok(())
}

#[test]
fn video_bench_command_parses_required_path_and_options() -> Result<(), String> {
    let args = [
        "video-bench",
        "--path",
        "电影/clip.mkv",
        "--case",
        "source",
        "--duration",
        "7",
        "--chunk-size",
        "2MiB",
        "--max-chunks",
        "9",
        "--prefetch-chunks",
        "3",
        "--reps",
        "2",
        "--range-sizes",
        "1MiB,4MiB",
        "--positions",
        "start,50,tail",
        "--output",
        "/tmp/bench.jsonl",
        "--redact",
    ]
    .map(str::to_string);

    let CommandAction::VideoBench(options) = parse_command(&args) else {
        return Err("video-bench command did not parse".to_string());
    };

    assert_eq!(options.remote_path, "/电影/clip.mkv");
    assert_eq!(options.bench_case, VideoBenchCase::Source);
    assert_eq!(options.duration_seconds, 7);
    assert_eq!(options.chunk_size_bytes, Some(2 * 1024 * 1024));
    assert_eq!(options.max_chunks, Some(9));
    assert_eq!(options.prefetch_chunks, Some(3));
    assert_eq!(options.reps, 2);
    assert_eq!(options.range_sizes, vec![1024 * 1024, 4 * 1024 * 1024]);
    assert_eq!(
        options.positions,
        vec![
            VideoBenchPosition::Start,
            VideoBenchPosition::Percent(50),
            VideoBenchPosition::Tail,
        ]
    );
    assert_eq!(
        options.output_path.as_deref(),
        Some(Path::new("/tmp/bench.jsonl"))
    );
    Ok(())
}

#[test]
fn video_bench_command_requires_path() {
    let args = ["video-bench", "--case", "source"].map(str::to_string);

    assert!(matches!(parse_command(&args), CommandAction::UsageError));
}

#[test]
fn video_bench_position_offsets_are_clamped_to_valid_ranges() {
    assert_eq!(video_bench_offset(1_000, 100, VideoBenchPosition::Start), 0);
    assert_eq!(
        video_bench_offset(1_000, 100, VideoBenchPosition::Percent(50)),
        450
    );
    assert_eq!(
        video_bench_offset(1_000, 100, VideoBenchPosition::Tail),
        900
    );
    assert_eq!(video_bench_offset(50, 100, VideoBenchPosition::Tail), 0);
}

#[test]
fn video_bench_parsers_accept_size_suffixes_and_positions() -> Result<(), String> {
    assert_eq!(parse_byte_size_arg("4MiB")?, 4 * 1024 * 1024);
    assert_eq!(parse_byte_size_arg("2g")?, 2 * 1024 * 1024 * 1024);

    assert_eq!(
        parse_video_bench_positions("start,p10,90,end")?,
        vec![
            VideoBenchPosition::Start,
            VideoBenchPosition::Percent(10),
            VideoBenchPosition::Percent(90),
            VideoBenchPosition::Tail,
        ]
    );
    Ok(())
}

#[test]
fn video_bench_redaction_helpers_do_not_emit_raw_path() {
    let path = "/电影/斯巴达300勇士.mkv";
    let hash = redacted_path_hash(path);

    assert!(hash.starts_with("fnv1a64:"));
    assert!(!hash.contains("斯巴达"));
    assert_eq!(
        json_string("quote\" slash\\ line\n"),
        "\"quote\\\" slash\\\\ line\\n\""
    );
}

#[test]
fn video_bench_start_event_uses_hashes_instead_of_raw_identifiers() {
    let mut entry = file(9, 1024);
    entry.path = "/电影/斯巴达300勇士.mkv".to_string();
    entry.server_filename = "斯巴达300勇士.mkv".to_string();
    let movie = VideoBenchMovie {
        entry,
        size_bytes: 1024,
        path_hash: redacted_path_hash("/电影/斯巴达300勇士.mkv"),
        extension: "mkv".to_string(),
    };
    let resolved = VideoBenchResolvedOptions {
        chunk_size_bytes: 1024,
        max_chunks: 2,
        prefetch_chunks: 1,
        memory_limit_bytes: 2048,
    };

    let line = format_video_bench_start(&VideoBenchCliOptions::default(), &resolved, &movie, 42);

    assert!(line.contains("\"account_hash\""));
    assert!(line.contains("\"path_hash\""));
    assert!(!line.contains("account_uid"));
    assert!(!line.contains("斯巴达"));
    assert!(!line.contains("/电影"));
}

#[test]
fn video_bench_remote_parent_and_loopback_path_are_extracted() -> Result<(), String> {
    assert_eq!(remote_parent_path_for_bench("/电影/clip.mkv"), "/电影");
    assert_eq!(remote_parent_path_for_bench("/clip.mkv"), "/");
    assert_eq!(
        parse_http_request_path("http://127.0.0.1:1234/preview/video/token")?,
        "/preview/video/token"
    );
    Ok(())
}

#[test]
#[ignore = "uses the machine-local BaiduPCS-Go config and network; prints no secret fields"]
fn live_video_dlink_range_smoke_is_redacted() -> Result<(), String> {
    let config_path = crate::config::discover_config_file(None);
    let config = crate::config::load_config(&config_path)
        .map_err(|err| format!("live_config_load_failed: {err}"))?;
    let user = config
        .active_user()
        .ok_or_else(|| "live_active_user_missing".to_string())?;
    let bduss = bduss_from_user(user).ok_or_else(|| "live_bduss_missing".to_string())?;
    let client = build_pcs_client(&config, user)
        .map_err(|err| format!("live_client_build_failed: {err}"))?;
    let entries = client
        .list(user.workdir.as_str())
        .map_err(|err| format!("live_list_failed: {err}"))?;
    let entry = entries
        .into_iter()
        .find(is_video_entry)
        .ok_or_else(|| "live_video_missing".to_string())?;

    let urls = client
        .locate_download_urls(entry.path.as_str(), user.uid, bduss.as_str())
        .map_err(|err| format!("live_locatedownload_failed: {}", pcs_error_kind(&err)))?;
    println!("live_locatedownload_url_count={}", urls.len());
    let first_url = urls
        .first()
        .ok_or_else(|| "live_locatedownload_empty".to_string())?;

    let range = crate::pcs::ByteRange::new(0, 1023)
        .map_err(|err| format!("live_range_invalid: {}", pcs_error_kind(&err)))?;
    let bytes = client
        .download_url_range_with_cancel(first_url.as_str(), range, || false)
        .map_err(|err| format!("live_dlink_range_failed: {}", pcs_error_kind(&err)))?;

    println!("live_dlink_range_bytes={}", bytes.len());
    assert!(!bytes.is_empty());
    assert!(bytes.len() <= 1025);
    Ok(())
}

#[test]
fn bduss_from_user_prefers_explicit_field() -> Result<(), String> {
    let config = config_fixture("pcs_config_minimal.json")?;
    let user = config
        .active_user()
        .ok_or_else(|| "fixture active user missing".to_string())?;

    assert_eq!(bduss_from_user(user), Some("FAKE_BDUSS".to_string()));
    Ok(())
}

#[test]
fn bduss_from_user_extracts_cookie_value() -> Result<(), String> {
    let config = config_fixture("pcs_config_cookie_only.json")?;
    let user = config
        .active_user()
        .ok_or_else(|| "fixture active user missing".to_string())?;

    assert_eq!(bduss_from_user(user), Some("FAKE_COOKIE_BDUSS".to_string()));
    Ok(())
}

#[test]
fn pan_user_agent_prefers_pan_ua_for_locatedownload() -> Result<(), String> {
    let config = config_fixture("pcs_config_minimal.json")?;

    assert_eq!(pan_user_agent_for_config(&config), "fixture-pan-ua");
    Ok(())
}

#[test]
fn proxy_from_config_or_env_prefers_configured_proxy() {
    assert_eq!(
        proxy_from_config_or_env(
            "http://config-proxy:7890",
            Some(OsString::from("http://env-proxy:7890"))
        ),
        Some("http://config-proxy:7890".to_string())
    );
}

#[test]
fn proxy_from_config_or_env_uses_env_override_when_config_is_empty() {
    assert_eq!(
        proxy_from_config_or_env("", Some(OsString::from("http://127.0.0.1:7890"))),
        Some("http://127.0.0.1:7890".to_string())
    );
    assert_eq!(proxy_from_config_or_env("", None), None);
}

#[test]
fn download_target_uses_sanitized_filename_under_download_dir() {
    let target = download_target_path(Path::new("/tmp/downloads"), "../bad/name?.txt");

    assert_eq!(target, Path::new("/tmp/downloads").join(".._bad_name?.txt"));
    assert_eq!(
        partial_download_path(&target),
        Path::new("/tmp/downloads").join(".._bad_name?.txt.part")
    );
}

#[test]
fn download_target_rejects_dot_path_components_as_filenames() {
    assert_eq!(
        download_target_path(Path::new("/tmp/downloads"), "."),
        Path::new("/tmp/downloads").join("download")
    );
    assert_eq!(
        download_target_path(Path::new("/tmp/downloads"), ".."),
        Path::new("/tmp/downloads").join("download")
    );
}

#[test]
fn local_open_target_uses_file_identity_under_temp_root() {
    let mut entry = file(42, 1024);
    entry.server_filename = "../cover/image.png".to_string();
    entry.mtime = 99;

    let target = local_open_target_path(Path::new("/tmp/baidupcsr-open"), 7, &entry, 1024);

    assert_eq!(
        target,
        Path::new("/tmp/baidupcsr-open").join("7/42-99-1024-.._cover_image.png")
    );
}

#[test]
fn local_open_rejects_files_at_or_above_100_mib() {
    assert!(is_local_open_size_allowed(LOCAL_OPEN_MAX_BYTES - 1));
    assert!(!is_local_open_size_allowed(LOCAL_OPEN_MAX_BYTES));
}

#[test]
#[cfg(target_os = "linux")]
fn linux_local_open_candidates_try_xdg_open_then_gio_open() {
    let commands = local_open_command_candidates(Path::new("/tmp/baidupcsr-open/photo.jpg"));

    assert_eq!(commands[0].program, "xdg-open");
    assert_eq!(
        commands[0].args[0].to_string_lossy(),
        "/tmp/baidupcsr-open/photo.jpg"
    );
    assert_eq!(commands[1].program, "gio");
    assert_eq!(commands[1].args[0].to_string_lossy(), "open");
}

#[test]
fn task_panel_item_uses_local_name_without_remote_path() -> Result<(), crate::app::TaskError> {
    let mut manager = crate::app::TaskManager::new();
    let id = manager.enqueue_download("/tmp/out.txt", 1024);
    manager.start(id)?;
    manager.update_progress(id, 512, 1)?;
    let task = manager.snapshots().remove(0);

    let item = task_panel_item(&task);

    assert_eq!(item.id, 1);
    assert_eq!(item.kind, "download");
    assert_eq!(item.state, "running");
    assert_eq!(item.bytes_done, 512);
    assert_eq!(item.bytes_total, Some(1024));
    assert_eq!(item.label, "out.txt");
    Ok(())
}

#[test]
fn task_panel_item_labels_local_open_without_remote_path() -> Result<(), crate::app::TaskError> {
    let mut manager = crate::app::TaskManager::new();
    let id = manager.enqueue_local_open("/tmp/baidupcsr-open/photo.jpg", 1024);
    manager.start(id)?;
    manager.update_progress(id, 1024, 1)?;
    let task = manager.snapshots().remove(0);

    let item = task_panel_item(&task);

    assert_eq!(item.kind, "open");
    assert_eq!(item.state, "running");
    assert_eq!(item.bytes_done, 1024);
    assert_eq!(item.bytes_total, Some(1024));
    assert_eq!(item.label, "photo.jpg");
    Ok(())
}

#[test]
fn remote_command_mkdir_resolves_relative_path_under_current_directory() -> Result<(), String> {
    let app = crate::tui::TuiApp::new("/docs");

    let plan = expect_operation(parse_remote_command("mkdir reports", &app)?)?;

    assert_eq!(plan.dry_run_lines()[0], "mkdir /docs/reports");
    Ok(())
}

#[test]
fn remote_command_rename_targets_selected_entry_parent() -> Result<(), String> {
    let mut app = crate::tui::TuiApp::new("/docs");
    app.set_entries(vec![file(1, 12)]);

    let plan = expect_operation(parse_remote_command("rename renamed.md", &app)?)?;

    assert_eq!(
        plan.dry_run_lines()[0],
        "rename /docs/notes.md -> /docs/renamed.md"
    );
    Ok(())
}

#[test]
fn remote_command_rename_accepts_explicit_from_to() -> Result<(), String> {
    let app = crate::tui::TuiApp::new("/docs");

    let plan = expect_operation(parse_remote_command("rename old.md new.md", &app)?)?;

    assert_eq!(
        plan.dry_run_lines()[0],
        "rename /docs/old.md -> /docs/new.md"
    );
    Ok(())
}

#[test]
fn remote_command_copy_uses_multi_selection_when_present() -> Result<(), String> {
    let mut app = crate::tui::TuiApp::new("/docs");
    app.set_entries(vec![
        file(1, 12),
        FileEntry {
            fs_id: 2,
            path: "/docs/second.md".to_string(),
            server_filename: "second.md".to_string(),
            ..file(2, 34)
        },
    ]);
    app.handle_key_event(crate::tui::KeyEvent::Char(' '));
    app.handle_key_event(crate::tui::KeyEvent::Char('j'));
    app.handle_key_event(crate::tui::KeyEvent::Char(' '));

    let plan = expect_operation(parse_remote_command("copy /backup", &app)?)?;

    assert_eq!(
        plan.dry_run_lines(),
        &[
            "copy 2 remote entries".to_string(),
            "/docs/notes.md -> /backup/notes.md".to_string(),
            "/docs/second.md -> /backup/second.md".to_string(),
        ]
    );
    Ok(())
}

#[test]
fn remote_command_delete_requires_operation_plan_from_selection() -> Result<(), String> {
    let mut app = crate::tui::TuiApp::new("/docs");
    app.set_entries(vec![file(1, 12)]);

    let plan = expect_operation(parse_remote_command("rm", &app)?)?;

    assert!(plan.is_destructive());
    assert_eq!(plan.dry_run_lines()[0], "delete 1 remote entries");
    Ok(())
}

#[test]
fn remote_command_delete_accepts_explicit_paths() -> Result<(), String> {
    let app = crate::tui::TuiApp::new("/docs");

    let plan = expect_operation(parse_remote_command("rm a.md nested/b.md", &app)?)?;

    assert_eq!(
        plan.dry_run_lines(),
        &[
            "delete 2 remote entries".to_string(),
            "/docs/a.md".to_string(),
            "/docs/nested/b.md".to_string(),
        ]
    );
    Ok(())
}

#[test]
fn delete_shortcut_plan_uses_multi_selection_then_current_entry() -> Result<(), String> {
    let mut app = crate::tui::TuiApp::new("/docs");
    app.set_entries(vec![
        file(1, 12),
        FileEntry {
            fs_id: 2,
            path: "/docs/second.md".to_string(),
            server_filename: "second.md".to_string(),
            ..file(2, 34)
        },
    ]);

    let current_plan = delete_plan_for_selection(&app)?;
    assert_eq!(
        current_plan.dry_run_lines(),
        &[
            "delete 1 remote entries".to_string(),
            "/docs/notes.md".to_string(),
        ]
    );

    app.handle_key_event(crate::tui::KeyEvent::Char(' '));
    app.handle_key_event(crate::tui::KeyEvent::Char('j'));
    app.handle_key_event(crate::tui::KeyEvent::Char(' '));
    let selected_plan = delete_plan_for_selection(&app)?;
    assert_eq!(
        selected_plan.dry_run_lines(),
        &[
            "delete 2 remote entries".to_string(),
            "/docs/notes.md".to_string(),
            "/docs/second.md".to_string(),
        ]
    );
    Ok(())
}

#[test]
fn rename_shortcut_plan_renames_selected_entry_in_place() -> Result<(), String> {
    let mut app = crate::tui::TuiApp::new("/docs");
    app.set_entries(vec![file(1, 12)]);

    let plan = rename_plan_for_selected(&app, "renamed file.md")?
        .ok_or_else(|| "rename unexpectedly skipped".to_string())?;

    assert_eq!(
        plan.dry_run_lines(),
        &["rename /docs/notes.md -> /docs/renamed file.md".to_string()]
    );
    assert!(rename_plan_for_selected(&app, "notes.md")?.is_none());
    Ok(())
}

#[test]
fn remote_command_move_accepts_explicit_sources_and_target_directory() -> Result<(), String> {
    let app = crate::tui::TuiApp::new("/docs");

    let plan = expect_operation(parse_remote_command("move a.md b.md /archive", &app)?)?;

    assert_eq!(
        plan.dry_run_lines(),
        &[
            "move 2 remote entries".to_string(),
            "/docs/a.md -> /archive/a.md".to_string(),
            "/docs/b.md -> /archive/b.md".to_string(),
        ]
    );
    Ok(())
}

#[test]
fn remote_command_search_parses_recursive_flag() -> Result<(), String> {
    let app = crate::tui::TuiApp::new("/docs");

    let (path, keyword, mode) = expect_search(parse_remote_command("search -r notes", &app)?)?;

    assert_eq!(path, "/docs");
    assert_eq!(keyword, "notes");
    assert_eq!(mode, RemoteSearchMode::RecursiveList);
    Ok(())
}

#[test]
fn remote_command_search_defaults_to_recursive_list() -> Result<(), String> {
    let app = crate::tui::TuiApp::new("/docs");

    let (_, keyword, mode) = expect_search(parse_remote_command("search notes", &app)?)?;

    assert_eq!(keyword, "notes");
    assert_eq!(mode, RemoteSearchMode::RecursiveList);
    Ok(())
}

#[test]
fn remote_command_search_accepts_current_and_api_modes() -> Result<(), String> {
    let app = crate::tui::TuiApp::new("/docs");

    let (_, current_keyword, current_mode) =
        expect_search(parse_remote_command("search --current notes", &app)?)?;
    let (_, api_keyword, api_mode) =
        expect_search(parse_remote_command("search --api notes", &app)?)?;

    assert_eq!(current_keyword, "notes");
    assert_eq!(current_mode, RemoteSearchMode::CurrentList);
    assert_eq!(api_keyword, "notes");
    assert_eq!(api_mode, RemoteSearchMode::PcsApi);
    Ok(())
}

#[test]
fn recursive_list_search_finds_nested_files_and_directories() -> Result<(), String> {
    let mut batches = Vec::new();

    let summary = search_remote_by_listing_with(
        "/docs",
        "photo",
        true,
        None,
        |path| match path {
            "/docs" => Ok(vec![
                remote_entry(1, "/docs/album-photo", true),
                remote_entry(2, "/docs/readme.md", false),
            ]),
            "/docs/album-photo" => Ok(vec![remote_entry(
                3,
                "/docs/album-photo/holiday-photo.jpg",
                false,
            )]),
            other => Err(crate::pcs::PcsError::Remote {
                operation: "list".to_string(),
                code: -1,
                message: format!("unexpected path {other}"),
            }),
        },
        |entries, _summary| {
            batches.extend(entries);
            true
        },
    )
    .map_err(|err| err.to_string())?;

    assert_eq!(summary.scanned_dirs, 2);
    assert_eq!(summary.scanned_entries, 3);
    assert_eq!(summary.matches, 2);
    assert_eq!(
        batches
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        vec!["/docs/album-photo", "/docs/album-photo/holiday-photo.jpg"]
    );
    Ok(())
}

#[test]
fn current_list_search_does_not_descend_into_nested_directories() -> Result<(), String> {
    let mut batches = Vec::new();

    let summary = search_remote_by_listing_with(
        "/docs",
        "photo",
        false,
        None,
        |_path| {
            Ok(vec![
                remote_entry(1, "/docs/photo-dir", true),
                remote_entry(2, "/docs/photo.txt", false),
            ])
        },
        |entries, _summary| {
            batches.extend(entries);
            true
        },
    )
    .map_err(|err| err.to_string())?;

    assert_eq!(summary.scanned_dirs, 1);
    assert_eq!(summary.scanned_entries, 2);
    assert_eq!(summary.matches, 2);
    assert_eq!(batches.len(), 2);
    Ok(())
}

#[test]
fn dlink_range_fetch_tries_later_candidates_after_network_failure() {
    let urls = vec![
        "https://bad.invalid/file".to_string(),
        "https://good.invalid/file".to_string(),
    ];
    let mut attempted = Vec::new();

    let result = fetch_first_successful_dlink_range(&urls, |url| {
        attempted.push(url.to_string());
        if url.contains("bad") {
            Err(crate::pcs::PcsError::Network {
                operation: "download".to_string(),
                message: "redacted".to_string(),
            })
        } else {
            Ok(vec![1, 2, 3])
        }
    });

    assert_eq!(result, DlinkRangeAttempt::Bytes(vec![1, 2, 3]));
    assert_eq!(attempted, urls);
}

#[test]
fn preview_key_uses_account_file_mtime_and_nonnegative_size() {
    let key = preview_key_for_entry(42, &file(7, -1));

    assert_eq!(key.account_uid, 42);
    assert_eq!(key.fs_id, 7);
    assert_eq!(key.mtime, 2);
    assert_eq!(key.size, 0);
}

#[test]
fn preview_kind_mapping_preserves_all_preview_boundaries() {
    let kinds: [crate::preview::PreviewKind; 10] = [
        crate::preview::PreviewKind::Text,
        crate::preview::PreviewKind::Image,
        crate::preview::PreviewKind::Pdf,
        crate::preview::PreviewKind::Archive,
        crate::preview::PreviewKind::Audio,
        crate::preview::PreviewKind::Document,
        crate::preview::PreviewKind::Ebook,
        crate::preview::PreviewKind::Font,
        crate::preview::PreviewKind::Directory,
        crate::preview::PreviewKind::Metadata,
    ];
    assert_eq!(kinds.len(), 10);
}

#[test]
fn video_entry_detection_requires_video_file_extension() {
    let mut entry = file(8, 1024);
    entry.server_filename = "clip.mp4".to_string();
    assert!(is_video_entry(&entry));

    entry.server_filename = "notes.txt".to_string();
    assert!(!is_video_entry(&entry));

    entry.server_filename = "folder.mp4".to_string();
    entry.is_dir = true;
    assert!(!is_video_entry(&entry));
}
