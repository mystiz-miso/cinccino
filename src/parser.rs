//! Recursive-descent parser for Circom v2.2.3.
//!
//! Features:
//! - Produces structured [`ast`] nodes (not a parse tree).
//! - Error recovery: continues after syntax errors and collects all diagnostics.
//! - Source location tracked on every node.

use crate::ast::*;
use crate::lexer::{tokenize, SpannedToken, Token};
use crate::span::Span;

// ── Diagnostics ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub span: Span,
    pub message: String,
}

// ── Parser state ────────────────────────────────────────────────────

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    errors: Vec<ParseError>,
    /// The declared `pragma circom` version, if any.
    version: Option<Version>,
}

/// Parse a Circom source string into an AST, collecting errors.
#[must_use = "check parse errors"]
pub fn parse(source: &str) -> (File, Vec<ParseError>) {
    let (tokens, lex_errors) = tokenize(source);
    let mut parser = Parser {
        tokens,
        pos: 0,
        errors: Vec::new(),
        version: None,
    };

    // Convert lex errors
    for span in lex_errors {
        parser.errors.push(ParseError {
            span: Span::new(span.start, span.end),
            message: "unexpected character".into(),
        });
    }

    let items = parser.parse_file();
    let span = if items.is_empty() {
        Span::new(0, source.len())
    } else {
        items
            .first()
            .unwrap()
            .span()
            .merge(items.last().unwrap().span())
    };

    let file = File { span, items };
    (file, parser.errors)
}

impl Item {
    fn span(&self) -> Span {
        match self {
            Item::Pragma(p) => p.span,
            Item::Include(i) => i.span,
            Item::TemplateDef(t) => t.span,
            Item::FunctionDef(f) => f.span,
            Item::BusDef(b) => b.span,
            Item::MainComponent(m) => m.span,
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|t| &t.token)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn advance(&mut self) -> Option<&SpannedToken> {
        if self.pos < self.tokens.len() {
            let tok = &self.tokens[self.pos];
            self.pos += 1;
            Some(tok)
        } else {
            None
        }
    }

    fn span_of(&self, pos: usize) -> Span {
        self.tokens
            .get(pos)
            .map(|t| Span::new(t.span.start, t.span.end))
            .unwrap_or_default()
    }

    fn current_span(&self) -> Span {
        self.span_of(self.pos)
    }

    fn prev_span(&self) -> Span {
        if self.pos > 0 {
            self.span_of(self.pos - 1)
        } else {
            Span::new(0, 0)
        }
    }

    fn check(&self, expected: &Token) -> bool {
        self.peek()
            .is_some_and(|t| std::mem::discriminant(t) == std::mem::discriminant(expected))
    }

    fn eat(&mut self, expected: &Token) -> bool {
        if self.check(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &Token) -> bool {
        if self.eat(expected) {
            true
        } else {
            let span = self.current_span();
            let found = self
                .peek()
                .map(|t| format!("{:?}", t))
                .unwrap_or("EOF".into());
            self.errors.push(ParseError {
                span,
                message: format!("expected {:?}, found {}", expected, found),
            });
            false
        }
    }

    fn expect_semi(&mut self) {
        self.expect(&Token::Semi);
    }

    fn keyword_as_ident(token: &Token) -> Option<&'static str> {
        match token {
            Token::Main => Some("main"),
            Token::Input => Some("input"),
            Token::Output => Some("output"),
            Token::Signal => Some("signal"),
            Token::Component => Some("component"),
            Token::Var => Some("var"),
            Token::Log => Some("log"),
            Token::Assert => Some("assert"),
            Token::Template => Some("template"),
            Token::Function => Some("function"),
            Token::Bus => Some("bus"),
            Token::Custom => Some("custom"),
            Token::Public => Some("public"),
            Token::Parallel => Some("parallel"),
            Token::Extern => Some("extern"),
            _ => None,
        }
    }

    /// Returns `true` for keywords that must not be used as declaration names
    /// (signal, variable, or component names). `main` is a contextual keyword
    /// and is intentionally excluded — it may appear as an identifier.
    fn is_reserved_keyword(token: &Token) -> bool {
        Self::keyword_as_ident(token).is_some() && !matches!(token, Token::Main)
    }

    /// Like `expect_ident`, but rejects reserved keywords as declaration names.
    fn expect_decl_name(&mut self) -> Identifier {
        if let Some(tok) = self.peek() {
            if Self::is_reserved_keyword(tok) {
                let span = self.current_span();
                let name = Self::keyword_as_ident(tok).unwrap();
                self.error(format!("keyword `{}` cannot be used as a name", name));
                self.advance();
                return Identifier {
                    span,
                    name: name.into(),
                };
            }
        }
        self.expect_ident()
    }

    fn expect_ident(&mut self) -> Identifier {
        match self.peek() {
            Some(Token::Ident(_)) => {
                let span = self.current_span();
                if let Token::Ident(name) = self.advance().unwrap().token.clone() {
                    Identifier { span, name }
                } else {
                    unreachable!()
                }
            }
            Some(tok) if Self::keyword_as_ident(tok).is_some() => {
                let span = self.current_span();
                let name = Self::keyword_as_ident(tok).unwrap();
                self.advance();
                Identifier {
                    span,
                    name: name.into(),
                }
            }
            _ => {
                let span = self.current_span();
                let found = self
                    .peek()
                    .map(|t| format!("{:?}", t))
                    .unwrap_or("EOF".into());
                self.errors.push(ParseError {
                    span,
                    message: format!("expected identifier, found {}", found),
                });
                Identifier {
                    span,
                    name: String::new(),
                }
            }
        }
    }

    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(ParseError {
            span: self.current_span(),
            message: message.into(),
        });
    }

    /// Emit an error if the declared pragma version is below `min`.
    /// No-op when no pragma has been seen (permissive by default).
    fn require_version(&mut self, min: &Version, feature: &str, span: Span) {
        if let Some(ref ver) = self.version {
            if ver < min {
                self.errors.push(ParseError {
                    span,
                    message: format!(
                        "{} require pragma circom >= {}.{}.{}",
                        feature, min.major, min.minor, min.patch
                    ),
                });
            }
        }
    }

    /// Skip tokens until we find something that could start a new statement/item.
    fn synchronize(&mut self) {
        while !self.at_end() {
            if self.eat(&Token::Semi) {
                return;
            }
            match self.peek() {
                Some(
                    Token::Pragma
                    | Token::Include
                    | Token::Template
                    | Token::Function
                    | Token::Bus
                    | Token::Component
                    | Token::Signal
                    | Token::Var
                    | Token::If
                    | Token::For
                    | Token::While
                    | Token::Return
                    | Token::Log
                    | Token::Assert,
                ) => return,
                Some(Token::RBrace) => return,
                _ => {
                    self.advance();
                }
            }
        }
    }
}

// ── File-level parsing ──────────────────────────────────────────────

impl Parser {
    fn parse_file(&mut self) -> Vec<Item> {
        let mut items = Vec::new();
        while !self.at_end() {
            match self.parse_item() {
                Some(item) => items.push(item),
                None => {
                    // Error recovery: skip token
                    if !self.at_end() {
                        self.error("unexpected token at top level");
                        self.advance();
                    }
                }
            }
        }
        items
    }

    fn parse_item(&mut self) -> Option<Item> {
        match self.peek()? {
            Token::Pragma => Some(Item::Pragma(self.parse_pragma())),
            Token::Include => Some(Item::Include(self.parse_include())),
            Token::Template | Token::Custom => Some(Item::TemplateDef(self.parse_template_def())),
            Token::Function => Some(Item::FunctionDef(self.parse_function_def())),
            Token::Bus => Some(Item::BusDef(self.parse_bus_def())),
            Token::Component => {
                // Could be `component main ...` or a regular component decl
                // at the top level, it must be `component main`
                Some(Item::MainComponent(self.parse_main_component()))
            }
            _ => None,
        }
    }
}

// ── Pragma ──────────────────────────────────────────────────────────

impl Parser {
    fn parse_pragma(&mut self) -> Pragma {
        let start = self.current_span();
        self.advance(); // eat `pragma`

        let kind = match self.peek() {
            Some(Token::Circom) => {
                self.advance(); // eat `circom`
                let version = self.parse_version();
                if self.version.is_some() {
                    self.errors.push(ParseError {
                        span: start.merge(self.prev_span()),
                        message: "duplicate pragma circom declaration".into(),
                    });
                } else {
                    self.version = Some(version.clone());
                }
                PragmaKind::Version(version)
            }
            Some(Token::Ident(s)) if s == "custom_templates" => {
                self.advance();
                PragmaKind::CustomTemplates
            }
            _ => {
                self.error("expected 'circom' or 'custom_templates' after pragma");
                PragmaKind::CustomTemplates
            }
        };
        self.expect_semi();
        Pragma {
            span: start.merge(self.prev_span()),
            kind,
        }
    }

    fn parse_version(&mut self) -> Version {
        let major = self.parse_number_as_u32();
        self.expect(&Token::Dot);
        let minor = self.parse_number_as_u32();
        self.expect(&Token::Dot);
        let patch = self.parse_number_as_u32();
        Version {
            major,
            minor,
            patch,
        }
    }

    fn parse_number_as_u32(&mut self) -> u32 {
        match self.peek() {
            Some(Token::NumberLit(_)) => {
                if let Token::NumberLit(s) = self.advance().unwrap().token.clone() {
                    match parse_number_literal_u32(&s) {
                        Some(n) => n,
                        None => {
                            self.error(format!("version number '{}' overflows u32", s));
                            0
                        }
                    }
                } else {
                    0
                }
            }
            _ => {
                self.error("expected version number");
                0
            }
        }
    }
}

// ── Include ─────────────────────────────────────────────────────────

impl Parser {
    fn parse_include(&mut self) -> Include {
        let start = self.current_span();
        self.advance(); // eat `include`
        let path = match self.peek() {
            Some(Token::StringLit(_)) => {
                if let Token::StringLit(s) = self.advance().unwrap().token.clone() {
                    s
                } else {
                    String::new()
                }
            }
            _ => {
                self.error("expected string literal after include");
                String::new()
            }
        };
        self.expect_semi();
        Include {
            span: start.merge(self.prev_span()),
            path,
        }
    }
}

// ── Template ────────────────────────────────────────────────────────

impl Parser {
    fn parse_template_def(&mut self) -> TemplateDef {
        let start = self.current_span();

        // Parse optional `custom`
        let is_custom = self.eat(&Token::Custom);

        self.expect(&Token::Template);

        // Parse optional `custom` after template keyword (both orderings work)
        let is_custom = is_custom || self.eat(&Token::Custom);

        // Parse optional `extern`
        let is_extern = self.eat(&Token::Extern);
        if is_extern {
            self.require_version(
                &Version {
                    major: 2,
                    minor: 2,
                    patch: 3,
                },
                "extern templates",
                start,
            );
        }

        // Parse optional `parallel`
        let is_parallel = self.eat(&Token::Parallel);

        let name = self.expect_decl_name();
        let params = self.parse_param_list();
        let body = self.parse_block();

        TemplateDef {
            span: start.merge(self.prev_span()),
            name,
            params,
            body,
            is_custom,
            is_parallel,
            is_extern,
        }
    }

    fn parse_param_list(&mut self) -> Vec<Identifier> {
        let mut params = Vec::new();
        self.expect(&Token::LParen);
        if !self.check(&Token::RParen) {
            params.push(self.expect_ident());
            while self.eat(&Token::Comma) {
                params.push(self.expect_ident());
            }
        }
        self.expect(&Token::RParen);
        params
    }
}

// ── Function ────────────────────────────────────────────────────────

impl Parser {
    fn parse_function_def(&mut self) -> FunctionDef {
        let start = self.current_span();
        self.advance(); // eat `function`
        let name = self.expect_decl_name();
        let params = self.parse_param_list();
        let body = self.parse_block();
        FunctionDef {
            span: start.merge(self.prev_span()),
            name,
            params,
            body,
        }
    }
}

// ── Bus definition ──────────────────────────────────────────────────

impl Parser {
    fn parse_bus_def(&mut self) -> BusDef {
        let start = self.current_span();
        self.require_version(
            &Version {
                major: 2,
                minor: 2,
                patch: 0,
            },
            "bus definitions",
            start,
        );
        self.advance(); // eat `bus`
        let name = self.expect_decl_name();
        let params = self.parse_param_list();
        self.expect(&Token::LBrace);
        let mut body = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            if let Some(member) = self.parse_bus_member() {
                body.push(member);
            } else {
                self.error("expected signal or bus member in bus definition");
                self.synchronize();
            }
        }
        self.expect(&Token::RBrace);
        // optional semicolon after bus def
        self.eat(&Token::Semi);
        BusDef {
            span: start.merge(self.prev_span()),
            name,
            params,
            body,
        }
    }

    fn parse_bus_member(&mut self) -> Option<BusMember> {
        match self.peek()? {
            Token::Signal => {
                let decl = self.parse_signal_decl()?;
                Some(BusMember::Signal(decl))
            }
            Token::Ident(_) => {
                // Bus field: `BusName(args) name[dims];`
                let field = self.parse_bus_field_decl();
                Some(BusMember::Bus(field))
            }
            _ => None,
        }
    }

    fn parse_bus_field_decl(&mut self) -> BusFieldDecl {
        let start = self.current_span();
        let bus_type = self.parse_bus_type();
        let tags = self.parse_optional_tags();
        let name = self.expect_decl_name();
        let dimensions = self.parse_dimensions();
        self.expect_semi();
        BusFieldDecl {
            span: start.merge(self.prev_span()),
            bus_type,
            tags,
            name,
            dimensions,
        }
    }

    fn parse_bus_type(&mut self) -> BusType {
        let start = self.current_span();
        let name = self.expect_ident();
        self.expect(&Token::LParen);
        let mut args = Vec::new();
        if !self.check(&Token::RParen) {
            args.push(self.parse_expression());
            while self.eat(&Token::Comma) {
                args.push(self.parse_expression());
            }
        }
        self.expect(&Token::RParen);
        BusType {
            span: start.merge(self.prev_span()),
            name,
            args,
        }
    }
}

// ── Main component ──────────────────────────────────────────────────

impl Parser {
    fn parse_main_component(&mut self) -> MainComponent {
        let start = self.current_span();
        self.advance(); // eat `component`
        self.expect(&Token::Main);

        let public_signals = if self.eat(&Token::LBrace) {
            // `{public [sig1, sig2]}`
            self.expect(&Token::Public);
            self.expect(&Token::LBracket);
            let mut sigs = Vec::new();
            if !self.check(&Token::RBracket) {
                sigs.push(self.expect_ident());
                while self.eat(&Token::Comma) {
                    sigs.push(self.expect_ident());
                }
            }
            self.expect(&Token::RBracket);
            self.expect(&Token::RBrace);
            sigs
        } else {
            Vec::new()
        };

        self.expect(&Token::Eq);
        let expr = self.parse_expression();
        self.expect_semi();

        MainComponent {
            span: start.merge(self.prev_span()),
            public_signals,
            expr,
        }
    }
}

// ── Block ───────────────────────────────────────────────────────────

impl Parser {
    fn parse_block(&mut self) -> Block {
        let start = self.current_span();
        self.expect(&Token::LBrace);
        let mut stmts = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            match self.parse_statement() {
                Some(stmt) => stmts.push(stmt),
                None => {
                    if !self.check(&Token::RBrace) && !self.at_end() {
                        self.error("unexpected token in block");
                        self.advance();
                    }
                }
            }
        }
        self.expect(&Token::RBrace);
        Block {
            span: start.merge(self.prev_span()),
            stmts,
        }
    }

    fn parse_block_or_single_stmt(&mut self) -> Block {
        if self.check(&Token::LBrace) {
            self.parse_block()
        } else {
            let start = self.current_span();
            match self.parse_statement() {
                Some(stmt) => {
                    let span = start.merge(self.prev_span());
                    Block {
                        span,
                        stmts: vec![stmt],
                    }
                }
                None => {
                    self.error("expected statement");
                    Block {
                        span: start,
                        stmts: vec![],
                    }
                }
            }
        }
    }
}

// ── Statements ──────────────────────────────────────────────────────

impl Parser {
    fn parse_statement(&mut self) -> Option<Statement> {
        let start = self.current_span();
        let kind = match self.peek()? {
            Token::Var => self.parse_var_decl_stmt(),
            Token::Signal => self.parse_signal_decl_stmt(),
            Token::Component => self.parse_component_decl_stmt(start),
            Token::If => self.parse_if_stmt(),
            Token::For => self.parse_for_stmt(),
            Token::While => self.parse_while_stmt(),
            Token::Return => self.parse_return_stmt(),
            Token::Log => self.parse_log_stmt(),
            Token::Assert => self.parse_assert_stmt(),
            Token::LBrace => {
                let block = self.parse_block();
                Some(StatementKind::Block(block))
            }
            Token::LParen => {
                // Could be tuple assignment: `(a, b) <== ...`
                self.parse_tuple_or_expr_stmt()
            }
            _ => self.parse_expr_stmt(),
        }?;

        Some(Statement {
            span: start.merge(self.prev_span()),
            kind,
        })
    }

    fn parse_var_decl_stmt(&mut self) -> Option<StatementKind> {
        let start = self.current_span();
        self.advance(); // eat `var`
        let mut names = Vec::new();
        names.push(self.parse_var_decl_entry());
        while self.eat(&Token::Comma) {
            names.push(self.parse_var_decl_entry());
        }

        self.expect_semi();
        Some(StatementKind::VarDecl(VarDecl {
            span: start.merge(self.prev_span()),
            names,
        }))
    }

    fn parse_var_decl_entry(&mut self) -> VarDeclEntry {
        let name = self.expect_decl_name();
        let dimensions = self.parse_dimensions();
        let init = if self.eat(&Token::Eq) {
            Some(self.parse_expression())
        } else {
            None
        };
        VarDeclEntry {
            name,
            dimensions,
            init,
        }
    }

    fn parse_bus_instance_decl(&mut self, start: Span, signal_kind: SignalKind) -> StatementKind {
        self.require_version(
            &Version {
                major: 2,
                minor: 2,
                patch: 0,
            },
            "bus instance declarations",
            self.current_span(),
        );
        let bus_type = self.parse_bus_type();
        let tags = self.parse_optional_tags();
        let name = self.expect_decl_name();
        let dimensions = self.parse_dimensions();

        let init = if self.check(&Token::LeftSignalAssign) {
            self.advance();
            Some((SignalAssignOp::SafeLeft, self.parse_expression()))
        } else if self.check(&Token::LeftUnsafeAssign) {
            self.advance();
            Some((SignalAssignOp::UnsafeLeft, self.parse_expression()))
        } else {
            None
        };
        self.expect_semi();

        StatementKind::BusDecl(BusInstanceDecl {
            span: start.merge(self.prev_span()),
            bus_type,
            signal_kind,
            tags,
            name,
            dimensions,
            init,
        })
    }

    fn parse_signal_decl_stmt(&mut self) -> Option<StatementKind> {
        let start = self.current_span();
        self.advance(); // eat `signal`

        let kind = match self.peek() {
            Some(Token::Input) => {
                self.advance();
                SignalKind::Input
            }
            Some(Token::Output) => {
                self.advance();
                SignalKind::Output
            }
            _ => SignalKind::Intermediate,
        };

        // Check for bus type: `signal input BusName() { tags } name`
        if self.is_bus_type_ahead() {
            return Some(self.parse_bus_instance_decl(start, kind));
        }

        let tags = self.parse_optional_tags();
        let mut names = Vec::new();
        names.push(self.parse_signal_decl_entry());
        while self.eat(&Token::Comma) {
            names.push(self.parse_signal_decl_entry());
        }
        self.expect_semi();

        Some(StatementKind::SignalDecl(SignalDecl {
            span: start.merge(self.prev_span()),
            kind,
            tags,
            names,
        }))
    }

    fn parse_signal_decl(&mut self) -> Option<SignalDecl> {
        let start = self.current_span();
        self.advance(); // eat `signal`

        let kind = match self.peek() {
            Some(Token::Input) => {
                self.advance();
                SignalKind::Input
            }
            Some(Token::Output) => {
                self.advance();
                SignalKind::Output
            }
            _ => SignalKind::Intermediate,
        };

        let tags = self.parse_optional_tags();
        let mut names = Vec::new();
        names.push(self.parse_signal_decl_entry());
        while self.eat(&Token::Comma) {
            names.push(self.parse_signal_decl_entry());
        }
        self.expect_semi();

        Some(SignalDecl {
            span: start.merge(self.prev_span()),
            kind,
            tags,
            names,
        })
    }

    fn is_bus_type_ahead(&self) -> bool {
        // Look for Ident(...)  pattern which indicates a bus type
        // vs just an identifier which would be a signal name
        if let Some(Token::Ident(_)) = self.peek() {
            if let Some(next) = self.tokens.get(self.pos + 1) {
                return matches!(next.token, Token::LParen);
            }
        }
        false
    }

    fn parse_signal_decl_entry(&mut self) -> SignalDeclEntry {
        let name = self.expect_decl_name();
        let dimensions = self.parse_dimensions();

        let init = if self.check(&Token::LeftSignalAssign) {
            self.advance();
            Some((SignalAssignOp::SafeLeft, self.parse_expression()))
        } else if self.check(&Token::LeftUnsafeAssign) {
            self.advance();
            Some((SignalAssignOp::UnsafeLeft, self.parse_expression()))
        } else {
            None
        };

        SignalDeclEntry {
            name,
            dimensions,
            init,
        }
    }

    fn parse_component_decl_stmt(&mut self, start: Span) -> Option<StatementKind> {
        self.advance(); // eat `component`

        // Check for `component main`
        if self.check(&Token::Main) {
            // This shouldn't appear inside a block, but handle gracefully
            self.error("component main should be at top level");
            self.synchronize();
            return Some(StatementKind::Error);
        }

        let mut is_parallel = self.eat(&Token::Parallel);

        let mut names = Vec::new();
        names.push(self.parse_component_decl_entry(&mut is_parallel));
        while self.eat(&Token::Comma) {
            names.push(self.parse_component_decl_entry(&mut is_parallel));
        }
        self.expect_semi();

        Some(StatementKind::ComponentDecl(ComponentDecl {
            span: start.merge(self.prev_span()),
            is_parallel,
            names,
        }))
    }

    fn parse_component_decl_entry(&mut self, is_parallel: &mut bool) -> ComponentDeclEntry {
        let name = self.expect_decl_name();
        let dimensions = self.parse_dimensions();
        let init = if self.eat(&Token::Eq) {
            // `parallel` can appear after `=` for per-component parallelism
            if self.eat(&Token::Parallel) {
                *is_parallel = true;
            }
            Some(self.parse_expression())
        } else {
            None
        };
        ComponentDeclEntry {
            name,
            dimensions,
            init,
        }
    }

    fn parse_if_stmt(&mut self) -> Option<StatementKind> {
        self.advance(); // eat `if`
        self.expect(&Token::LParen);
        let cond = self.parse_expression();
        self.expect(&Token::RParen);
        let then_body = self.parse_block_or_single_stmt();
        let else_body = if self.eat(&Token::Else) {
            Some(self.parse_block_or_single_stmt())
        } else {
            None
        };
        Some(StatementKind::IfElse(IfElse {
            cond,
            then_body,
            else_body,
        }))
    }

    fn parse_for_stmt(&mut self) -> Option<StatementKind> {
        self.advance(); // eat `for`
        self.expect(&Token::LParen);
        let init = self.parse_for_init();
        let cond = self.parse_expression();
        self.expect_semi();
        let step = self.parse_for_step();
        self.expect(&Token::RParen);
        let body = self.parse_block_or_single_stmt();
        Some(StatementKind::For(ForLoop {
            init: Box::new(init),
            cond,
            step: Box::new(step),
            body,
        }))
    }

    fn parse_for_init(&mut self) -> Statement {
        let start = self.current_span();
        if self.check(&Token::Var) {
            let kind = self.parse_var_decl_stmt().unwrap_or(StatementKind::Error);
            Statement {
                span: start.merge(self.prev_span()),
                kind,
            }
        } else {
            // Expression statement
            let kind = self.parse_expr_stmt().unwrap_or(StatementKind::Error);
            Statement {
                span: start.merge(self.prev_span()),
                kind,
            }
        }
    }

    fn parse_for_step(&mut self) -> Statement {
        let start = self.current_span();
        let expr = self.parse_expression();

        // Check for ++ or --
        if self.eat(&Token::PlusPlus) {
            return Statement {
                span: start.merge(self.prev_span()),
                kind: StatementKind::Increment(expr),
            };
        }
        if self.eat(&Token::MinusMinus) {
            return Statement {
                span: start.merge(self.prev_span()),
                kind: StatementKind::Decrement(expr),
            };
        }

        // Check for compound assignment
        if let Some(op) = self.peek_compound_op() {
            self.advance();
            let rhs = self.parse_expression();
            return Statement {
                span: start.merge(self.prev_span()),
                kind: StatementKind::CompoundAssign(CompoundAssignStmt { lhs: expr, op, rhs }),
            };
        }

        // Check for assignment
        if let Some(op) = self.peek_assign_op() {
            self.advance();
            let rhs = self.parse_expression();
            return Statement {
                span: start.merge(self.prev_span()),
                kind: StatementKind::Assignment(AssignStmt { lhs: expr, op, rhs }),
            };
        }

        Statement {
            span: start.merge(self.prev_span()),
            kind: StatementKind::Expression(expr),
        }
    }

    fn parse_while_stmt(&mut self) -> Option<StatementKind> {
        self.advance(); // eat `while`
        self.expect(&Token::LParen);
        let cond = self.parse_expression();
        self.expect(&Token::RParen);
        let body = self.parse_block_or_single_stmt();
        Some(StatementKind::While(WhileLoop { cond, body }))
    }

    fn parse_return_stmt(&mut self) -> Option<StatementKind> {
        self.advance(); // eat `return`
        let value = self.parse_expression();
        self.expect_semi();
        Some(StatementKind::Return(ReturnStmt { value }))
    }

    fn parse_log_stmt(&mut self) -> Option<StatementKind> {
        self.advance(); // eat `log`
        self.expect(&Token::LParen);
        let mut args = Vec::new();
        if !self.check(&Token::RParen) {
            args.push(self.parse_log_arg());
            while self.eat(&Token::Comma) {
                args.push(self.parse_log_arg());
            }
        }
        self.expect(&Token::RParen);
        self.expect_semi();
        Some(StatementKind::Log(LogStmt { args }))
    }

    fn parse_log_arg(&mut self) -> LogArg {
        if let Some(Token::StringLit(_)) = self.peek() {
            if let Token::StringLit(s) = self.advance().unwrap().token.clone() {
                return LogArg::String(s);
            }
        }
        LogArg::Expr(self.parse_expression())
    }

    fn parse_assert_stmt(&mut self) -> Option<StatementKind> {
        self.advance(); // eat `assert`
        self.expect(&Token::LParen);
        let expr = self.parse_expression();
        self.expect(&Token::RParen);
        self.expect_semi();
        Some(StatementKind::Assert(AssertStmt { expr }))
    }

    fn parse_tuple_or_expr_stmt(&mut self) -> Option<StatementKind> {
        // Save position to backtrack
        let saved = self.pos;

        // Try parsing as tuple: (expr, expr, ...) <op> ...
        self.advance(); // eat `(`

        // Check if this is a tuple (has comma)
        let mut depth = 1;
        let mut has_comma = false;
        let mut check_pos = self.pos;
        while check_pos < self.tokens.len() && depth > 0 {
            match &self.tokens[check_pos].token {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                Token::Comma if depth == 1 => has_comma = true,
                _ => {}
            }
            check_pos += 1;
        }

        self.pos = saved;

        if has_comma {
            self.parse_tuple_assign_stmt()
        } else {
            self.parse_expr_stmt()
        }
    }

    fn parse_tuple_assign_stmt(&mut self) -> Option<StatementKind> {
        self.expect(&Token::LParen);
        let mut targets = Vec::new();
        if self.check(&Token::Underscore) {
            self.advance();
            targets.push(None);
        } else {
            targets.push(Some(self.parse_expression()));
        }
        while self.eat(&Token::Comma) {
            if self.check(&Token::Underscore) {
                self.advance();
                targets.push(None);
            } else {
                targets.push(Some(self.parse_expression()));
            }
        }
        self.expect(&Token::RParen);

        let op = self.peek_assign_op().unwrap_or(AssignOp::SafeLeft);
        if !self.eat_assign_op() {
            self.error("expected assignment operator after tuple");
        }

        let rhs = self.parse_expression();
        self.expect_semi();

        Some(StatementKind::TupleAssign(TupleAssignStmt {
            targets,
            op,
            rhs,
        }))
    }

    fn parse_expr_stmt(&mut self) -> Option<StatementKind> {
        let expr = self.parse_expression();

        // Check for ++ or --
        if self.eat(&Token::PlusPlus) {
            self.expect_semi();
            return Some(StatementKind::Increment(expr));
        }
        if self.eat(&Token::MinusMinus) {
            self.expect_semi();
            return Some(StatementKind::Decrement(expr));
        }

        // Check for constraint eq: ===
        if self.eat(&Token::ConstraintEq) {
            let rhs = self.parse_expression();
            self.expect_semi();
            return Some(StatementKind::ConstraintEq(ConstraintEqStmt {
                lhs: expr,
                rhs,
            }));
        }

        // Check for compound assignment
        if let Some(op) = self.peek_compound_op() {
            self.advance();
            let rhs = self.parse_expression();
            self.expect_semi();
            return Some(StatementKind::CompoundAssign(CompoundAssignStmt {
                lhs: expr,
                op,
                rhs,
            }));
        }

        // Check for assignment
        if let Some(op) = self.peek_assign_op() {
            self.advance();
            let rhs = self.parse_expression();
            self.expect_semi();
            return Some(StatementKind::Assignment(AssignStmt { lhs: expr, op, rhs }));
        }

        // Check for anonymous component call with signal assignment
        // e.g., `x.y <== expr`
        // Already handled by assignment above

        self.expect_semi();
        Some(StatementKind::Expression(expr))
    }

    fn peek_assign_op(&self) -> Option<AssignOp> {
        match self.peek()? {
            Token::Eq => Some(AssignOp::Eq),
            Token::LeftSignalAssign => Some(AssignOp::SafeLeft),
            Token::RightSignalAssign => Some(AssignOp::SafeRight),
            Token::LeftUnsafeAssign => Some(AssignOp::UnsafeLeft),
            Token::RightUnsafeAssign => Some(AssignOp::UnsafeRight),
            _ => None,
        }
    }

    fn eat_assign_op(&mut self) -> bool {
        if self.peek_assign_op().is_some() {
            self.advance();
            true
        } else {
            false
        }
    }

    fn peek_compound_op(&self) -> Option<CompoundOp> {
        match self.peek()? {
            Token::PlusAssign => Some(CompoundOp::AddAssign),
            Token::MinusAssign => Some(CompoundOp::SubAssign),
            Token::StarAssign => Some(CompoundOp::MulAssign),
            Token::PowerAssign => Some(CompoundOp::PowAssign),
            Token::SlashAssign => Some(CompoundOp::DivAssign),
            Token::IntDivAssign => Some(CompoundOp::IntDivAssign),
            Token::ModAssign => Some(CompoundOp::ModAssign),
            Token::ShlAssign => Some(CompoundOp::ShlAssign),
            Token::ShrAssign => Some(CompoundOp::ShrAssign),
            Token::BitAndAssign => Some(CompoundOp::BitAndAssign),
            Token::BitOrAssign => Some(CompoundOp::BitOrAssign),
            Token::BitXorAssign => Some(CompoundOp::BitXorAssign),
            _ => None,
        }
    }
}

// ── Utility parsers ─────────────────────────────────────────────────

impl Parser {
    fn parse_optional_tags(&mut self) -> Vec<Identifier> {
        let mut tags = Vec::new();
        if self.eat(&Token::LBrace) {
            let span = self.prev_span();
            if !self.check(&Token::RBrace) {
                tags.push(self.expect_ident());
                while self.eat(&Token::Comma) {
                    tags.push(self.expect_ident());
                }
            }
            self.expect(&Token::RBrace);
            self.require_version(
                &Version {
                    major: 2,
                    minor: 1,
                    patch: 0,
                },
                "signal tags",
                span,
            );
        }
        tags
    }

    fn parse_dimensions(&mut self) -> Vec<Expression> {
        let mut dims = Vec::new();
        while self.check(&Token::LBracket) {
            self.advance();
            dims.push(self.parse_expression());
            self.expect(&Token::RBracket);
        }
        dims
    }
}

// ── Expression parsing (Pratt parser) ───────────────────────────────

impl Parser {
    pub(crate) fn parse_expression(&mut self) -> Expression {
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> Expression {
        let start = self.current_span();
        let expr = self.parse_or();
        if self.eat(&Token::Question) {
            let then_expr = self.parse_expression();
            self.expect(&Token::Colon);
            let else_expr = self.parse_expression();
            let span = start.merge(self.prev_span());
            Expression {
                span,
                kind: Box::new(ExpressionKind::Ternary(expr, then_expr, else_expr)),
            }
        } else {
            expr
        }
    }

    fn parse_or(&mut self) -> Expression {
        let start = self.current_span();
        let mut left = self.parse_and();
        while self.check(&Token::Or) {
            self.advance();
            let right = self.parse_and();
            let span = start.merge(self.prev_span());
            left = Expression {
                span,
                kind: Box::new(ExpressionKind::Binary(left, BinaryOp::Or, right)),
            };
        }
        left
    }

    fn parse_and(&mut self) -> Expression {
        let start = self.current_span();
        let mut left = self.parse_bit_or();
        while self.check(&Token::And) {
            self.advance();
            let right = self.parse_bit_or();
            let span = start.merge(self.prev_span());
            left = Expression {
                span,
                kind: Box::new(ExpressionKind::Binary(left, BinaryOp::And, right)),
            };
        }
        left
    }

    fn parse_bit_or(&mut self) -> Expression {
        let start = self.current_span();
        let mut left = self.parse_bit_xor();
        while self.check(&Token::Pipe) {
            self.advance();
            let right = self.parse_bit_xor();
            let span = start.merge(self.prev_span());
            left = Expression {
                span,
                kind: Box::new(ExpressionKind::Binary(left, BinaryOp::BitOr, right)),
            };
        }
        left
    }

    fn parse_bit_xor(&mut self) -> Expression {
        let start = self.current_span();
        let mut left = self.parse_bit_and();
        while self.check(&Token::Caret) {
            self.advance();
            let right = self.parse_bit_and();
            let span = start.merge(self.prev_span());
            left = Expression {
                span,
                kind: Box::new(ExpressionKind::Binary(left, BinaryOp::BitXor, right)),
            };
        }
        left
    }

    fn parse_bit_and(&mut self) -> Expression {
        let start = self.current_span();
        let mut left = self.parse_equality();
        while self.check(&Token::Amp) {
            self.advance();
            let right = self.parse_equality();
            let span = start.merge(self.prev_span());
            left = Expression {
                span,
                kind: Box::new(ExpressionKind::Binary(left, BinaryOp::BitAnd, right)),
            };
        }
        left
    }

    fn parse_equality(&mut self) -> Expression {
        let start = self.current_span();
        let mut left = self.parse_relational();
        while let Some(op) = match self.peek() {
            Some(Token::EqEq) => Some(BinaryOp::Eq),
            Some(Token::Ne) => Some(BinaryOp::Ne),
            _ => None,
        } {
            self.advance();
            let right = self.parse_relational();
            let span = start.merge(self.prev_span());
            left = Expression {
                span,
                kind: Box::new(ExpressionKind::Binary(left, op, right)),
            };
        }
        left
    }

    fn parse_relational(&mut self) -> Expression {
        let start = self.current_span();
        let mut left = self.parse_shift();
        while let Some(op) = match self.peek() {
            Some(Token::Lt) => Some(BinaryOp::Lt),
            Some(Token::Gt) => Some(BinaryOp::Gt),
            Some(Token::Le) => Some(BinaryOp::Le),
            Some(Token::Ge) => Some(BinaryOp::Ge),
            _ => None,
        } {
            self.advance();
            let right = self.parse_shift();
            let span = start.merge(self.prev_span());
            left = Expression {
                span,
                kind: Box::new(ExpressionKind::Binary(left, op, right)),
            };
        }
        left
    }

    fn parse_shift(&mut self) -> Expression {
        let start = self.current_span();
        let mut left = self.parse_additive();
        while let Some(op) = match self.peek() {
            Some(Token::Shl) => Some(BinaryOp::Shl),
            Some(Token::Shr) => Some(BinaryOp::Shr),
            _ => None,
        } {
            self.advance();
            let right = self.parse_additive();
            let span = start.merge(self.prev_span());
            left = Expression {
                span,
                kind: Box::new(ExpressionKind::Binary(left, op, right)),
            };
        }
        left
    }

    fn parse_additive(&mut self) -> Expression {
        let start = self.current_span();
        let mut left = self.parse_multiplicative();
        while let Some(op) = match self.peek() {
            Some(Token::Plus) => Some(BinaryOp::Add),
            Some(Token::Minus) => Some(BinaryOp::Sub),
            _ => None,
        } {
            self.advance();
            let right = self.parse_multiplicative();
            let span = start.merge(self.prev_span());
            left = Expression {
                span,
                kind: Box::new(ExpressionKind::Binary(left, op, right)),
            };
        }
        left
    }

    fn parse_multiplicative(&mut self) -> Expression {
        let start = self.current_span();
        let mut left = self.parse_power();
        while let Some(op) = match self.peek() {
            Some(Token::Star) => Some(BinaryOp::Mul),
            Some(Token::Slash) => Some(BinaryOp::Div),
            Some(Token::IntDiv) => Some(BinaryOp::IntDiv),
            Some(Token::Mod) => Some(BinaryOp::Mod),
            _ => None,
        } {
            self.advance();
            let right = self.parse_power();
            let span = start.merge(self.prev_span());
            left = Expression {
                span,
                kind: Box::new(ExpressionKind::Binary(left, op, right)),
            };
        }
        left
    }

    fn parse_power(&mut self) -> Expression {
        let start = self.current_span();
        let base = self.parse_unary();
        // Power is right-associative
        if self.check(&Token::Power) {
            self.advance();
            let exp = self.parse_power(); // right-recursive
            let span = start.merge(self.prev_span());
            Expression {
                span,
                kind: Box::new(ExpressionKind::Binary(base, BinaryOp::Pow, exp)),
            }
        } else {
            base
        }
    }

    fn parse_unary(&mut self) -> Expression {
        let start = self.current_span();
        match self.peek() {
            Some(Token::Minus) => {
                self.advance();
                let expr = self.parse_unary();
                let span = start.merge(self.prev_span());
                Expression {
                    span,
                    kind: Box::new(ExpressionKind::Unary(UnaryOp::Neg, expr)),
                }
            }
            Some(Token::Bang) => {
                self.advance();
                let expr = self.parse_unary();
                let span = start.merge(self.prev_span());
                Expression {
                    span,
                    kind: Box::new(ExpressionKind::Unary(UnaryOp::Not, expr)),
                }
            }
            Some(Token::Tilde) => {
                self.advance();
                let expr = self.parse_unary();
                let span = start.merge(self.prev_span());
                Expression {
                    span,
                    kind: Box::new(ExpressionKind::Unary(UnaryOp::BitNot, expr)),
                }
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_call_or_anon_comp(&mut self, start: Span, callee: Expression) -> Expression {
        self.advance();
        let args = self.parse_call_args();
        self.expect(&Token::RParen);

        // Check for anonymous component: Template(params)(inputs)
        if self.check(&Token::LParen) {
            self.advance();
            let inputs = self.parse_anon_comp_inputs();
            self.expect(&Token::RParen);
            Expression {
                span: start.merge(self.prev_span()),
                kind: Box::new(ExpressionKind::AnonymousComp(AnonymousComp {
                    template: callee,
                    template_args: args,
                    inputs,
                })),
            }
        } else {
            Expression {
                span: start.merge(self.prev_span()),
                kind: Box::new(ExpressionKind::Call(callee, args)),
            }
        }
    }

    fn parse_postfix(&mut self) -> Expression {
        let start = self.current_span();
        let mut expr = self.parse_primary();

        loop {
            match self.peek() {
                Some(Token::LBracket) => {
                    self.advance();
                    let index = self.parse_expression();
                    self.expect(&Token::RBracket);
                    let span = start.merge(self.prev_span());
                    expr = Expression {
                        span,
                        kind: Box::new(ExpressionKind::Index(expr, index)),
                    };
                }
                Some(Token::Dot) => {
                    self.advance();
                    let field = self.expect_ident();
                    let span = start.merge(self.prev_span());
                    expr = Expression {
                        span,
                        kind: Box::new(ExpressionKind::Member(expr, field)),
                    };
                }
                Some(Token::LParen) => {
                    expr = self.parse_call_or_anon_comp(start, expr);
                }
                _ => break,
            }
        }
        expr
    }

    fn parse_call_args(&mut self) -> Vec<Expression> {
        let mut args = Vec::new();
        if !self.check(&Token::RParen) {
            args.push(self.parse_expression());
            while self.eat(&Token::Comma) {
                args.push(self.parse_expression());
            }
        }
        args
    }

    fn parse_anon_comp_inputs(&mut self) -> Vec<AnonCompInput> {
        let mut inputs = Vec::new();
        if !self.check(&Token::RParen) {
            inputs.push(self.parse_anon_comp_input());
            while self.eat(&Token::Comma) {
                inputs.push(self.parse_anon_comp_input());
            }
        }
        inputs
    }

    fn parse_anon_comp_input(&mut self) -> AnonCompInput {
        // Check for named input: `name <== expr`
        if let Some(Token::Ident(_)) = self.peek() {
            // Look ahead for <==
            if let Some(next) = self.tokens.get(self.pos + 1) {
                if matches!(next.token, Token::LeftSignalAssign) {
                    let name = self.expect_ident();
                    self.advance(); // eat <==
                    let expr = self.parse_expression();
                    return AnonCompInput::Named(name, expr);
                }
            }
        }
        AnonCompInput::Positional(self.parse_expression())
    }

    fn parse_array_lit_tail(&mut self, start: Span) -> Expression {
        let mut elems = Vec::new();
        if !self.check(&Token::RBracket) {
            elems.push(self.parse_expression());
            while self.eat(&Token::Comma) {
                if self.check(&Token::RBracket) {
                    break; // trailing comma
                }
                elems.push(self.parse_expression());
            }
        }
        self.expect(&Token::RBracket);
        Expression {
            span: start.merge(self.prev_span()),
            kind: Box::new(ExpressionKind::ArrayLit(elems)),
        }
    }

    fn parse_paren_tail(&mut self, start: Span) -> Expression {
        let expr = self.parse_expression();
        self.expect(&Token::RParen);
        Expression {
            span: start.merge(self.prev_span()),
            kind: Box::new(ExpressionKind::Paren(expr)),
        }
    }

    fn parse_primary_number(&mut self, start: Span) -> Expression {
        if let Token::NumberLit(s) = self.advance().unwrap().token.clone() {
            Expression {
                span: start,
                kind: Box::new(ExpressionKind::Number(s)),
            }
        } else {
            unreachable!()
        }
    }

    fn parse_primary_ident(&mut self, start: Span) -> Expression {
        if let Token::Ident(s) = self.advance().unwrap().token.clone() {
            Expression {
                span: start,
                kind: Box::new(ExpressionKind::Ident(s)),
            }
        } else {
            unreachable!()
        }
    }

    fn parse_primary_error(&mut self, start: Span) -> Expression {
        self.error("expected expression");
        // Advance past the unexpected token so callers don't loop
        // on the same position
        if self.peek().is_some() {
            self.advance();
        }
        Expression {
            span: start,
            kind: Box::new(ExpressionKind::Error),
        }
    }

    fn parse_primary(&mut self) -> Expression {
        let start = self.current_span();
        match self.peek() {
            Some(Token::NumberLit(_)) => self.parse_primary_number(start),
            Some(Token::Ident(_)) => self.parse_primary_ident(start),
            Some(Token::Parallel) => {
                self.advance();
                let expr = self.parse_expression();
                Expression {
                    span: start.merge(self.prev_span()),
                    kind: Box::new(ExpressionKind::Parallel(expr)),
                }
            }
            Some(Token::Underscore) => {
                self.advance();
                Expression {
                    span: start,
                    kind: Box::new(ExpressionKind::Underscore),
                }
            }
            Some(Token::LParen) => {
                self.advance();
                self.parse_paren_tail(start)
            }
            Some(Token::LBracket) => {
                self.advance();
                self.parse_array_lit_tail(start)
            }
            _ => self.parse_primary_error(start),
        }
    }
}

/// Parse a Circom numeric literal string into a `u32`. Handles both
/// decimal (`123`) and hex (`0x6a09e667` / `0X...`) forms; returns
/// `None` if the value overflows u32 or the string is malformed. The
/// lexer ensures only these two shapes ever reach us.
pub(crate) fn parse_number_literal_u32(s: &str) -> Option<u32> {
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(rest, 16).ok()
    } else {
        s.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(src: &str) -> File {
        let (file, errors) = parse(src);
        assert!(errors.is_empty(), "unexpected errors: {:#?}", errors);
        file
    }

    fn parse_with_errors(src: &str) -> (File, Vec<ParseError>) {
        parse(src)
    }

    const RESERVED_KEYWORDS: [&str; 14] = [
        "signal",
        "template",
        "function",
        "component",
        "var",
        "bus",
        "input",
        "output",
        "public",
        "custom",
        "parallel",
        "extern",
        "log",
        "assert",
    ];

    // ── Pragma tests ────────────────────────────────────────────

    #[test]
    fn test_pragma_version() {
        let file = parse_ok("pragma circom 2.2.3;");
        assert_eq!(file.items.len(), 1);
        match &file.items[0] {
            Item::Pragma(p) => match &p.kind {
                PragmaKind::Version(v) => {
                    assert_eq!(v.major, 2);
                    assert_eq!(v.minor, 2);
                    assert_eq!(v.patch, 3);
                }
                _ => panic!("expected version pragma"),
            },
            _ => panic!("expected pragma item"),
        }
    }

    #[test]
    fn test_duplicate_pragma_error() {
        let (_, errors) = parse_with_errors("pragma circom 2.0.0; pragma circom 2.1.0;");
        assert!(
            errors.iter().any(|e| e.message.contains("duplicate")),
            "should report error for duplicate pragma, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_pragma_custom_templates() {
        let file = parse_ok("pragma custom_templates;");
        match &file.items[0] {
            Item::Pragma(p) => assert_eq!(p.kind, PragmaKind::CustomTemplates),
            _ => panic!("expected pragma"),
        }
    }

    #[test]
    fn test_pragma_version_overflow() {
        let (_, errors) = parse_with_errors("pragma circom 99999999999.0.0;");
        assert!(
            errors.iter().any(|e| e.message.contains("overflows")),
            "expected overflow error, got: {:?}",
            errors
        );
    }

    // ── Include tests ───────────────────────────────────────────

    #[test]
    fn test_include() {
        let file = parse_ok(r#"include "circomlib/poseidon.circom";"#);
        match &file.items[0] {
            Item::Include(i) => assert_eq!(i.path, "circomlib/poseidon.circom"),
            _ => panic!("expected include"),
        }
    }

    // ── Template tests ──────────────────────────────────────────

    #[test]
    fn test_template_basic() {
        let file = parse_ok("template Multiplier2() { signal input a; signal input b; signal output c; c <== a * b; }");
        match &file.items[0] {
            Item::TemplateDef(t) => {
                assert_eq!(t.name.name, "Multiplier2");
                assert_eq!(t.params.len(), 0);
                assert!(!t.is_custom);
                assert!(!t.is_parallel);
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_template_with_params() {
        let file = parse_ok("template Bits2Num(n) { signal input in[n]; signal output out; }");
        match &file.items[0] {
            Item::TemplateDef(t) => {
                assert_eq!(t.name.name, "Bits2Num");
                assert_eq!(t.params.len(), 1);
                assert_eq!(t.params[0].name, "n");
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_template_parallel() {
        let file = parse_ok("template parallel ParallelMul() {}");
        match &file.items[0] {
            Item::TemplateDef(t) => {
                assert!(t.is_parallel);
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_template_custom() {
        let file = parse_ok("pragma custom_templates; template custom MyCustom() {}");
        match &file.items[1] {
            Item::TemplateDef(t) => {
                assert!(t.is_custom);
            }
            _ => panic!("expected template"),
        }
    }

    // ── Function tests ──────────────────────────────────────────

    #[test]
    fn test_function_basic() {
        let file = parse_ok("function nbits(a) { var n = 1; var r = 0; while (n - 1 < a) { r++; n *= 2; } return r; }");
        match &file.items[0] {
            Item::FunctionDef(f) => {
                assert_eq!(f.name.name, "nbits");
                assert_eq!(f.params.len(), 1);
                assert_eq!(f.params[0].name, "a");
            }
            _ => panic!("expected function"),
        }
    }

    // ── Bus tests ───────────────────────────────────────────────

    #[test]
    fn test_bus_simple() {
        let file = parse_ok("bus Point() { signal x; signal y; }");
        match &file.items[0] {
            Item::BusDef(b) => {
                assert_eq!(b.name.name, "Point");
                assert_eq!(b.body.len(), 2);
            }
            _ => panic!("expected bus"),
        }
    }

    #[test]
    fn test_bus_with_params() {
        let file = parse_ok("bus PointN(dim) { signal x[dim]; }");
        match &file.items[0] {
            Item::BusDef(b) => {
                assert_eq!(b.name.name, "PointN");
                assert_eq!(b.params.len(), 1);
            }
            _ => panic!("expected bus"),
        }
    }

    #[test]
    fn test_bus_nested() {
        let src = "bus Point() { signal x; signal y; } bus Line() { Point() start; Point() end; }";
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 2);
        match &file.items[1] {
            Item::BusDef(b) => {
                assert_eq!(b.name.name, "Line");
                assert_eq!(b.body.len(), 2);
                assert!(matches!(&b.body[0], BusMember::Bus(_)));
            }
            _ => panic!("expected bus"),
        }
    }

    // ── Main component tests ────────────────────────────────────

    #[test]
    fn test_main_component_no_public() {
        let file = parse_ok("component main = Multiplier2();");
        match &file.items[0] {
            Item::MainComponent(m) => {
                assert!(m.public_signals.is_empty());
            }
            _ => panic!("expected main component"),
        }
    }

    #[test]
    fn test_main_component_with_public() {
        let file = parse_ok("component main {public [in1, in2]} = Multiplier2();");
        match &file.items[0] {
            Item::MainComponent(m) => {
                assert_eq!(m.public_signals.len(), 2);
                assert_eq!(m.public_signals[0].name, "in1");
                assert_eq!(m.public_signals[1].name, "in2");
            }
            _ => panic!("expected main component"),
        }
    }

    // ── Signal declaration tests ────────────────────────────────

    #[test]
    fn test_signal_input() {
        let src = "template T() { signal input a; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[0].kind {
                StatementKind::SignalDecl(s) => {
                    assert_eq!(s.kind, SignalKind::Input);
                    assert_eq!(s.names[0].name.name, "a");
                }
                _ => panic!("expected signal decl"),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_signal_with_tags() {
        let src = "template T() { signal input {binary} in[n]; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[0].kind {
                StatementKind::SignalDecl(s) => {
                    assert_eq!(s.tags.len(), 1);
                    assert_eq!(s.tags[0].name, "binary");
                    assert!(!s.names[0].dimensions.is_empty());
                }
                _ => panic!("expected signal decl"),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_signal_init_on_decl() {
        let src = "template T() { signal output out <== in1 * in2; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[0].kind {
                StatementKind::SignalDecl(s) => {
                    assert!(s.names[0].init.is_some());
                }
                _ => panic!("expected signal decl"),
            },
            _ => panic!("expected template"),
        }
    }

    // ── Variable declaration tests ──────────────────────────────

    #[test]
    fn test_var_decl() {
        let src = "function f() { var x = 5; return x; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::VarDecl(v) => {
                    assert_eq!(v.names[0].name.name, "x");
                    assert!(v.names[0].init.is_some());
                }
                _ => panic!("expected var decl"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_var_decl_array() {
        let src = "function f() { var x[3]; return x; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::VarDecl(v) => {
                    assert_eq!(v.names[0].dimensions.len(), 1);
                }
                _ => panic!("expected var decl"),
            },
            _ => panic!("expected function"),
        }
    }

    // ── Component declaration tests ─────────────────────────────

    #[test]
    fn test_component_decl() {
        let src = "template T() { component c = Multiplier2(); }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[0].kind {
                StatementKind::ComponentDecl(c) => {
                    assert_eq!(c.names[0].name.name, "c");
                    assert!(c.names[0].init.is_some());
                }
                _ => panic!("expected component decl"),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_component_array() {
        let src = "template T() { component ands[2]; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[0].kind {
                StatementKind::ComponentDecl(c) => {
                    assert_eq!(c.names[0].dimensions.len(), 1);
                    assert!(c.names[0].init.is_none());
                }
                _ => panic!("expected component decl"),
            },
            _ => panic!("expected template"),
        }
    }

    // ── Assignment tests ────────────────────────────────────────

    #[test]
    fn test_signal_assign_left() {
        let src = "template T() { signal input a; signal output b; b <== a; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[2].kind {
                StatementKind::Assignment(a) => {
                    assert_eq!(a.op, AssignOp::SafeLeft);
                }
                _ => panic!("expected assignment, got {:?}", t.body.stmts[2].kind),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_constraint_eq() {
        let src = "template T() { signal a; signal b; a === b; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => {
                assert!(matches!(
                    &t.body.stmts[2].kind,
                    StatementKind::ConstraintEq(_)
                ));
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_compound_assign() {
        let src = "function f() { var x = 0; x += 1; return x; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[1].kind {
                StatementKind::CompoundAssign(c) => {
                    assert_eq!(c.op, CompoundOp::AddAssign);
                }
                _ => panic!("expected compound assign"),
            },
            _ => panic!("expected function"),
        }
    }

    // ── Control flow tests ──────────────────────────────────────

    #[test]
    fn test_if_else() {
        let src = "function f(x) { if (x >= 0) { return x; } else { return 0; } }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::IfElse(ie) => {
                    assert!(ie.else_body.is_some());
                }
                _ => panic!("expected if/else"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_for_loop() {
        let src = "function f() { var y = 0; for (var i = 0; i < 100; i++) { y++; } return y; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => {
                assert!(matches!(&f.body.stmts[1].kind, StatementKind::For(_)));
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_while_loop() {
        let src = "function f() { var i = 0; while (i < 100) { i++; } return i; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => {
                assert!(matches!(&f.body.stmts[1].kind, StatementKind::While(_)));
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_return_stmt() {
        let src = "function f() { return 42; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::Return(r) => {
                    assert!(matches!(*r.value.kind, ExpressionKind::Number(_)));
                }
                _ => panic!("expected return"),
            },
            _ => panic!("expected function"),
        }
    }

    // ── Expression tests ────────────────────────────────────────

    #[test]
    fn test_operator_precedence() {
        // a + b * c should parse as a + (b * c)
        let src = "function f() { return a + b * c; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::Return(r) => match r.value.kind.as_ref() {
                    ExpressionKind::Binary(_, BinaryOp::Add, rhs) => {
                        assert!(matches!(
                            *rhs.kind,
                            ExpressionKind::Binary(_, BinaryOp::Mul, _)
                        ));
                    }
                    other => panic!("expected Add at top level, got {:?}", other),
                },
                _ => panic!("expected return"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_power_right_associative() {
        // a ** b ** c should parse as a ** (b ** c)
        let src = "function f() { return a ** b ** c; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::Return(r) => match r.value.kind.as_ref() {
                    ExpressionKind::Binary(_, BinaryOp::Pow, rhs) => {
                        assert!(matches!(
                            *rhs.kind,
                            ExpressionKind::Binary(_, BinaryOp::Pow, _)
                        ));
                    }
                    other => panic!("expected Pow at top, got {:?}", other),
                },
                _ => panic!("expected return"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_ternary() {
        let src = "function f(x) { return x > 0 ? x : 0; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::Return(r) => {
                    assert!(matches!(*r.value.kind, ExpressionKind::Ternary(_, _, _)));
                }
                _ => panic!("expected return"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_unary_neg() {
        let src = "function f() { return -x; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::Return(r) => {
                    assert!(matches!(
                        *r.value.kind,
                        ExpressionKind::Unary(UnaryOp::Neg, _)
                    ));
                }
                _ => panic!("expected return"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_array_index() {
        let src = "function f() { return x[0]; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::Return(r) => {
                    assert!(matches!(*r.value.kind, ExpressionKind::Index(_, _)));
                }
                _ => panic!("expected return"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_member_access() {
        let src = "template T() { signal a; a.tag = 5; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[1].kind {
                StatementKind::Assignment(a) => {
                    assert!(matches!(*a.lhs.kind, ExpressionKind::Member(_, _)));
                }
                _ => panic!("expected assignment, got {:?}", t.body.stmts[1].kind),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_function_call() {
        let src = "function f() { return nbits(32); }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::Return(r) => {
                    assert!(matches!(*r.value.kind, ExpressionKind::Call(_, _)));
                }
                _ => panic!("expected return"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_array_literal() {
        let src = "function f() { return [1, 2, 3]; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::Return(r) => match r.value.kind.as_ref() {
                    ExpressionKind::ArrayLit(elems) => {
                        assert_eq!(elems.len(), 3);
                    }
                    _ => panic!("expected array literal"),
                },
                _ => panic!("expected return"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_hex_number_literal_in_array() {
        // Regression: circomlib's sha256/constants.circom uses hex
        // literals; pre-fix the lexer split `0x6a09e667` into
        // `0` + `x6a09e667` and the parser reported
        // `expected RBracket, found Ident(...)`.
        let src = r#"
            template H() {
                signal output out;
                var c[8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                            0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
                out <== c[0];
            }
        "#;
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => {
                // The `var c[8] = [...];` is statement 1 (after the signal
                // output decl); validate it parsed as an array of 8 numbers.
                let stmts = &t.body.stmts;
                let init = stmts
                    .iter()
                    .find_map(|s| match &s.kind {
                        StatementKind::VarDecl(v) => v.names[0].init.clone(),
                        _ => None,
                    })
                    .expect("expected a var with initializer");
                match init.kind.as_ref() {
                    ExpressionKind::ArrayLit(elems) => {
                        assert_eq!(elems.len(), 8);
                        for (i, e) in elems.iter().enumerate() {
                            match e.kind.as_ref() {
                                ExpressionKind::Number(n) => {
                                    assert!(
                                        n.starts_with("0x"),
                                        "elem {} not hex: {n}",
                                        i
                                    );
                                }
                                other => panic!("elem {i} not a Number: {other:?}"),
                            }
                        }
                    }
                    other => panic!("expected array literal, got {other:?}"),
                }
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_parse_number_literal_u32_hex_and_decimal() {
        assert_eq!(parse_number_literal_u32("42"), Some(42));
        assert_eq!(parse_number_literal_u32("0x6a09e667"), Some(0x6a09e667));
        assert_eq!(parse_number_literal_u32("0X6A09E667"), Some(0x6a09e667));
        // Overflow:
        assert_eq!(parse_number_literal_u32("99999999999"), None);
    }

    // ── Tuple assignment tests ──────────────────────────────────

    #[test]
    fn test_tuple_assign() {
        let src =
            "template T() { signal output a; signal output b; (a, b) <== SomeTemplate()(inp); }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[2].kind {
                StatementKind::TupleAssign(ta) => {
                    assert_eq!(ta.targets.len(), 2);
                    assert_eq!(ta.op, AssignOp::SafeLeft);
                }
                _ => panic!("expected tuple assign, got {:?}", t.body.stmts[2].kind),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_tuple_with_underscore() {
        let src = "template T() { signal output a; (_, a) <== SomeTemplate()(inp); }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[1].kind {
                StatementKind::TupleAssign(ta) => {
                    assert!(ta.targets[0].is_none());
                    assert!(ta.targets[1].is_some());
                }
                _ => panic!("expected tuple assign"),
            },
            _ => panic!("expected template"),
        }
    }

    // ── Log and Assert tests ────────────────────────────────────

    #[test]
    fn test_log_stmt() {
        let src = r#"template T() { log("value:", x); }"#;
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[0].kind {
                StatementKind::Log(l) => {
                    assert_eq!(l.args.len(), 2);
                    assert!(matches!(&l.args[0], LogArg::String(s) if s == "value:"));
                    assert!(matches!(&l.args[1], LogArg::Expr(_)));
                }
                _ => panic!("expected log"),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_assert_stmt() {
        let src = "template T() { assert(x > 0); }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => {
                assert!(matches!(&t.body.stmts[0].kind, StatementKind::Assert(_)));
            }
            _ => panic!("expected template"),
        }
    }

    // ── Anonymous component tests ───────────────────────────────

    #[test]
    fn test_anonymous_component() {
        let src = "template T() { signal output out; out <== Multiplier2()(a, b); }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[1].kind {
                StatementKind::Assignment(a) => {
                    assert!(matches!(*a.rhs.kind, ExpressionKind::AnonymousComp(_)));
                }
                _ => panic!("expected assignment"),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_anonymous_component_named_inputs() {
        let src = "template T() { signal output out; out <== A(n)(b <== in1, a <== in0); }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[1].kind {
                StatementKind::Assignment(a) => match a.rhs.kind.as_ref() {
                    ExpressionKind::AnonymousComp(ac) => {
                        assert_eq!(ac.inputs.len(), 2);
                        assert!(
                            matches!(&ac.inputs[0], AnonCompInput::Named(n, _) if n.name == "b")
                        );
                    }
                    _ => panic!("expected anonymous comp"),
                },
                _ => panic!("expected assignment"),
            },
            _ => panic!("expected template"),
        }
    }

    // ── Increment / Decrement tests ─────────────────────────────

    #[test]
    fn test_increment() {
        let src = "function f() { var i = 0; i++; return i; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => {
                assert!(matches!(&f.body.stmts[1].kind, StatementKind::Increment(_)));
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_decrement() {
        let src = "function f() { var i = 10; i--; return i; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => {
                assert!(matches!(&f.body.stmts[1].kind, StatementKind::Decrement(_)));
            }
            _ => panic!("expected function"),
        }
    }

    // ── Error recovery tests ────────────────────────────────────

    #[test]
    fn test_error_recovery_missing_semicolon() {
        let (file, errors) = parse_with_errors("template T() { signal input a signal output b; }");
        // Should still parse despite the missing semicolon
        assert!(!errors.is_empty());
        // The template itself should be parsed
        assert_eq!(file.items.len(), 1);
    }

    #[test]
    fn test_error_recovery_unclosed_brace() {
        let (_file, errors) = parse_with_errors("template T() { signal input a; ");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_error_recovery_multiple_errors() {
        let (_file, errors) = parse_with_errors("template T() { signal a signal b signal c; }");
        // Parser should report multiple errors, not just the first
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_signal_named_as_keyword() {
        // Reserved keywords must not be accepted as signal names
        for kw in RESERVED_KEYWORDS {
            let src = format!("template T() {{ signal input {}; }}", kw);
            let (_file, errors) = parse_with_errors(&src);
            assert!(
                !errors.is_empty(),
                "expected error for signal named `{}`, but got none",
                kw,
            );
        }
    }

    #[test]
    fn test_var_named_as_keyword() {
        for kw in RESERVED_KEYWORDS {
            let src = format!("template T() {{ var {}; }}", kw);
            let (_file, errors) = parse_with_errors(&src);
            assert!(
                !errors.is_empty(),
                "expected error for var named `{}`, but got none",
                kw,
            );
        }
    }

    #[test]
    fn test_component_named_as_keyword() {
        for kw in RESERVED_KEYWORDS {
            let src = format!("template T() {{ component {} = Foo(); }}", kw);
            let (_file, errors) = parse_with_errors(&src);
            assert!(
                !errors.is_empty(),
                "expected error for component named `{}`, but got none",
                kw,
            );
        }
    }

    #[test]
    fn test_bus_instance_named_as_keyword() {
        for kw in RESERVED_KEYWORDS {
            let src = format!(
                "pragma circom 2.2.0; template T() {{ signal input MyBus() {}; }}",
                kw
            );
            let (_file, errors) = parse_with_errors(&src);
            assert!(
                !errors.is_empty(),
                "expected error for bus instance named `{}`, but got none",
                kw,
            );
        }
    }

    #[test]
    fn test_main_allowed_as_identifier() {
        // `main` is a contextual keyword — allowed as a signal, var, and bus
        // instance name. Note: `component main` is excluded because it has
        // special top-level semantics in circom (`component main = ...`).
        let src = "template T() { signal input main; }";
        let _file = parse_ok(src);

        let src = "template T() { var main; }";
        let _file = parse_ok(src);

        let src = "pragma circom 2.2.0; template T() { signal input MyBus() main; }";
        let _file = parse_ok(src);
    }

    #[test]
    fn test_template_named_as_keyword() {
        for kw in RESERVED_KEYWORDS {
            let src = format!("template {}() {{}}", kw);
            let (_file, errors) = parse_with_errors(&src);
            assert!(
                !errors.is_empty(),
                "expected error for template named `{}`, but got none",
                kw,
            );
        }
    }

    #[test]
    fn test_function_named_as_keyword() {
        for kw in RESERVED_KEYWORDS {
            let src = format!("function {}() {{ return 0; }}", kw);
            let (_file, errors) = parse_with_errors(&src);
            assert!(
                !errors.is_empty(),
                "expected error for function named `{}`, but got none",
                kw,
            );
        }
    }

    #[test]
    fn test_bus_def_named_as_keyword() {
        for kw in RESERVED_KEYWORDS {
            let src = format!("pragma circom 2.2.0; bus {}() {{}}", kw);
            let (_file, errors) = parse_with_errors(&src);
            assert!(
                !errors.is_empty(),
                "expected error for bus named `{}`, but got none",
                kw,
            );
        }
    }

    #[test]
    fn test_bus_field_named_as_keyword() {
        for kw in RESERVED_KEYWORDS {
            let src = format!("pragma circom 2.2.0; bus B() {{ MyBus() {}; }}", kw);
            let (_file, errors) = parse_with_errors(&src);
            assert!(
                !errors.is_empty(),
                "expected error for bus field named `{}`, but got none",
                kw,
            );
        }
    }

    // ── Complex integration tests ───────────────────────────────

    #[test]
    fn test_full_multiplier_circuit() {
        let src = r#"
            pragma circom 2.0.0;

            template Multiplier2() {
                signal input a;
                signal input b;
                signal output c;

                c <== a * b;
            }

            component main {public [a]} = Multiplier2();
        "#;
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 3); // pragma + template + main
    }

    #[test]
    fn test_full_bits2num() {
        let src = r#"
            pragma circom 2.1.0;

            template Bits2Num(n) {
                signal input {binary} in[n];
                signal output {maxbit} out;

                var lc1 = 0;
                var e2 = 1;
                for (var i = 0; i < n; i++) {
                    lc1 += in[i] * e2;
                    e2 = e2 + e2;
                }

                out.maxbit = n;
                lc1 ==> out;
            }
        "#;
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 2);
    }

    #[test]
    fn test_extern_custom_template() {
        let src = r#"
            pragma circom 2.2.3;
            pragma custom_templates;

            template custom extern A() {
                signal input in;
                signal output out;
            }
        "#;
        let file = parse_ok(src);
        match &file.items[2] {
            Item::TemplateDef(t) => {
                assert!(t.is_custom);
                assert!(t.is_extern);
                assert_eq!(t.name.name, "A");
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_bitwise_operations() {
        let src = "function f(x) { return (x >> 3) & 1; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::Return(r) => {
                    assert!(matches!(
                        *r.value.kind,
                        ExpressionKind::Binary(_, BinaryOp::BitAnd, _)
                    ));
                }
                _ => panic!("expected return"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_multidimensional_array() {
        let src = "template T() { signal input matrix[3][4]; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[0].kind {
                StatementKind::SignalDecl(s) => {
                    assert_eq!(s.names[0].dimensions.len(), 2);
                }
                _ => panic!("expected signal decl"),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_nested_if_else() {
        let src = "function f(x) { if (x > 0) { if (x > 10) { return 2; } else { return 1; } } else { return 0; } }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::IfElse(ie) => {
                    assert!(ie.else_body.is_some());
                    match &ie.then_body.stmts[0].kind {
                        StatementKind::IfElse(inner) => {
                            assert!(inner.else_body.is_some());
                        }
                        _ => panic!("expected nested if/else"),
                    }
                }
                _ => panic!("expected if/else"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_dot_access_on_component() {
        let src = "template T() { component c = Mul(); c.a <== 5; c.b <== 3; signal output out; out <== c.c; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => {
                // c.a <== 5
                match &t.body.stmts[1].kind {
                    StatementKind::Assignment(a) => {
                        assert!(matches!(*a.lhs.kind, ExpressionKind::Member(_, _)));
                        assert_eq!(a.op, AssignOp::SafeLeft);
                    }
                    _ => panic!("expected assignment, got {:?}", t.body.stmts[1].kind),
                }
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_parallel_component_instantiation() {
        let src = "template T() { component c = parallel Heavy(); }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[0].kind {
                StatementKind::ComponentDecl(c) => {
                    assert!(c.is_parallel);
                }
                _ => panic!("expected component decl"),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_boolean_operators_precedence() {
        // a && b || c should parse as (a && b) || c
        let src = "function f() { return a && b || c; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::Return(r) => match r.value.kind.as_ref() {
                    ExpressionKind::Binary(_, BinaryOp::Or, _) => {}
                    other => panic!("expected Or at top, got {:?}", other),
                },
                _ => panic!("expected return"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_complex_expression() {
        let src = "function f() { return (a + b) * c - d / e % f; }";
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 1);
    }

    #[test]
    fn test_bus_with_tagged_signals() {
        let src = "bus Book() { signal {maxvalue} title[50]; signal {maxvalue} year; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::BusDef(b) => {
                assert_eq!(b.name.name, "Book");
                assert_eq!(b.body.len(), 2);
                match &b.body[0] {
                    BusMember::Signal(s) => {
                        assert_eq!(s.tags.len(), 1);
                        assert_eq!(s.tags[0].name, "maxvalue");
                    }
                    _ => panic!("expected signal member"),
                }
            }
            _ => panic!("expected bus"),
        }
    }

    #[test]
    fn test_multiple_var_decl() {
        let src = "function f() { var a, b, c; return a; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::VarDecl(v) => {
                    assert_eq!(v.names.len(), 3);
                }
                _ => panic!("expected var decl"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_var_decl_per_variable_init() {
        let src = "function f() { var a = 1, b = 2; return a + b; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::VarDecl(v) => {
                    assert_eq!(v.names.len(), 2);
                    assert!(v.names[0].init.is_some(), "a should have init");
                    assert!(v.names[1].init.is_some(), "b should have init");
                }
                _ => panic!("expected var decl"),
            },
            _ => panic!("expected function"),
        }
    }

    // ── Version gate tests ──────────────────────────────────────

    #[test]
    fn test_tags_accepted_with_version_2_1_0() {
        let src = "pragma circom 2.1.0; template T() { signal input {binary} x; }";
        let file = parse_ok(src);
        match &file.items[1] {
            Item::TemplateDef(t) => match &t.body.stmts[0].kind {
                StatementKind::SignalDecl(s) => {
                    assert_eq!(s.tags.len(), 1);
                    assert_eq!(s.tags[0].name, "binary");
                }
                _ => panic!("expected signal decl"),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_tags_rejected_before_version_2_1_0() {
        let src = "pragma circom 2.0.0; template T() { signal input {binary} x; }";
        let (_, errors) = parse_with_errors(src);
        assert!(
            errors.iter().any(|e| e.message.contains("tags")),
            "expected version gate error for tags, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_tags_accepted_with_higher_version() {
        let src = "pragma circom 2.2.0; template T() { signal input {binary} x; }";
        let file = parse_ok(src);
        match &file.items[1] {
            Item::TemplateDef(t) => match &t.body.stmts[0].kind {
                StatementKind::SignalDecl(s) => {
                    assert_eq!(s.tags.len(), 1);
                }
                _ => panic!("expected signal decl"),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_bus_def_accepted_with_version_2_2_0() {
        let src = "pragma circom 2.2.0; bus Point() { signal x; signal y; }";
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 2);
        assert!(matches!(&file.items[1], Item::BusDef(_)));
    }

    #[test]
    fn test_bus_def_rejected_before_version_2_2_0() {
        let src = "pragma circom 2.1.0; bus Point() { signal x; signal y; }";
        let (_, errors) = parse_with_errors(src);
        assert!(
            errors.iter().any(|e| e.message.contains("bus")),
            "expected version gate error for bus, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_bus_instance_rejected_before_version_2_2_0() {
        let src =
            "pragma circom 2.1.0; bus Point() { signal x; } template T() { signal input Point() p; }";
        let (_, errors) = parse_with_errors(src);
        assert!(
            errors.iter().any(|e| e.message.contains("bus")),
            "expected version gate error for bus instance, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_bus_accepted_with_version_2_2_3() {
        let src = "pragma circom 2.2.3; bus Point() { signal x; }";
        let file = parse_ok(src);
        assert!(matches!(&file.items[1], Item::BusDef(_)));
    }

    #[test]
    fn test_no_pragma_allows_all_features() {
        // Without a pragma, no version gate errors
        let src = "bus Point() { signal x; }";
        let file = parse_ok(src);
        assert!(matches!(&file.items[0], Item::BusDef(_)));
    }

    #[test]
    fn test_tags_in_bus_rejected_before_2_1_0() {
        let src = "pragma circom 2.0.0; bus Book() { signal {maxvalue} title[50]; }";
        let (_, errors) = parse_with_errors(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("tags") || e.message.contains("bus")),
            "expected version gate error, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_extern_template_accepted_with_version_2_2_3() {
        let src = r#"
            pragma circom 2.2.3;
            pragma custom_templates;
            template custom extern A() { signal input in; signal output out; }
        "#;
        let file = parse_ok(src);
        match &file.items[2] {
            Item::TemplateDef(t) => {
                assert!(t.is_extern);
                assert!(t.is_custom);
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn test_extern_template_rejected_before_version_2_2_3() {
        let src = r#"
            pragma circom 2.2.0;
            pragma custom_templates;
            template custom extern A() { signal input in; signal output out; }
        "#;
        let (_, errors) = parse_with_errors(src);
        assert!(
            errors.iter().any(|e| e.message.contains("extern")),
            "expected version gate error for extern templates, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_extern_template_rejected_with_version_2_1_0() {
        let src = r#"
            pragma circom 2.1.0;
            pragma custom_templates;
            template custom extern B() { signal input in; }
        "#;
        let (_, errors) = parse_with_errors(src);
        assert!(
            errors.iter().any(|e| e.message.contains("extern")),
            "expected version gate error for extern templates, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_no_pragma_allows_extern_template() {
        let src = r#"
            pragma custom_templates;
            template custom extern A() { signal input in; signal output out; }
        "#;
        let file = parse_ok(src);
        match &file.items[1] {
            Item::TemplateDef(t) => {
                assert!(t.is_extern);
            }
            _ => panic!("expected template"),
        }
    }
}
