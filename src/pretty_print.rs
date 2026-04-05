//! Pretty-printer for the Circom AST.
//!
//! Implements [`std::fmt::Display`] for [`File`] and all AST node types,
//! producing canonical Circom source code from the AST.
//!
//! # Example
//!
//! ```
//! use cinccino::parser;
//!
//! let src = "pragma circom 2.0.0;\ntemplate T() {\n    signal input x;\n}\n";
//! let (file, _errors) = parser::parse(src);
//! let output = file.to_string();
//! assert!(output.contains("pragma circom"));
//! ```

use std::fmt::{self, Display};

use crate::ast::*;
use crate::span::Span;
use crate::trivia::{self, line_of, Comment};

// ── Indentation helper ─────────────────────────────────────────────────

/// Configuration for the pretty-printer.
pub struct FormatConfig {
    /// The string to use for one level of indentation.
    pub indent: String,
    /// Maximum line length before wrapping.  `None` means no limit.
    pub max_line_length: Option<usize>,
}

impl FormatConfig {
    /// Create a config from basic LSP formatting options (indent only).
    pub fn from_lsp(tab_size: u32, insert_spaces: bool) -> Self {
        let indent = if insert_spaces {
            " ".repeat(tab_size as usize)
        } else {
            "\t".to_string()
        };
        Self {
            indent,
            max_line_length: None,
        }
    }

    /// Set the maximum line length for wrapping.
    pub fn with_max_line_length(mut self, max: usize) -> Self {
        self.max_line_length = Some(max);
        self
    }
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            indent: "    ".to_string(),
            max_line_length: None,
        }
    }
}

/// Format a parsed file using the given configuration.
pub fn format_with_config(file: &File, config: &FormatConfig) -> String {
    let mut buf = String::new();
    {
        let mut w = IndentWriter::with_config(&mut buf, config);
        for (i, item) in file.items.iter().enumerate() {
            if i > 0 {
                w.write_str("\n").unwrap();
            }
            write_item(&mut w, item).unwrap();
        }
    }
    buf
}

/// Format a parsed file while preserving comments from the original source.
///
/// Extracts comments from `source`, then interleaves them into the
/// formatted output at positions corresponding to their original locations
/// relative to AST nodes.
pub fn format_with_trivia(source: &str, file: &File, config: &FormatConfig) -> String {
    let comments = trivia::extract_comments(source);
    if comments.is_empty() {
        return format_with_config(file, config);
    }

    let mut buf = String::new();
    {
        let mut tf = TriviaFormatter::new(&mut buf, config, &comments, source);
        for (i, item) in file.items.iter().enumerate() {
            if i > 0 {
                tf.w.write_str("\n").unwrap();
            }
            tf.write_item_with_trivia(item).unwrap();
        }
        // Emit any trailing comments after the last item.
        tf.emit_remaining_comments().unwrap();
    }
    buf
}

// ── Trivia-aware formatter ─────────────────────────────────────────────

/// Wraps `IndentWriter` and interleaves comments during formatting.
struct TriviaFormatter<'a, W: fmt::Write> {
    w: IndentWriter<'a, W>,
    /// All comments sorted by position.
    comments: &'a [Comment],
    /// Index of the next comment to emit.
    cursor: usize,
    /// Original source text (for line-number lookups).
    source: &'a str,
}

impl<'a, W: fmt::Write> TriviaFormatter<'a, W> {
    fn new(
        f: &'a mut W,
        config: &'a FormatConfig,
        comments: &'a [Comment],
        source: &'a str,
    ) -> Self {
        Self {
            w: IndentWriter::with_config(f, config),
            comments,
            cursor: 0,
            source,
        }
    }

    /// Emit all leading comments whose span starts before `before_pos`.
    fn emit_leading_comments(&mut self, before_pos: usize) -> fmt::Result {
        while self.cursor < self.comments.len()
            && self.comments[self.cursor].span.start < before_pos
        {
            let comment = &self.comments[self.cursor];
            // Trailing comments are handled by emit_trailing_comment.
            // Skip (advance cursor) so subsequent leading comments are
            // not blocked.
            if self.is_trailing_comment(comment) {
                debug_assert!(
                    false,
                    "emit_leading_comments skipping trailing comment {:?} — \
                     ensure emit_trailing_comment is called for the owning node",
                    comment.text,
                );
                self.cursor += 1;
                continue;
            }
            self.write_indented_comment(&comment.text)?;
            self.w.write_str("\n")?;
            self.cursor += 1;
        }
        Ok(())
    }

    /// Write a comment with proper indentation.  For multi-line block
    /// comments, each continuation line is re-indented by one extra space
    /// (to align with the `*` in `/* ... */` style).
    fn write_indented_comment(&mut self, text: &str) -> fmt::Result {
        let mut lines = text.split('\n');
        if let Some(first) = lines.next() {
            self.w.write_indent()?;
            self.w.write_str(first)?;
        }
        for line in lines {
            self.w.write_str("\n")?;
            self.w.write_indent()?;
            // Preserve original relative indentation: strip leading
            // whitespace from the source line, then prepend a single
            // space so continuation lines align with the `*`.
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                // Blank line inside block comment — just write indent.
            } else {
                self.w.write_str(" ")?;
                self.w.write_str(trimmed)?;
            }
        }
        Ok(())
    }

    /// Emit a trailing comment if one exists on the same line as `node_end_pos`.
    /// This is called after writing a statement's semicolon but before the newline.
    fn emit_trailing_comment(&mut self, node_end_pos: usize) -> fmt::Result {
        if self.cursor >= self.comments.len() {
            return Ok(());
        }
        let comment = &self.comments[self.cursor];
        // A trailing comment must be on the same line as the node end.
        // When node_end_pos is past EOF, clamp to source.len() (which
        // line_of handles correctly) rather than len()-1, which would
        // report the wrong line if the file ends with '\n'.
        let clamped = node_end_pos.min(self.source.len());
        let node_line = line_of(self.source, clamped);
        let comment_line = line_of(self.source, comment.span.start);
        if comment_line == node_line && self.is_trailing_comment(comment) {
            self.w.write_str(" ")?;
            self.w.write_str(&comment.text)?;
            self.cursor += 1;
        }
        Ok(())
    }

    fn is_trailing_comment(&self, comment: &Comment) -> bool {
        let line_start = self.source[..comment.span.start]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let before = &self.source[line_start..comment.span.start];
        before.chars().any(|c| !c.is_whitespace())
    }

    /// Emit all remaining comments (e.g., at end of file).
    fn emit_remaining_comments(&mut self) -> fmt::Result {
        while self.cursor < self.comments.len() {
            let comment = &self.comments[self.cursor];
            self.w.write_indent()?;
            self.w.write_str(&comment.text)?;
            self.w.write_str("\n")?;
            self.cursor += 1;
        }
        Ok(())
    }

    // ── Item-level trivia ──────────────────────────────────────────────

    fn write_item_with_trivia(&mut self, item: &Item) -> fmt::Result {
        let span = item_span(item);
        self.emit_leading_comments(span.start)?;
        match item {
            Item::Pragma(n) => self.write_pragma_with_trivia(n),
            Item::Include(n) => self.write_include_with_trivia(n),
            Item::TemplateDef(n) => self.write_template_def_with_trivia(n),
            Item::FunctionDef(n) => self.write_function_def_with_trivia(n),
            Item::BusDef(n) => self.write_bus_def_with_trivia(n),
            Item::MainComponent(n) => self.write_main_component_with_trivia(n),
        }
    }

    fn write_pragma_with_trivia(&mut self, node: &Pragma) -> fmt::Result {
        match &node.kind {
            PragmaKind::Version(v) => self.w.write_fmt(format_args!(
                "pragma circom {}.{}.{};",
                v.major, v.minor, v.patch
            ))?,
            PragmaKind::CustomTemplates => self.w.write_str("pragma custom_templates;")?,
        }
        self.emit_trailing_comment(node.span.end)?;
        self.w.write_str("\n")
    }

    fn write_include_with_trivia(&mut self, node: &Include) -> fmt::Result {
        self.w.write_str("include \"")?;
        write_escaped_str(&mut self.w, &node.path)?;
        self.w.write_str("\";")?;
        self.emit_trailing_comment(node.span.end)?;
        self.w.write_str("\n")
    }

    fn write_main_component_with_trivia(&mut self, node: &MainComponent) -> fmt::Result {
        self.w.write_str("component main")?;
        if !node.public_signals.is_empty() {
            self.w.write_str(" {public [")?;
            // Build a closing hint that reflects the actual remaining line width:
            // "]} = <expr>;"
            let expr_width = measure_expr(&node.expr);
            let mut closing_hint = String::from("]} = ");
            closing_hint.extend(std::iter::repeat_n(' ', expr_width.min(20)));
            closing_hint.push(';');
            write_comma_sep_idents_maybe_wrap(&mut self.w, &node.public_signals, &closing_hint)?;
            self.w.write_str("]}")?;
        }
        self.w.write_str(" = ")?;
        write_expr(&mut self.w, &node.expr)?;
        self.w.write_str(";")?;
        self.emit_trailing_comment(node.span.end)?;
        self.w.write_str("\n")
    }

    fn write_template_def_with_trivia(&mut self, node: &TemplateDef) -> fmt::Result {
        if node.is_custom {
            self.w.write_str("custom ")?;
        }
        if node.is_parallel {
            self.w.write_str("parallel ")?;
        }
        self.w.write_str("template ")?;
        if node.is_extern {
            self.w.write_str("extern ")?;
        }
        self.w.write_fmt(format_args!("{}(", node.name.name))?;
        write_comma_sep_idents_maybe_wrap(&mut self.w, &node.params, ") {")?;
        self.w.write_str(") ")?;
        self.write_block_with_trivia(&node.body)?;
        self.w.write_str("\n")
    }

    fn write_function_def_with_trivia(&mut self, node: &FunctionDef) -> fmt::Result {
        self.w
            .write_fmt(format_args!("function {}(", node.name.name))?;
        write_comma_sep_idents_maybe_wrap(&mut self.w, &node.params, ") {")?;
        self.w.write_str(") ")?;
        self.write_block_with_trivia(&node.body)?;
        self.w.write_str("\n")
    }

    fn write_bus_def_with_trivia(&mut self, node: &BusDef) -> fmt::Result {
        self.w.write_fmt(format_args!("bus {}(", node.name.name))?;
        write_comma_sep_idents_maybe_wrap(&mut self.w, &node.params, ") {")?;
        self.w.write_str(") {\n")?;
        self.w.indent();
        for member in &node.body {
            // Bus members don't have top-level spans easily, use the signal/bus span
            let member_span = bus_member_span(member);
            self.emit_leading_comments(member_span.start)?;
            write_bus_member_no_newline(&mut self.w, member)?;
            self.emit_trailing_comment(member_span.end)?;
            self.w.write_str("\n")?;
        }
        // Emit comments before closing brace
        self.emit_leading_comments(node.span.end)?;
        self.w.dedent();
        self.w.write_indent()?;
        self.w.write_str("}\n")
    }

    // ── Block & statement trivia ───────────────────────────────────────

    fn write_block_with_trivia(&mut self, node: &Block) -> fmt::Result {
        self.w.write_str("{\n")?;
        self.w.indent();
        for stmt in &node.stmts {
            self.write_statement_with_trivia(stmt)?;
        }
        // Emit comments before the closing brace
        self.emit_leading_comments(node.span.end)?;
        self.w.dedent();
        self.w.write_indent()?;
        self.w.write_str("}")
    }

    fn write_statement_with_trivia(&mut self, node: &Statement) -> fmt::Result {
        self.emit_leading_comments(node.span.start)?;
        self.w.write_indent()?;
        match &node.kind {
            StatementKind::VarDecl(n) => {
                write_var_decl(&mut self.w, n)?;
                self.w.write_str(";")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::SignalDecl(n) => {
                write_signal_decl(&mut self.w, n)?;
                self.w.write_str(";")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::ComponentDecl(n) => {
                write_component_decl(&mut self.w, n)?;
                self.w.write_str(";")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::BusDecl(n) => {
                write_bus_instance_decl(&mut self.w, n)?;
                self.w.write_str(";")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::Assignment(n) => {
                write_assign_stmt(&mut self.w, n)?;
                self.w.write_str(";")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::CompoundAssign(n) => {
                write_compound_assign_stmt(&mut self.w, n)?;
                self.w.write_str(";")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::ConstraintEq(n) => {
                write_constraint_eq_stmt(&mut self.w, n)?;
                self.w.write_str(";")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::TupleAssign(n) => {
                write_tuple_assign_stmt(&mut self.w, n)?;
                self.w.write_str(";")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::IfElse(n) => {
                self.write_if_else_with_trivia(n)?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::For(n) => {
                self.write_for_loop_with_trivia(n)?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::While(n) => {
                self.write_while_loop_with_trivia(n)?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::Return(n) => {
                self.w.write_str("return ")?;
                write_expr(&mut self.w, &n.value)?;
                self.w.write_str(";")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::Log(n) => {
                write_log_stmt(&mut self.w, n)?;
                self.w.write_str(";")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::Assert(n) => {
                self.w.write_str("assert(")?;
                write_expr(&mut self.w, &n.expr)?;
                self.w.write_str(");")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::Increment(expr) => {
                write_expr(&mut self.w, expr)?;
                self.w.write_str("++;")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::Decrement(expr) => {
                write_expr(&mut self.w, expr)?;
                self.w.write_str("--;")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::Expression(expr) => {
                write_expr(&mut self.w, expr)?;
                self.w.write_str(";")?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::Block(blk) => {
                self.write_block_with_trivia(blk)?;
                self.emit_trailing_comment(node.span.end)?;
                self.w.write_str("\n")
            }
            StatementKind::Error => self.w.write_str("/* error */;\n"),
        }
    }

    fn write_if_else_with_trivia(&mut self, node: &IfElse) -> fmt::Result {
        self.w.write_str("if (")?;
        write_expr(&mut self.w, &node.cond)?;
        self.w.write_str(") ")?;
        self.write_block_with_trivia(&node.then_body)?;
        if let Some(else_body) = &node.else_body {
            // Flatten `else { if (...) { } }` into `else if (...) { }`.
            if let Some(inner_if) = as_single_if_else(else_body) {
                self.w.write_str(" else ")?;
                self.write_if_else_with_trivia(inner_if)?;
            } else {
                self.w.write_str(" else ")?;
                self.write_block_with_trivia(else_body)?;
            }
        }
        Ok(())
    }

    fn write_for_loop_with_trivia(&mut self, node: &ForLoop) -> fmt::Result {
        self.w.write_str("for (")?;
        write_for_init(&mut self.w, &node.init)?;
        self.w.write_str("; ")?;
        write_expr(&mut self.w, &node.cond)?;
        self.w.write_str("; ")?;
        write_for_step(&mut self.w, &node.step)?;
        self.w.write_str(") ")?;
        self.write_block_with_trivia(&node.body)
    }

    fn write_while_loop_with_trivia(&mut self, node: &WhileLoop) -> fmt::Result {
        self.w.write_str("while (")?;
        write_expr(&mut self.w, &node.cond)?;
        self.w.write_str(") ")?;
        self.write_block_with_trivia(&node.body)
    }
}

/// Get the span of an Item.
fn item_span(item: &Item) -> Span {
    match item {
        Item::Pragma(n) => n.span,
        Item::Include(n) => n.span,
        Item::TemplateDef(n) => n.span,
        Item::FunctionDef(n) => n.span,
        Item::BusDef(n) => n.span,
        Item::MainComponent(n) => n.span,
    }
}

/// If a block contains exactly one statement and that statement is an `IfElse`,
/// return a reference to the inner `IfElse`.  Used to flatten `else { if ... }`
/// into `else if ...`.
fn as_single_if_else(block: &Block) -> Option<&IfElse> {
    if block.stmts.len() == 1 {
        if let StatementKind::IfElse(inner) = &block.stmts[0].kind {
            return Some(inner);
        }
    }
    None
}

/// Get the span of a BusMember.
fn bus_member_span(member: &BusMember) -> Span {
    match member {
        BusMember::Signal(s) => s.span,
        BusMember::Bus(b) => b.span,
    }
}

struct IndentWriter<'a, W: fmt::Write> {
    f: &'a mut W,
    level: usize,
    indent: &'a str,
    /// Current column position (0-based).
    col: usize,
    /// Maximum line length, if any.
    max_line_length: Option<usize>,
}

const DEFAULT_INDENT: &str = "    ";

impl<'a, W: fmt::Write> IndentWriter<'a, W> {
    fn new(f: &'a mut W) -> Self {
        Self {
            f,
            level: 0,
            indent: DEFAULT_INDENT,
            col: 0,
            max_line_length: None,
        }
    }

    fn with_config(f: &'a mut W, config: &'a FormatConfig) -> Self {
        Self {
            f,
            level: 0,
            indent: &config.indent,
            col: 0,
            max_line_length: config.max_line_length,
        }
    }

    fn indent(&mut self) {
        self.level += 1;
    }

    fn dedent(&mut self) {
        debug_assert!(self.level > 0, "dedent() called with level already at 0");
        self.level = self.level.saturating_sub(1);
    }

    /// Current indentation width in characters.
    fn indent_width(&self) -> usize {
        self.level * self.indent.chars().count()
    }

    fn write_indent(&mut self) -> fmt::Result {
        for _ in 0..self.level {
            self.f.write_str(self.indent)?;
        }
        self.col = self.indent_width();
        Ok(())
    }

    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.f.write_str(s)?;
        // Track column using char count instead of byte length so that
        // multi-byte UTF-8 characters are counted correctly for
        // line-length decisions.
        if let Some(last_nl) = s.rfind('\n') {
            self.col = s[last_nl + 1..].chars().count();
        } else {
            self.col += s.chars().count();
        }
        Ok(())
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        // Write directly through a column-tracking adapter to avoid
        // heap-allocating a String on every call.
        use fmt::Write as _;
        struct ColTracker<'b, W: fmt::Write> {
            inner: &'b mut W,
            col: &'b mut usize,
        }
        impl<W: fmt::Write> fmt::Write for ColTracker<'_, W> {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.inner.write_str(s)?;
                if let Some(last_nl) = s.rfind('\n') {
                    *self.col = s[last_nl + 1..].chars().count();
                } else {
                    *self.col += s.chars().count();
                }
                Ok(())
            }
        }
        let mut tracker = ColTracker {
            inner: &mut *self.f,
            col: &mut self.col,
        };
        tracker.write_fmt(args)
    }

    /// Check if `extra_chars` more characters would exceed the line limit.
    fn would_exceed(&self, extra_chars: usize) -> bool {
        match self.max_line_length {
            Some(max) => self.col + extra_chars > max,
            None => false,
        }
    }
}

// ── Flat-width measurement ─────────────────────────────────────────

/// A zero-allocation writer that only counts characters written.
struct CountWriter(usize);

impl fmt::Write for CountWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0 += s.chars().count();
        Ok(())
    }
}

/// Measure the flat (single-line) width of an expression.
fn measure_expr(node: &Expression) -> usize {
    let mut cw = CountWriter(0);
    let mut w = IndentWriter::new(&mut cw);
    write_expr(&mut w, node).unwrap();
    cw.0
}

/// Measure the flat width of comma-separated expressions: "a, b, c".
fn measure_comma_sep_exprs(exprs: &[Expression]) -> usize {
    if exprs.is_empty() {
        return 0;
    }
    let items: usize = exprs.iter().map(measure_expr).sum();
    // ", " between each pair
    items + (exprs.len() - 1) * 2
}

/// Measure the flat width of comma-separated identifiers: "a, b, c".
fn measure_comma_sep_idents(idents: &[Identifier]) -> usize {
    if idents.is_empty() {
        return 0;
    }
    let items: usize = idents.iter().map(|id| id.name.chars().count()).sum();
    items + (idents.len() - 1) * 2
}

// ── Wrapping helpers ──────────────────────────────────────────────

/// Write comma-separated expressions, wrapping to one-per-line if they
/// would exceed the line limit.  Caller has already written the opening
/// delimiter (e.g. "(") and must write the closing one after this returns.
fn write_comma_sep_exprs_maybe_wrap<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    exprs: &[Expression],
    closing: &str,
) -> fmt::Result {
    if exprs.is_empty() {
        return Ok(());
    }
    let flat_width = measure_comma_sep_exprs(exprs) + closing.len();
    if w.would_exceed(flat_width) {
        // Wrapped: one expression per line
        w.indent();
        for (i, expr) in exprs.iter().enumerate() {
            w.write_str("\n")?;
            w.write_indent()?;
            write_expr(w, expr)?;
            if i + 1 < exprs.len() {
                w.write_str(",")?;
            }
        }
        w.write_str("\n")?;
        w.dedent();
        w.write_indent()?;
        Ok(())
    } else {
        // Flat: all on one line
        for (i, expr) in exprs.iter().enumerate() {
            if i > 0 {
                w.write_str(", ")?;
            }
            write_expr(w, expr)?;
        }
        Ok(())
    }
}

/// Write comma-separated identifiers, wrapping if needed.
fn write_comma_sep_idents_maybe_wrap<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    idents: &[Identifier],
    closing: &str,
) -> fmt::Result {
    if idents.is_empty() {
        return Ok(());
    }
    let flat_width = measure_comma_sep_idents(idents) + closing.len();
    if w.would_exceed(flat_width) {
        w.indent();
        for (i, ident) in idents.iter().enumerate() {
            w.write_str("\n")?;
            w.write_indent()?;
            w.write_str(&ident.name)?;
            if i + 1 < idents.len() {
                w.write_str(",")?;
            }
        }
        w.write_str("\n")?;
        w.dedent();
        w.write_indent()?;
        Ok(())
    } else {
        write_comma_sep_idents(w, idents)
    }
}

fn write_escaped_str<W: fmt::Write>(w: &mut IndentWriter<'_, W>, s: &str) -> fmt::Result {
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        let esc = match ch {
            '\\' => "\\\\",
            '"' => "\\\"",
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' => "\\t",
            _ => continue,
        };
        if start < i {
            w.write_str(&s[start..i])?;
        }
        w.write_str(esc)?;
        start = i + ch.len_utf8();
    }
    if start < s.len() {
        w.write_str(&s[start..])?;
    }
    Ok(())
}

// ── Display impls ──────────────────────────────────────────────────────

impl Display for File {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut w = IndentWriter::new(f);
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                w.write_str("\n")?;
            }
            write_item(&mut w, item)?;
        }
        Ok(())
    }
}

fn write_item<W: fmt::Write>(w: &mut IndentWriter<'_, W>, item: &Item) -> fmt::Result {
    match item {
        Item::Pragma(n) => write_pragma(w, n),
        Item::Include(n) => write_include(w, n),
        Item::TemplateDef(n) => write_template_def(w, n),
        Item::FunctionDef(n) => write_function_def(w, n),
        Item::BusDef(n) => write_bus_def(w, n),
        Item::MainComponent(n) => write_main_component(w, n),
    }
}

fn write_pragma<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &Pragma) -> fmt::Result {
    match &node.kind {
        PragmaKind::Version(v) => w.write_fmt(format_args!(
            "pragma circom {}.{}.{};\n",
            v.major, v.minor, v.patch
        )),
        PragmaKind::CustomTemplates => w.write_str("pragma custom_templates;\n"),
    }
}

fn write_include<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &Include) -> fmt::Result {
    w.write_str("include \"")?;
    write_escaped_str(w, &node.path)?;
    w.write_str("\";\n")
}

fn write_template_def<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    node: &TemplateDef,
) -> fmt::Result {
    if node.is_custom {
        w.write_str("custom ")?;
    }
    if node.is_parallel {
        w.write_str("parallel ")?;
    }
    w.write_str("template ")?;
    if node.is_extern {
        w.write_str("extern ")?;
    }
    w.write_fmt(format_args!("{}(", node.name.name))?;
    write_comma_sep_idents_maybe_wrap(w, &node.params, ") {")?;
    w.write_str(") ")?;
    write_block(w, &node.body)?;
    w.write_str("\n")
}

fn write_function_def<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    node: &FunctionDef,
) -> fmt::Result {
    w.write_fmt(format_args!("function {}(", node.name.name))?;
    write_comma_sep_idents_maybe_wrap(w, &node.params, ") {")?;
    w.write_str(") ")?;
    write_block(w, &node.body)?;
    w.write_str("\n")
}

fn write_bus_def<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &BusDef) -> fmt::Result {
    w.write_fmt(format_args!("bus {}(", node.name.name))?;
    write_comma_sep_idents_maybe_wrap(w, &node.params, ") {")?;
    w.write_str(") {\n")?;
    w.indent();
    for member in &node.body {
        write_bus_member(w, member)?;
    }
    w.dedent();
    w.write_indent()?;
    w.write_str("}\n")
}

fn write_bus_member<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &BusMember) -> fmt::Result {
    match node {
        BusMember::Signal(s) => {
            w.write_indent()?;
            write_signal_decl(w, s)?;
            w.write_str(";\n")
        }
        BusMember::Bus(b) => {
            w.write_indent()?;
            write_bus_field_decl(w, b)?;
            w.write_str(";\n")
        }
    }
}

/// Like `write_bus_member` but writes the semicolon without a trailing newline,
/// so that `emit_trailing_comment` can append a comment before the newline.
fn write_bus_member_no_newline<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    node: &BusMember,
) -> fmt::Result {
    match node {
        BusMember::Signal(s) => {
            w.write_indent()?;
            write_signal_decl(w, s)?;
            w.write_str(";")
        }
        BusMember::Bus(b) => {
            w.write_indent()?;
            write_bus_field_decl(w, b)?;
            w.write_str(";")
        }
    }
}

fn write_bus_field_decl<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    node: &BusFieldDecl,
) -> fmt::Result {
    write_bus_type(w, &node.bus_type)?;
    if !node.tags.is_empty() {
        w.write_str(" {")?;
        write_comma_sep_idents(w, &node.tags)?;
        w.write_str("}")?;
    }
    w.write_fmt(format_args!(" {}", node.name.name))?;
    for dim in &node.dimensions {
        w.write_str("[")?;
        write_expr(w, dim)?;
        w.write_str("]")?;
    }
    Ok(())
}

fn write_main_component<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    node: &MainComponent,
) -> fmt::Result {
    w.write_str("component main")?;
    if !node.public_signals.is_empty() {
        w.write_str(" {public [")?;
        let expr_width = measure_expr(&node.expr);
        let mut closing_hint = String::from("]} = ");
        closing_hint.extend(std::iter::repeat_n(' ', expr_width.min(20)));
        closing_hint.push(';');
        write_comma_sep_idents_maybe_wrap(w, &node.public_signals, &closing_hint)?;
        w.write_str("]}")?;
    }
    w.write_str(" = ")?;
    write_expr(w, &node.expr)?;
    w.write_str(";\n")
}

fn write_block<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &Block) -> fmt::Result {
    w.write_str("{\n")?;
    w.indent();
    for stmt in &node.stmts {
        write_statement(w, stmt)?;
    }
    w.dedent();
    w.write_indent()?;
    w.write_str("}")
}

fn write_statement<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &Statement) -> fmt::Result {
    w.write_indent()?;
    match &node.kind {
        StatementKind::VarDecl(n) => {
            write_var_decl(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::SignalDecl(n) => {
            write_signal_decl(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::ComponentDecl(n) => {
            write_component_decl(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::BusDecl(n) => {
            write_bus_instance_decl(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::Assignment(n) => {
            write_assign_stmt(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::CompoundAssign(n) => {
            write_compound_assign_stmt(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::ConstraintEq(n) => {
            write_constraint_eq_stmt(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::TupleAssign(n) => {
            write_tuple_assign_stmt(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::IfElse(n) => {
            write_if_else(w, n)?;
            w.write_str("\n")
        }
        StatementKind::For(n) => {
            write_for_loop(w, n)?;
            w.write_str("\n")
        }
        StatementKind::While(n) => {
            write_while_loop(w, n)?;
            w.write_str("\n")
        }
        StatementKind::Return(n) => {
            w.write_str("return ")?;
            write_expr(w, &n.value)?;
            w.write_str(";\n")
        }
        StatementKind::Log(n) => {
            write_log_stmt(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::Assert(n) => {
            w.write_str("assert(")?;
            write_expr(w, &n.expr)?;
            w.write_str(");\n")
        }
        StatementKind::Increment(expr) => {
            write_expr(w, expr)?;
            w.write_str("++;\n")
        }
        StatementKind::Decrement(expr) => {
            write_expr(w, expr)?;
            w.write_str("--;\n")
        }
        StatementKind::Expression(expr) => {
            write_expr(w, expr)?;
            w.write_str(";\n")
        }
        StatementKind::Block(blk) => {
            write_block(w, blk)?;
            w.write_str("\n")
        }
        StatementKind::Error => w.write_str("/* error */;\n"),
    }
}

fn write_var_decl<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &VarDecl) -> fmt::Result {
    w.write_str("var ")?;
    for (i, entry) in node.names.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        w.write_str(&entry.name.name)?;
        for dim in &entry.dimensions {
            w.write_str("[")?;
            write_expr(w, dim)?;
            w.write_str("]")?;
        }
        if let Some(init) = &entry.init {
            w.write_str(" = ")?;
            write_expr(w, init)?;
        }
    }
    Ok(())
}

fn write_signal_decl<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &SignalDecl) -> fmt::Result {
    w.write_str("signal ")?;
    match node.kind {
        SignalKind::Input => w.write_str("input ")?,
        SignalKind::Output => w.write_str("output ")?,
        SignalKind::Intermediate => {}
    }
    if !node.tags.is_empty() {
        w.write_str("{")?;
        write_comma_sep_idents(w, &node.tags)?;
        w.write_str("} ")?;
    }
    for (i, entry) in node.names.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        w.write_str(&entry.name.name)?;
        for dim in &entry.dimensions {
            w.write_str("[")?;
            write_expr(w, dim)?;
            w.write_str("]")?;
        }
        if let Some((op, init)) = &entry.init {
            match op {
                SignalAssignOp::SafeLeft => w.write_str(" <== ")?,
                SignalAssignOp::UnsafeLeft => w.write_str(" <-- ")?,
            }
            write_expr(w, init)?;
        }
    }
    Ok(())
}

fn write_component_decl<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    node: &ComponentDecl,
) -> fmt::Result {
    w.write_str("component ")?;
    if node.is_parallel {
        w.write_str("parallel ")?;
    }
    for (i, entry) in node.names.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        w.write_str(&entry.name.name)?;
        for dim in &entry.dimensions {
            w.write_str("[")?;
            write_expr(w, dim)?;
            w.write_str("]")?;
        }
        if let Some(init) = &entry.init {
            w.write_str(" = ")?;
            write_expr(w, init)?;
        }
    }
    Ok(())
}

fn write_bus_instance_decl<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    node: &BusInstanceDecl,
) -> fmt::Result {
    write_bus_type(w, &node.bus_type)?;
    w.write_str(" ")?;
    match node.signal_kind {
        SignalKind::Input => w.write_str("input ")?,
        SignalKind::Output => w.write_str("output ")?,
        SignalKind::Intermediate => {}
    }
    if !node.tags.is_empty() {
        w.write_str("{")?;
        write_comma_sep_idents(w, &node.tags)?;
        w.write_str("} ")?;
    }
    w.write_str(&node.name.name)?;
    for dim in &node.dimensions {
        w.write_str("[")?;
        write_expr(w, dim)?;
        w.write_str("]")?;
    }
    if let Some((op, init)) = &node.init {
        match op {
            SignalAssignOp::SafeLeft => w.write_str(" <== ")?,
            SignalAssignOp::UnsafeLeft => w.write_str(" <-- ")?,
        }
        write_expr(w, init)?;
    }
    Ok(())
}

fn write_bus_type<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &BusType) -> fmt::Result {
    w.write_str(&node.name.name)?;
    if !node.args.is_empty() {
        w.write_str("(")?;
        write_comma_sep_exprs_maybe_wrap(w, &node.args, ")")?;
        w.write_str(")")?;
    }
    Ok(())
}

fn write_assign_stmt<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &AssignStmt) -> fmt::Result {
    write_expr(w, &node.lhs)?;
    match node.op {
        AssignOp::Eq => w.write_str(" = ")?,
        AssignOp::SafeLeft => w.write_str(" <== ")?,
        AssignOp::SafeRight => w.write_str(" ==> ")?,
        AssignOp::UnsafeLeft => w.write_str(" <-- ")?,
        AssignOp::UnsafeRight => w.write_str(" --> ")?,
    }
    write_expr(w, &node.rhs)
}

fn write_compound_assign_stmt<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    node: &CompoundAssignStmt,
) -> fmt::Result {
    write_expr(w, &node.lhs)?;
    let op_str = match node.op {
        CompoundOp::AddAssign => " += ",
        CompoundOp::SubAssign => " -= ",
        CompoundOp::MulAssign => " *= ",
        CompoundOp::PowAssign => " **= ",
        CompoundOp::DivAssign => " /= ",
        CompoundOp::IntDivAssign => " \\= ",
        CompoundOp::ModAssign => " %= ",
        CompoundOp::ShlAssign => " <<= ",
        CompoundOp::ShrAssign => " >>= ",
        CompoundOp::BitAndAssign => " &= ",
        CompoundOp::BitOrAssign => " |= ",
        CompoundOp::BitXorAssign => " ^= ",
    };
    w.write_str(op_str)?;
    write_expr(w, &node.rhs)
}

fn write_constraint_eq_stmt<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    node: &ConstraintEqStmt,
) -> fmt::Result {
    write_expr(w, &node.lhs)?;
    w.write_str(" === ")?;
    write_expr(w, &node.rhs)
}

fn write_tuple_assign_stmt<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    node: &TupleAssignStmt,
) -> fmt::Result {
    w.write_str("(")?;
    for (i, target) in node.targets.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        match target {
            Some(expr) => write_expr(w, expr)?,
            None => w.write_str("_")?,
        }
    }
    w.write_str(")")?;
    match node.op {
        AssignOp::Eq => w.write_str(" = ")?,
        AssignOp::SafeLeft => w.write_str(" <== ")?,
        AssignOp::SafeRight => w.write_str(" ==> ")?,
        AssignOp::UnsafeLeft => w.write_str(" <-- ")?,
        AssignOp::UnsafeRight => w.write_str(" --> ")?,
    }
    write_expr(w, &node.rhs)
}

fn write_if_else<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &IfElse) -> fmt::Result {
    w.write_str("if (")?;
    write_expr(w, &node.cond)?;
    w.write_str(") ")?;
    write_block(w, &node.then_body)?;
    if let Some(else_body) = &node.else_body {
        // Flatten `else { if (...) { } }` into `else if (...) { }`.
        if let Some(inner_if) = as_single_if_else(else_body) {
            w.write_str(" else ")?;
            write_if_else(w, inner_if)?;
        } else {
            w.write_str(" else ")?;
            write_block(w, else_body)?;
        }
    }
    Ok(())
}

fn write_for_loop<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &ForLoop) -> fmt::Result {
    w.write_str("for (")?;
    write_for_init(w, &node.init)?;
    w.write_str("; ")?;
    write_expr(w, &node.cond)?;
    w.write_str("; ")?;
    write_for_step(w, &node.step)?;
    w.write_str(") ")?;
    write_block(w, &node.body)
}

fn write_for_init<W: fmt::Write>(w: &mut IndentWriter<'_, W>, stmt: &Statement) -> fmt::Result {
    match &stmt.kind {
        StatementKind::VarDecl(n) => write_var_decl(w, n),
        StatementKind::Assignment(n) => write_assign_stmt(w, n),
        StatementKind::Expression(expr) => write_expr(w, expr),
        other => {
            debug_assert!(false, "unexpected for-init variant: {other:?}");
            w.write_fmt(format_args!("/* unexpected: {other:?} */"))
        }
    }
}

fn write_for_step<W: fmt::Write>(w: &mut IndentWriter<'_, W>, stmt: &Statement) -> fmt::Result {
    match &stmt.kind {
        StatementKind::Assignment(n) => write_assign_stmt(w, n),
        StatementKind::CompoundAssign(n) => write_compound_assign_stmt(w, n),
        StatementKind::Increment(expr) => {
            write_expr(w, expr)?;
            w.write_str("++")
        }
        StatementKind::Decrement(expr) => {
            write_expr(w, expr)?;
            w.write_str("--")
        }
        StatementKind::Expression(expr) => write_expr(w, expr),
        other => {
            debug_assert!(false, "unexpected for-step variant: {other:?}");
            w.write_fmt(format_args!("/* unexpected: {other:?} */"))
        }
    }
}

fn write_while_loop<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &WhileLoop) -> fmt::Result {
    w.write_str("while (")?;
    write_expr(w, &node.cond)?;
    w.write_str(") ")?;
    write_block(w, &node.body)
}

fn write_log_stmt<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &LogStmt) -> fmt::Result {
    w.write_str("log(")?;
    for (i, arg) in node.args.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        match arg {
            LogArg::Expr(expr) => write_expr(w, expr)?,
            LogArg::String(s) => {
                w.write_str("\"")?;
                write_escaped_str(w, s)?;
                w.write_str("\"")?;
            }
        }
    }
    w.write_str(")")
}

/// Write an expression to the output.
///
/// **Precedence note:** Binary expressions are printed without
/// precedence-aware parenthesization.  This is correct for ASTs produced
/// by the parser because the parser preserves explicit `Paren` nodes.
/// However, programmatically-constructed ASTs that omit `Paren` nodes
/// may produce output with different semantics when re-parsed (e.g.,
/// `Binary(Binary(a, Add, b), Mul, c)` prints as `a + b * c`, which
/// re-parses as `a + (b * c)`).
fn write_expr<W: fmt::Write>(w: &mut IndentWriter<'_, W>, node: &Expression) -> fmt::Result {
    match node.kind.as_ref() {
        ExpressionKind::Number(n) => w.write_str(n),
        ExpressionKind::Ident(name) => w.write_str(name),
        ExpressionKind::Unary(op, expr) => {
            match op {
                UnaryOp::Neg => w.write_str("-")?,
                UnaryOp::Not => w.write_str("!")?,
                UnaryOp::BitNot => w.write_str("~")?,
            }
            write_expr(w, expr)
        }
        ExpressionKind::Binary(lhs, op, rhs) => {
            write_expr(w, lhs)?;
            let op_str = match op {
                BinaryOp::Add => " + ",
                BinaryOp::Sub => " - ",
                BinaryOp::Mul => " * ",
                BinaryOp::Div => " / ",
                BinaryOp::IntDiv => " \\ ",
                BinaryOp::Mod => " % ",
                BinaryOp::Pow => " ** ",
                BinaryOp::Shl => " << ",
                BinaryOp::Shr => " >> ",
                BinaryOp::BitAnd => " & ",
                BinaryOp::BitOr => " | ",
                BinaryOp::BitXor => " ^ ",
                BinaryOp::And => " && ",
                BinaryOp::Or => " || ",
                BinaryOp::Eq => " == ",
                BinaryOp::Ne => " != ",
                BinaryOp::Lt => " < ",
                BinaryOp::Gt => " > ",
                BinaryOp::Le => " <= ",
                BinaryOp::Ge => " >= ",
            };
            w.write_str(op_str)?;
            write_expr(w, rhs)
        }
        ExpressionKind::Ternary(cond, then_expr, else_expr) => {
            write_expr(w, cond)?;
            w.write_str(" ? ")?;
            write_expr(w, then_expr)?;
            w.write_str(" : ")?;
            write_expr(w, else_expr)
        }
        ExpressionKind::Index(expr, index) => {
            write_expr(w, expr)?;
            w.write_str("[")?;
            write_expr(w, index)?;
            w.write_str("]")
        }
        ExpressionKind::Member(expr, ident) => {
            write_expr(w, expr)?;
            w.write_fmt(format_args!(".{}", ident.name))
        }
        ExpressionKind::Call(callee, args) => {
            write_expr(w, callee)?;
            w.write_str("(")?;
            write_comma_sep_exprs_maybe_wrap(w, args, ")")?;
            w.write_str(")")
        }
        ExpressionKind::AnonymousComp(comp) => write_anonymous_comp(w, comp),
        ExpressionKind::ArrayLit(elems) => {
            w.write_str("[")?;
            write_comma_sep_exprs_maybe_wrap(w, elems, "]")?;
            w.write_str("]")
        }
        ExpressionKind::Paren(expr) => {
            w.write_str("(")?;
            write_expr(w, expr)?;
            w.write_str(")")
        }
        ExpressionKind::Parallel(expr) => {
            w.write_str("parallel ")?;
            write_expr(w, expr)
        }
        ExpressionKind::Underscore => w.write_str("_"),
        ExpressionKind::Error => w.write_str("/* error */"),
    }
}

fn write_anonymous_comp<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    node: &AnonymousComp,
) -> fmt::Result {
    write_expr(w, &node.template)?;
    w.write_str("(")?;
    write_comma_sep_exprs_maybe_wrap(w, &node.template_args, ")(")?;
    w.write_str(")(")?;
    write_anon_comp_inputs_maybe_wrap(w, &node.inputs, ")")?;
    w.write_str(")")
}

/// Write anonymous component inputs with optional wrapping.
fn write_anon_comp_inputs_maybe_wrap<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    inputs: &[AnonCompInput],
    closing: &str,
) -> fmt::Result {
    if inputs.is_empty() {
        return Ok(());
    }
    // Measure flat width
    let flat_width: usize = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| {
            let w = match input {
                AnonCompInput::Positional(expr) => measure_expr(expr),
                AnonCompInput::Named(ident, expr) => {
                    ident.name.chars().count() + " <== ".len() + measure_expr(expr)
                }
            };
            if i > 0 {
                w + 2
            } else {
                w
            }
        })
        .sum();
    if w.would_exceed(flat_width + closing.len()) {
        w.indent();
        for (i, input) in inputs.iter().enumerate() {
            w.write_str("\n")?;
            w.write_indent()?;
            match input {
                AnonCompInput::Positional(expr) => write_expr(w, expr)?,
                AnonCompInput::Named(ident, expr) => {
                    w.write_fmt(format_args!("{} <== ", ident.name))?;
                    write_expr(w, expr)?;
                }
            }
            if i + 1 < inputs.len() {
                w.write_str(",")?;
            }
        }
        w.write_str("\n")?;
        w.dedent();
        w.write_indent()?;
    } else {
        for (i, input) in inputs.iter().enumerate() {
            if i > 0 {
                w.write_str(", ")?;
            }
            match input {
                AnonCompInput::Positional(expr) => write_expr(w, expr)?,
                AnonCompInput::Named(ident, expr) => {
                    w.write_fmt(format_args!("{} <== ", ident.name))?;
                    write_expr(w, expr)?;
                }
            }
        }
    }
    Ok(())
}

fn write_comma_sep_idents<W: fmt::Write>(
    w: &mut IndentWriter<'_, W>,
    idents: &[Identifier],
) -> fmt::Result {
    for (i, ident) in idents.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        w.write_str(&ident.name)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn pretty_print_pragma() {
        let src = "pragma circom 2.0.0;\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        assert_eq!(file.to_string(), "pragma circom 2.0.0;\n");
    }

    #[test]
    fn pretty_print_include() {
        let src = "include \"other.circom\";\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        assert_eq!(file.to_string(), "include \"other.circom\";\n");
    }

    #[test]
    fn pretty_print_simple_template() {
        let src = r#"template Foo(n) {
    signal input a;
    signal output b;
    b <== a;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("template Foo(n)"));
        assert!(output.contains("signal input a"));
        assert!(output.contains("signal output b"));
        assert!(output.contains("b <== a"));
    }

    #[test]
    fn pretty_print_function() {
        let src = r#"function add(a, b) {
    return a + b;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("function add(a, b)"));
        assert!(output.contains("return a + b"));
    }

    #[test]
    fn pretty_print_control_flow() {
        let src = r#"template T() {
    var x = 0;
    if (x) {
        x = 1;
    } else {
        x = 2;
    }
    for (var i = 0; i < 10; i++) {
        x += i;
    }
    while (x) {
        x = x - 1;
    }
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("if (x)"));
        assert!(output.contains("} else {"));
        assert!(output.contains("for (var i = 0; i < 10; i++)"));
        assert!(output.contains("while (x)"));
    }

    #[test]
    fn pretty_print_main_component() {
        let src = "component main {public [a, b]} = MyTemplate(10);\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("component main {public [a, b]}"));
        assert!(output.contains("MyTemplate(10)"));
    }

    #[test]
    fn pretty_print_constraint_eq() {
        let src = r#"template T() {
    signal input a;
    signal input b;
    a === b;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("a === b"));
    }

    #[test]
    fn roundtrip_parse_print_reparse() {
        // Parse -> print -> reparse should produce the same AST structure
        let src = r#"pragma circom 2.0.0;

template Num2Bits(n) {
    signal input in;
    signal output out[n];
    var lc1 = 0;
    var e2 = 1;
    for (var i = 0; i < n; i++) {
        out[i] <-- (in >> i) & 1;
        out[i] * (out[i] - 1) === 0;
        lc1 += out[i] * e2;
        e2 = e2 + e2;
    }
    lc1 === in;
}
"#;
        let (file1, errors1) = parser::parse(src);
        assert!(errors1.is_empty(), "first parse errors: {errors1:?}");

        let printed = file1.to_string();
        let (file2, errors2) = parser::parse(&printed);
        assert!(errors2.is_empty(), "reparse errors: {errors2:?}");

        // Compare structure via Debug representation (spans may differ
        // but the AST shape and content should be identical)
        assert_eq!(
            format!("{:?}", file1.items),
            format!("{:?}", file2.items),
            "round-trip AST differs"
        );

        // Print again — should be stable
        let printed2 = file2.to_string();
        assert_eq!(printed, printed2, "pretty-print is not idempotent");
    }

    #[test]
    fn roundtrip_circomlib_fixtures() {
        use std::fs;

        let fixtures_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
        for entry in fs::read_dir(fixtures_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().map(|e| e == "circom").unwrap_or(false) {
                let src = fs::read_to_string(&path).unwrap();
                let (file1, errors1) = parser::parse(&src);
                // Skip files with parse errors (some may use unsupported features)
                if !errors1.is_empty() {
                    continue;
                }

                let printed = file1.to_string();
                let (file2, errors2) = parser::parse(&printed);
                assert!(
                    errors2.is_empty(),
                    "reparse of {} failed: {errors2:?}\n---printed---\n{printed}",
                    path.display()
                );

                // Idempotency: print again and compare
                let printed2 = file2.to_string();
                assert_eq!(
                    printed,
                    printed2,
                    "pretty-print of {} is not idempotent",
                    path.display()
                );
            }
        }
    }

    // ── Trivia-aware formatting tests ──────────────────────────────────

    fn format_trivia(src: &str) -> String {
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        format_with_trivia(src, &file, &FormatConfig::default())
    }

    #[test]
    fn trivia_leading_comment_before_statement() {
        let src = "template T() {\n    // comment\n    var x = 1;\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("    // comment\n    var x = 1;"),
            "output: {output}"
        );
    }

    #[test]
    fn trivia_trailing_comment_after_statement() {
        let src = "template T() {\n    var x = 1; // inline\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("var x = 1; // inline\n"),
            "output: {output}"
        );
    }

    #[test]
    fn trivia_comment_before_template() {
        let src = "// Top-level comment\ntemplate T() {\n    var x = 1;\n}\n";
        let output = format_trivia(src);
        assert!(
            output.starts_with("// Top-level comment\ntemplate T()"),
            "output: {output}"
        );
    }

    #[test]
    fn trivia_comment_at_end_of_block() {
        let src = "template T() {\n    var x = 1;\n    // end comment\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("    var x = 1;\n    // end comment\n}"),
            "output: {output}"
        );
    }

    #[test]
    fn trivia_block_comment() {
        let src = "/* file header */\npragma circom 2.0.0;\n";
        let output = format_trivia(src);
        assert!(
            output.starts_with("/* file header */\npragma circom 2.0.0;"),
            "output: {output}"
        );
    }

    #[test]
    fn trivia_multiple_comments_in_block() {
        let src = r#"template T() {
    // first
    var x = 1;
    // second
    var y = 2;
}
"#;
        let output = format_trivia(src);
        assert!(
            output.contains("    // first\n    var x = 1;"),
            "output: {output}"
        );
        assert!(
            output.contains("    // second\n    var y = 2;"),
            "output: {output}"
        );
    }

    #[test]
    fn trivia_comment_in_for_loop_body() {
        let src = r#"template T() {
    for (var i = 0; i < 10; i++) {
        // loop body
        var x = i;
    }
}
"#;
        let output = format_trivia(src);
        assert!(
            output.contains("        // loop body\n        var x = i;"),
            "output: {output}"
        );
    }

    #[test]
    fn trivia_no_comments_same_as_plain() {
        let src = "template T() {\n    var x = 1;\n}\n";
        let (file, _) = parser::parse(src);
        let plain = format_with_config(&file, &FormatConfig::default());
        let trivia = format_with_trivia(src, &file, &FormatConfig::default());
        assert_eq!(plain, trivia);
    }

    #[test]
    fn trivia_idempotent() {
        let src = r#"// File comment
pragma circom 2.0.0;

// Template doc
template T() {
    // Input signal
    signal input x;
    var y = x; // use x
}
"#;
        let output1 = format_trivia(src);
        let output2 = format_trivia(&output1);
        assert_eq!(output1, output2, "trivia formatting is not idempotent");
    }

    #[test]
    fn trivia_comment_between_items() {
        let src = r#"pragma circom 2.0.0;

// A template
template A() {
    var x = 1;
}

// Another template
template B() {
    var y = 2;
}
"#;
        let output = format_trivia(src);
        assert!(
            output.contains("// A template\ntemplate A()"),
            "output: {output}"
        );
        assert!(
            output.contains("// Another template\ntemplate B()"),
            "output: {output}"
        );
    }

    #[test]
    fn trivia_trailing_comment_on_signal() {
        let src = "template T() {\n    signal input a; // the input\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("signal input a; // the input\n"),
            "output: {output}"
        );
    }

    // ── Line-length wrapping tests ────────────────────────────────────

    fn format_with_max_line(src: &str, max_line_length: usize) -> String {
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let config = FormatConfig {
            max_line_length: Some(max_line_length),
            ..FormatConfig::default()
        };
        format_with_config(&file, &config)
    }

    #[test]
    fn wrap_call_args_when_exceeds_max_line() {
        // A call with many arguments that exceeds the line limit
        let src = "template T() {\n    var x = Foo(alpha, bravo, charlie, delta, echo);\n}\n";
        // With a generous limit, stays on one line
        let wide = format_with_max_line(src, 120);
        assert!(
            wide.contains("Foo(alpha, bravo, charlie, delta, echo)"),
            "should stay flat with wide limit: {wide}"
        );
        // With a tight limit, wraps arguments
        let narrow = format_with_max_line(src, 40);
        assert!(
            narrow.contains("Foo(\n"),
            "should wrap call args with tight limit: {narrow}"
        );
        // Each arg on its own line, indented one level deeper
        assert!(
            narrow.contains("        alpha,\n"),
            "each arg on own line, indented: {narrow}"
        );
    }

    #[test]
    fn wrap_array_literal_when_exceeds_max_line() {
        let src = "template T() {\n    var x = [alpha, bravo, charlie, delta, echo];\n}\n";
        let narrow = format_with_max_line(src, 40);
        assert!(
            narrow.contains("[\n"),
            "should wrap array literal: {narrow}"
        );
        // Each element on its own line, indented one level deeper
        assert!(
            narrow.contains("        alpha,\n"),
            "each element on own line: {narrow}"
        );
    }

    #[test]
    fn wrap_template_params_when_exceeds_max_line() {
        let src =
            "template MyLongTemplate(alpha, bravo, charlie, delta, echo) {\n    var x = 1;\n}\n";
        let narrow = format_with_max_line(src, 40);
        assert!(
            narrow.contains("MyLongTemplate(\n"),
            "should wrap template params: {narrow}"
        );
    }

    #[test]
    fn no_wrap_when_within_limit() {
        let src = "template T() {\n    var x = Foo(a, b);\n}\n";
        let output = format_with_max_line(src, 80);
        assert!(
            output.contains("Foo(a, b)"),
            "short call should stay flat: {output}"
        );
    }

    #[test]
    fn wrap_none_max_line_length_means_no_wrapping() {
        let src = "template T() {\n    var x = Foo(alpha, bravo, charlie, delta, echo, foxtrot, golf, hotel);\n}\n";
        let config = FormatConfig {
            max_line_length: None,
            ..FormatConfig::default()
        };
        let (file, _) = parser::parse(src);
        let output = format_with_config(&file, &config);
        // Everything on one line
        assert!(
            output.contains("Foo(alpha, bravo, charlie, delta, echo, foxtrot, golf, hotel)"),
            "None limit means no wrapping: {output}"
        );
    }

    #[test]
    fn wrap_is_idempotent() {
        let src = "template T() {\n    var x = Foo(alpha, bravo, charlie, delta, echo);\n}\n";
        let first = format_with_max_line(src, 40);
        let second = format_with_max_line(&first, 40);
        assert_eq!(first, second, "wrapping should be idempotent");
    }

    #[test]
    fn wrap_preserves_semantics() {
        // Wrapped output should re-parse without errors
        let src = "template T() {\n    var x = Foo(alpha, bravo, charlie, delta, echo);\n}\n";
        let wrapped = format_with_max_line(src, 40);
        let (_file1, _) = parser::parse(src);
        let (file2, errors) = parser::parse(&wrapped);
        assert!(
            errors.is_empty(),
            "wrapped output has parse errors: {errors:?}"
        );
        // Re-format the wrapped output — should be identical (idempotent)
        let rewrapped = format_with_max_line(&wrapped, 40);
        assert_eq!(wrapped, rewrapped, "wrap is not idempotent");
        // The AST should have the same structure (ignoring spans)
        assert_eq!(file2.items.len(), 1);
    }

    #[test]
    fn wrap_anonymous_comp_args() {
        let src =
            "template T() {\n    var x = MyTemplate(alpha, bravo)(charlie, delta, echo);\n}\n";
        let narrow = format_with_max_line(src, 40);
        // Should wrap at least one of the argument lists
        let lines: Vec<&str> = narrow.lines().collect();
        let max_len = lines.iter().map(|l| l.len()).max().unwrap_or(0);
        // With wrapping, no line should vastly exceed the limit
        // (some slight overflow is acceptable for indentation)
        assert!(
            max_len <= 60,
            "lines should respect approximate limit, max was {max_len}: {narrow}"
        );
    }

    #[test]
    fn wrap_main_component_public_signals() {
        let src = "component main {public [alpha, bravo, charlie, delta, echo, foxtrot]} = MyTemplate(10);\n";
        let narrow = format_with_max_line(src, 50);
        // The public signal list or the whole line should wrap
        let lines: Vec<&str> = narrow.lines().collect();
        let max_len = lines.iter().map(|l| l.len()).max().unwrap_or(0);
        assert!(
            max_len <= 70,
            "main component should wrap, max was {max_len}: {narrow}"
        );
    }

    #[test]
    fn trivia_trailing_comment_on_pragma() {
        let src = "pragma circom 2.0.0; // version\ntemplate T() {\n    var x = 1;\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("pragma circom 2.0.0; // version\n"),
            "trailing comment on pragma should be preserved on same line, output: {output}"
        );
    }

    #[test]
    fn trivia_trailing_comment_on_include() {
        let src = "pragma circom 2.0.0;\ninclude \"foo.circom\"; // reason\ntemplate T() {\n    var x = 1;\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("include \"foo.circom\"; // reason\n"),
            "trailing comment on include should be preserved on same line, output: {output}"
        );
    }

    #[test]
    fn trivia_trailing_comment_on_main_component() {
        let src = "pragma circom 2.0.0;\ntemplate T() {\n    signal input x;\n}\ncomponent main = T(); // entry point\n";
        let output = format_trivia(src);
        assert!(
            output.contains("component main = T(); // entry point\n"),
            "trailing comment on main component should be on same line, output: {output}"
        );
    }

    #[test]
    fn pretty_print_function_def() {
        let src = "function f(a, b) {\n    return a + b;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("function f(a, b)"), "output: {output}");
        assert!(output.contains("return a + b;"), "output: {output}");
    }

    #[test]
    fn trivia_main_component_with_public() {
        let src = "pragma circom 2.0.0;\n// tmpl\ntemplate T() {\n    signal input a;\n}\ncomponent main {public [a]} = T();\n";
        let output = format_trivia(src);
        assert!(
            output.contains("component main {public [a]} = T();"),
            "output: {output}"
        );
        assert!(output.contains("// tmpl"), "output: {output}");
    }

    #[test]
    fn trivia_if_else() {
        let src = "// hdr\ntemplate T() {\n    var x = 1;\n    if (x) {\n        x = 2;\n    } else {\n        x = 3;\n    }\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("if (x)"), "output: {output}");
        assert!(output.contains("} else {"), "output: {output}");
    }

    #[test]
    fn trivia_while_loop() {
        let src = "// hdr\ntemplate T() {\n    var i = 0;\n    while (i < 10) {\n        i = i + 1;\n    }\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("while (i < 10)"), "output: {output}");
    }

    #[test]
    fn trivia_for_loop() {
        let src = "// hdr\ntemplate T() {\n    for (var i = 0; i < 10; i++) {\n        var x = i;\n    }\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("for (var i = 0; i < 10; i++)"),
            "output: {output}"
        );
    }

    #[test]
    fn trivia_signal_declarations() {
        let src = "// hdr\ntemplate T() {\n    signal input a;\n    signal output b;\n    signal c;\n    b <== a;\n    c <-- a;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("signal input a;"), "output: {output}");
        assert!(output.contains("signal output b;"), "output: {output}");
        assert!(output.contains("b <== a;"), "output: {output}");
        assert!(output.contains("c <-- a;"), "output: {output}");
    }

    #[test]
    fn trivia_component_declaration() {
        let src = "// hdr\ntemplate T() {\n    component c = OtherT();\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("component c = OtherT();"),
            "output: {output}"
        );
    }

    #[test]
    fn trivia_assert() {
        let src = "// hdr\ntemplate T() {\n    var x = 1;\n    assert(x == 1);\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("assert(x == 1);"), "output: {output}");
    }

    #[test]
    fn trivia_log() {
        let src = "// hdr\ntemplate T() {\n    var x = 1;\n    log(x);\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("log(x);"), "output: {output}");
    }

    #[test]
    fn trivia_ternary_expression() {
        let src = "// hdr\ntemplate T() {\n    var x = 1;\n    var y = x ? 2 : 3;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("x ? 2 : 3"), "output: {output}");
    }

    #[test]
    fn trivia_array_expression() {
        let src = "// hdr\ntemplate T() {\n    var x[3] = [1, 2, 3];\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("[1, 2, 3]"), "output: {output}");
    }

    #[test]
    fn trivia_constraint_equality() {
        let src =
            "// hdr\ntemplate T() {\n    signal input a;\n    signal input b;\n    a === b;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("a === b;"), "output: {output}");
    }

    #[test]
    fn trivia_unary_expressions() {
        let src = "// hdr\ntemplate T() {\n    var x = 1;\n    var y = -x;\n    var z = !x;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("-x"), "output: {output}");
        assert!(output.contains("!x"), "output: {output}");
    }

    #[test]
    fn trivia_signal_array() {
        let src = "// hdr\ntemplate T() {\n    signal input a[3];\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("signal input a[3];"), "output: {output}");
    }

    #[test]
    fn trivia_increment_decrement() {
        let src = "// hdr\ntemplate T() {\n    var x = 0;\n    x++;\n    x--;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("x++;"), "output: {output}");
        assert!(output.contains("x--;"), "output: {output}");
    }

    #[test]
    fn trivia_compound_assign() {
        let src = "// hdr\ntemplate T() {\n    var x = 1;\n    x += 2;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("x += 2;"), "output: {output}");
    }

    #[test]
    fn trivia_return_statement() {
        let src = "// a func\nfunction f() {\n    return 42;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("return 42;"), "output: {output}");
    }

    #[test]
    fn trivia_expression_statement() {
        let src = "// hdr\ntemplate T() {\n    var x = 1;\n    x;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("x;"), "output: {output}");
    }

    #[test]
    fn trivia_remaining_comments_at_eof() {
        let src = "// hdr\ntemplate T() {\n    var x = 1;\n}\n// end\n";
        let output = format_trivia(src);
        assert!(output.contains("// end"), "output: {output}");
    }

    #[test]
    fn trivia_multiple_functions_with_comments() {
        let src = "// first\nfunction f() {\n    return 1;\n}\n\n// second\nfunction g() {\n    return 2;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("// first"), "output: {output}");
        assert!(output.contains("// second"), "output: {output}");
        assert!(output.contains("function f()"), "output: {output}");
        assert!(output.contains("function g()"), "output: {output}");
    }

    #[test]
    fn trivia_function_with_body() {
        let src = "// fn\nfunction f(a, b) {\n    var c = a + b;\n    return c;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("function f(a, b)"), "output: {output}");
        assert!(output.contains("return c;"), "output: {output}");
    }

    #[test]
    fn trivia_include_statement() {
        let src = "pragma circom 2.0.0;\n// lib\ninclude \"lib.circom\";\n// tmpl\ntemplate T() {\n    var x = 1;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("// lib"), "output: {output}");
        assert!(
            output.contains("include \"lib.circom\";"),
            "output: {output}"
        );
    }

    #[test]
    fn display_log_statement() {
        let src = "template T() {\n    var x = 1;\n    log(x);\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("log(x);"), "output: {output}");
    }

    #[test]
    fn display_assert_statement() {
        let src = "template T() {\n    var x = 1;\n    assert(x);\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("assert(x);"), "output: {output}");
    }

    #[test]
    fn display_increment_decrement() {
        let src = "template T() {\n    var x = 0;\n    x++;\n    x--;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("x++;"), "output: {output}");
        assert!(output.contains("x--;"), "output: {output}");
    }

    #[test]
    fn display_compound_assign() {
        let src = "template T() {\n    var x = 1;\n    x += 2;\n    x -= 1;\n    x *= 3;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("x += 2;"), "output: {output}");
        assert!(output.contains("x -= 1;"), "output: {output}");
        assert!(output.contains("x *= 3;"), "output: {output}");
    }

    #[test]
    fn display_constraint_equality() {
        let src = "template T() {\n    signal input a;\n    signal input b;\n    a === b;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("a === b;"), "output: {output}");
    }

    #[test]
    fn display_return_statement() {
        let src = "function f() {\n    return 42;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("return 42;"), "output: {output}");
    }

    #[test]
    fn display_if_else_statement() {
        let src = "template T() {\n    var x = 1;\n    if (x) {\n        x = 2;\n    } else {\n        x = 3;\n    }\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("if (x)"), "output: {output}");
        assert!(output.contains("} else {"), "output: {output}");
    }

    #[test]
    fn display_for_loop() {
        let src =
            "template T() {\n    for (var i = 0; i < 10; i++) {\n        var x = i;\n    }\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("for ("), "output: {output}");
    }

    #[test]
    fn display_while_loop() {
        let src =
            "template T() {\n    var i = 0;\n    while (i < 10) {\n        i = i + 1;\n    }\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("while ("), "output: {output}");
    }

    #[test]
    fn display_component_declaration() {
        let src = "template T() {\n    component c = OtherT();\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(
            output.contains("component c = OtherT();"),
            "output: {output}"
        );
    }

    #[test]
    fn display_assignment_ops() {
        let src = "template T() {\n    signal input a;\n    signal output b;\n    b <== a;\n    b <-- a;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("b <== a;"), "output: {output}");
        assert!(output.contains("b <-- a;"), "output: {output}");
    }

    #[test]
    fn display_expression_statement() {
        let src = "template T() {\n    var x = 1;\n    x;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("    x;\n"), "output: {output}");
    }

    #[test]
    fn display_ternary_and_binary() {
        let src = "template T() {\n    var x = 1;\n    var y = x > 0 ? x + 1 : x - 1;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("x > 0 ? x + 1 : x - 1"), "output: {output}");
    }

    #[test]
    fn display_multiple_var_dimensions() {
        let src = "template T() {\n    var x[2][3];\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("var x[2][3];"), "output: {output}");
    }

    #[test]
    fn display_signal_with_init() {
        let src = "template T() {\n    signal input a;\n    signal output b <== a;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(
            output.contains("signal output b <== a;"),
            "output: {output}"
        );
    }

    #[test]
    fn display_include_escape() {
        let src = "include \"path/to/lib.circom\";\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(
            output.contains("include \"path/to/lib.circom\";"),
            "output: {output}"
        );
    }

    #[test]
    fn display_main_component() {
        let src =
            "pragma circom 2.0.0;\ntemplate T() {\n    signal input a;\n}\ncomponent main = T();\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("component main = T();"), "output: {output}");
    }

    #[test]
    fn display_main_component_public_signals() {
        let src = "pragma circom 2.0.0;\ntemplate T() {\n    signal input a;\n}\ncomponent main {public [a]} = T();\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(
            output.contains("component main {public [a]} = T();"),
            "output: {output}"
        );
    }

    #[test]
    fn display_function_def() {
        let src = "function f(a, b) {\n    return a + b;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("function f(a, b)"), "output: {output}");
    }

    #[test]
    fn display_access_expression() {
        let src = "template T() {\n    signal input a[3];\n    var x = a[0];\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("a[0]"), "output: {output}");
    }

    #[test]
    fn display_member_access() {
        let src = "template T() {\n    component c = OtherT();\n    var x = c.out;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("c.out"), "output: {output}");
    }

    #[test]
    fn display_bitwise_ops() {
        let src = "template T() {\n    var x = 1;\n    var a = ~x;\n    var b = x & 3;\n    var c = x | 2;\n    var d = x ^ 1;\n    var e = x << 2;\n    var f = x >> 1;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("~x"), "output: {output}");
        assert!(output.contains("x & 3"), "output: {output}");
        assert!(output.contains("x | 2"), "output: {output}");
        assert!(output.contains("x ^ 1"), "output: {output}");
        assert!(output.contains("x << 2"), "output: {output}");
        assert!(output.contains("x >> 1"), "output: {output}");
    }

    #[test]
    fn display_logical_ops() {
        let src = "template T() {\n    var x = 1;\n    var a = x && 1;\n    var b = x || 0;\n    var c = x == 1;\n    var d = x != 0;\n    var e = x <= 1;\n    var f = x >= 0;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("x && 1"), "output: {output}");
        assert!(output.contains("x || 0"), "output: {output}");
        assert!(output.contains("x == 1"), "output: {output}");
        assert!(output.contains("x != 0"), "output: {output}");
        assert!(output.contains("x <= 1"), "output: {output}");
        assert!(output.contains("x >= 0"), "output: {output}");
    }

    #[test]
    fn display_power_and_div_ops() {
        let src = "template T() {\n    var x = 2;\n    var a = x ** 3;\n    var b = x % 2;\n    var c = x \\ 3;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("x ** 3"), "output: {output}");
        assert!(output.contains("x % 2"), "output: {output}");
        assert!(output.contains("x \\ 3"), "output: {output}");
    }

    #[test]
    fn display_for_with_decrement() {
        let src =
            "template T() {\n    for (var i = 10; i > 0; i--) {\n        var x = i;\n    }\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("i--"), "output: {output}");
    }

    #[test]
    fn display_for_with_compound() {
        let src =
            "template T() {\n    for (var i = 0; i < 10; i += 2) {\n        var x = i;\n    }\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("i += 2"), "output: {output}");
    }

    #[test]
    fn display_log_with_string() {
        let src = "template T() {\n    log(\"hello\");\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("log(\"hello\")"), "output: {output}");
    }

    #[test]
    fn display_nested_parentheses() {
        let src = "template T() {\n    var x = (1 + 2) * 3;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("(1 + 2) * 3"), "output: {output}");
    }

    #[test]
    fn display_signal_intermediate() {
        let src = "template T() {\n    signal s;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("signal s;"), "output: {output}");
    }

    #[test]
    fn display_multiple_signals() {
        let src = "template T() {\n    signal input a, b;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("signal input a, b;"), "output: {output}");
    }

    #[test]
    fn display_signal_with_tags() {
        let src = "template T() {\n    signal input {binary} a;\n}\n";
        let (file, errors) = parser::parse(src);
        if errors.is_empty() {
            let output = file.to_string();
            assert!(output.contains("{binary}"), "output: {output}");
        }
    }

    #[test]
    fn display_component_array() {
        let src = "template T() {\n    component c[3];\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("component c[3];"), "output: {output}");
    }

    #[test]
    fn display_multiple_includes() {
        let src = "include \"a.circom\";\ninclude \"b.circom\";\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(
            output.contains("include \"a.circom\";\n"),
            "output: {output}"
        );
        assert!(
            output.contains("include \"b.circom\";\n"),
            "output: {output}"
        );
    }

    #[test]
    fn display_right_arrow_assign() {
        let src = "template T() {\n    signal input a;\n    signal output b;\n    a ==> b;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("a ==> b;"), "output: {output}");
    }

    #[test]
    fn display_unsafe_right_assign() {
        let src = "template T() {\n    signal input a;\n    signal output b;\n    a --> b;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("a --> b;"), "output: {output}");
    }

    #[test]
    fn display_bus_def() {
        let src = "pragma circom 2.0.0;\nbus MyBus() {\n    signal input a;\n}\n";
        let (file, errors) = parser::parse(src);
        if errors.is_empty() {
            let output = file.to_string();
            assert!(output.contains("bus MyBus()"), "output: {output}");
        }
    }

    #[test]
    fn display_include_with_escape() {
        // Test escaped characters in include path
        let src = "include \"path\\\\to\\nfile.circom\";\n";
        let (file, errors) = parser::parse(src);
        if errors.is_empty() {
            let output = file.to_string();
            assert!(output.contains("include \""), "output: {output}");
        }
    }

    #[test]
    fn display_anonymous_comp() {
        let src = "template T() {\n    signal input a;\n    signal output b;\n    b <== AnotherT()(a);\n}\n";
        let (file, errors) = parser::parse(src);
        if errors.is_empty() {
            let output = file.to_string();
            assert!(output.contains("AnotherT()"), "output: {output}");
        }
    }

    #[test]
    fn display_custom_templates() {
        let src = "pragma custom_templates;\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(
            output.contains("pragma custom_templates;"),
            "output: {output}"
        );
    }

    #[test]
    fn display_multi_compound_ops() {
        let src = "template T() {\n    var x = 10;\n    x -= 1;\n    x *= 2;\n    x /= 3;\n    x %= 4;\n    x **= 2;\n    x \\= 3;\n    x <<= 1;\n    x >>= 1;\n    x &= 7;\n    x |= 3;\n    x ^= 5;\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("x -= 1;"), "output: {output}");
        assert!(output.contains("x *= 2;"), "output: {output}");
        assert!(output.contains("x /= 3;"), "output: {output}");
        assert!(output.contains("x %= 4;"), "output: {output}");
        assert!(output.contains("x **= 2;"), "output: {output}");
        assert!(output.contains("x \\= 3;"), "output: {output}");
        assert!(output.contains("x <<= 1;"), "output: {output}");
        assert!(output.contains("x >>= 1;"), "output: {output}");
        assert!(output.contains("x &= 7;"), "output: {output}");
        assert!(output.contains("x |= 3;"), "output: {output}");
        assert!(output.contains("x ^= 5;"), "output: {output}");
    }

    #[test]
    fn trivia_bus_def() {
        let src = "pragma circom 2.0.0;\n// bus comment\nbus MyBus() {\n    signal input a;\n}\n";
        let (_, errors) = parser::parse(src);
        if errors.is_empty() {
            let output = format_trivia(src);
            assert!(output.contains("bus MyBus()"), "output: {output}");
            assert!(output.contains("// bus comment"), "output: {output}");
        }
    }

    #[test]
    fn display_log_with_multiple_args() {
        let src = "template T() {\n    var x = 1;\n    log(x, \"val=\", x);\n}\n";
        let (file, errors) = parser::parse(src);
        if errors.is_empty() {
            let output = file.to_string();
            assert!(output.contains("log("), "output: {output}");
        }
    }

    #[test]
    fn trivia_bus_decl_statement() {
        let src = "pragma circom 2.0.0;\nbus MyBus() {\n    signal input a;\n}\n// hdr\ntemplate T() {\n    MyBus input b;\n}\n";
        let (_, errors) = parser::parse(src);
        if errors.is_empty() {
            let output = format_trivia(src);
            assert!(output.contains("MyBus"), "output: {output}");
        }
    }

    #[test]
    fn display_bus_instance() {
        let src = "pragma circom 2.0.0;\nbus MyBus() {\n    signal input a;\n}\ntemplate T() {\n    MyBus input b;\n}\n";
        let (file, errors) = parser::parse(src);
        if errors.is_empty() {
            let output = file.to_string();
            assert!(output.contains("MyBus"), "output: {output}");
        }
    }

    #[test]
    fn display_for_with_assignment_step() {
        let src = "template T() {\n    for (var i = 0; i < 10; i = i + 1) {\n        var x = i;\n    }\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("i = i + 1"), "output: {output}");
    }

    #[test]
    fn trivia_for_with_decrement_step() {
        let src = "// hdr\ntemplate T() {\n    for (var i = 10; i > 0; i--) {\n        var x = i;\n    }\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("i--"), "output: {output}");
    }

    #[test]
    fn trivia_for_with_compound_step() {
        let src = "// hdr\ntemplate T() {\n    for (var i = 0; i < 10; i += 2) {\n        var x = i;\n    }\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("i += 2"), "output: {output}");
    }

    #[test]
    fn display_escaped_string_in_include() {
        // Verify that the escaped string writer handles backslash and quotes
        let mut buf = String::new();
        let mut w = IndentWriter::new(&mut buf);
        write_escaped_str(&mut w, "hello\\world\"test\nfoo").unwrap();
        assert!(buf.contains("\\\\"), "should escape backslash: {buf}");
        assert!(buf.contains("\\\""), "should escape quote: {buf}");
        assert!(buf.contains("\\n"), "should escape newline: {buf}");
    }

    #[test]
    fn trivia_trailing_then_leading_comment() {
        // Trailing comment on one stmt, then leading comment on next
        let src = "// hdr\ntemplate T() {\n    var x = 1; // trailing\n    // leading\n    var y = 2;\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("var x = 1; // trailing"),
            "output: {output}"
        );
        assert!(output.contains("// leading"), "output: {output}");
        assert!(output.contains("var y = 2;"), "output: {output}");
    }

    #[test]
    fn format_config_with_max_line_length() {
        let config = FormatConfig::default().with_max_line_length(80);
        assert_eq!(config.max_line_length, Some(80));
    }

    #[test]
    fn trivia_extern_template() {
        let src = "// hdr\ntemplate extern T() {\n    signal input x;\n}\n";
        let (_, errors) = parser::parse(src);
        if errors.is_empty() {
            let output = format_trivia(src);
            assert!(output.contains("extern"), "output: {output}");
        }
    }

    #[test]
    fn trivia_if_without_else() {
        let src =
            "// hdr\ntemplate T() {\n    var x = 1;\n    if (x) {\n        x = 2;\n    }\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("if (x)"), "output: {output}");
        assert!(!output.contains("else"), "no else in output: {output}");
    }

    #[test]
    fn trivia_multiple_var_names() {
        let src = "// hdr\ntemplate T() {\n    var a, b, c;\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("var a, b, c;"), "output: {output}");
    }

    #[test]
    fn trivia_signal_init_with_op() {
        let src = "// hdr\ntemplate T() {\n    signal input a;\n    signal output b <== a;\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("signal output b <== a;"),
            "output: {output}"
        );
    }

    #[test]
    fn trivia_component_array_init() {
        let src = "// hdr\ntemplate T() {\n    component c[3];\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("component c[3];"), "output: {output}");
    }

    #[test]
    fn trivia_nested_index_member() {
        let src = "// hdr\ntemplate T() {\n    component c = OtherT();\n    var x = c.out[0];\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("c.out[0]"), "output: {output}");
    }

    #[test]
    fn parse_lex_error_coverage() {
        // Exercise lex error and parse error paths for coverage
        let src = "template T() {\n    var x = @;\n}\n";
        let (file, errors) = parser::parse(src);
        // Expected: parse/lex errors
        assert!(!errors.is_empty());
        // Should still produce a partial AST
        let _ = file.to_string();
    }

    #[test]
    fn parse_error_recovery() {
        // More parse error paths
        let src = "template T( {\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(!errors.is_empty());
        let _ = file.to_string();
    }

    #[test]
    fn parse_empty_file() {
        let (file, errors) = parser::parse("");
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.is_empty());
    }

    #[test]
    fn trivia_trailing_comments_on_statements() {
        let src = "// hdr\ntemplate T() {\n    var x = 1; // init x\n    signal input a; // the input\n    x = x + a; // update\n}\n";
        let output = format_trivia(src);
        assert!(output.contains("var x = 1; // init x"), "output: {output}");
        assert!(
            output.contains("signal input a; // the input"),
            "output: {output}"
        );
        assert!(output.contains("x = x + a; // update"), "output: {output}");
    }

    #[test]
    fn trivia_leading_comment_on_function() {
        let src = "// a function\nfunction f() {\n    return 0;\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("// a function\nfunction f()"),
            "leading comment should be before function, output: {output}"
        );
    }

    #[test]
    fn trivia_comment_inside_block() {
        let src = "template T() {\n    // inner comment\n    var x = 1;\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("// inner comment"),
            "comment inside block should be preserved, output: {output}"
        );
    }

    #[test]
    fn trivia_trailing_comment_on_statement() {
        let src = "template T() {\n    var x = 1; // init\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("var x = 1; // init"),
            "trailing comment on statement should be preserved, output: {output}"
        );
    }

    #[test]
    fn trivia_end_of_file_comment() {
        let src = "pragma circom 2.0.0;\n// end of file\n";
        let output = format_trivia(src);
        assert!(
            output.contains("// end of file"),
            "end-of-file comment should be preserved, output: {output}"
        );
    }

    #[test]
    fn format_with_trivia_idempotent() {
        let src = "pragma circom 2.0.0;\n\ntemplate T() {\n    signal input a;\n    signal output b;\n    b <== a;\n}\n\ncomponent main = T();\n";
        let config = FormatConfig::default();
        let (ast, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let first = format_with_trivia(src, &ast, &config);
        let (ast2, errors2) = parser::parse(&first);
        assert!(errors2.is_empty());
        let second = format_with_trivia(&first, &ast2, &config);
        assert_eq!(first, second, "formatting should be idempotent");
    }

    #[test]
    fn measure_comma_sep_idents_empty() {
        assert_eq!(measure_comma_sep_idents(&[]), 0);
    }

    #[test]
    fn measure_comma_sep_idents_single() {
        let idents = vec![Identifier {
            name: "abc".to_string(),
            span: Span { start: 0, end: 3 },
        }];
        assert_eq!(measure_comma_sep_idents(&idents), 3);
    }

    #[test]
    fn measure_comma_sep_idents_multiple() {
        let idents = vec![
            Identifier {
                name: "a".to_string(),
                span: Span { start: 0, end: 1 },
            },
            Identifier {
                name: "bb".to_string(),
                span: Span { start: 3, end: 5 },
            },
        ];
        // "a, bb" = 1 + 2 + 2 = 5
        assert_eq!(measure_comma_sep_idents(&idents), 5);
    }

    #[test]
    fn trivia_trailing_comment_on_bus_member() {
        let src = "pragma circom 2.0.0;\nbus MyBus() {\n    signal input a; // important\n}\n";
        let (_, errors) = parser::parse(src);
        if errors.is_empty() {
            let output = format_trivia(src);
            assert!(
                output.contains("signal input a; // important"),
                "trailing comment on bus member should be preserved, output: {output}"
            );
        }
    }

    #[test]
    fn trivia_trailing_comment_on_if_else() {
        let src = "// hdr\ntemplate T() {\n    var x = 1;\n    if (x) {\n        x = 2;\n    } // check x\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("} // check x"),
            "trailing comment on if statement should be preserved, output: {output}"
        );
    }

    #[test]
    fn trivia_trailing_comment_on_for_loop() {
        let src = "// hdr\ntemplate T() {\n    for (var i = 0; i < 10; i++) {\n        var x = i;\n    } // loop\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("} // loop"),
            "trailing comment on for loop should be preserved, output: {output}"
        );
    }

    #[test]
    fn trivia_trailing_comment_on_while_loop() {
        let src = "// hdr\ntemplate T() {\n    var i = 0;\n    while (i < 10) {\n        i = i + 1;\n    } // while\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("} // while"),
            "trailing comment on while loop should be preserved, output: {output}"
        );
    }

    #[test]
    fn trivia_multiline_block_comment_indentation() {
        let src = "template T() {\n    /* line1\n       line2 */\n    var x = 1;\n}\n";
        let output = format_trivia(src);
        // Each continuation line should be re-indented to match the block level.
        assert!(
            output.contains("    /* line1\n"),
            "first line should be indented, output: {output}"
        );
        // Continuation line should have indent + space + trimmed content
        assert!(
            output.contains("     line2 */"),
            "continuation line should be re-indented, output: {output}"
        );
    }

    #[test]
    fn trivia_else_if_chain_is_flat() {
        let src = "template T() {\n    var x = 1;\n    if (x) {\n        x = 2;\n    } else {\n        if (x) {\n            x = 3;\n        }\n    }\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(
            output.contains("} else if (x) {"),
            "else-if should be flattened, output: {output}"
        );
        assert!(
            !output.contains("} else {\n        if"),
            "should not have nested else-if, output: {output}"
        );
    }

    #[test]
    fn trivia_else_if_chain_with_trivia() {
        let src = "// hdr\ntemplate T() {\n    var x = 1;\n    if (x) {\n        x = 2;\n    } else {\n        if (x) {\n            x = 3;\n        }\n    }\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("} else if (x) {"),
            "trivia else-if should be flattened, output: {output}"
        );
    }

    #[test]
    fn trivia_block_comment_preservation() {
        let src = "/* file header */\npragma circom 2.0.0;\n\n/* template doc */\ntemplate T() {\n    signal input a;\n}\n";
        let output = format_trivia(src);
        assert!(
            output.contains("/* file header */"),
            "block comment should be preserved, output: {output}"
        );
        assert!(
            output.contains("/* template doc */"),
            "block comment before template should be preserved, output: {output}"
        );
    }
}
