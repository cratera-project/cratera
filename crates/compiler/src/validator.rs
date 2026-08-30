use thiserror::Error;

pub const MAX_CODE_SIZE: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Code exceeds maximum size of {MAX_CODE_SIZE} bytes (got {0} bytes)")]
    CodeTooLarge(usize),
    #[error("External crates are not allowed: found `{0}`")]
    ExternalCrate(String),
    #[error("Forbidden pattern detected: {0}")]
    ForbiddenPattern(String),
    #[error("Code cannot be empty")]
    EmptyCode,
}

pub struct CodeValidator;

impl CodeValidator {
    pub fn validate(code: &str) -> Result<(), ValidationError> {
        if code.trim().is_empty() {
            return Err(ValidationError::EmptyCode);
        }
        if code.len() > MAX_CODE_SIZE {
            return Err(ValidationError::CodeTooLarge(code.len()));
        }
        for line in code.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("extern crate ") {
                let name = rest
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("unknown");
                if name != "std" && name != "core" && name != "alloc" {
                    return Err(ValidationError::ExternalCrate(name.to_string()));
                }
            }
        }
        for (pattern, description) in [
            ("include_str!", "include_str!"),
            ("include_bytes!", "include_bytes!"),
            ("include!", "include!"),
            ("std::process", "process spawning"),
            ("Command::new", "process spawning"),
            ("#[proc_macro", "proc macro"),
            ("#[link(", "link attribute"),
        ] {
            if code.contains(pattern) {
                return Err(ValidationError::ForbiddenPattern(description.to_string()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_std() {
        assert!(
            CodeValidator::validate(
                "use std::collections::HashMap;\nfn add(a: i32, b: i32) -> i32 { a + b }"
            )
            .is_ok()
        );
    }

    #[test]
    fn valid_ffi_abi() {
        assert!(
            CodeValidator::validate(
                "pub extern \"C\" fn rust_add(a: i32, b: i32) -> i32 { a + b }"
            )
            .is_ok()
        );
        assert!(CodeValidator::validate("unsafe extern \"C\" { fn abs(x: i32) -> i32; }").is_ok());
    }

    #[test]
    fn empty_and_huge() {
        assert!(matches!(
            CodeValidator::validate(""),
            Err(ValidationError::EmptyCode)
        ));
        assert!(matches!(
            CodeValidator::validate(&"x".repeat(MAX_CODE_SIZE + 1)),
            Err(ValidationError::CodeTooLarge(_))
        ));
    }

    #[test]
    fn extern_crate() {
        assert!(matches!(
            CodeValidator::validate("extern crate malicious;"),
            Err(ValidationError::ExternalCrate(_))
        ));
        assert!(CodeValidator::validate("extern crate std;\nfn main() {}").is_ok());
    }

    #[test]
    fn forbidden_escape_attempts() {
        assert!(CodeValidator::validate("const X: &str = include_str!(\"a\");").is_err());
        assert!(CodeValidator::validate("const X: &[u8] = include_bytes!(\"a\");").is_err());
        assert!(CodeValidator::validate("include!(\"malicious.rs\");").is_err());
        assert!(
            CodeValidator::validate("fn main() { let _ = std::process::Command::new(\"echo\"); }")
                .is_err()
        );
        assert!(CodeValidator::validate("fn f() { std::process::exit(1); }").is_err());
        assert!(CodeValidator::validate("#[proc_macro]\nfn m() {}").is_err());
        assert!(CodeValidator::validate("#[link(name = \"c\")]\nextern {}").is_err());
    }
}
