mod protocol;
mod result;
mod verdict;

pub use protocol::{JobRequest, JobResponse, read_frame, read_line_bytes, write_frame};
pub use result::HarnessResult;
pub use verdict::Verdict;
