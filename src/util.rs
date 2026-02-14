/// Truncate a string to `max_chars` characters, appending "..." if truncated.
///
/// Unlike byte-index slicing (`&s[..n]`), this is safe for multi-byte UTF-8
/// strings and will never panic on non-ASCII input.
pub fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => format!("{}...", &s[..idx]),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_no_truncation() {
        assert_eq!(truncate_with_ellipsis("hello", 10), "hello");
    }

    #[test]
    fn ascii_exact_boundary() {
        assert_eq!(truncate_with_ellipsis("hello", 5), "hello");
    }

    #[test]
    fn ascii_truncated() {
        assert_eq!(truncate_with_ellipsis("hello world", 5), "hello...");
    }

    #[test]
    fn empty_string() {
        assert_eq!(truncate_with_ellipsis("", 10), "");
    }

    #[test]
    fn emoji_safe_truncation() {
        // Each emoji is 4 bytes; byte-index slicing would panic
        let s = "\u{1F600}\u{1F601}\u{1F602}\u{1F603}\u{1F604}"; // 5 emoji
        let result = truncate_with_ellipsis(s, 3);
        assert_eq!(result, "\u{1F600}\u{1F601}\u{1F602}...");
    }

    #[test]
    fn cjk_safe_truncation() {
        // Each CJK character is 3 bytes
        let s = "\u{4F60}\u{597D}\u{4E16}\u{754C}"; // 4 chars
        let result = truncate_with_ellipsis(s, 2);
        assert_eq!(result, "\u{4F60}\u{597D}...");
    }

    #[test]
    fn mixed_ascii_and_multibyte() {
        let s = "hi \u{1F600} world";
        let result = truncate_with_ellipsis(s, 4);
        assert_eq!(result, "hi \u{1F600}...");
    }

    #[test]
    fn zero_max_chars() {
        assert_eq!(truncate_with_ellipsis("hello", 0), "...");
    }
}
