use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImageHashError {
    #[error("missing checksum file {0}")]
    MissingSidecar(PathBuf),
    #[error("invalid checksum file {0}")]
    InvalidSidecar(PathBuf),
    #[error("checksum mismatch for {0}")]
    Mismatch(PathBuf),
    #[error("{0}")]
    Io(String),
}

pub fn sidecar_path(image: &Path) -> PathBuf {
    let mut name = image.as_os_str().to_os_string();
    name.push(".sha256");
    PathBuf::from(name)
}

pub fn parse_sha256_text(text: &str) -> Option<[u8; 32]> {
    let token = text.split_whitespace().next()?;
    if token.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&token[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn sha256_file(path: &Path) -> Result<[u8; 32], ImageHashError> {
    let mut file = File::open(path).map_err(|e| ImageHashError::Io(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| ImageHashError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

pub fn verify_image(path: &Path) -> Result<(), ImageHashError> {
    let sidecar = sidecar_path(path);
    let text = std::fs::read_to_string(&sidecar)
        .map_err(|_| ImageHashError::MissingSidecar(sidecar.clone()))?;
    let expected = parse_sha256_text(&text).ok_or(ImageHashError::InvalidSidecar(sidecar))?;
    let actual = sha256_file(path)?;
    if expected != actual {
        return Err(ImageHashError::Mismatch(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_plain_and_gnu_lines() {
        let hex = "a".repeat(64);
        assert!(parse_sha256_text(&hex).is_some());
        assert!(parse_sha256_text(&format!("{hex}  vmlinux.bin")).is_some());
        assert!(parse_sha256_text("abcd").is_none());
        assert!(parse_sha256_text("").is_none());
    }

    #[test]
    fn verify_matches_and_detects_mismatch() {
        let dir = std::env::temp_dir().join(format!("cratera-hash-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let image = dir.join("rootfs.ext4");
        let mut f = File::create(&image).unwrap();
        f.write_all(b"cratera-image").unwrap();
        drop(f);
        let digest = sha256_file(&image).unwrap();
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        std::fs::write(sidecar_path(&image), format!("{hex}\n")).unwrap();
        assert!(verify_image(&image).is_ok());
        std::fs::write(sidecar_path(&image), format!("{}\n", "0".repeat(64))).unwrap();
        assert!(matches!(
            verify_image(&image),
            Err(ImageHashError::Mismatch(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
