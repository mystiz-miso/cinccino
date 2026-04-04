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
    fn test_underscore_token() {
        let src = "(_,out) <== A()(inp);";
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty());
        assert_eq!(tokens[0].token, Token::LParen);
        assert_eq!(tokens[1].token, Token::Underscore);
        assert_eq!(tokens[2].token, Token::Comma);
    }
}
