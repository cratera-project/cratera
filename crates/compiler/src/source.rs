pub const RUN_TEST_LIMIT: usize = 3;

pub fn splice_harness(harness: &str, solution: &str) -> Result<String, SpliceError> {
    if harness.trim().is_empty() {
        return Ok(solution.to_string());
    }
    let n = harness.matches("{{SOLUTION}}").count();
    if n == 0 {
        return Err(SpliceError::MissingMarker);
    }
    if n > 1 {
        return Err(SpliceError::MultipleMarkers);
    }
    Ok(harness.replacen("{{SOLUTION}}", solution.trim_end(), 1))
}

pub fn limit_main_tests(source: &str, max_tests: usize) -> Result<String, SpliceError> {
    if max_tests == 0 {
        return Ok(source.to_string());
    }
    let Some((open, close)) = find_main_braces(source) else {
        return Ok(source.to_string());
    };
    let body = &source[open + 1..close];
    let Some(cut) = cut_after_n_tests(body, max_tests) else {
        return Ok(source.to_string());
    };
    Ok(format!(
        "{}{}\n}}{}",
        &source[..=open],
        body[..cut].trim_end(),
        &source[close + 1..]
    ))
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SpliceError {
    #[error("harness must contain {{{{SOLUTION}}}}")]
    MissingMarker,
    #[error("harness must contain exactly one {{{{SOLUTION}}}} marker")]
    MultipleMarkers,
    #[error("harness fn main() could not be parsed")]
    NoMain,
}

fn find_main_braces(src: &str) -> Option<(usize, usize)> {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut found = None;
    while i + 7 <= bytes.len() {
        if bytes[i..].starts_with(b"fn main") {
            let after = i + 7;
            if after < bytes.len() {
                let c = bytes[after];
                if c.is_ascii_alphanumeric() || c == b'_' {
                    i += 1;
                    continue;
                }
            }
            let mut j = match skip_ws_and_comments(bytes, after) {
                Some(j) => j,
                None => {
                    i += 1;
                    continue;
                }
            };
            if bytes.get(j) != Some(&b'(') {
                i += 1;
                continue;
            }
            j = match skip_balanced(bytes, j, b'(', b')') {
                Some(j) => j,
                None => {
                    i += 1;
                    continue;
                }
            };
            j = match skip_ws_and_comments(bytes, j) {
                Some(j) => j,
                None => {
                    i += 1;
                    continue;
                }
            };
            if bytes.get(j) != Some(&b'{') {
                i += 1;
                continue;
            }
            if let Some(close) = skip_balanced(bytes, j, b'{', b'}') {
                found = Some((j, close - 1));
            }
        }
        i += 1;
    }
    found
}

fn cut_after_n_tests(body: &str, max_tests: usize) -> Option<usize> {
    let bytes = body.as_bytes();
    let mut i = 0;
    let mut tests = 0;
    let mut last = 0;
    while i < bytes.len() {
        i = skip_ws_and_comments(bytes, i)?;
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'}' {
            break;
        }
        if is_ident_at(bytes, i, b"assert") && matches_assert_macro(bytes, i) {
            i = skip_macro_call(bytes, i)?;
            i = skip_optional_semi(bytes, i);
            tests += 1;
            last = i;
            if tests >= max_tests {
                return Some(last);
            }
            continue;
        }
        if bytes[i] == b'{' {
            i = skip_balanced(bytes, i, b'{', b'}')?;
            i = skip_optional_semi(bytes, i);
            tests += 1;
            last = i;
            if tests >= max_tests {
                return Some(last);
            }
            continue;
        }
        if bytes[i] == b'(' {
            i = skip_balanced(bytes, i, b'(', b')')?;
        } else if bytes[i] == b'[' {
            i = skip_balanced(bytes, i, b'[', b']')?;
        } else {
            i += 1;
        }
    }
    if tests == 0 { None } else { Some(last) }
}

fn skip_optional_semi(bytes: &[u8], i: usize) -> usize {
    let Some(j) = skip_ws_and_comments(bytes, i) else {
        return i;
    };
    if bytes.get(j) == Some(&b';') {
        j + 1
    } else {
        j
    }
}

fn matches_assert_macro(bytes: &[u8], i: usize) -> bool {
    for name in [
        b"assert_eq!".as_slice(),
        b"assert_ne!".as_slice(),
        b"assert!".as_slice(),
    ] {
        if bytes[i..].starts_with(name) {
            return true;
        }
    }
    false
}

fn is_ident_at(bytes: &[u8], i: usize, prefix: &[u8]) -> bool {
    bytes[i..].starts_with(prefix)
}

fn skip_macro_call(bytes: &[u8], i: usize) -> Option<usize> {
    let bang = bytes[i..].iter().position(|&b| b == b'!')?;
    let j = skip_ws_and_comments(bytes, i + bang + 1)?;
    if bytes.get(j) != Some(&b'(') {
        return Some(j);
    }
    skip_balanced(bytes, j, b'(', b')')
}

fn skip_ws_and_comments(bytes: &[u8], mut i: usize) -> Option<usize> {
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = i.saturating_add(2);
            continue;
        }
        return Some(i.min(bytes.len()));
    }
}

fn skip_balanced(bytes: &[u8], start: usize, open: u8, close: u8) -> Option<usize> {
    if bytes.get(start) != Some(&open) {
        return None;
    }
    let mut i = start + 1;
    let mut depth = 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            i = skip_string(bytes, i)?;
            continue;
        }
        if b == b'\'' {
            i = skip_char(bytes, i);
            continue;
        }
        if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

fn skip_string(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn skip_char(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    if i < bytes.len() && bytes[i] == b'\\' {
        i += 2;
    } else {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'\'' {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_ok() {
        let out = splice_harness("{{SOLUTION}}\n\nfn main() {}", "fn add() {}").unwrap();
        assert!(out.starts_with("fn add() {}"));
        assert!(!out.contains("{{SOLUTION}}"));
    }

    #[test]
    fn splice_empty() {
        assert_eq!(splice_harness("", "print('hi')").unwrap(), "print('hi')");
        assert_eq!(splice_harness("   ", "print('hi')").unwrap(), "print('hi')");
    }

    #[test]
    fn splice_missing() {
        assert_eq!(
            splice_harness("fn main() {}", "x"),
            Err(SpliceError::MissingMarker)
        );
    }

    #[test]
    fn weekly_style_keeps_first_three_asserts() {
        let src = r#"
fn add(a: i32, b: i32) -> i32 { a + b }
fn main() {
    assert_eq!(add(1, 2), 3);
    assert_eq!(add(0, 0), 0);
    assert_eq!(add(-1, 1), 0);
    assert_eq!(add(10, 10), 20);
    println!("all tests passed");
}
"#;
        let out = limit_main_tests(src, 3).unwrap();
        assert!(out.contains("assert_eq!(add(-1, 1), 0)"));
        assert!(!out.contains("assert_eq!(add(10, 10), 20)"));
        assert!(!out.contains("all tests passed"));
        assert!(out.contains("fn add"));
    }

    #[test]
    fn block_style_keeps_first_three_blocks() {
        let src = r#"
fn main() {
    { assert_eq!(1, 1); }
    { assert_eq!(2, 2); }
    { assert_eq!(3, 3); }
    { assert_eq!(4, 4); }
}
"#;
        let out = limit_main_tests(src, 3).unwrap();
        assert!(out.contains("assert_eq!(3, 3)"));
        assert!(!out.contains("assert_eq!(4, 4)"));
    }

    #[test]
    fn fewer_than_limit_keeps_all() {
        let src = "fn main() { assert_eq!(1, 1); }\n";
        let out = limit_main_tests(src, 3).unwrap();
        assert!(out.contains("assert_eq!(1, 1)"));
    }

    #[test]
    fn no_tests_preserves_source() {
        let src = "fn main() { println!(\"hi\"); }\n";
        assert_eq!(limit_main_tests(src, 3).unwrap(), src);
    }

    #[test]
    fn multibyte_utf8_does_not_panic() {
        let src = r#"
// こんにちは世界 - non-ASCII comment before main
fn main() {
    let s = "こんにちは世界";
    assert_eq!(s.len(), 21);
    assert_eq!(1, 1);
    assert_eq!(2, 2);
    assert_eq!(3, 3);
}
"#;
        let out = limit_main_tests(src, 3).unwrap();
        assert!(out.contains("assert_eq!(s.len(), 21)"));
        assert!(out.contains("assert_eq!(2, 2)"));
        assert!(!out.contains("assert_eq!(3, 3)"));
    }
}
