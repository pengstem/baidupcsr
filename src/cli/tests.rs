use std::ffi::OsString;
use std::fmt;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::pcs::FileEntry;

use super::{
    active_player_log_line, bduss_from_user, breadcrumb_preferred_selection, build_pcs_client,
    delete_plan_for_selection, download_target_path, error_chain_label,
    format_gateway_range_error_event, format_gateway_range_event, format_gateway_summary_event,
    format_source_error_event, format_source_range_event, format_video_bench_end,
    format_video_bench_start, is_local_open_size_allowed, is_video_entry,
    local_open_command_candidates, local_open_target_path, pan_user_agent_for_config,
    parse_byte_size_arg, parse_command, parse_remote_command, parse_video_bench_positions,
    persist_workdir_if_active, preview_key_for_entry, proxy_from_config_or_env, redacted_path_hash,
    remote_parent_path_for_bench, rename_plan_for_selected, run_remote_search_task,
    task_panel_item, video_bench_offset, CommandAction, ParsedRemoteCommand, PcsVideoUpstream,
    RemoteSearchMode, TransferTaskEvent, VideoBenchCase, VideoBenchCliOptions,
    VideoBenchHttpResponse, VideoBenchMovie, VideoBenchOutput, VideoBenchPosition,
    VideoBenchRangeFetch, VideoBenchResolvedOptions, LOCAL_OPEN_MAX_BYTES,
};

#[derive(Clone)]
struct SharedTestWriter {
    bytes: Arc<Mutex<Vec<u8>>>,
    fail_flush: bool,
}

impl Write for SharedTestWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes
            .lock()
            .map_err(|_| io::Error::other("test writer lock poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.fail_flush {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "synthetic flush failure",
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug)]
struct ChainedTestError {
    message: &'static str,
    source: Option<Box<ChainedTestError>>,
}

impl fmt::Display for ChainedTestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for ChainedTestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

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

fn test_pcs_client(address: SocketAddr) -> Result<crate::pcs::PcsClient, crate::pcs::PcsError> {
    crate::pcs::PcsClient::new(
        crate::pcs::PcsClientOptions {
            app_id: 266_719,
            pcs_addr: address.to_string(),
            enable_https: false,
        },
        crate::pcs::PcsAuth::from_cookie_header("BDUSS=redacted"),
        crate::pcs::PcsTransportOptions {
            user_agent: "cli-async-test".to_string(),
            pan_user_agent: "cli-async-test".to_string(),
            proxy: None,
            timeout: Duration::from_secs(2),
        },
    )
}

async fn read_test_http_request(socket: &mut TcpStream) -> std::io::Result<String> {
    let mut request = Vec::new();
    let mut chunk = [0; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
        if request.len() > 16 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "test HTTP request headers exceeded 16 KiB",
            ));
        }
    }
    String::from_utf8(request)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

async fn write_test_http_response(
    socket: &mut TcpStream,
    status: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).await?;
    socket.write_all(body).await
}

async fn write_test_range_response(
    socket: &mut TcpStream,
    range: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: {range}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).await?;
    socket.write_all(body).await
}

async fn list_fixture_client(
    responses: Vec<Vec<FileEntry>>,
) -> Result<
    (
        crate::pcs::PcsClient,
        JoinHandle<std::io::Result<Vec<String>>>,
    ),
    Box<dyn std::error::Error>,
> {
    let bodies = responses
        .into_iter()
        .map(|entries| serde_json::to_vec(&serde_json::json!({ "errno": 0, "list": entries })))
        .collect::<Result<Vec<_>, _>>()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let client = test_pcs_client(address)?;
    let server = tokio::spawn(async move {
        let mut request_lines = Vec::with_capacity(bodies.len());
        for body in bodies {
            let (mut socket, _) = listener.accept().await?;
            let request = read_test_http_request(&mut socket).await?;
            request_lines.push(request.lines().next().unwrap_or_default().to_string());
            write_test_http_response(&mut socket, "200 OK", &body).await?;
        }
        Ok(request_lines)
    });
    Ok((client, server))
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
fn byte_size_parser_accepts_legacy_binary_suffix_aliases() -> Result<(), String> {
    assert_eq!(parse_byte_size_arg("2k")?, 2 * 1024);
    assert_eq!(parse_byte_size_arg("2kb")?, 2 * 1024);
    assert_eq!(parse_byte_size_arg("2m")?, 2 * 1024 * 1024);
    assert_eq!(parse_byte_size_arg("2mb")?, 2 * 1024 * 1024);
    assert_eq!(parse_byte_size_arg("2g")?, 2 * 1024 * 1024 * 1024);
    assert_eq!(parse_byte_size_arg("2gb")?, 2 * 1024 * 1024 * 1024);
    Ok(())
}

#[test]
fn byte_size_parser_rejects_values_that_overflow_u64() {
    assert!(parse_byte_size_arg("18446744073709551616").is_err());
    assert!(parse_byte_size_arg("18446744073709551615g").is_err());
}

fn video_bench_test_movie() -> VideoBenchMovie {
    let remote_path = "/电影/斯巴达300勇士.mkv";
    let mut entry = file(9, 1024 * 1024);
    entry.path = remote_path.to_string();
    entry.server_filename = "斯巴达300勇士.mkv".to_string();
    VideoBenchMovie {
        entry,
        size_bytes: 1024 * 1024,
        path_hash: redacted_path_hash(remote_path),
        extension: "mkv".to_string(),
    }
}

#[test]
fn video_bench_start_event_uses_hashes_instead_of_raw_identifiers() -> Result<(), serde_json::Error>
{
    let movie = video_bench_test_movie();
    let resolved = VideoBenchResolvedOptions {
        chunk_size_bytes: 1024,
        max_chunks: 2,
        prefetch_chunks: 1,
        memory_limit_bytes: 2048,
    };

    let event = format_video_bench_start(&VideoBenchCliOptions::default(), &resolved, &movie, 42);
    let line = serde_json::to_string(&event)?;

    assert!(event["account_hash"].is_string());
    assert!(event["movie"]["path_hash"].is_string());
    assert!(event.get("account_uid").is_none());
    assert!(!line.contains("斯巴达"));
    assert!(!line.contains("/电影"));
    Ok(())
}

#[test]
fn video_bench_remote_parent_is_extracted() {
    assert_eq!(remote_parent_path_for_bench("/电影/clip.mkv"), "/电影");
    assert_eq!(remote_parent_path_for_bench("/clip.mkv"), "/");
}

#[test]
fn video_bench_formatters_emit_valid_single_line_json_objects() -> Result<(), String> {
    const BENCHMARK_BYTES: usize = 1_294_529;
    let movie = video_bench_test_movie();
    let options = VideoBenchCliOptions::default();
    let resolved = VideoBenchResolvedOptions {
        chunk_size_bytes: 1024 * 1024,
        max_chunks: 2,
        prefetch_chunks: 1,
        memory_limit_bytes: 2 * 1024 * 1024,
    };
    let fetch = VideoBenchRangeFetch {
        bytes_len: BENCHMARK_BYTES,
        candidate_index: 1,
        refresh_count: 0,
    };
    let response = VideoBenchHttpResponse {
        status_code: 206,
        body_len: BENCHMARK_BYTES,
        content_length: Some(BENCHMARK_BYTES as u64),
        content_range: Some("bytes 0-1294528/1294529".to_string()),
    };
    let stats = crate::preview::VideoProxyStats {
        cache_hits: 3,
        upstream_fetches: 2,
        prefetch_fetches: 1,
        used_bytes: 1024 * 1024,
        window_start: Some(0),
        window_end_inclusive: Some(1024 * 1024 - 1),
        ..crate::preview::VideoProxyStats::default()
    };
    let escaped_error = "bad quote \" slash \\ line\ncontrol \u{0001}";
    let escaped_stop_reason = "done quote \" slash \\ line\ncontrol \u{0001}";
    let elapsed = Duration::from_secs(1);
    let events = [
        format_video_bench_start(&options, &resolved, &movie, 42),
        format_video_bench_end(&options),
        format_source_range_event(
            &movie,
            VideoBenchPosition::Start,
            0,
            0,
            BENCHMARK_BYTES as u64,
            &fetch,
            elapsed,
        ),
        format_source_error_event(
            &movie,
            VideoBenchPosition::Tail,
            1,
            0,
            1024,
            elapsed,
            escaped_error,
        ),
        format_gateway_range_event(
            &movie,
            0,
            BENCHMARK_BYTES as u64,
            &response,
            elapsed,
            true,
            None,
        ),
        format_gateway_range_error_event(&movie, 0, 1024, elapsed, escaped_error),
        format_gateway_summary_event(
            &movie,
            1,
            BENCHMARK_BYTES as u64,
            elapsed,
            &stats,
            escaped_stop_reason,
        ),
    ];

    for event in &events {
        if !event.is_object() {
            return Err(format!("event must be a JSON object: {event:?}"));
        }
        let line = serde_json::to_string(event).map_err(|error| error.to_string())?;
        if line
            .chars()
            .any(|character| matches!(character, '\n' | '\r'))
        {
            return Err(format!("event contains a literal line break: {line:?}"));
        }
        if let Some(throughput) = event.get("throughput_mib_s") {
            if !throughput.is_number() {
                return Err(format!("throughput must be numeric: {line:?}"));
            }
        }
    }

    let sanitized_error = events[3]["error"]
        .as_str()
        .ok_or_else(|| "source error field must be a string".to_string())?;
    assert!(sanitized_error.contains("bad quote \" slash \\ line"));
    assert!(!sanitized_error
        .chars()
        .any(|character| matches!(character, '\n' | '\r')));
    assert!(!sanitized_error.chars().any(char::is_control));
    assert_eq!(events[6]["stop_reason"], escaped_stop_reason);
    assert_eq!(events[2]["throughput_mib_s"], 1.235);
    assert_eq!(events[3]["throughput_mib_s"], 0.0);
    assert_eq!(events[4]["throughput_mib_s"], 1.235);
    assert_eq!(events[5]["throughput_mib_s"], 0.0);
    assert_eq!(events[6]["throughput_mib_s"], 1.235);
    Ok(())
}

#[test]
fn video_bench_redacted_events_do_not_leak_sensitive_inputs() -> Result<(), serde_json::Error> {
    let raw_path = "/电影/斯巴达300勇士.mkv";
    let raw_token = "SECRET_TOKEN_123";
    let raw_url = "https://d.pcs.baidu.com/file/private?token=SECRET_TOKEN_123";
    let movie = video_bench_test_movie();
    let options = VideoBenchCliOptions {
        remote_path: raw_path.to_string(),
        redact: true,
        ..VideoBenchCliOptions::default()
    };
    let resolved = VideoBenchResolvedOptions {
        chunk_size_bytes: 1024,
        max_chunks: 2,
        prefetch_chunks: 1,
        memory_limit_bytes: 2048,
    };
    let sensitive_error = format!("fetch failed for {raw_path} via {raw_url} token={raw_token}");
    let stats = crate::preview::VideoProxyStats {
        last_error: Some(crate::preview::VideoError::Upstream(
            sensitive_error.clone(),
        )),
        ..crate::preview::VideoProxyStats::default()
    };
    let events = [
        format_video_bench_start(&options, &resolved, &movie, 42),
        format_source_error_event(
            &movie,
            VideoBenchPosition::Start,
            0,
            0,
            1024,
            Duration::from_millis(1),
            sensitive_error.as_str(),
        ),
        format_gateway_range_error_event(
            &movie,
            0,
            1024,
            Duration::from_millis(1),
            sensitive_error.as_str(),
        ),
        format_gateway_summary_event(&movie, 0, 0, Duration::from_millis(1), &stats, "error"),
    ];
    let events = events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");

    assert!(!events.contains(raw_path));
    assert!(!events.contains(raw_token));
    assert!(!events.contains(raw_url));
    Ok(())
}

#[test]
fn video_bench_output_emits_one_ndjson_object_per_line() -> Result<(), String> {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedTestWriter {
        bytes: Arc::clone(&bytes),
        fail_flush: false,
    };
    let mut output = VideoBenchOutput {
        writer: Box::new(writer),
    };

    output.emit(&serde_json::json!({ "type": "first" }))?;
    output.emit(&serde_json::json!({ "type": "second" }))?;
    output.flush()?;

    let written = String::from_utf8(
        bytes
            .lock()
            .map_err(|_| "test writer lock poisoned".to_string())?
            .clone(),
    )
    .map_err(|error| error.to_string())?;
    let lines = written.lines().collect::<Vec<_>>();
    assert!(written.ends_with('\n'));
    assert_eq!(lines.len(), 2);
    let first: serde_json::Value =
        serde_json::from_str(lines[0]).map_err(|error| error.to_string())?;
    let second: serde_json::Value =
        serde_json::from_str(lines[1]).map_err(|error| error.to_string())?;
    assert_eq!(first["type"], "first");
    assert_eq!(second["type"], "second");
    Ok(())
}

#[test]
fn video_bench_output_propagates_flush_errors() {
    let mut output = VideoBenchOutput {
        writer: Box::new(SharedTestWriter {
            bytes: Arc::new(Mutex::new(Vec::new())),
            fail_flush: true,
        }),
    };

    assert!(output.flush().is_err());
}

#[test]
fn error_chain_label_includes_every_cause() {
    let error = ChainedTestError {
        message: "outer request failed",
        source: Some(Box::new(ChainedTestError {
            message: "transport failed",
            source: Some(Box::new(ChainedTestError {
                message: "socket reset",
                source: None,
            })),
        })),
    };

    let label = error_chain_label(&error);

    assert!(label.contains("outer request failed"));
    assert!(label.contains("transport failed"));
    assert!(label.contains("socket reset"));
}

#[tokio::test]
#[ignore = "uses the machine-local BaiduPCS-Go config and network; prints no secret fields"]
async fn live_video_dlink_range_smoke_is_redacted() -> Result<(), String> {
    let config_path = crate::config::discover_config_file(None);
    let config = crate::config::load_config(&config_path)
        .map_err(|err| format!("live_config_load_failed: {err}"))?;
    let user = config
        .active_user()
        .ok_or_else(|| "live_active_user_missing".to_string())?;
    let bduss = bduss_from_user(user).ok_or_else(|| "live_bduss_missing".to_string())?;
    let client = build_pcs_client(&config, user)
        .map_err(|err| format!("live_client_build_failed: {err}"))?;
    let cancel = CancellationToken::new();
    let entries = client
        .list(user.workdir.as_str(), &cancel)
        .await
        .map_err(|err| format!("live_list_failed: {err}"))?;
    let entry = entries
        .into_iter()
        .find(is_video_entry)
        .ok_or_else(|| "live_video_missing".to_string())?;

    let urls = client
        .locate_download_urls(entry.path.as_str(), user.uid, bduss.as_str(), &cancel)
        .await
        .map_err(|err| format!("live_locatedownload_failed: {}", pcs_error_kind(&err)))?;
    println!("live_locatedownload_url_count={}", urls.len());
    let first_url = urls
        .first()
        .ok_or_else(|| "live_locatedownload_empty".to_string())?;

    let range = crate::pcs::ByteRange::new(0, 1023)
        .map_err(|err| format!("live_range_invalid: {}", pcs_error_kind(&err)))?;
    let bytes = client
        .download_url_range(first_url.as_str(), range, &cancel)
        .await
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

#[tokio::test]
async fn async_recursive_list_search_emits_nested_matches() -> Result<(), Box<dyn std::error::Error>>
{
    let (client, server) = list_fixture_client(vec![
        vec![
            remote_entry(1, "/docs/album-photo", true),
            remote_entry(2, "/docs/readme.md", false),
        ],
        vec![remote_entry(
            3,
            "/docs/album-photo/holiday-photo.jpg",
            false,
        )],
    ])
    .await?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let mut manager = crate::app::TaskManager::new();
    let task_id = manager.enqueue_remote_operation("search");
    let summary = run_remote_search_task(
        client,
        tx.clone(),
        task_id,
        "/docs",
        "photo",
        RemoteSearchMode::RecursiveList,
        &CancellationToken::new(),
    )
    .await?;
    drop(tx);
    let server_result = tokio::time::timeout(Duration::from_secs(1), server).await?;
    let requests = server_result??;
    let mut batches = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let TransferTaskEvent::SearchBatch { entries, .. } = event {
            batches.extend(entries);
        }
    }

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
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("path=%2Fdocs"));
    assert!(requests[1].contains("path=%2Fdocs%2Falbum-photo"));
    Ok(())
}

#[tokio::test]
async fn async_current_list_search_does_not_descend_into_nested_directories(
) -> Result<(), Box<dyn std::error::Error>> {
    let (client, server) = list_fixture_client(vec![vec![
        remote_entry(1, "/docs/photo-dir", true),
        remote_entry(2, "/docs/photo.txt", false),
    ]])
    .await?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let mut manager = crate::app::TaskManager::new();
    let task_id = manager.enqueue_remote_operation("search");
    let summary = run_remote_search_task(
        client,
        tx.clone(),
        task_id,
        "/docs",
        "photo",
        RemoteSearchMode::CurrentList,
        &CancellationToken::new(),
    )
    .await?;
    drop(tx);
    let server_result = tokio::time::timeout(Duration::from_secs(1), server).await?;
    let requests = server_result??;
    let mut batches = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let TransferTaskEvent::SearchBatch { entries, .. } = event {
            batches.extend(entries);
        }
    }

    assert_eq!(summary.scanned_dirs, 1);
    assert_eq!(summary.scanned_entries, 2);
    assert_eq!(summary.matches, 2);
    assert_eq!(batches.len(), 2);
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("path=%2Fdocs"));
    Ok(())
}

#[tokio::test]
async fn async_dlink_range_fetch_tries_later_candidates_after_network_failure(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let located_body = serde_json::to_vec(&serde_json::json!({
        "errno": 0,
        "urls": [
            { "url": format!("http://{address}/bad"), "encrypt": 0 },
            { "url": format!("http://{address}/good"), "encrypt": 0 }
        ]
    }))?;
    let server = tokio::spawn(async move {
        let mut request_lines = Vec::new();
        for _ in 0..3 {
            let (mut socket, _) = listener.accept().await?;
            let request = read_test_http_request(&mut socket).await?;
            let first_line = request.lines().next().unwrap_or_default().to_string();
            let (status, body): (&str, &[u8]) = if first_line.contains("method=locatedownload") {
                ("200 OK", &located_body)
            } else if first_line.contains(" /bad ") {
                (
                    "502 Bad Gateway",
                    br#"{"errno":-1,"error_msg":"fixture failure"}"#,
                )
            } else if first_line.contains(" /good ") {
                ("206 Partial Content", &[1, 2, 3])
            } else {
                ("404 Not Found", br#"{"errno":-1}"#)
            };
            let is_good_range = first_line.contains(" /good ");
            request_lines.push(first_line);
            if is_good_range {
                write_test_range_response(&mut socket, "bytes 0-2/3", body).await?;
            } else {
                write_test_http_response(&mut socket, status, body).await?;
            }
        }
        Ok::<_, std::io::Error>(request_lines)
    });
    let client = test_pcs_client(address)?;
    let upstream = PcsVideoUpstream::new(client, "/video.mp4".to_string(), 42, "bduss".to_string());
    let bytes = crate::app::RangeFetcher::fetch_range(
        &upstream,
        "/video.mp4",
        crate::pcs::ByteRange::new(0, 2)?,
        &CancellationToken::new(),
    )
    .await?;
    let server_result = tokio::time::timeout(Duration::from_secs(1), server).await?;
    let requests = server_result??;

    assert_eq!(bytes, vec![1, 2, 3]);
    assert_eq!(requests.len(), 3);
    assert!(requests[0].contains("method=locatedownload"));
    assert!(requests[1].contains(" /bad "));
    assert!(requests[2].contains(" /good "));
    Ok(())
}

#[tokio::test]
async fn async_dlink_range_fetch_promotes_successful_candidate_for_following_ranges(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let located_body = serde_json::to_vec(&serde_json::json!({
        "errno": 0,
        "urls": [
            { "url": format!("http://{address}/bad"), "encrypt": 0 },
            { "url": format!("http://{address}/good"), "encrypt": 0 }
        ]
    }))?;
    let server = tokio::spawn(async move {
        let mut request_lines = Vec::new();
        while request_lines.len() < 5 {
            let Ok(Ok((mut socket, _))) =
                tokio::time::timeout(Duration::from_millis(250), listener.accept()).await
            else {
                break;
            };
            let request = read_test_http_request(&mut socket).await?;
            let first_line = request.lines().next().unwrap_or_default().to_string();
            request_lines.push(first_line.clone());
            if first_line.contains("method=locatedownload") {
                write_test_http_response(&mut socket, "200 OK", &located_body).await?;
            } else if first_line.contains(" /bad ") {
                write_test_http_response(
                    &mut socket,
                    "502 Bad Gateway",
                    br#"{"errno":-1,"error_msg":"fixture failure"}"#,
                )
                .await?;
            } else if first_line.contains(" /good ") {
                let second_range = request.to_ascii_lowercase().contains("range: bytes=3-5");
                if second_range {
                    write_test_range_response(&mut socket, "bytes 3-5/6", &[4, 5, 6]).await?;
                } else {
                    write_test_range_response(&mut socket, "bytes 0-2/6", &[1, 2, 3]).await?;
                }
            } else {
                write_test_http_response(&mut socket, "404 Not Found", br#"{"errno":-1}"#).await?;
            }
        }
        Ok::<_, std::io::Error>(request_lines)
    });
    let client = test_pcs_client(address)?;
    let upstream = PcsVideoUpstream::new(client, "/video.mp4".to_string(), 42, "bduss".to_string());
    let cancel = CancellationToken::new();
    let first = crate::app::RangeFetcher::fetch_range(
        &upstream,
        "/video.mp4",
        crate::pcs::ByteRange::new(0, 2)?,
        &cancel,
    )
    .await?;
    let second = crate::app::RangeFetcher::fetch_range(
        &upstream,
        "/video.mp4",
        crate::pcs::ByteRange::new(3, 5)?,
        &cancel,
    )
    .await?;
    let server_result = tokio::time::timeout(Duration::from_secs(1), server).await?;
    let requests = server_result??;

    assert_eq!(first, vec![1, 2, 3]);
    assert_eq!(second, vec![4, 5, 6]);
    assert_eq!(requests.len(), 4);
    assert!(requests[0].contains("method=locatedownload"));
    assert!(requests[1].contains(" /bad "));
    assert!(requests[2].contains(" /good "));
    assert!(requests[3].contains(" /good "));
    assert_eq!(
        requests
            .iter()
            .filter(|line| line.contains(" /bad "))
            .count(),
        1
    );
    Ok(())
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
