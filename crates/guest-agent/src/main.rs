use cratera_common::{JobRequest, JobResponse, read_frame, write_frame};
use nix::mount::{MsFlags, mount};
use nix::sys::reboot::{RebootMode, reboot};
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use vsock::{VMADDR_CID_ANY, VsockAddr, VsockListener};

const VSOCK_PORT: u32 = 52;
const COMPILE_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_CAPTURE: usize = 64 * 1024;

fn main() {
    let _ = setup_fs();
    if let Err(e) = serve_once() {
        eprintln!("cratera-agent: {e}");
    }
    halt();
}

fn setup_fs() -> io::Result<()> {
    for dir in ["/proc", "/sys", "/dev", "/tmp", "/dev/shm", "/run"] {
        let _ = fs::create_dir_all(dir);
    }
    let _ = mount(
        Some("proc"),
        "/proc",
        Some("proc"),
        MsFlags::empty(),
        None::<&str>,
    );
    let _ = mount(
        Some("sysfs"),
        "/sys",
        Some("sysfs"),
        MsFlags::empty(),
        None::<&str>,
    );
    let _ = mount(
        Some("devtmpfs"),
        "/dev",
        Some("devtmpfs"),
        MsFlags::empty(),
        None::<&str>,
    );
    let _ = mount(
        Some("tmpfs"),
        "/dev/shm",
        Some("tmpfs"),
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
        Some("mode=1777,size=64m"),
    );
    let _ = mount(
        Some("tmpfs"),
        "/tmp",
        Some("tmpfs"),
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
        Some("mode=1777,size=256m"),
    );
    let _ = mount(
        Some("tmpfs"),
        "/root",
        Some("tmpfs"),
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
        Some("mode=0700,size=64m"),
    );
    Ok(())
}

fn serve_once() -> io::Result<()> {
    let listener = VsockListener::bind(&VsockAddr::new(VMADDR_CID_ANY, VSOCK_PORT))?;
    loop {
        let (mut stream, _) = listener.accept()?;
        let bytes = match read_frame(&mut stream) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let req: JobRequest = serde_json::from_slice(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let out = serde_json::to_vec(&execute(&req))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_frame(&mut stream, &out)?;
        let _ = stream.shutdown(std::net::Shutdown::Write);
        let mut dummy = [0u8; 1];
        let _ = stream.read(&mut dummy);
        return Ok(());
    }
}

fn execute(req: &JobRequest) -> JobResponse {
    let source_path = req.source_file.as_deref().unwrap_or("/tmp/job.rs");
    if let Err(e) = fs::write(source_path, &req.source) {
        return JobResponse {
            compile_stderr: format!("write source ({source_path}): {e}"),
            ..Default::default()
        };
    }

    let default_compile = vec![
        "rustc".to_string(),
        "--edition".to_string(),
        "2024".to_string(),
        "-C".to_string(),
        "panic=abort".to_string(),
        "-C".to_string(),
        "opt-level=2".to_string(),
        "-C".to_string(),
        "link-arg=-fno-use-linker-plugin".to_string(),
        "-o".to_string(),
        "/tmp/job".to_string(),
        source_path.to_string(),
    ];

    let compile_cmd = match &req.compile_cmd {
        Some(cmd) if cmd.is_empty() => None,
        Some(cmd) => Some(cmd.as_slice()),
        None if req.source_file.is_none() => Some(default_compile.as_slice()),
        None => None,
    };

    let mut compile_ms = 0;
    if let Some(cmd_tokens) = compile_cmd
        && !cmd_tokens.is_empty()
    {
        let t0 = Instant::now();
        let compile = run_cmd(build_command(cmd_tokens), COMPILE_TIMEOUT, false);
        compile_ms = t0.elapsed().as_millis() as u64;

        match compile {
            CmdOut::Failed {
                stderr, timed_out, ..
            } => {
                return JobResponse {
                    compile_stderr: cap(&stderr),
                    timed_out,
                    compile_ms,
                    ..Default::default()
                };
            }
            CmdOut::Done { status, stderr, .. } if !status.success() => {
                return JobResponse {
                    compile_stderr: cap(&stderr),
                    oom: status.signal() == Some(9),
                    compile_ms,
                    ..Default::default()
                };
            }
            CmdOut::Done { .. } => {}
        }
    }

    let default_run = vec!["/tmp/job".to_string()];
    let run_cmd_tokens = req.run_cmd.as_deref().unwrap_or(default_run.as_slice());
    let run_cmd_obj = build_command(run_cmd_tokens);

    let run = run_cmd(
        run_cmd_obj,
        Duration::from_millis(req.timeout_ms.max(1)),
        true,
    );

    match run {
        CmdOut::Failed {
            stderr,
            timed_out,
            memory_kb,
            elapsed_us,
        } => JobResponse {
            compilation_success: true,
            stderr: cap(&stderr),
            timed_out,
            compile_ms,
            run_ms: elapsed_us,
            run_rss_kb: memory_kb,
            ..Default::default()
        },
        CmdOut::Done {
            status,
            stdout,
            stderr,
            memory_kb,
            elapsed_us,
        } => {
            let sig = status.signal();
            JobResponse {
                compilation_success: true,
                exit_code: status.code().or(sig),
                stdout: cap(&stdout),
                stderr: cap(&stderr),
                oom: sig == Some(9),
                compile_ms,
                run_ms: elapsed_us,
                run_rss_kb: memory_kb,
                ..Default::default()
            }
        }
    }
}

fn build_command(tokens: &[String]) -> Command {
    let mut cmd = Command::new(tokens.first().map(|s| s.as_str()).unwrap_or("/tmp/job"));
    if tokens.len() > 1 {
        cmd.args(&tokens[1..]);
    }
    cmd
}

enum CmdOut {
    Done {
        status: std::process::ExitStatus,
        stdout: String,
        stderr: String,
        memory_kb: u64,
        elapsed_us: u64,
    },
    Failed {
        stderr: String,
        timed_out: bool,
        memory_kb: u64,
        elapsed_us: u64,
    },
}

fn parse_status_kb(status: &str, key: &str) -> u64 {
    for line in status.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name != key {
            continue;
        }
        return rest
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
    }
    0
}

fn live_rss_kb(pid: u32) -> u64 {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .map(|t| parse_status_kb(&t, "RssAnon"))
        .unwrap_or(0)
}

fn measured_rss_kb(kb: u64) -> u64 {
    if kb < 32 { 0 } else { kb }
}

fn wait4_pid(pid: u32) -> io::Result<(ExitStatus, u64)> {
    unsafe {
        let mut status: libc::c_int = 0;
        let mut usage: libc::rusage = std::mem::zeroed();
        loop {
            let r = libc::wait4(pid as libc::pid_t, &mut status, 0, &mut usage);
            if r < 0 {
                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return Err(err);
            }
            return Ok((ExitStatus::from_raw(status), usage.ru_maxrss.max(0) as u64));
        }
    }
}

fn pidfd_open(pid: u32) -> Option<i32> {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0i32) };
    if fd < 0 { None } else { Some(fd as i32) }
}

fn pidfd_kill(fd: i32, sig: i32) {
    let _ = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd,
            sig,
            std::ptr::null::<libc::c_void>(),
            0i32,
        )
    };
}

fn run_cmd(mut cmd: Command, timeout: Duration, sample_rss: bool) -> CmdOut {
    let stdout_path = Path::new("/tmp/job.stdout");
    let stderr_path = Path::new("/tmp/job.stderr");
    let failed =
        |stderr: String, timed_out: bool, memory_kb: u64, elapsed_us: u64| CmdOut::Failed {
            stderr,
            timed_out,
            memory_kb,
            elapsed_us,
        };
    let stdout_file = match File::create(stdout_path) {
        Ok(f) => f,
        Err(e) => return failed(e.to_string(), false, 0, 0),
    };
    let stderr_file = match File::create(stderr_path) {
        Ok(f) => f,
        Err(e) => return failed(e.to_string(), false, 0, 0),
    };

    let mut child = match cmd
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", "/tmp")
        .env("TMPDIR", "/tmp")
        .env("XDG_CACHE_HOME", "/tmp/.cache")
        .env("XDG_CONFIG_HOME", "/tmp/.config")
        .env("XDG_DATA_HOME", "/tmp/.local/share")
        .current_dir("/tmp")
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return failed(e.to_string(), false, 0, 0),
    };
    let pid = child.id();
    let start = Instant::now();
    let pidfd = pidfd_open(pid);
    let finished = Arc::new(AtomicBool::new(false));
    let timed_out = Arc::new(AtomicBool::new(false));
    let peak = Arc::new(AtomicU64::new(live_rss_kb(pid)));

    {
        let finished = Arc::clone(&finished);
        let timed_out = Arc::clone(&timed_out);
        thread::spawn(move || {
            thread::sleep(timeout);
            if finished.load(Ordering::SeqCst) {
                return;
            }
            timed_out.store(true, Ordering::SeqCst);
            if let Some(fd) = pidfd {
                pidfd_kill(fd, libc::SIGKILL);
            } else {
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
            }
        });
    }

    if sample_rss {
        let finished = Arc::clone(&finished);
        let peak = Arc::clone(&peak);
        thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                peak.fetch_max(live_rss_kb(pid), Ordering::Relaxed);
                thread::sleep(Duration::from_micros(200));
            }
        });
    }

    let waited = wait4_pid(pid);
    finished.store(true, Ordering::SeqCst);
    let elapsed_us = start.elapsed().as_micros() as u64;

    if let Some(fd) = pidfd {
        unsafe {
            libc::close(fd);
        }
    }

    match waited {
        Ok((status, _rss)) => {
            std::mem::forget(child);
            let memory_kb = if sample_rss {
                measured_rss_kb(peak.load(Ordering::Relaxed))
            } else {
                0
            };
            if timed_out.load(Ordering::SeqCst) {
                return failed(read_cap(stderr_path), true, memory_kb, elapsed_us);
            }
            CmdOut::Done {
                status,
                stdout: read_cap(stdout_path),
                stderr: read_cap(stderr_path),
                memory_kb,
                elapsed_us,
            }
        }
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            failed(e.to_string(), false, 0, elapsed_us)
        }
    }
}

fn read_cap(path: &Path) -> String {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut buf = vec![0u8; MAX_CAPTURE];
    let n = f.read(&mut buf).unwrap_or(0);
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

fn cap(s: &str) -> String {
    let n = s.len().min(MAX_CAPTURE);
    String::from_utf8_lossy(&s.as_bytes()[..n]).into_owned()
}

fn halt() -> ! {
    nix::unistd::sync();
    std::thread::sleep(Duration::from_millis(50));
    let _ = reboot(RebootMode::RB_POWER_OFF);
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::{measured_rss_kb, parse_status_kb};

    #[test]
    fn parses_rss_anon_not_file_rss() {
        let status = "Name:\tjob\nVmRSS:\t  2160 kB\nVmHWM:\t  2160 kB\nRssAnon:\t   192 kB\nRssFile:\t  1968 kB\n";
        assert_eq!(parse_status_kb(status, "RssAnon"), 192);
        assert_eq!(parse_status_kb(status, "VmHWM"), 2160);
    }

    #[test]
    fn drops_one_page_rss() {
        assert_eq!(measured_rss_kb(4), 0);
        assert_eq!(measured_rss_kb(31), 0);
        assert_eq!(measured_rss_kb(32), 32);
        assert_eq!(measured_rss_kb(400), 400);
    }
}
