//! Small text and path helpers.

use std::path::{Component, Path, PathBuf};

pub fn len(s: &str) -> usize {
    s.chars().count()
}

/// Pad or cut a string to exactly `w` characters.
pub fn fit(s: &str, w: usize) -> String {
    if w == 0 {
        return String::new();
    }
    let n = len(s);
    if n <= w {
        let mut r = s.to_string();
        r.extend(std::iter::repeat_n(' ', w - n));
        r
    } else {
        let mut r: String = s.chars().take(w - 1).collect();
        r.push('»');
        r
    }
}

/// Cut from the left (for paths), keeping the tail visible.
pub fn fit_left(s: &str, w: usize) -> String {
    if w == 0 {
        return String::new();
    }
    let n = len(s);
    if n <= w {
        return s.to_string();
    }
    let mut r = String::from("…");
    r.extend(s.chars().skip(n - (w - 1)));
    r
}

pub fn center(s: &str, w: usize) -> String {
    let n = len(s);
    if n >= w {
        return fit(s, w);
    }
    let l = (w - n) / 2;
    format!("{}{}{}", " ".repeat(l), s, " ".repeat(w - n - l))
}

pub fn pad_left(s: &str, w: usize) -> String {
    let n = len(s);
    if n >= w { s.to_string() } else { format!("{}{}", " ".repeat(w - n), s) }
}

/// 1234567 -> "1,234,567"
pub fn n0(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub fn human(n: u64) -> String {
    const U: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        return format!("{n}B");
    }
    let s = format!("{v:.1}");
    format!("{}{}", s.trim_end_matches(".0"), U[i])
}

/// Size for a fixed-width column: 1,234,567 or 1,205K or 12,345M.
pub fn size_col(n: u64, w: usize) -> String {
    let s = n0(n);
    if len(&s) <= w {
        return s;
    }
    for (shift, unit) in [(10, "K"), (20, "M"), (30, "G")] {
        let s = format!("{}{}", n0(n >> shift), unit);
        if len(&s) <= w || unit == "G" {
            return s;
        }
    }
    unreachable!()
}

/// Case-insensitive wildcard match with `*` and `?`.
pub fn wildcard(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let s: Vec<char> = name.to_lowercase().chars().collect();
    let (mut pi, mut si) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Make a path absolute and resolve `.` and `..` without touching the disk.
pub fn normalize(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if out.parent().is_some() {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn key(p: &Path) -> String {
    let s = normalize(p).to_string_lossy().trim_end_matches(['\\', '/']).to_string();
    if cfg!(windows) { s.to_lowercase() } else { s }
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    key(a) == key(b)
}

/// True if `child` is inside `parent`.
pub fn is_inside(child: &Path, parent: &Path) -> bool {
    let p = key(parent) + std::path::MAIN_SEPARATOR_STR;
    key(child).starts_with(&p)
}

pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_works() {
        assert!(wildcard("*.txt", "Readme.TXT"));
        assert!(wildcard("a?c*", "abcdef"));
        assert!(wildcard("*", "anything"));
        assert!(!wildcard("*.txt", "readme.md"));
    }

    #[test]
    fn numbers() {
        assert_eq!(n0(1234567), "1,234,567");
        assert_eq!(n0(12), "12");
        assert_eq!(size_col(123_456_789_012, 10), "117,737M");
        assert_eq!(base64(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn paths() {
        let base = std::env::temp_dir();
        assert!(is_inside(&base.join("a").join("b"), &base.join("a")));
        assert!(!is_inside(&base.join("ab"), &base.join("a")));
        assert!(same_path(&base.join("a").join(".."), &base));
    }
}
