use logos::Logos;

/// All tokens in the Circom v2.2.3 language.
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*")]
#[logos(skip r"/\*[^*]*\*+(?:[^/*][^*]*\*+)*/")]
pub enum Token {
    // ── Keywords ──────────────────────────────────────────────
    #[token("pragma")]
    Pragma,
    #[token("circom")]
    Circom,
    #[token("include")]
    Include,
    #[token("template")]
    Template,
    #[token("custom")]
    Custom,
    #[token("function")]
    Function,
    #[token("component")]
    Component,
    #[token("bus")]
    Bus,
    #[token("signal")]
    Signal,
    #[token("input")]
    Input,
    #[token("output")]
    Output,
    #[token("public")]
    Public,
    #[token("var")]
    Var,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("for")]
    For,
    #[token("while")]
    While,
    #[token("return")]
    Return,
    #[token("log")]
    Log,
    #[token("assert")]
    Assert,
    #[token("parallel")]
    Parallel,
    #[token("main")]
    Main,
    #[token("extern")]
    Extern,

    // ── Literals ──────────────────────────────────────────────
    #[regex(r"[0-9]+", |lex| lex.slice().to_string())]
    NumberLit(String),

    /// String literal with outer quotes stripped.
    ///
    /// **Note:** Escape sequences (e.g., `\"`, `\\`) are stored verbatim and
    /// are *not* interpreted at the lexer/parser stage. Downstream consumers
    /// (e.g., LSP hover) must interpret escapes themselves.
    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        s[1..s.len()-1].to_string()
    })]
    StringLit(String),

    // ── Identifier ────────────────────────────────────────────
    #[regex(r"[a-zA-Z_$][a-zA-Z0-9_$]*", priority = 1, callback = |lex| lex.slice().to_string())]
    Ident(String),

    // ── Circom-specific operators ─────────────────────────────
    #[token("<==")]
    LeftSignalAssign,
    #[token("==>")]
    RightSignalAssign,
    #[token("<--")]
    LeftUnsafeAssign,
    #[token("-->")]
    RightUnsafeAssign,
    #[token("===")]
    ConstraintEq,

    // ── Compound assignment ───────────────────────────────────
    #[token("+=")]
    PlusAssign,
    #[token("-=")]
    MinusAssign,
    #[token("*=")]
    StarAssign,
    #[token("**=")]
    PowerAssign,
    #[token("/=")]
    SlashAssign,
    #[token("\\=")]
    IntDivAssign,
    #[token("%=")]
    ModAssign,
    #[token("<<=")]
    ShlAssign,
    #[token(">>=")]
    ShrAssign,
    #[token("&=")]
    BitAndAssign,
    #[token("|=")]
    BitOrAssign,
    #[token("^=")]
    BitXorAssign,

    // ── Increment / Decrement ─────────────────────────────────
    #[token("++")]
    PlusPlus,
    #[token("--")]
    MinusMinus,

    // ── Comparison / relational (multi-char first) ────────────
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("==")]
    EqEq,
    #[token("!=")]
    Ne,

    // ── Boolean ───────────────────────────────────────────────
    #[token("&&")]
    And,
    #[token("||")]
    Or,
    #[token("!")]
    Bang,

    // ── Arithmetic ────────────────────────────────────────────
    #[token("**")]
    Power,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("\\")]
    IntDiv,
    #[token("%")]
    Mod,

    // ── Bitwise ───────────────────────────────────────────────
    #[token("<<")]
    Shl,
    #[token(">>")]
    Shr,
    #[token("&")]
    Amp,
    #[token("|")]
    Pipe,
    #[token("^")]
    Caret,
    #[token("~")]
    Tilde,

    // ── Single-char relational ────────────────────────────────
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,

    // ── Punctuation ───────────────────────────────────────────
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(";")]
    Semi,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("=")]
    Eq,
    #[token("?")]
    Question,
    #[token(":")]
    Colon,
    #[token("_", priority = 2)]
    Underscore,
}

/// A token together with its byte span in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub span: std::ops::Range<usize>,
}

/// Tokenize `source` into a list of spanned tokens, collecting lexer errors.
pub fn tokenize(source: &str) -> (Vec<SpannedToken>, Vec<std::ops::Range<usize>>) {
    let mut tokens = Vec::new();
    let mut errors = Vec::new();

    let lex = Token::lexer(source);
    for (result, span) in lex.spanned() {
        match result {
            Ok(token) => tokens.push(SpannedToken { token, span }),
            Err(_) => errors.push(span),
        }
    }

    (tokens, errors)
}

// ── Comment extraction (trivia) ─────────────────────────────────────
//
// The lexer above skips comments so that the parser only sees syntactic
// tokens. For formatter-level trivia preservation, we scan the source
// independently and return the comments in source order.

/// A Circom comment, preserved as trivia for the formatter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// Inclusive start byte offset of the comment (including the
    /// leading `//` or `/*`).
    pub start: usize,
    /// Exclusive end byte offset (including `*/` for block comments,
    /// excluding the terminating `\n` for line comments).
    pub end: usize,
    /// The comment kind — line (`// ...`) or block (`/* ... */`).
    pub kind: CommentKind,
    /// The raw comment text as it appears in the source (including the
    /// `//` or `/* … */` delimiters).
    pub text: String,
    /// Whether this comment sits on the same line as source that
    /// precedes it (i.e., there is non-whitespace earlier on the line).
    pub trailing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentKind {
    /// `// line comment` up to end of line.
    Line,
    /// `/* block comment */`, possibly spanning multiple lines.
    Block,
}

/// Scan `source` and return every comment in source order. String
/// literals are skipped so that `"//"` inside a string is not
/// mis-identified as a comment.
///
/// Unterminated block comments are reported as best-effort — the
/// comment is considered to run until the end of the source.
fn skip_string_literal(bytes: &[u8], mut i: usize) -> usize {
    let len = bytes.len();
    i += 1;
    while i < len {
        match bytes[i] {
            b'\\' if i + 1 < len => i += 2,
            b'"' => {
                i += 1;
                break;
            }
            _ => i += 1,
        }
    }
    i
}

fn read_line_comment(source: &str, bytes: &[u8], start: usize) -> (Comment, usize) {
    let trailing = is_trailing_at(bytes, start);
    let mut j = start + 2;
    while j < bytes.len() && bytes[j] != b'\n' {
        j += 1;
    }
    (
        Comment {
            start,
            end: j,
            kind: CommentKind::Line,
            text: source[start..j].to_string(),
            trailing,
        },
        j,
    )
}

fn read_block_comment(source: &str, bytes: &[u8], start: usize) -> (Comment, usize) {
    let len = bytes.len();
    let trailing = is_trailing_at(bytes, start);
    let mut j = start + 2;
    while j + 1 < len && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
        j += 1;
    }
    let end = if j + 1 < len { j + 2 } else { len };
    (
        Comment {
            start,
            end,
            kind: CommentKind::Block,
            text: source[start..end].to_string(),
            trailing,
        },
        end,
    )
}

pub fn extract_comments(source: &str) -> Vec<Comment> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < len {
        let b = bytes[i];

        // Skip string literals so their contents don't trigger false
        // positives.
        if b == b'"' {
            i = skip_string_literal(bytes, i);
            continue;
        }

        // Line comment.
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            let (c, next) = read_line_comment(source, bytes, i);
            out.push(c);
            i = next;
            continue;
        }

        // Block comment.
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let (c, next) = read_block_comment(source, bytes, i);
            out.push(c);
            i = next;
            continue;
        }

        i += 1;
    }

    out
}

/// Return `true` when `offset` in `bytes` has non-whitespace content
/// earlier on the same line (i.e., the character at `offset` is a
/// trailing/inline token rather than a leading one).
fn is_trailing_at(bytes: &[u8], offset: usize) -> bool {
    if offset == 0 {
        return false;
    }
    let mut k = offset;
    while k > 0 {
        k -= 1;
        match bytes[k] {
            b'\n' => return false,
            b' ' | b'\t' | b'\r' => continue,
            _ => return true,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_keywords() {
        let src = "pragma circom 2.2.3;";
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty(), "unexpected lex errors: {:?}", errors);
        assert_eq!(tokens[0].token, Token::Pragma);
        assert_eq!(tokens[1].token, Token::Circom);
        assert_eq!(tokens[2].token, Token::NumberLit("2".into()));
        assert_eq!(tokens[3].token, Token::Dot);
        assert_eq!(tokens[4].token, Token::NumberLit("2".into()));
        assert_eq!(tokens[5].token, Token::Dot);
        assert_eq!(tokens[6].token, Token::NumberLit("3".into()));
        assert_eq!(tokens[7].token, Token::Semi);
    }

    #[test]
    fn test_signal_operators() {
        let src = "a <== b; c ==> d; x <-- y; z --> w; p === q;";
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty());
        // a <== b ; -> indices 0..3
        assert_eq!(tokens[1].token, Token::LeftSignalAssign);
        // c ==> d ; -> indices 4..7
        assert_eq!(tokens[5].token, Token::RightSignalAssign);
        // x <-- y ; -> indices 8..11
        assert_eq!(tokens[9].token, Token::LeftUnsafeAssign);
        // z --> w ; -> indices 12..15
        assert_eq!(tokens[13].token, Token::RightUnsafeAssign);
        // p === q ; -> indices 16..19
        assert_eq!(tokens[17].token, Token::ConstraintEq);
    }

    #[test]
    fn test_comments_skipped() {
        let src = "signal // line comment\ninput /* block comment */ x;";
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty());
        assert_eq!(tokens[0].token, Token::Signal);
        assert_eq!(tokens[1].token, Token::Input);
        assert_eq!(tokens[2].token, Token::Ident("x".into()));
        assert_eq!(tokens[3].token, Token::Semi);
    }

    #[test]
    fn test_string_literal() {
        let src = r#"include "circomlib/poseidon.circom";"#;
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty());
        assert_eq!(tokens[0].token, Token::Include);
        assert_eq!(
            tokens[1].token,
            Token::StringLit("circomlib/poseidon.circom".into())
        );
        assert_eq!(tokens[2].token, Token::Semi);
    }

    #[test]
    fn test_string_literal_with_escapes() {
        let src = r#""hello\"world""#;
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty(), "unexpected lex errors: {:?}", errors);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token, Token::StringLit(r#"hello\"world"#.into()));
    }

    #[test]
    fn test_string_literal_with_backslash() {
        let src = r#""value: \\""#;
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty(), "unexpected lex errors: {:?}", errors);
        assert_eq!(tokens[0].token, Token::StringLit(r#"value: \\"#.into()));
    }

    #[test]
    fn test_compound_assignment_operators() {
        let src = "+= -= *= **= /= \\= %= <<= >>= &= |= ^=";
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty());
        assert_eq!(tokens[0].token, Token::PlusAssign);
        assert_eq!(tokens[1].token, Token::MinusAssign);
        assert_eq!(tokens[2].token, Token::StarAssign);
        assert_eq!(tokens[3].token, Token::PowerAssign);
        assert_eq!(tokens[4].token, Token::SlashAssign);
        assert_eq!(tokens[5].token, Token::IntDivAssign);
        assert_eq!(tokens[6].token, Token::ModAssign);
        assert_eq!(tokens[7].token, Token::ShlAssign);
        assert_eq!(tokens[8].token, Token::ShrAssign);
        assert_eq!(tokens[9].token, Token::BitAndAssign);
        assert_eq!(tokens[10].token, Token::BitOrAssign);
        assert_eq!(tokens[11].token, Token::BitXorAssign);
    }

    #[test]
    fn test_increment_decrement() {
        let src = "i++ j--";
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty());
        assert_eq!(tokens[1].token, Token::PlusPlus);
        assert_eq!(tokens[3].token, Token::MinusMinus);
    }

    #[test]
    fn test_all_keywords() {
        let src = "pragma circom include template custom function component bus \
                    signal input output public var if else for while return \
                    log assert parallel main extern";
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty());
        let expected = vec![
            Token::Pragma,
            Token::Circom,
            Token::Include,
            Token::Template,
            Token::Custom,
            Token::Function,
            Token::Component,
            Token::Bus,
            Token::Signal,
            Token::Input,
            Token::Output,
            Token::Public,
            Token::Var,
            Token::If,
            Token::Else,
            Token::For,
            Token::While,
            Token::Return,
            Token::Log,
            Token::Assert,
            Token::Parallel,
            Token::Main,
            Token::Extern,
        ];
        for (i, exp) in expected.iter().enumerate() {
            assert_eq!(&tokens[i].token, exp, "mismatch at index {}", i);
        }
    }

    #[test]
    fn test_extract_comments_line() {
        let src = "signal // a line comment\ninput x;";
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].kind, CommentKind::Line);
        assert_eq!(comments[0].text, "// a line comment");
        assert!(comments[0].trailing);
    }

    #[test]
    fn test_extract_comments_block() {
        let src = "signal /* block\ncomment */ x;";
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].kind, CommentKind::Block);
        assert_eq!(comments[0].text, "/* block\ncomment */");
    }

    #[test]
    fn test_extract_comments_leading() {
        let src = "// leading\nsignal x;";
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 1);
        assert!(!comments[0].trailing, "should be leading");
    }

    #[test]
    fn test_extract_comments_inside_string_ignored() {
        let src = r#"log("// not a comment"); signal x;"#;
        let comments = extract_comments(src);
        assert!(
            comments.is_empty(),
            "should not find comments in strings: {comments:?}"
        );
    }

    #[test]
    fn test_extract_comments_multiple() {
        let src = "// one\ntemplate T() {\n    // two\n    signal /* three */ x;\n}\n";
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 3);
        assert_eq!(comments[0].text, "// one");
        assert_eq!(comments[1].text, "// two");
        assert_eq!(comments[2].text, "/* three */");
    }

    #[test]
    fn test_underscore_token() {
        let src = "(_,out) <== A()(inp);";
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty());
        assert_eq!(tokens[0].token, Token::LParen);
        assert_eq!(tokens[1].token, Token::Underscore);
        assert_eq!(tokens[2].token, Token::Comma);
    }
}
