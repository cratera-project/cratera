use cratera_common::{JobRequest, JobResponse, read_frame, read_line_bytes, write_frame};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};
use tracing::{info, warn};

mod image_hash;
pub use image_hash::{ImageHashError, verify_image};

const GUEST_VCPU: u8 = 2;
const GUEST_MEM_MIB: u32 = 2048;
const BOOT_WAIT: Duration = Duration::from_secs(20);
const POWEROFF_GRACE: Duration = Duration::from_secs(3);
const SNAP_WAIT: Duration = Duration::from_secs(60);
const SNAP_CREATE_WAIT: Duration = Duration::from_secs(180);
const JAIL_MEMORY_MAX: &str = "3221225472";
const CPU_PERIOD_US: u32 = 100_000;

fn jail_cpu_max_value(vcpu: u8, override_val: Option<&str>) -> String {
    if let Some(v) = override_val.map(str::trim).filter(|s| !s.is_empty()) {
        return v.to_string();
    }
    format!("{} {CPU_PERIOD_US}", u32::from(vcpu.max(1)) * CPU_PERIOD_US)
}
const SNAP_MEMORY_MAX: &str = "6442450944";
const MAX_CONCURRENT_JOBS_LIMIT: usize = 1024;
const MAX_QUEUED_JOBS_LIMIT: usize = 100_000;

static NEXT_ID: AtomicU32 = AtomicU32::new(3);

#[derive(Debug, Clone, thiserror::Error)]
pub enum ExecError {
    #[error("executor queue wait timed out")]
    Busy,
    #[error("executor queue is full")]
    QueueFull,
    #[error("{0}")]
    Failed(String),
}

#[derive(Debug, Clone, Default)]
pub struct CgroupStats {
    pub usage_usec: Option<u64>,
    pub memory_peak: Option<u64>,
    pub oom_kill: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct JobOutcome {
    pub job: JobResponse,
    pub job_id: String,
    pub language: String,
    pub copy_ms: u64,
    pub boot_ms: u64,
    pub wall_ms: u64,
    pub restored: bool,
    pub cgroup: CgroupStats,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LanguageSpec {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub compile: Option<String>,
    #[serde(default)]
    pub run: String,
    #[serde(default)]
    pub is_rust: bool,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct LanguagesFile {
    #[serde(default)]
    pub languages: std::collections::HashMap<String, LanguageSpec>,
    #[serde(flatten)]
    pub direct: std::collections::HashMap<String, LanguageSpec>,
}

#[derive(Clone, Debug)]
pub struct LanguageRegistry {
    pub default_language: String,
    pub specs: std::collections::HashMap<String, LanguageSpec>,
}

#[derive(Clone, Debug)]
pub struct ResolvedLanguage {
    pub key: String,
    pub name: String,
    pub source_file: String,
    pub compile_cmd: Option<Vec<String>>,
    pub run_cmd: Vec<String>,
    pub is_rust: bool,
}

impl LanguageRegistry {
    pub fn from_env_or_file() -> Self {
        let mut specs = Self::builtin_specs();

        let candidate_paths = [
            std::env::var("CRATERA_LANGUAGES_FILE").ok(),
            Some("languages.toml".to_string()),
            Some("../languages.toml".to_string()),
            Some("../../languages.toml".to_string()),
            Some("/opt/cratera/languages.toml".to_string()),
            Some("/etc/cratera/languages.toml".to_string()),
        ];

        for path_opt in candidate_paths.into_iter().flatten() {
            if let Ok(content) = std::fs::read_to_string(&path_opt)
                && let Ok(parsed) = toml::from_str::<LanguagesFile>(&content)
            {
                for (k, v) in parsed.languages {
                    if v.enabled.unwrap_or(true) && !v.run.is_empty() {
                        specs.insert(k.to_lowercase(), v);
                    }
                }
                for (k, v) in parsed.direct {
                    if v.enabled.unwrap_or(true) && !v.run.is_empty() {
                        specs.insert(k.to_lowercase(), v);
                    }
                }
                break;
            }
        }

        let default_lang = std::env::var("CRATERA_LANGUAGE")
            .unwrap_or_else(|_| "rust".to_string())
            .to_lowercase();

        if let Some(spec) = specs.get_mut(&default_lang) {
            if let Ok(src) = std::env::var("CRATERA_SOURCE_FILE") {
                spec.source = src;
            }
            if let Ok(compile) = std::env::var("CRATERA_COMPILE_CMD") {
                spec.compile = if compile.trim().is_empty() || compile.trim() == "none" {
                    None
                } else {
                    Some(compile)
                };
            }
            if let Ok(run) = std::env::var("CRATERA_RUN_CMD") {
                spec.run = run;
            }
        }

        Self {
            default_language: default_lang,
            specs,
        }
    }

    pub fn resolve(&self, lang: Option<&str>) -> Option<ResolvedLanguage> {
        let raw = lang.unwrap_or(&self.default_language).trim().to_lowercase();
        let key = match raw.as_str() {
            "ts" | "typescript" => "typescript".to_string(),
            "js" | "javascript" | "node" | "nodejs" => "node".to_string(),
            "py" | "python" | "python3" => "python".to_string(),
            "rs" | "rust" => "rust".to_string(),
            "cs" | "c#" | "csharp" | "dotnet" => "csharp".to_string(),
            "c++" | "cpp" | "cc" | "cxx" => "cpp".to_string(),
            "c" | "clang" | "gcc" => "c".to_string(),
            "go" | "golang" => "go".to_string(),
            "java" => "java".to_string(),
            other => other.to_string(),
        };
        let spec = self.specs.get(&key)?;

        let source_file = if spec.source.starts_with('/') {
            spec.source.clone()
        } else {
            format!("/tmp/{}", spec.source)
        };

        let compile_cmd = spec.compile.as_ref().and_then(|cmd| {
            if cmd.trim().is_empty() || cmd.trim() == "none" {
                None
            } else {
                let expanded = cmd.replace("{file}", &source_file);
                Some(expanded.split_whitespace().map(String::from).collect())
            }
        });

        let run_expanded = spec.run.replace("{file}", &source_file);
        let run_cmd = run_expanded.split_whitespace().map(String::from).collect();

        Some(ResolvedLanguage {
            key,
            name: if spec.name.is_empty() {
                spec.source.clone()
            } else {
                spec.name.clone()
            },
            source_file,
            compile_cmd,
            run_cmd,
            is_rust: spec.is_rust,
        })
    }

    fn builtin_specs() -> std::collections::HashMap<String, LanguageSpec> {
        let mut map = std::collections::HashMap::new();
        map.insert(
            "rust".to_string(),
            LanguageSpec {
                name: "Rust".into(),
                source: "job.rs".into(),
                compile: Some("rustc --edition 2024 -C panic=abort -C opt-level=2 -C link-arg=-fno-use-linker-plugin -o /tmp/job {file}".into()),
                run: "/tmp/job".into(),
                is_rust: true,
                enabled: Some(true),
            },
        );
        map.insert(
            "python".to_string(),
            LanguageSpec {
                name: "Python".into(),
                source: "job.py".into(),
                compile: None,
                run: "python3 {file}".into(),
                is_rust: false,
                enabled: Some(true),
            },
        );
        map.insert(
            "cpp".to_string(),
            LanguageSpec {
                name: "C++".into(),
                source: "job.cpp".into(),
                compile: Some("g++ -O3 -std=c++20 -o /tmp/job {file}".into()),
                run: "/tmp/job".into(),
                is_rust: false,
                enabled: Some(true),
            },
        );
        map.insert(
            "c".to_string(),
            LanguageSpec {
                name: "C".into(),
                source: "job.c".into(),
                compile: Some("gcc -O3 -std=c17 -o /tmp/job {file}".into()),
                run: "/tmp/job".into(),
                is_rust: false,
                enabled: Some(true),
            },
        );
        map.insert(
            "go".to_string(),
            LanguageSpec {
                name: "Go".into(),
                source: "main.go".into(),
                compile: Some("go build -o /tmp/job {file}".into()),
                run: "/tmp/job".into(),
                is_rust: false,
                enabled: Some(true),
            },
        );
        map.insert(
            "node".to_string(),
            LanguageSpec {
                name: "JavaScript".into(),
                source: "job.js".into(),
                compile: None,
                run: "node {file}".into(),
                is_rust: false,
                enabled: Some(true),
            },
        );
        map.insert(
            "typescript".to_string(),
            LanguageSpec {
                name: "TypeScript".into(),
                source: "job.ts".into(),
                compile: Some(
                    "esbuild {file} --bundle --platform=node --outfile=/tmp/job.js".into(),
                ),
                run: "node /tmp/job.js".into(),
                is_rust: false,
                enabled: Some(true),
            },
        );
        map.insert(
            "java".to_string(),
            LanguageSpec {
                name: "Java".into(),
                source: "Solution.java".into(),
                compile: Some("javac -d /tmp {file}".into()),
                run: "java -cp /tmp Solution".into(),
                is_rust: false,
                enabled: Some(true),
            },
        );
        map.insert(
            "csharp".to_string(),
            LanguageSpec {
                name: "C#".into(),
                source: "Program.cs".into(),
                compile: Some("mono /usr/lib/mono/4.5/mcs.exe -out:/tmp/job.exe {file}".into()),
                run: "mono /tmp/job.exe".into(),
                is_rust: false,
                enabled: Some(true),
            },
        );
        map.insert(
            "zig".to_string(),
            LanguageSpec {
                name: "Zig".into(),
                source: "job.zig".into(),
                compile: Some("zig build-exe -O ReleaseFast -femit-bin=/tmp/job {file}".into()),
                run: "/tmp/job".into(),
                is_rust: false,
                enabled: Some(true),
            },
        );
        map
    }
}

#[derive(Clone)]
pub struct ExecutorConfig {
    pub firecracker: PathBuf,
    pub jailer: PathBuf,
    pub kernel: PathBuf,
    pub rootfs: PathBuf,
    pub work_dir: PathBuf,
    pub use_jailer: bool,
    pub jail_uid: u32,
    pub jail_gid: u32,
    pub use_snapshot: bool,
    pub snapshot_dir: PathBuf,
    pub vcpu: u8,
    pub mem_mib: u32,
    pub compile_timeout: Duration,
    pub max_concurrent_jobs: usize,
    pub max_queued_jobs: usize,
    pub queue_timeout: Duration,
    pub jail_mem_max: String,
    pub jail_cpu_max: String,
    pub jail_pids_max: u32,
    pub languages: LanguageRegistry,
}

impl ExecutorConfig {
    pub fn from_env() -> Self {
        fn env_path(cratera_key: &str, grade_key: &str, default: &str) -> PathBuf {
            let val = std::env::var(cratera_key)
                .or_else(|_| std::env::var(grade_key))
                .unwrap_or_else(|_| default.to_string());
            PathBuf::from(val)
        }
        fn env_flag_dual(cratera_key: &str, grade_key: &str, default: bool) -> bool {
            std::env::var(cratera_key)
                .or_else(|_| std::env::var(grade_key))
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(default)
        }
        let rootfs = if let Ok(val) =
            std::env::var("CRATERA_ROOTFS").or_else(|_| std::env::var("GRADE_ROOTFS"))
        {
            PathBuf::from(val)
        } else if Path::new("./images/rootfs.squashfs").exists() {
            PathBuf::from("./images/rootfs.squashfs")
        } else {
            PathBuf::from("./images/rootfs.ext4")
        };
        let snapshot_dir = std::env::var("CRATERA_SNAPSHOT_DIR")
            .or_else(|_| std::env::var("GRADE_SNAPSHOT_DIR"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                rootfs
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("snapshot")
            });
        let vcpu = std::env::var("CRATERA_VCPU")
            .or_else(|_| std::env::var("GRADE_VCPU"))
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(GUEST_VCPU);
        let mem_mib = std::env::var("CRATERA_MEM_MIB")
            .or_else(|_| std::env::var("GRADE_MEM_MIB"))
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(GUEST_MEM_MIB);
        let compile_timeout_secs = std::env::var("CRATERA_COMPILE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(12);
        let max_concurrent_jobs = std::env::var("CRATERA_MAX_CONCURRENT_JOBS")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|value| (1..=MAX_CONCURRENT_JOBS_LIMIT).contains(value))
            .unwrap_or(1);
        let max_queued_jobs = std::env::var("CRATERA_MAX_QUEUED_JOBS")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|value| *value <= MAX_QUEUED_JOBS_LIMIT)
            .unwrap_or(64);
        let queue_timeout_ms = std::env::var("CRATERA_QUEUE_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10_000);
        let jail_mem_max =
            std::env::var("CRATERA_JAIL_MEM_MAX").unwrap_or_else(|_| JAIL_MEMORY_MAX.into());
        let jail_cpu_max =
            jail_cpu_max_value(vcpu, std::env::var("CRATERA_JAIL_CPU_MAX").ok().as_deref());
        let jail_pids_max = std::env::var("CRATERA_JAIL_PIDS_MAX")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(64);

        Self {
            firecracker: env_path(
                "CRATERA_FIRECRACKER",
                "GRADE_FIRECRACKER",
                "./images/firecracker",
            ),
            jailer: env_path("CRATERA_JAILER", "GRADE_JAILER", "./images/jailer"),
            kernel: env_path("CRATERA_KERNEL", "GRADE_KERNEL", "./images/vmlinux.bin"),
            rootfs,
            work_dir: env_path("CRATERA_WORK_DIR", "GRADE_WORK_DIR", "/var/tmp/cratera"),
            use_jailer: env_flag_dual("CRATERA_USE_JAILER", "GRADE_USE_JAILER", false),
            jail_uid: std::env::var("CRATERA_JAIL_UID")
                .or_else(|_| std::env::var("GRADE_JAIL_UID"))
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(20001),
            jail_gid: std::env::var("CRATERA_JAIL_GID")
                .or_else(|_| std::env::var("GRADE_JAIL_GID"))
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(20001),
            use_snapshot: env_flag_dual("CRATERA_USE_SNAPSHOT", "GRADE_USE_SNAPSHOT", false),
            snapshot_dir,
            vcpu,
            mem_mib,
            compile_timeout: Duration::from_secs(compile_timeout_secs),
            max_concurrent_jobs,
            max_queued_jobs,
            queue_timeout: Duration::from_millis(queue_timeout_ms),
            jail_mem_max,
            jail_cpu_max,
            jail_pids_max,
            languages: LanguageRegistry::from_env_or_file(),
        }
    }

    pub fn verify_guest_images(&self, require_checksums: bool) -> Result<(), String> {
        for (label, path) in [("kernel", &self.kernel), ("rootfs", &self.rootfs)] {
            match verify_image(path) {
                Ok(()) => {}
                Err(ImageHashError::MissingSidecar(_)) if !require_checksums => {
                    warn!(
                        image = %path.display(),
                        "{label} checksum file missing; skip (not production)"
                    );
                }
                Err(e) => return Err(format!("{label}: {e}")),
            }
        }
        Ok(())
    }
}

pub struct FirecrackerExecutor {
    cfg: ExecutorConfig,
    limiter: ExecutionLimiter,
}

#[derive(Clone)]
struct ExecutionLimiter {
    slots: Arc<Semaphore>,
    admissions: Arc<Semaphore>,
    queue_timeout: Duration,
}

struct ExecutionPermit {
    _slot: OwnedSemaphorePermit,
    _admission: OwnedSemaphorePermit,
}

impl ExecutionLimiter {
    fn new(max_concurrent: usize, max_queued: usize, queue_timeout: Duration) -> Self {
        let max_concurrent = max_concurrent.clamp(1, MAX_CONCURRENT_JOBS_LIMIT);
        let max_queued = max_queued.min(MAX_QUEUED_JOBS_LIMIT);
        Self {
            slots: Arc::new(Semaphore::new(max_concurrent)),
            admissions: Arc::new(Semaphore::new(max_concurrent.saturating_add(max_queued))),
            queue_timeout,
        }
    }

    async fn acquire(&self) -> Result<ExecutionPermit, ExecError> {
        let admission =
            self.admissions
                .clone()
                .try_acquire_owned()
                .map_err(|error| match error {
                    tokio::sync::TryAcquireError::NoPermits => ExecError::QueueFull,
                    tokio::sync::TryAcquireError::Closed => {
                        ExecError::Failed("executor closed".into())
                    }
                })?;
        let slot = match tokio::time::timeout(
            self.queue_timeout,
            self.slots.clone().acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => return Err(ExecError::Failed("executor closed".into())),
            Err(_) => return Err(ExecError::Busy),
        };
        Ok(ExecutionPermit {
            _slot: slot,
            _admission: admission,
        })
    }
}

impl FirecrackerExecutor {
    pub fn new(cfg: ExecutorConfig) -> Self {
        let limiter = ExecutionLimiter::new(
            cfg.max_concurrent_jobs,
            cfg.max_queued_jobs,
            cfg.queue_timeout,
        );
        Self { cfg, limiter }
    }

    pub fn config(&self) -> &ExecutorConfig {
        &self.cfg
    }

    pub fn ensure_snapshot(&self) -> Result<(), ExecError> {
        if !self.cfg.use_snapshot {
            return Ok(());
        }
        if !self.cfg.use_jailer {
            warn!("CRATERA_USE_SNAPSHOT ignored without jailer (vsock paths are not portable)");
            return Ok(());
        }
        let snap = snap_paths(&self.cfg);
        if snap.ready() {
            info!(
                snap = %snap.state.display(),
                mem = %snap.mem.display(),
                "using existing Firecracker snapshot"
            );
            return Ok(());
        }
        info!("creating Firecracker snapshot (agent listen)");
        create_golden_snapshot(&self.cfg)?;
        chmod_snapshot_group(&self.cfg)?;
        if !snap.ready() {
            return Err(ExecError::Failed(
                "snapshot create did not write files".into(),
            ));
        }
        info!(
            snap = %snap.state.display(),
            mem = %snap.mem.display(),
            "Firecracker snapshot ready"
        );
        Ok(())
    }

    pub async fn run_harness(
        &self,
        source: String,
        timeout_ms: u64,
        lang: Option<ResolvedLanguage>,
    ) -> Result<JobOutcome, ExecError> {
        let permit = self.limiter.acquire().await?;
        let cfg = self.cfg.clone();
        let target_lang = lang.unwrap_or_else(|| {
            self.cfg
                .languages
                .resolve(None)
                .expect("default language must resolve")
        });
        let (tx, rx) = oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let result = run_sync(&cfg, &source, timeout_ms, target_lang, tx);
            drop(permit);
            if let Err(e) = &result {
                warn!(error = %e, "job failed after verdict channel closed");
            }
        });
        rx.await
            .map_err(|_| ExecError::Failed("job task dropped before verdict".into()))?
    }
}

struct JobLayout {
    id: String,
    jail_root: PathBuf,
    host_vsock: PathBuf,
    host_api: PathBuf,
    vm_json: PathBuf,
    cid: u32,
}

struct SnapPaths {
    state: PathBuf,
    mem: PathBuf,
}

impl SnapPaths {
    fn ready(&self) -> bool {
        self.state.is_file() && self.mem.is_file()
    }
}

fn snap_paths(cfg: &ExecutorConfig) -> SnapPaths {
    SnapPaths {
        state: cfg.snapshot_dir.join("vm.snap"),
        mem: cfg.snapshot_dir.join("vm.mem"),
    }
}

fn run_sync(
    cfg: &ExecutorConfig,
    source: &str,
    timeout_ms: u64,
    lang: ResolvedLanguage,
    tx: oneshot::Sender<Result<JobOutcome, ExecError>>,
) -> Result<JobOutcome, ExecError> {
    let wall_start = Instant::now();
    let t_copy = Instant::now();
    let layout = match prepare_job(cfg) {
        Ok(layout) => layout,
        Err(e) => {
            let _ = tx.send(Err(e.clone()));
            return Err(e);
        }
    };
    let copy_ms = t_copy.elapsed().as_millis() as u64;

    let snap = snap_paths(cfg);
    let restore = cfg.use_snapshot && cfg.use_jailer && snap.ready();
    let wall = cfg.compile_timeout + Duration::from_millis(timeout_ms) + BOOT_WAIT + POWEROFF_GRACE;

    let result = run_vm(
        cfg, &layout, source, timeout_ms, wall, copy_ms, restore, wall_start, &lang, tx,
    );
    cleanup(&layout.jail_root);
    result
}

fn prepare_job(cfg: &ExecutorConfig) -> Result<JobLayout, ExecError> {
    fs::create_dir_all(&cfg.work_dir).map_err(io_err)?;
    let cid = NEXT_ID.fetch_add(1, Ordering::Relaxed).max(3);
    let id = format!("job-{cid}");

    let jail_root = if cfg.use_jailer {
        cfg.work_dir.join("firecracker").join(&id).join("root")
    } else {
        cfg.work_dir.join(&id)
    };

    let build = || -> Result<JobLayout, ExecError> {
        for dir in ["kernel", "disk", "config", "vsock", "snapshot"] {
            fs::create_dir_all(jail_root.join(dir)).map_err(io_err)?;
        }

        let kernel_dst = jail_root.join("kernel/vmlinux.bin");
        let rootfs_dst = jail_root.join("disk/rootfs.ext4");
        hardlink_or_copy(&cfg.kernel, &kernel_dst)?;
        hardlink_or_copy(&cfg.rootfs, &rootfs_dst)?;

        let snap = snap_paths(cfg);
        if cfg.use_snapshot && cfg.use_jailer && snap.ready() {
            hardlink_or_copy(&snap.state, &jail_root.join("snapshot/vm.snap"))?;
            hardlink_or_copy(&snap.mem, &jail_root.join("snapshot/vm.mem"))?;
        }

        let (kernel_path, rootfs_path, uds_path) = if cfg.use_jailer {
            (
                "/kernel/vmlinux.bin".to_string(),
                "/disk/rootfs.ext4".to_string(),
                "/vsock/job.sock".to_string(),
            )
        } else {
            (
                kernel_dst.to_string_lossy().into_owned(),
                rootfs_dst.to_string_lossy().into_owned(),
                jail_root
                    .join("vsock/job.sock")
                    .to_string_lossy()
                    .into_owned(),
            )
        };

        let vm = vm_config_json(
            &kernel_path,
            &rootfs_path,
            &uds_path,
            cid,
            cfg.vcpu,
            cfg.mem_mib,
        );
        let vm_json = jail_root.join("config/vm.json");
        fs::write(
            &vm_json,
            serde_json::to_vec_pretty(&vm).map_err(|e| ExecError::Failed(e.to_string()))?,
        )
        .map_err(io_err)?;

        if cfg.use_jailer {
            chown_runtime(cfg, &jail_root)?;
        }

        Ok(JobLayout {
            id,
            host_vsock: jail_root.join("vsock/job.sock"),
            host_api: jail_root.join("api.sock"),
            jail_root: jail_root.clone(),
            vm_json,
            cid,
        })
    };

    match build() {
        Ok(layout) => Ok(layout),
        Err(e) => {
            cleanup(&jail_root);
            Err(e)
        }
    }
}

fn vm_config_json(
    kernel_path: &str,
    rootfs_path: &str,
    uds_path: &str,
    cid: u32,
    vcpu: u8,
    mem_mib: u32,
) -> serde_json::Value {
    json!({
        "boot-source": {
            "kernel_image_path": kernel_path,
            "boot_args": "reboot=k panic=1 pci=off nomodule root=/dev/vda ro init=/sbin/cratera-agent quiet"
        },
        "machine-config": {
            "vcpu_count": vcpu,
            "mem_size_mib": mem_mib,
            "smt": false
        },
        "drives": [{
            "drive_id": "rootfs",
            "path_on_host": rootfs_path,
            "is_root_device": true,
            "is_read_only": true
        }],
        "vsock": {
            "guest_cid": cid,
            "uds_path": uds_path
        }
    })
}

fn chown_runtime(cfg: &ExecutorConfig, jail_root: &Path) -> Result<(), ExecError> {
    let uid = cfg.jail_uid;
    let gid = cfg.jail_gid;
    for rel in ["", "config", "vsock", "snapshot", "kernel", "disk"] {
        let path = if rel.is_empty() {
            jail_root.to_path_buf()
        } else {
            jail_root.join(rel)
        };
        let _ = std::os::unix::fs::chown(&path, Some(uid), Some(gid));
    }
    let vm_json = jail_root.join("config/vm.json");
    let _ = std::os::unix::fs::chown(&vm_json, Some(uid), Some(gid));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_vm(
    cfg: &ExecutorConfig,
    layout: &JobLayout,
    source: &str,
    timeout_ms: u64,
    wall: Duration,
    copy_ms: u64,
    restore: bool,
    wall_start: Instant,
    lang: &ResolvedLanguage,
    tx: oneshot::Sender<Result<JobOutcome, ExecError>>,
) -> Result<JobOutcome, ExecError> {
    let mut child = match if restore {
        spawn_vm(cfg, layout, SpawnMode::ApiOnly, false)
    } else {
        spawn_vm(cfg, layout, SpawnMode::ConfigNoApi, false)
    } {
        Ok(child) => child,
        Err(e) => {
            let _ = tx.send(Err(e.clone()));
            return Err(e);
        }
    };
    let boot_t0 = Instant::now();
    info!(job = %layout.id, cid = layout.cid, restore, language = %lang.key, "microVM started");

    let rpc = (|| {
        if restore && let Err(e) = load_snapshot(cfg, layout) {
            warn!(error = %e, "snapshot restore failed; killing VM");
            return Err(e);
        }
        let mut stream = wait_connect(
            &layout.host_vsock,
            BOOT_WAIT.min(wall.saturating_sub(boot_t0.elapsed())),
        )?;
        let boot_ms = boot_t0.elapsed().as_millis() as u64;
        let job = send_job(
            &mut stream,
            source,
            timeout_ms,
            lang,
            wall.saturating_sub(boot_t0.elapsed()),
        )?;
        Ok((job, boot_ms))
    })();
    let (rpc, boot_ms) = match rpc {
        Ok((job, boot_ms)) => (Ok(job), boot_ms),
        Err(e) => (Err(e), boot_t0.elapsed().as_millis() as u64),
    };
    let died = child.try_wait().ok().flatten();
    let oom = died.and_then(|s| s.signal()) == Some(9);
    let wall_ms = wall_start.elapsed().as_millis() as u64;
    let cgroup = read_cgroup_stats(&layout.id);
    let mapped = match rpc {
        Ok(job) => Ok(JobOutcome {
            job,
            job_id: layout.id.clone(),
            language: lang.key.clone(),
            copy_ms,
            boot_ms,
            wall_ms,
            restored: restore,
            cgroup: cgroup.clone(),
        }),
        Err(_) if wall_start.elapsed() >= wall => Ok(JobOutcome {
            job: JobResponse {
                timed_out: true,
                compilation_success: true,
                run_ms: wall_start.elapsed().as_micros() as u64,
                ..Default::default()
            },
            job_id: layout.id.clone(),
            language: lang.key.clone(),
            copy_ms,
            boot_ms,
            wall_ms,
            restored: restore,
            cgroup: cgroup.clone(),
        }),
        Err(_) if oom => Ok(JobOutcome {
            job: JobResponse {
                oom: true,
                compilation_success: true,
                run_ms: wall_start.elapsed().as_micros() as u64,
                ..Default::default()
            },
            job_id: layout.id.clone(),
            language: lang.key.clone(),
            copy_ms,
            boot_ms,
            wall_ms,
            restored: restore,
            cgroup,
        }),
        Err(e) => Err(e),
    };
    let _ = tx.send(mapped.clone());

    let reap_t0 = Instant::now();
    reap_vm(layout, &mut child);
    info!(
        job = %layout.id,
        reap_ms = reap_t0.elapsed().as_millis() as u64,
        "microVM reaped"
    );
    mapped
}

fn load_snapshot(cfg: &ExecutorConfig, layout: &JobLayout) -> Result<(), ExecError> {
    wait_path(&layout.host_api, Duration::from_secs(5))?;
    let (snap_path, mem_path) = if cfg.use_jailer {
        (
            "/snapshot/vm.snap".to_string(),
            "/snapshot/vm.mem".to_string(),
        )
    } else {
        (
            layout
                .jail_root
                .join("snapshot/vm.snap")
                .to_string_lossy()
                .into_owned(),
            layout
                .jail_root
                .join("snapshot/vm.mem")
                .to_string_lossy()
                .into_owned(),
        )
    };
    fc_put(
        &layout.host_api,
        "/snapshot/load",
        &json!({
            "snapshot_path": snap_path,
            "mem_backend": {
                "backend_path": mem_path,
                "backend_type": "File"
            },
            "resume_vm": true
        }),
    )?;
    Ok(())
}

fn create_golden_snapshot(cfg: &ExecutorConfig) -> Result<(), ExecError> {
    fs::create_dir_all(&cfg.snapshot_dir).map_err(io_err)?;
    let t0 = Instant::now();
    let layout = prepare_job(cfg)?;
    let fc_log = layout.jail_root.join("config/fc.log");
    let _ = fs::write(&fc_log, b"");
    if cfg.use_jailer {
        let spec = format!("{}:{}", cfg.jail_uid, cfg.jail_gid);
        let _ = Command::new("chown")
            .args([&spec, fc_log.to_str().unwrap_or(".")])
            .status();
    }
    let mut child = spawn_vm(cfg, &layout, SpawnMode::ConfigWithApi, true)?;
    let result = (|| {
        wait_path(&layout.host_api, BOOT_WAIT)?;
        info!("snapshot vm api ready");
        wait_connect_probe(&layout.host_vsock, BOOT_WAIT)?;
        info!("snapshot vm agent listening");
        std::thread::sleep(Duration::from_millis(100));
        fc_patch(&layout.host_api, "/vm", &json!({ "state": "Paused" }))?;
        info!("snapshot vm paused");
        let (snap_path, mem_path) = if cfg.use_jailer {
            (
                "/snapshot/vm.snap".to_string(),
                "/snapshot/vm.mem".to_string(),
            )
        } else {
            (
                layout
                    .jail_root
                    .join("snapshot/vm.snap")
                    .to_string_lossy()
                    .into_owned(),
                layout
                    .jail_root
                    .join("snapshot/vm.mem")
                    .to_string_lossy()
                    .into_owned(),
            )
        };
        info!("writing Firecracker snapshot files");
        fc_http_timeout(
            &layout.host_api,
            "PUT",
            "/snapshot/create",
            &json!({
                "snapshot_type": "Full",
                "snapshot_path": snap_path,
                "mem_file_path": mem_path
            }),
            SNAP_CREATE_WAIT,
        )?;
        info!("snapshot files written in jail");
        let src_state = layout.jail_root.join("snapshot/vm.snap");
        let src_mem = layout.jail_root.join("snapshot/vm.mem");
        fs::copy(&src_state, snap_paths(cfg).state).map_err(io_err)?;
        fs::copy(&src_mem, snap_paths(cfg).mem).map_err(io_err)?;
        chmod_snapshot_group(cfg)?;
        Ok(())
    })();
    reap_vm(&layout, &mut child);
    let _ = Command::new("pkill")
        .args(["-9", "-x", "firecracker"])
        .status();
    let _ = Command::new("pkill").args(["-9", "-x", "jailer"]).status();
    if let Err(e) = &result
        && let Ok(log) = fs::read_to_string(&fc_log)
    {
        let tail: String = log
            .chars()
            .rev()
            .take(800)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        if !tail.is_empty() {
            warn!(error = %e, fc_log = %tail, "snapshot create failed");
        }
    }
    cleanup(&layout.jail_root);
    match result {
        Ok(()) => {
            info!(
                create_ms = t0.elapsed().as_millis() as u64,
                "golden snapshot written"
            );
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(snap_paths(cfg).state);
            let _ = fs::remove_file(snap_paths(cfg).mem);
            Err(e)
        }
    }
}

#[derive(Clone, Copy)]
enum SpawnMode {
    ConfigNoApi,
    ConfigWithApi,
    ApiOnly,
}

fn spawn_vm(
    cfg: &ExecutorConfig,
    layout: &JobLayout,
    mode: SpawnMode,
    snapshot_create: bool,
) -> Result<Child, ExecError> {
    let api_guest = "/api.sock";
    let api_host = layout.host_api.to_string_lossy().into_owned();
    let cfg_guest = "/config/vm.json";
    let cfg_host = layout.vm_json.to_string_lossy().into_owned();
    let mem_max = if snapshot_create {
        SNAP_MEMORY_MAX
    } else {
        &cfg.jail_mem_max
    };

    let mut cmd = if cfg.use_jailer {
        let mut c = Command::new(&cfg.jailer);
        c.args([
            "--id",
            &layout.id,
            "--exec-file",
            cfg.firecracker
                .to_str()
                .ok_or_else(|| ExecError::Failed("firecracker path".into()))?,
            "--uid",
            &cfg.jail_uid.to_string(),
            "--gid",
            &cfg.jail_gid.to_string(),
            "--chroot-base-dir",
            cfg.work_dir
                .to_str()
                .ok_or_else(|| ExecError::Failed("work dir".into()))?,
            "--cgroup-version",
            "2",
            "--cgroup",
            &format!("memory.max={mem_max}"),
            "--cgroup",
            &format!("cpu.max={}", cfg.jail_cpu_max),
            "--cgroup",
            &format!("pids.max={}", cfg.jail_pids_max),
            "--seccomp-level",
            "2",
            "--new-pid-ns",
            "--",
        ]);
        match mode {
            SpawnMode::ConfigNoApi => {
                c.args(["--no-api", "--config-file", cfg_guest]);
            }
            SpawnMode::ConfigWithApi => {
                c.args([
                    "--api-sock",
                    api_guest,
                    "--config-file",
                    cfg_guest,
                    "--log-path",
                    "/config/fc.log",
                    "--level",
                    "Info",
                ]);
            }
            SpawnMode::ApiOnly => {
                c.args(["--api-sock", api_guest]);
            }
        }
        c
    } else {
        let mut c = Command::new(&cfg.firecracker);
        match mode {
            SpawnMode::ConfigNoApi => {
                c.args(["--no-api", "--config-file", &cfg_host]);
            }
            SpawnMode::ConfigWithApi => {
                c.args(["--api-sock", &api_host, "--config-file", &cfg_host]);
            }
            SpawnMode::ApiOnly => {
                c.args(["--api-sock", &api_host]);
            }
        }
        c
    };
    cmd.env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(io_err)
}

const CGROUP_BASES: &[&str] = &[
    "/sys/fs/cgroup/firecracker",
    "/sys/fs/cgroup/jailer/firecracker",
    "/sys/fs/cgroup/jailer",
    "/sys/fs/cgroup",
];

fn cgroup_key_u64(text: &str, key: &str) -> Option<u64> {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() == Some(key) {
            return parts.next().and_then(|s| s.parse().ok());
        }
    }
    None
}

fn read_cgroup_u64(dir: &Path, file: &str) -> Option<u64> {
    fs::read_to_string(dir.join(file)).ok()?.trim().parse().ok()
}

fn read_cgroup_stats(job_id: &str) -> CgroupStats {
    for base in CGROUP_BASES {
        let dir = Path::new(base).join(job_id);
        if !dir.is_dir() {
            continue;
        }
        let cpu = fs::read_to_string(dir.join("cpu.stat")).ok();
        let events = fs::read_to_string(dir.join("memory.events")).ok();
        return CgroupStats {
            usage_usec: cpu.as_deref().and_then(|t| cgroup_key_u64(t, "usage_usec")),
            memory_peak: read_cgroup_u64(&dir, "memory.peak")
                .or_else(|| read_cgroup_u64(&dir, "memory.current")),
            oom_kill: events
                .as_deref()
                .and_then(|t| cgroup_key_u64(t, "oom_kill")),
        };
    }
    CgroupStats::default()
}

fn reap_vm(layout: &JobLayout, child: &mut Child) {
    let _ = child.kill();

    for base in CGROUP_BASES {
        let kill_path = format!("{base}/{}/cgroup.kill", layout.id);
        let p = Path::new(&kill_path);
        if p.is_file() {
            let _ = fs::write(p, b"1\n");
        }
    }

    let pattern = format!("--id {}\\b", layout.id);
    let _ = Command::new("pkill")
        .args(["-9", "-f", "--", &pattern])
        .status();

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }

    for base in CGROUP_BASES {
        let dir_path = format!("{base}/{}", layout.id);
        let p = Path::new(&dir_path);
        if p.is_dir() {
            let deadline = Instant::now() + Duration::from_millis(500);
            while Instant::now() < deadline {
                if fs::remove_dir(p).is_ok() || !p.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

fn send_job(
    stream: &mut UnixStream,
    source: &str,
    timeout_ms: u64,
    lang: &ResolvedLanguage,
    remaining: Duration,
) -> Result<JobResponse, ExecError> {
    let bytes = serde_json::to_vec(&JobRequest {
        source: source.to_string(),
        timeout_ms,
        source_file: Some(lang.source_file.clone()),
        compile_cmd: lang.compile_cmd.clone(),
        run_cmd: Some(lang.run_cmd.clone()),
    })
    .map_err(|e| ExecError::Failed(e.to_string()))?;
    write_frame(stream, &bytes).map_err(io_err)?;
    stream
        .set_read_timeout(Some(remaining.max(Duration::from_secs(1))))
        .map_err(io_err)?;
    let resp_bytes = read_frame(stream).map_err(io_err)?;
    serde_json::from_slice(&resp_bytes).map_err(|e| ExecError::Failed(e.to_string()))
}

fn wait_connect(uds: &Path, timeout: Duration) -> Result<UnixStream, ExecError> {
    let start = Instant::now();
    let mut last = String::new();
    while start.elapsed() < timeout {
        match UnixStream::connect(uds) {
            Ok(mut stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
                if stream.write_all(b"CONNECT 52\n").is_err() {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                match read_line_bytes(&mut stream) {
                    Ok(line) if line.starts_with("OK") => {
                        let _ = stream.set_read_timeout(None);
                        let _ = stream.set_write_timeout(None);
                        return Ok(stream);
                    }
                    Ok(line) => last = format!("vsock handshake: {line}"),
                    Err(e) => last = e.to_string(),
                }
            }
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(ExecError::Failed(format!("agent vsock not ready: {last}")))
}

fn wait_connect_probe(uds: &Path, timeout: Duration) -> Result<(), ExecError> {
    let _stream = wait_connect(uds, timeout)?;
    Ok(())
}

fn wait_path(path: &Path, timeout: Duration) -> Result<(), ExecError> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(ExecError::Failed(format!(
        "timed out waiting for {}",
        path.display()
    )))
}

fn fc_patch(sock: &Path, url_path: &str, body: &serde_json::Value) -> Result<(), ExecError> {
    fc_http_timeout(sock, "PATCH", url_path, body, SNAP_WAIT)
}

fn fc_put(sock: &Path, url_path: &str, body: &serde_json::Value) -> Result<(), ExecError> {
    fc_http_timeout(sock, "PUT", url_path, body, SNAP_WAIT)
}

fn fc_http_timeout(
    sock: &Path,
    method: &str,
    url_path: &str,
    body: &serde_json::Value,
    read_timeout: Duration,
) -> Result<(), ExecError> {
    let payload = serde_json::to_vec(body).map_err(|e| ExecError::Failed(e.to_string()))?;
    let mut stream = UnixStream::connect(sock).map_err(io_err)?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(io_err)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(io_err)?;
    let req = format!(
        "{method} {url_path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(req.as_bytes()).map_err(io_err)?;
    stream.write_all(&payload).map_err(io_err)?;
    stream.flush().map_err(io_err)?;
    let text = read_http_response(&mut stream)?;
    let status = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("0");
    if status.starts_with('2') {
        Ok(())
    } else {
        Err(ExecError::Failed(format!(
            "firecracker {method} {url_path} -> {status}: {}",
            text.chars().take(400).collect::<String>()
        )))
    }
}

fn read_http_response(stream: &mut impl Read) -> Result<String, ExecError> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    let header_end = loop {
        let n = stream.read(&mut tmp).map_err(io_err)?;
        if n == 0 {
            break None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break Some(pos + 4);
        }
        if buf.len() > 64 * 1024 {
            return Err(ExecError::Failed(
                "firecracker HTTP headers too large".into(),
            ));
        }
    };
    let Some(header_end) = header_end else {
        return Err(ExecError::Failed("firecracker closed API socket".into()));
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]);
    let content_len = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    });
    if let Some(len) = content_len {
        while buf.len() < header_end + len {
            let n = stream.read(&mut tmp).map_err(io_err)?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn chmod_snapshot_group(cfg: &ExecutorConfig) -> Result<(), ExecError> {
    use std::os::unix::fs::PermissionsExt;
    let snap = snap_paths(cfg);
    for path in [&snap.state, &snap.mem] {
        let _ = std::os::unix::fs::chown(path, Some(0), Some(cfg.jail_gid));
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o664));
    }
    Ok(())
}

fn hardlink_or_copy(src: &Path, dst: &Path) -> Result<(), ExecError> {
    let _ = fs::remove_file(dst);
    if fs::hard_link(src, dst).is_ok() {
        return Ok(());
    }
    fs::copy(src, dst).map_err(io_err)?;
    Ok(())
}

fn cleanup(jail_root: &Path) {
    let to_remove = if jail_root.file_name().and_then(|n| n.to_str()) == Some("root") {
        jail_root.parent().unwrap_or(jail_root)
    } else {
        jail_root
    };
    if let Err(e) = fs::remove_dir_all(to_remove) {
        warn!(path = %to_remove.display(), error = %e, "job cleanup failed");
    }
}

fn io_err(e: std::io::Error) -> ExecError {
    ExecError::Failed(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::os::unix::net::UnixListener;
    use std::thread;

    #[tokio::test]
    async fn execution_limiter_runs_up_to_configured_capacity() {
        let limiter = ExecutionLimiter::new(2, 0, Duration::from_secs(1));
        let first = limiter.acquire().await.unwrap();
        let second = limiter.acquire().await.unwrap();

        assert!(matches!(limiter.acquire().await, Err(ExecError::QueueFull)));

        drop(first);
        drop(second);
        assert!(limiter.acquire().await.is_ok());
    }

    #[tokio::test]
    async fn execution_limiter_bounds_the_waiting_queue() {
        let limiter = ExecutionLimiter::new(1, 1, Duration::from_secs(1));
        let running = limiter.acquire().await.unwrap();
        let queued_limiter = limiter.clone();
        let queued = tokio::spawn(async move { queued_limiter.acquire().await });
        tokio::task::yield_now().await;

        assert!(matches!(limiter.acquire().await, Err(ExecError::QueueFull)));

        drop(running);
        assert!(queued.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn execution_limiter_times_out_queued_work() {
        let limiter = ExecutionLimiter::new(1, 1, Duration::from_millis(1));
        let _running = limiter.acquire().await.unwrap();

        assert!(matches!(limiter.acquire().await, Err(ExecError::Busy)));
        assert!(matches!(limiter.acquire().await, Err(ExecError::Busy)));
    }

    #[tokio::test]
    async fn execution_limiter_reclaims_cancelled_queue_admission() {
        let limiter = ExecutionLimiter::new(1, 1, Duration::from_millis(1));
        let _running = limiter.acquire().await.unwrap();
        let queued_limiter = limiter.clone();
        let queued = tokio::spawn(async move { queued_limiter.acquire().await });
        tokio::task::yield_now().await;
        queued.abort();
        let _ = queued.await;

        assert!(matches!(limiter.acquire().await, Err(ExecError::Busy)));
    }

    #[test]
    fn http_204_does_not_wait_for_peer_close() {
        let path = std::env::temp_dir().join(format!("grade-fc-http-{}.sock", std::process::id()));
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut ignore = [0u8; 512];
            let _ = s.read(&mut ignore);
            s.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
            s.flush().unwrap();
            let _ = s.read(&mut [0u8; 1]);
        });
        let mut client = UnixStream::connect(&path).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client.write_all(b"PATCH /vm HTTP/1.1\r\n\r\n").unwrap();
        client.flush().unwrap();
        let started = Instant::now();
        let text = read_http_response(&mut client).unwrap();
        let _ = fs::remove_file(&path);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "must not wait for the peer to close"
        );
        assert!(text.contains("204"));
        drop(server);
    }

    #[test]
    fn http_reads_content_length_body() {
        let raw = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 5\r\n\r\nbad!!";
        let text = read_http_response(&mut Cursor::new(&raw[..])).unwrap();
        assert!(text.ends_with("bad!!"));
    }

    #[test]
    fn test_language_registry_defaults_to_rust() {
        let registry = LanguageRegistry::from_env_or_file();
        let resolved = registry.resolve(None).expect("rust should resolve");
        assert_eq!(resolved.key, "rust");
        assert_eq!(resolved.source_file, "/tmp/job.rs");
        assert!(resolved.is_rust);
        assert!(resolved.compile_cmd.is_some());
    }

    #[test]
    fn test_language_registry_resolves_python() {
        let registry = LanguageRegistry::from_env_or_file();
        let resolved = registry
            .resolve(Some("python"))
            .expect("python should resolve");
        assert_eq!(resolved.key, "python");
        assert_eq!(resolved.source_file, "/tmp/job.py");
        assert!(!resolved.is_rust);
        assert!(resolved.compile_cmd.is_none());
        assert_eq!(resolved.run_cmd, vec!["python3", "/tmp/job.py"]);
    }

    #[test]
    fn test_all_nine_languages_resolve_and_expand_commands() {
        let registry = LanguageRegistry::from_env_or_file();

        let cases = [
            // (input_alias, expected_key, expected_src, has_compile, expected_run_bin)
            ("rust", "rust", "/tmp/job.rs", true, "/tmp/job"),
            ("rs", "rust", "/tmp/job.rs", true, "/tmp/job"),
            ("python", "python", "/tmp/job.py", false, "python3"),
            ("py", "python", "/tmp/job.py", false, "python3"),
            ("python3", "python", "/tmp/job.py", false, "python3"),
            ("typescript", "typescript", "/tmp/job.ts", true, "node"),
            ("ts", "typescript", "/tmp/job.ts", true, "node"),
            ("node", "node", "/tmp/job.js", false, "node"),
            ("js", "node", "/tmp/job.js", false, "node"),
            ("javascript", "node", "/tmp/job.js", false, "node"),
            ("nodejs", "node", "/tmp/job.js", false, "node"),
            ("go", "go", "/tmp/main.go", true, "/tmp/job"),
            ("golang", "go", "/tmp/main.go", true, "/tmp/job"),
            ("cpp", "cpp", "/tmp/job.cpp", true, "/tmp/job"),
            ("c++", "cpp", "/tmp/job.cpp", true, "/tmp/job"),
            ("cc", "cpp", "/tmp/job.cpp", true, "/tmp/job"),
            ("cxx", "cpp", "/tmp/job.cpp", true, "/tmp/job"),
            ("c", "c", "/tmp/job.c", true, "/tmp/job"),
            ("clang", "c", "/tmp/job.c", true, "/tmp/job"),
            ("gcc", "c", "/tmp/job.c", true, "/tmp/job"),
            ("java", "java", "/tmp/Solution.java", true, "java"),
            ("csharp", "csharp", "/tmp/Program.cs", true, "mono"),
            ("cs", "csharp", "/tmp/Program.cs", true, "mono"),
            ("c#", "csharp", "/tmp/Program.cs", true, "mono"),
            ("dotnet", "csharp", "/tmp/Program.cs", true, "mono"),
            ("zig", "zig", "/tmp/job.zig", true, "/tmp/job"),
        ];

        for (alias, expected_key, expected_src, has_compile, expected_run_bin) in cases {
            let resolved = registry
                .resolve(Some(alias))
                .unwrap_or_else(|| panic!("failed to resolve alias '{alias}'"));
            assert_eq!(resolved.key, expected_key, "alias '{alias}' key mismatch");
            assert_eq!(
                resolved.source_file, expected_src,
                "alias '{alias}' source mismatch"
            );
            assert_eq!(
                resolved.compile_cmd.is_some(),
                has_compile,
                "alias '{alias}' compile command presence mismatch"
            );
            assert_eq!(
                resolved.run_cmd[0], expected_run_bin,
                "alias '{alias}' run binary mismatch"
            );
        }
    }

    #[test]
    fn test_language_case_insensitivity_and_whitespace() {
        let registry = LanguageRegistry::from_env_or_file();
        let cases = [
            "  PYTHON  ",
            "Rust",
            "C++",
            "  gO  ",
            "Ts",
            "  c#  ",
            "NODEJS",
        ];
        for input in cases {
            assert!(
                registry.resolve(Some(input)).is_some(),
                "failed to resolve '{input}'"
            );
        }
    }

    #[test]
    fn test_unknown_language_returns_none() {
        let registry = LanguageRegistry::from_env_or_file();
        assert!(registry.resolve(Some("brainfuck")).is_none());
        assert!(registry.resolve(Some("unknown_lang")).is_none());
        assert!(registry.resolve(Some("nonexistent_language_xyz")).is_none());
    }

    #[test]
    fn cgroup_key_u64_parses_stat_and_events() {
        let cpu = "usage_usec 12345\nuser_usec 100\nsystem_usec 50\n";
        assert_eq!(cgroup_key_u64(cpu, "usage_usec"), Some(12345));
        let events = "low 0\nhigh 1\nmax 0\noom 2\noom_kill 3\n";
        assert_eq!(cgroup_key_u64(events, "oom_kill"), Some(3));
        assert_eq!(cgroup_key_u64(events, "missing"), None);
    }

    #[test]
    fn jail_cpu_max_follows_vcpu_or_override() {
        assert_eq!(jail_cpu_max_value(2, None), "200000 100000");
        assert_eq!(jail_cpu_max_value(1, None), "100000 100000");
        assert_eq!(jail_cpu_max_value(2, Some("50000 100000")), "50000 100000");
    }
}
