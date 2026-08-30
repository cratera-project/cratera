use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

const MAX_FRAME: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub source: String,
    pub timeout_ms: u64,
    #[serde(default)]
    pub source_file: Option<String>,
    #[serde(default)]
    pub compile_cmd: Option<Vec<String>>,
    #[serde(default)]
    pub run_cmd: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobResponse {
    pub compilation_success: bool,
    #[serde(default)]
    pub compile_stderr: String,
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default)]
    pub oom: bool,
    pub compile_ms: u64,
    pub run_ms: u64,
    #[serde(default)]
    pub run_rss_kb: u64,
}

pub fn write_frame<W: Write>(writer: &mut W, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame too large",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()
}

pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn read_line_bytes<R: Read>(reader: &mut R) -> io::Result<String> {
    let mut buf = Vec::new();
    let mut b = [0u8; 1];
    loop {
        reader.read_exact(&mut b)?;
        if b[0] == b'\n' {
            break;
        }
        buf.push(b[0]);
        if buf.len() > 4096 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_roundtrip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"hello").unwrap();
        let mut cur = Cursor::new(buf);
        assert_eq!(read_frame(&mut cur).unwrap(), b"hello");
    }

    #[test]
    fn read_line_stops_at_newline() {
        let mut cur = Cursor::new(b"OK 3\nrest");
        assert_eq!(read_line_bytes(&mut cur).unwrap(), "OK 3");
        let mut rest = Vec::new();
        cur.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"rest");
    }
}
