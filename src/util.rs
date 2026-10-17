//! Small shared helpers.

/// Format epoch milliseconds in local time, `-` when absent.
pub fn fmt_ms_local(ms: Option<i64>) -> String {
    match ms.and_then(chrono::DateTime::from_timestamp_millis) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => "-".to_string(),
    }
}

/// Collapse CR/LF for single-line snippet display.
pub fn one_line(s: &str) -> String {
    s.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

/// Character-wise case-insensitive substring search.
pub fn find_ci(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    for start in 0..=(hay.len() - needle.len()) {
        let mut ok = true;
        for k in 0..needle.len() {
            let a = hay[start + k];
            let b = needle[k];
            if !(a == b || a.to_lowercase().eq(b.to_lowercase())) {
                ok = false;
                break;
            }
        }
        if ok {
            return Some(start);
        }
    }
    None
}

/// Build a one-line context snippet around the earliest occurrence of any of
/// `tokens` in `text`. `None` when no token occurs.
pub fn snippet_for(text: &str, tokens: &[String], context: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut best: Option<(usize, usize)> = None; // (byte-agnostic char pos, token len)
    for t in tokens {
        let tl: Vec<char> = t.chars().collect();
        if let Some(p) = find_ci(&chars, &tl) {
            match best {
                None => best = Some((p, tl.len())),
                Some((bp, _)) if p < bp => best = Some((p, tl.len())),
                _ => {}
            }
        }
    }
    let (pos, tlen) = best?;
    let start = pos.saturating_sub(context);
    let end = (pos + tlen + context).min(chars.len());
    let mut s: String = one_line(&chars[start..end].iter().collect::<String>());
    if start > 0 {
        s = format!("…{s}");
    }
    if end < chars.len() {
        s = format!("{s}…");
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_basic() {
        let text = "先讨论方案A，然后我们再看登录流程的细节，细节稍后再展开讨论。";
        let s = snippet_for(text, &["登录流程".to_string()], 4).unwrap();
        assert!(s.contains("登录流程"));
        assert!(s.starts_with('…'));
        assert!(s.ends_with('…'));
    }

    #[test]
    fn snippet_ci_ascii() {
        let s = snippet_for("Hello World", &["world".to_string()], 3).unwrap();
        assert!(s.contains("World") || s.contains("world"));
    }

    #[test]
    fn find_ci_bounds() {
        let hay: Vec<char> = "abc".chars().collect();
        let needle: Vec<char> = "abcd".chars().collect();
        assert_eq!(find_ci(&hay, &needle), None);
    }
}
