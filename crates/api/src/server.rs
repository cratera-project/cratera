use anyhow::Context;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{get, post};
use axum::{Json, Router};
use cratera_common::HarnessResult;
use cratera_compiler::{CodeValidator, RUN_TEST_LIMIT, limit_main_tests, splice_harness};
use cratera_executor::{ExecError, ExecutorConfig, FirecrackerExecutor};
use serde::Deserialize;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

const MAX_REQUEST_BODY_SIZE: usize = 512 * 1024;
const DEFAULT_RUN_MS: u64 = 2000;
const DEFAULT_SUBMIT_MS: u64 = 5000;
const MAX_TIME_MS: u64 = 10_000;

pub struct AppState {
    pub executor: FirecrackerExecutor,
    pub internal_key: String,
    pub run_timeout_ms: u64,
    pub submit_timeout_ms: u64,
    pub max_time_ms: u64,
}

fn default_mode() -> String {
    "run".to_string()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessRequest {
    pub code: String,
    #[serde(default)]
    pub harness: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub language: Option<String>,
}

pub fn pid_file_path() -> PathBuf {
    let base = std::env::var("CRATERA_WORK_DIR").unwrap_or_else(|_| "/var/tmp/cratera".into());
    let _ = fs::create_dir_all(&base);
    PathBuf::from(base).join("server.pid")
}

pub fn log_file_path() -> PathBuf {
    let base = std::env::var("CRATERA_WORK_DIR").unwrap_or_else(|_| "/var/tmp/cratera".into());
    let _ = fs::create_dir_all(&base);
    PathBuf::from(base).join("server.log")
}

pub fn get_server_pid() -> Option<u32> {
    let pid_file = pid_file_path();
    if let Ok(content) = fs::read_to_string(&pid_file) {
        content.trim().parse::<u32>().ok()
    } else {
        None
    }
}

pub async fn is_server_running() -> bool {
    let addr = std::env::var("CRATERA_BIND")
        .or_else(|_| std::env::var("GRADE_BIND"))
        .unwrap_or_else(|_| "127.0.0.1:3100".into());
    tokio::net::TcpStream::connect(addr.as_str()).await.is_ok()
}

pub async fn get_server_addr() -> String {
    let bind = std::env::var("CRATERA_BIND")
        .or_else(|_| std::env::var("GRADE_BIND"))
        .unwrap_or_else(|_| "127.0.0.1:3100".into());
    if let Some(pid) = get_server_pid() {
        format!("{bind} [PID {pid}]")
    } else {
        bind
    }
}

pub async fn stop_server() -> bool {
    let pid_path = pid_file_path();
    let mut stopped = false;

    if let Some(pid) = get_server_pid() {
        let _ = std::process::Command::new("kill")
            .args(["-15", &pid.to_string()])
            .output();
        let _ = fs::remove_file(&pid_path);
        stopped = true;
    }

    // Wait for the socket to be released
    for _ in 0..20 {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        if !is_server_running().await {
            return true;
        }
    }

    // If still alive, issue SIGKILL
    if let Some(pid) = get_server_pid() {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
        let _ = fs::remove_file(&pid_path);
        stopped = true;
    }

    stopped || !is_server_running().await
}

pub async fn start_server_background() -> anyhow::Result<String> {
    let bind_addr = std::env::var("CRATERA_BIND")
        .or_else(|_| std::env::var("GRADE_BIND"))
        .unwrap_or_else(|_| "127.0.0.1:3100".into());

    if is_server_running().await {
        if let Some(pid) = get_server_pid() {
            return Ok(format!("{bind_addr} [PID {pid}]"));
        }
        return Ok(bind_addr);
    }

    let exe = std::env::current_exe().context("Failed to get current binary path")?;
    let log_path = log_file_path();
    let pid_path = pid_file_path();

    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .context("Failed to open server log file")?;

    #[cfg(unix)]
    use std::os::unix::process::CommandExt;

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("serve");
    cmd.stdout(std::process::Stdio::from(log_file.try_clone()?));
    cmd.stderr(std::process::Stdio::from(log_file));

    #[cfg(unix)]
    cmd.process_group(0); // Detach process group so closing parent terminal won't kill child

    let child = cmd
        .spawn()
        .context("Failed to spawn background cratera server process")?;
    let pid = child.id();
    let _ = fs::write(&pid_path, pid.to_string());

    // Poll until /health is listening and responding (up to 3 seconds)
    for _ in 0..60 {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        if is_server_running().await {
            return Ok(format!("{bind_addr} [PID {pid}]"));
        }
    }

    Ok(format!("{bind_addr} [PID {pid}]"))
}

pub async fn start_server() -> anyhow::Result<()> {
    let key = std::env::var("CRATERA_INTERNAL_KEY")
        .or_else(|_| std::env::var("GRADE_INTERNAL_KEY"))
        .unwrap_or_else(|_| {
            tracing::warn!("CRATERA_INTERNAL_KEY unset; using development default");
            "dev-key".into()
        });
    let production = std::env::var("NODE_ENV").as_deref() == Ok("production");
    if production && api_key_unfit_for_production(&key) {
        anyhow::bail!(
            "CRATERA_INTERNAL_KEY is a placeholder or shorter than 16 characters; set a random production key"
        );
    }

    let run_ms = std::env::var("CRATERA_RUN_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_RUN_MS);
    let submit_ms = std::env::var("CRATERA_SUBMIT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SUBMIT_MS);
    let max_time_ms = std::env::var("CRATERA_MAX_TIME_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(MAX_TIME_MS);

    let cfg = ExecutorConfig::try_from_env().map_err(anyhow::Error::msg)?;
    if production && !cfg.use_jailer {
        anyhow::bail!("Jailer required in production (CRATERA_USE_JAILER=0 is set)");
    }
    cfg.verify_guest_images(production)
        .map_err(|e| anyhow::anyhow!("guest image checksum: {e}"))?;
    tokio::fs::create_dir_all(&cfg.work_dir)
        .await
        .context("create work dir")?;
    info!(
        firecracker = %cfg.firecracker.display(),
        kernel = %cfg.kernel.display(),
        rootfs = %cfg.rootfs.display(),
        jailer = cfg.use_jailer,
        snapshot = cfg.use_snapshot,
        max_concurrent_jobs = cfg.max_concurrent_jobs,
        max_queued_jobs = cfg.max_queued_jobs,
        queue_timeout_ms = cfg.queue_timeout.as_millis() as u64,
        default_language = %cfg.languages.default_language,
        "executor config"
    );

    let executor = FirecrackerExecutor::new(cfg);
    if let Err(e) = executor.ensure_snapshot() {
        tracing::warn!(error = %e, "snapshot unavailable; jobs will cold-boot");
    }
    let state = Arc::new(AppState {
        executor,
        internal_key: key,
        run_timeout_ms: run_ms,
        submit_timeout_ms: submit_ms,
        max_time_ms,
    });

    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({"ok": true})) }),
        )
        .route("/harness", post(harness))
        .layer(TraceLayer::new_for_http())
        .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BODY_SIZE))
        .with_state(state);

    let addr: SocketAddr = std::env::var("CRATERA_BIND")
        .or_else(|_| std::env::var("GRADE_BIND"))
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 3100)));
    if production && !addr.ip().is_loopback() {
        anyhow::bail!("CRATERA_BIND must be loopback in production (got {addr})");
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "cratera listening");
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn harness(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<HarnessRequest>,
) -> Result<Json<HarnessResult>, (StatusCode, Json<serde_json::Value>)> {
    if !bearer_ok(&headers, &state.internal_key) {
        return Err(json_err(StatusCode::UNAUTHORIZED, "unauthorized"));
    }

    let resolved_lang = state
        .executor
        .config()
        .languages
        .resolve(req.language.as_deref())
        .ok_or_else(|| json_err(StatusCode::BAD_REQUEST, "unsupported language"))?;

    if resolved_lang.is_rust
        && let Err(e) = CodeValidator::validate(&req.code)
    {
        return Err(json_err(StatusCode::BAD_REQUEST, &e.to_string()));
    }

    let source = splice_harness(&req.harness, &req.code)
        .map_err(|e| json_err(StatusCode::BAD_REQUEST, &e.to_string()))?;

    let (source, time_ms) = match req.mode.as_str() {
        "run" => {
            let src = if resolved_lang.is_rust {
                limit_main_tests(&source, RUN_TEST_LIMIT).unwrap_or(source)
            } else {
                source
            };
            (src, state.run_timeout_ms)
        }
        "submit" => (source, state.submit_timeout_ms),
        _ => (source, state.run_timeout_ms),
    };
    let time_ms = time_ms.min(state.max_time_ms);
    let language = resolved_lang.key.clone();
    let t0 = Instant::now();
    match state
        .executor
        .run_harness(source, time_ms, Some(resolved_lang))
        .await
    {
        Ok(outcome) => {
            let compile_ms = outcome.job.compile_ms;
            let timed_out = outcome.job.timed_out;
            let oom = outcome.job.oom;
            let run_us = outcome.job.run_ms;
            let rss_kb = outcome.job.run_rss_kb;
            let result = HarnessResult::from_job(outcome.job, outcome.wall_ms).with_host_timings(
                compile_ms,
                outcome.copy_ms,
                outcome.boot_ms,
                outcome.wall_ms,
                outcome.restored,
            );
            info!(
                job_id = %outcome.job_id,
                language = %outcome.language,
                verdict = %result.verdict,
                timed_out,
                oom,
                copy_ms = outcome.copy_ms,
                boot_ms = outcome.boot_ms,
                compile_ms,
                run_us,
                wall_ms = outcome.wall_ms,
                http_ms = t0.elapsed().as_millis() as u64,
                restored = outcome.restored,
                rss_kb,
                cgroup_usage_usec = outcome.cgroup.usage_usec.unwrap_or(0),
                cgroup_memory_peak = outcome.cgroup.memory_peak.unwrap_or(0),
                cgroup_oom_kill = outcome.cgroup.oom_kill.unwrap_or(0),
                "job_record"
            );
            Ok(Json(result))
        }
        Err(ExecError::Busy) => {
            tracing::warn!(language = %language, reason = "queue_timeout", "job_record");
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(
                    serde_json::json!({"error":"queue timeout","code":"queue_timeout","unavailable":true}),
                ),
            ))
        }
        Err(ExecError::QueueFull) => {
            tracing::warn!(language = %language, reason = "queue_full", "job_record");
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(
                    serde_json::json!({"error":"queue full","code":"queue_full","unavailable":true}),
                ),
            ))
        }
        Err(ExecError::Failed(msg)) => {
            tracing::error!(language = %language, error = %msg, "job_record");
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error":"judge failed","unavailable":true})),
            ))
        }
    }
}

fn bearer_ok(headers: &HeaderMap, expected: &str) -> bool {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(got) = value.strip_prefix("Bearer ") else {
        return false;
    };
    keys_match(got.as_bytes(), expected.as_bytes())
}

pub(crate) fn api_key_unfit_for_production(key: &str) -> bool {
    let k = key.trim();
    k.len() < 16 || k.to_ascii_lowercase().starts_with("dev-key")
}

fn keys_match(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn json_err(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({"error": message})))
}

#[cfg(test)]
mod tests {
    use super::api_key_unfit_for_production;

    #[test]
    fn rejects_short_and_example_keys() {
        assert!(api_key_unfit_for_production("dev-key"));
        assert!(api_key_unfit_for_production("dev-key-change-me-please"));
        assert!(api_key_unfit_for_production("short"));
        assert!(!api_key_unfit_for_production(
            "a-sufficiently-long-random-token"
        ));
    }
}
