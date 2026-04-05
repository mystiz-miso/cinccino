//! Trivia extraction for Circom source files.
//!
//! "Trivia" refers to non-semantic tokens like comments and whitespace.
//! The lexer (`logos`) skips comments entirely, so we extract them from
//! the raw source text in a separate pass.

use crate::span::Span;

/// A comment extracted from source text.
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    /// Byte-offset span in the original source.
    pub span: Span,
    /// The comment text (including delimiters: `//` or `/* */`).
    pub text: String,
    /// Line or block comment.
    pub kind: CommentKind,
}

/// Whether a comment is a line comment (`//`) or block comment (`/* */`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentKind {
    /// `// ...` (up to end of line)
    Line,
    /// `/* ... */`
    Block,
}

/// Extract all comments from Circom source text.
///
/// Skips comment-like sequences inside double-quoted string literals.
/// Returns comments sorted by their start position.
pub fn extract_comments(source: &str) -> Vec<Comment> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut comments = Vec::new();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            // Skip string literals
            b'"' => {
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' {
                        // Skip escaped character; guard against unterminated
                        // escape at end of input.
                        i += if i + 1 < len { 2 } else { 1 };
                        continue;
                    }
                    if bytes[i] == b'"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            // Potential comment start
            b'/' if i + 1 < len => {
                if bytes[i + 1] == b'/' {
                    // Line comment
                    let start = i;
                    i += 2;
                    while i < len && bytes[i] != b'\n' {
                        i += 1;
                    }
                    let text = source[start..i].to_string();
                    comments.push(Comment {
                        span: Span::new(start, i),
                        text,
                        kind: CommentKind::Line,
                    });
                } else if bytes[i + 1] == b'*' {
                    // Block comment
                    let start = i;
                    i += 2;
                    let mut terminated = false;
                    while i + 1 < len {
                        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                            i += 2;
                            terminated = true;
                            break;
                        }
                        i += 1;
                    }
                    // For unterminated block comments, include all remaining
                    // bytes so the last byte is not truncated.
                    if !terminated {
                        i = len;
                    }
                    let text = source[start..i].to_string();
                    comments.push(Comment {
                        span: Span::new(start, i),
                        text,
                        kind: CommentKind::Block,
                    });
                } else {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    comments
}

/// Determine the line number (0-based) for a byte offset.
pub(crate) fn line_of(source: &str, offset: usize) -> usize {
    debug_assert!(
        offset <= source.len(),
        "line_of: offset {offset} exceeds source length {}",
        source.len()
    );
    source[..offset.min(source.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Classify each comment as leading (before a node) or trailing (after a
    /// node on the same line) and return them grouped by position.
    fn classify_comments(source: &str, comments: &[Comment]) -> CommentMap {
        let mut map = CommentMap::new();

        for comment in comments {
            let comment_line = line_of(source, comment.span.start);
            let line_start = source[..comment.span.start]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(0);
            let before = &source[line_start..comment.span.start];
            let is_trailing = before.chars().any(|c| !c.is_whitespace());

            if is_trailing {
                map.trailing.push((comment_line, comment.clone()));
            } else {
                map.leading.push((comment_line, comment.clone()));
            }
        }

        map
    }

    /// Storage for classified comments.
    #[derive(Debug, Clone)]
    struct CommentMap {
        leading: Vec<(usize, Comment)>,
        trailing: Vec<(usize, Comment)>,
    }

    impl CommentMap {
        fn new() -> Self {
            Self {
                leading: Vec::new(),
                trailing: Vec::new(),
            }
        }
    }

    #[test]
    fn extract_line_comment() {
        let src = "// hello\n";
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "// hello");
        assert_eq!(comments[0].kind, CommentKind::Line);
        assert_eq!(comments[0].span, Span::new(0, 8));
    }

    #[test]
    fn extract_block_comment() {
        let src = "/* hello */";
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "/* hello */");
        assert_eq!(comments[0].kind, CommentKind::Block);
    }

    #[test]
    fn skip_comments_in_strings() {
        let src = r#""// not a comment" // real comment"#;
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "// real comment");
    }

    #[test]
    fn multiple_comments() {
        let src = "// first\nvar x = 1; // second\n/* third */\n";
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 3);
        assert_eq!(comments[0].text, "// first");
        assert_eq!(comments[1].text, "// second");
        assert_eq!(comments[2].text, "/* third */");
    }

    #[test]
    fn classify_leading_vs_trailing() {
        let src = "// leading\nvar x = 1; // trailing\n";
        let comments = extract_comments(src);
        let map = classify_comments(src, &comments);
        assert_eq!(map.leading.len(), 1);
        assert_eq!(map.leading[0].1.text, "// leading");
        assert_eq!(map.trailing.len(), 1);
        assert_eq!(map.trailing[0].1.text, "// trailing");
    }

    #[test]
    fn block_comment_multiline() {
        let src = "/* line1\n   line2 */\n";
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].kind, CommentKind::Block);
        assert_eq!(comments[0].text, "/* line1\n   line2 */");
    }

    #[test]
    fn no_comments() {
        let src = "var x = 1;\n";
        let comments = extract_comments(src);
        assert!(comments.is_empty());
    }

    #[test]
    fn escaped_quote_in_string() {
        let src = r#""escaped \" // not comment" // real"#;
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "// real");
    }

    #[test]
    fn unterminated_block_comment_preserves_last_byte() {
        let src = "/* abc";
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "/* abc");
        assert_eq!(comments[0].span, Span::new(0, 6));
    }

    #[test]
    fn unterminated_string_swallows_trailing_comments() {
        // An unterminated string `"hello // world` has no closing quote,
        // so the scanner consumes to EOF.  Any trailing comment-like
        // text inside the string is NOT extracted as a comment.
        // This documents the current behavior.
        let src = r#""hello // world"#;
        let comments = extract_comments(src);
        assert!(
            comments.is_empty(),
            "unterminated string should swallow everything to EOF; got: {comments:?}"
        );
    }

    #[test]
    fn unterminated_string_followed_by_comment_on_next_line() {
        // If the unterminated string ends at EOF, nothing after it is seen.
        let src = "\"hello\n// real comment";
        let comments = extract_comments(src);
        // The scanner sees `"hello\n// real comment` as one unterminated
        // string — every byte after the opening `"` is consumed because
        // there is no closing `"`.  So no comments are extracted.
        assert!(
            comments.is_empty(),
            "unterminated string consumes to EOF; got: {comments:?}"
        );
    }
}
