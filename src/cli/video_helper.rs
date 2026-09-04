use super::*;

use std::io::{BufWriter, Write as _};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

pub(crate) const VIDEO_HELPER_COMMAND: &str = "__video-proxy-helper";

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct VideoHelperRequest {
    pub(crate) remote_path: String,
    pub(crate) content_length: u64,
    pub(crate) token: String,
    pub(crate) chunk_size_bytes: u64,
    pub(crate) max_chunks: usize,
    pub(crate) prefetch_chunks: usize,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct VideoHelperReady {
    port: u16,
}

pub(crate) struct VideoHelperClient {
    pub(crate) session: crate::preview::VideoPreviewSession,
    pub(crate) stats: Arc<Mutex<crate::preview::VideoProxyStats>>,
    pub(crate) child: tokio::process::Child,
    pub(crate) lifetime: tokio::process::ChildStdin,
}

impl VideoHelperClient {
    pub(crate) fn port(&self) -> u16 {
        self.session.port
    }

    pub(crate) fn stats(&self) -> crate::preview::VideoProxyStats {
        self.stats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(crate) fn mpv_args(&self) -> Vec<String> {
        self.session.mpv_args()
    }

    pub(crate) async fn shutdown(mut self) -> Result<(), crate::preview::VideoError> {
        drop(self.lifetime);
        let _ = self.child.start_kill();
        self.child
            .wait()
            .await
            .map(|_| ())
            .map_err(|error| crate::preview::VideoError::Io(error.to_string()))
    }
}

pub(crate) async fn spawn_video_helper(
    remote_path: String,
    content_length: u64,
    token: crate::preview::VideoToken,
    budget: crate::preview::VideoCacheBudget,
    prefetch_chunks: usize,
) -> Result<VideoHelperClient, crate::preview::VideoError> {
    let executable = std::env::current_exe()
        .map_err(|error| crate::preview::VideoError::Io(error.to_string()))?;
    let mut child = tokio::process::Command::new(executable)
        .arg(VIDEO_HELPER_COMMAND)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| crate::preview::VideoError::Io(error.to_string()))?;
    let request = VideoHelperRequest {
        remote_path,
        content_length,
        token: token.as_str().to_string(),
        chunk_size_bytes: budget.chunk_size_bytes,
        max_chunks: budget.max_chunks,
        prefetch_chunks,
    };
    let request = serde_json::to_vec(&request)
        .map_err(|error| crate::preview::VideoError::Io(error.to_string()))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| crate::preview::VideoError::Io("video helper stdin missing".to_string()))?;
    stdin
        .write_all(&request)
        .await
        .map_err(|error| crate::preview::VideoError::Io(error.to_string()))?;
    stdin
        .write_all(b"\n")
        .await
        .map_err(|error| crate::preview::VideoError::Io(error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| crate::preview::VideoError::Io("video helper stdout missing".to_string()))?;
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    let ready_line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
        .await
        .map_err(|error| crate::preview::VideoError::Io(error.to_string()))?
        .map_err(|error| crate::preview::VideoError::Io(error.to_string()))?
        .ok_or_else(|| {
            crate::preview::VideoError::Io("video helper exited before ready".to_string())
        })?;
    let port = serde_json::from_str::<VideoHelperReady>(&ready_line)
        .map_err(|error| crate::preview::VideoError::Io(error.to_string()))?
        .port;
    let stats = Arc::new(Mutex::new(crate::preview::VideoProxyStats::default()));
    let stats_for_task = Arc::clone(&stats);
    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(stats) = serde_json::from_str(&line) else {
                continue;
            };
            *stats_for_task
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = stats;
        }
    });
    Ok(VideoHelperClient {
        session: crate::preview::VideoPreviewSession::new(content_length, token, port, budget),
        stats,
        child,
        lifetime: stdin,
    })
}

pub(crate) async fn run_video_helper() -> Result<(), String> {
    let mut input = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let line = input.next_line().await.map_err(|error| error.to_string())?;
    let line = line.ok_or_else(|| "video helper request missing".to_string())?;
    let request: VideoHelperRequest =
        serde_json::from_str(&line).map_err(|error| error.to_string())?;
    let config_path = crate::config::discover_config_file(None);
    let config = crate::config::load_config(&config_path).map_err(|error| error.to_string())?;
    let user = config
        .active_user()
        .ok_or_else(|| "active_user_missing".to_string())?;
    let account_bduss = bduss_from_user(user).ok_or_else(|| "bduss_missing".to_string())?;
    let client = build_pcs_client(&config, user)?;
    let token = crate::preview::VideoToken::new(request.token)
        .map_err(|error| format!("video_token_error: {error:?}"))?;
    let budget =
        crate::preview::VideoCacheBudget::new(request.chunk_size_bytes, request.max_chunks)
            .map_err(|error| format!("video_budget_error: {error:?}"))?;
    let upstream = Arc::new(PcsVideoUpstream::new(
        client,
        request.remote_path,
        user.uid,
        account_bduss,
    ));
    let cancel = tokio_util::sync::CancellationToken::new();
    let server = crate::preview::VideoLoopbackServer::spawn_with_prefetch(
        request.content_length,
        token,
        budget,
        request.prefetch_chunks,
        upstream,
        cancel,
    )
    .await
    .map_err(|error| error.to_string())?;
    let mut output = BufWriter::new(std::io::stdout());
    write_video_helper_message(
        &mut output,
        &VideoHelperReady {
            port: server.port(),
        },
    )?;
    let mut stats = server.stats.clone();
    loop {
        tokio::select! {
            changed = stats.changed() => {
                if changed.is_err() {
                    break;
                }
                let current = stats.borrow_and_update().clone();
                if write_video_helper_message(&mut output, &current).is_err() {
                    break;
                }
            }
            _ = input.next_line() => {
                break;
            }
        }
    }
    let _ = server.shutdown().await;
    Ok(())
}

fn write_video_helper_message<T: serde::Serialize>(
    output: &mut BufWriter<std::io::Stdout>,
    message: &T,
) -> Result<(), String> {
    serde_json::to_writer(&mut *output, message).map_err(|error| error.to_string())?;
    output.write_all(b"\n").map_err(|error| error.to_string())?;
    output.flush().map_err(|error| error.to_string())
}
