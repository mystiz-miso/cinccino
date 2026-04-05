/// Byte-offset span within a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Create a span covering `self` through `other`.
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Dummy span for synthesized nodes.
    pub fn dummy() -> Self {
        Self {
            start: usize::MAX,
            end: usize::MAX,
        }
    }
}

impl Default for Span {
    fn default() -> Self {
        Self::dummy()
    }
}

/// A line/column position in a source file (both 0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

/// Pre-computed index for converting byte offsets to line/column positions.
///
/// Build once from source text, then call `line_col()` for any byte offset.
pub struct LineIndex {
    /// Byte offset of the start of each line.
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build a `LineIndex` from the full source text.
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts }
    }

    /// Convert a byte offset to a 0-based line/column position.
    ///
    /// Returns `None` if `offset` is beyond the source length.
    pub fn line_col(&self, offset: usize) -> Option<LineCol> {
        if self.line_starts.is_empty() {
            return None;
        }
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next_line) => next_line.saturating_sub(1),
        };
        if line >= self.line_starts.len() {
            return None;
        }
        let col = offset - self.line_starts[line];
        Some(LineCol {
            line: line as u32,
            col: col as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_index_single_line() {
        let idx = LineIndex::new("hello");
        assert_eq!(idx.line_col(0), Some(LineCol { line: 0, col: 0 }));
        assert_eq!(idx.line_col(4), Some(LineCol { line: 0, col: 4 }));
    }

    #[test]
    fn test_line_index_multi_line() {
        let src = "abc\ndef\nghi";
        let idx = LineIndex::new(src);
        // 'a' at offset 0 -> line 0, col 0
        assert_eq!(idx.line_col(0), Some(LineCol { line: 0, col: 0 }));
        // 'd' at offset 4 -> line 1, col 0
        assert_eq!(idx.line_col(4), Some(LineCol { line: 1, col: 0 }));
        // 'g' at offset 8 -> line 2, col 0
        assert_eq!(idx.line_col(8), Some(LineCol { line: 2, col: 0 }));
        // 'h' at offset 9 -> line 2, col 1
        assert_eq!(idx.line_col(9), Some(LineCol { line: 2, col: 1 }));
    }

    #[test]
    fn test_line_index_newline_boundary() {
        let src = "ab\ncd\n";
        let idx = LineIndex::new(src);
        // '\n' at offset 2 -> line 0, col 2
        assert_eq!(idx.line_col(2), Some(LineCol { line: 0, col: 2 }));
        // 'c' at offset 3 -> line 1, col 0
        assert_eq!(idx.line_col(3), Some(LineCol { line: 1, col: 0 }));
        // '\n' at offset 5 -> line 1, col 2
        assert_eq!(idx.line_col(5), Some(LineCol { line: 1, col: 2 }));
        // offset 6 -> line 2, col 0 (empty line at end)
        assert_eq!(idx.line_col(6), Some(LineCol { line: 2, col: 0 }));
    }

    #[test]
    fn test_span_merge() {
        let a = Span::new(5, 10);
        let b = Span::new(8, 15);
        let merged = a.merge(b);
        assert_eq!(merged, Span::new(5, 15));
    }

    #[test]
    fn test_span_default() {
        let span = Span::default();
        assert_eq!(span, Span::dummy());
    }

    #[test]
    fn test_line_col_beyond_end() {
        let idx = LineIndex::new("ab\ncd");
        // Offset beyond end
        assert_eq!(idx.line_col(100), Some(LineCol { line: 1, col: 97 }));
    }
}
