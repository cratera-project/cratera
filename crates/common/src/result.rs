use crate::{JobResponse, Verdict};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessResult {
    pub compilation_success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compilation_error: Option<String>,
    pub passed: bool,
    pub status: String,
    pub verdict: Verdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    pub execution_time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_kb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compile_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wall_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored: Option<bool>,
}

impl HarnessResult {
    pub fn from_job(job: JobResponse, _wall_ms: u64) -> Self {
        let compile_ms = job.compile_ms;
        let mut out = Self::from_job_inner(job);
        out.compile_ms = (compile_ms > 0).then_some(compile_ms);
        out
    }

    fn from_job_inner(job: JobResponse) -> Self {
        if !job.compilation_success {
            return Self::verdict(Verdict::CE, 0, None, Some(job.compile_stderr), None, None);
        }
        let mem = (job.run_rss_kb >= 32).then_some(job.run_rss_kb);
        let stdout = nonempty(job.stdout);
        let stderr = nonempty(job.stderr);

        if job.timed_out {
            return Self::verdict(Verdict::TLE, job.run_ms, mem, None, stdout, stderr);
        }
        if job.oom {
            return Self::verdict(Verdict::MLE, job.run_ms, mem, None, None, stderr);
        }
        if job.exit_code.unwrap_or(1) == 0 {
            return Self::verdict(Verdict::AC, job.run_ms, mem, None, stdout, stderr);
        }
        let is_assertion = stderr
            .as_deref()
            .is_some_and(|s| s.contains("assertion") || s.contains("assert"))
            || stdout
                .as_deref()
                .is_some_and(|s| s.contains("assertion") || s.contains("assert"));
        let verdict = if is_assertion {
            Verdict::WA
        } else {
            Verdict::RE
        };
        Self::verdict(verdict, job.run_ms, mem, None, stdout, stderr)
    }

    fn verdict(
        verdict: Verdict,
        time_us: u64,
        memory_kb: Option<u64>,
        compilation_error: Option<String>,
        stdout: Option<String>,
        stderr: Option<String>,
    ) -> Self {
        Self {
            compilation_success: verdict != Verdict::CE,
            compilation_error: compilation_error.filter(|s| !s.is_empty()),
            passed: verdict == Verdict::AC,
            status: verdict.status().to_string(),
            verdict,
            stdout,
            stderr,
            execution_time: time_us,
            memory_kb,
            compile_ms: None,
            copy_ms: None,
            boot_ms: None,
            wall_ms: None,
            restored: None,
        }
    }

    pub fn with_host_timings(
        mut self,
        compile_ms: u64,
        copy_ms: u64,
        boot_ms: u64,
        wall_ms: u64,
        restored: bool,
    ) -> Self {
        self.compile_ms = (compile_ms > 0).then_some(compile_ms);
        self.copy_ms = Some(copy_ms);
        self.boot_ms = Some(boot_ms);
        self.wall_ms = Some(wall_ms);
        self.restored = Some(restored);
        self
    }
}

fn nonempty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JobResponse;

    #[test]
    fn ce_from_compile_fail() {
        let r = HarnessResult::from_job(
            JobResponse {
                compilation_success: false,
                compile_stderr: "error: expected `;`".into(),
                ..Default::default()
            },
            10,
        );
        assert_eq!(r.verdict, Verdict::CE);
        assert!(!r.passed);
        assert_eq!(r.execution_time, 0);
        assert_eq!(r.memory_kb, None);
    }

    #[test]
    fn wa_from_assertion_panic() {
        let r = HarnessResult::from_job(
            JobResponse {
                compilation_success: true,
                exit_code: Some(101),
                stderr: "assertion `left == right` failed".into(),
                ..Default::default()
            },
            10,
        );
        assert_eq!(r.verdict, Verdict::WA);
    }

    #[test]
    fn re_from_other_crash() {
        let r = HarnessResult::from_job(
            JobResponse {
                compilation_success: true,
                exit_code: Some(1),
                stderr: "index out of bounds".into(),
                ..Default::default()
            },
            10,
        );
        assert_eq!(r.verdict, Verdict::RE);
    }

    #[test]
    fn ac_uses_guest_run_ms_and_rss_not_wall() {
        let r = HarnessResult::from_job(
            JobResponse {
                compilation_success: true,
                exit_code: Some(0),
                run_ms: 7,
                run_rss_kb: 400,
                ..Default::default()
            },
            999,
        );
        assert_eq!(r.verdict, Verdict::AC);
        assert_eq!(r.execution_time, 7);
        assert_eq!(r.memory_kb, Some(400));
    }

    #[test]
    fn one_page_rss_is_not_reported() {
        let r = HarnessResult::from_job(
            JobResponse {
                compilation_success: true,
                exit_code: Some(0),
                run_ms: 20,
                run_rss_kb: 4,
                ..Default::default()
            },
            20,
        );
        assert_eq!(r.verdict, Verdict::AC);
        assert_eq!(r.memory_kb, None);
    }
}
