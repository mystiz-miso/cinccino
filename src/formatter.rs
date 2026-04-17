//! Source-level formatter for Circom.
//!
//! The formatter parses the source, reuses the AST from [`crate::parser`]
//! and the structure from [`crate::pretty_print`], and produces a
//! canonical, opinionated rendering that:
//!
//! - preserves line and block comments (issue #93) by interleaving them
//!   with AST nodes based on the original byte offsets, and
//! - honours a configurable maximum line length (issue #94), wrapping
//!   long argument lists, declarations and binary chains at sensible
//!   points.
//!
//! The public entry point used by the LSP handler and by tests is
//! [`format_source`].

use std::fmt::Write as _;

use crate::ast::*;
use crate::lexer::{extract_comments, Comment, CommentKind};
use crate::parser;
use crate::span::LineIndex;

/// User-configurable options for the formatter.
#[derive(Debug, Clone, Copy)]
pub struct FormatConfig {
    /// Soft maximum line length. Lines longer than this are wrapped
    /// where the formatter has a natural break point (call arguments,
    /// signal-declaration entries, binary-operator chains). String and
    /// comment contents are never broken.
    pub max_line_length: usize,
    /// Width of a single indentation step, in spaces.
    pub indent_width: usize,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            max_line_length: 100,
            indent_width: 4,
        }
    }
}

/// An error produced by the formatter. Currently the only failure mode
/// is a source with parse errors — the formatter refuses to rewrite
/// malformed input because it cannot guarantee semantic preservation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// One or more parse errors were encountered. The original source
    /// is returned unchanged by [`format_source_safe`].
    ParseFailed,
}

/// Format `source` according to `config`. Returns an error if the
/// source cannot be parsed.
pub fn format_source(source: &str, config: &FormatConfig) -> Result<String, FormatError> {
    let (file, errors) = parser::parse(source);
    if !errors.is_empty() {
        return Err(FormatError::ParseFailed);
    }
    let comments = extract_comments(source);
    Ok(render(source, &file, &comments, config))
}

/// Like [`format_source`] but returns the original text verbatim if the
/// source cannot be parsed. This is the variant the LSP handler uses —
/// we don't want to corrupt a partially-edited buffer.
pub fn format_source_safe(source: &str, config: &FormatConfig) -> String {
    match format_source(source, config) {
        Ok(formatted) => formatted,
        Err(_) => source.to_string(),
    }
}

// ── Renderer ────────────────────────────────────────────────────────

struct Renderer<'a> {
    buf: String,
    comments: &'a [Comment],
    next_comment: usize,
    level: usize,
    config: &'a FormatConfig,
    line_index: LineIndex,
}

impl<'a> Renderer<'a> {
    fn new(source: &str, comments: &'a [Comment], config: &'a FormatConfig) -> Self {
        Self {
            buf: String::new(),
            comments,
            next_comment: 0,
            level: 0,
            config,
            line_index: LineIndex::new(source),
        }
    }

    fn line_of(&self, offset: usize) -> Option<u32> {
        self.line_index.line_col(offset).map(|lc| lc.line)
    }

    fn indent_str(&self) -> String {
        " ".repeat(self.level * self.config.indent_width)
    }

    /// Width available on the current continuation line after
    /// indentation.
    fn avail(&self) -> usize {
        self.config
            .max_line_length
            .saturating_sub(self.level * self.config.indent_width)
    }

    fn write_indent(&mut self) {
        let n = self.level * self.config.indent_width;
        for _ in 0..n {
            self.buf.push(' ');
        }
    }

    fn ensure_newline(&mut self) {
        if !self.buf.ends_with('\n') {
            self.buf.push('\n');
        }
    }

    // ── Comment placement ─────────────────────────────────────────

    /// Emit every pending comment whose start offset is `< before`.
    /// Each emitted comment goes on its own indented line.
    fn flush_leading_comments_before(&mut self, before: usize) {
        while self.next_comment < self.comments.len()
            && self.comments[self.next_comment].start < before
        {
            let c = self.comments[self.next_comment].clone();
            self.next_comment += 1;
            // Leading comments go on their own line, at the current
            // indentation.
            self.ensure_newline();
            self.write_indent();
            self.write_comment_text(&c);
            self.buf.push('\n');
        }
    }

    /// Emit trailing comments that live on the same source line as a
    /// statement whose span ends at `end_offset`. Each trailing
    /// comment is appended to the current line with two leading
    /// spaces; subsequent trailing comments on the same line are kept
    /// adjacent.
    fn flush_trailing_comments_for(&mut self, end_offset: usize) -> bool {
        let stmt_line = self.line_of(end_offset.saturating_sub(1));
        let mut wrote = false;
        while self.next_comment < self.comments.len() {
            let c = &self.comments[self.next_comment];
            if !c.trailing {
                break;
            }
            let c_line = self.line_of(c.start);
            if c_line != stmt_line {
                break;
            }
            let c = c.clone();
            self.next_comment += 1;
            if self.buf.ends_with('\n') {
                self.buf.pop();
            }
            self.buf.push(' ');
            self.buf.push(' ');
            self.write_comment_text(&c);
            self.buf.push('\n');
            wrote = true;
        }
        wrote
    }

    /// Emit all remaining comments (end-of-file trivia).
    fn flush_remaining_comments(&mut self) {
        while self.next_comment < self.comments.len() {
            let c = self.comments[self.next_comment].clone();
            self.next_comment += 1;
            self.ensure_newline();
            self.write_indent();
            self.write_comment_text(&c);
            self.buf.push('\n');
        }
    }

    fn write_comment_text(&mut self, c: &Comment) {
        match c.kind {
            CommentKind::Line => self.buf.push_str(&c.text),
            CommentKind::Block => {
                // For multi-line block comments we reindent
                // continuation lines to the current indentation so the
                // output stays tidy.
                let indent = self.indent_str();
                let mut first = true;
                for line in c.text.split('\n') {
                    if first {
                        self.buf.push_str(line);
                        first = false;
                    } else {
                        self.buf.push('\n');
                        // Reindent continuation lines. Preserve the
                        // original leading-whitespace structure for
                        // Javadoc-style ` * foo` lines.
                        let trimmed = line.trim_start();
                        self.buf.push_str(&indent);
                        if !trimmed.is_empty() {
                            // Keep one leading space before ` *` lines
                            // so they line up nicely with `/*`.
                            if trimmed.starts_with('*') {
                                self.buf.push(' ');
                            }
                            self.buf.push_str(trimmed);
                        }
                    }
                }
            }
        }
    }

    // ── Top-level ─────────────────────────────────────────────────

    fn render_file(&mut self, file: &File) {
        for (i, item) in file.items.iter().enumerate() {
            let start = item_span(item).start;
            self.flush_leading_comments_before(start);
            if i > 0 && !self.buf.ends_with("\n\n") {
                self.ensure_newline();
                self.buf.push('\n');
            }
            self.render_item(item);
            let end = item_span(item).end;
            self.flush_trailing_comments_for(end);
        }
        self.flush_remaining_comments();
    }

    fn render_item(&mut self, item: &Item) {
        match item {
            Item::Pragma(p) => self.render_pragma(p),
            Item::Include(i) => self.render_include(i),
            Item::TemplateDef(t) => self.render_template_def(t),
            Item::FunctionDef(f) => self.render_function_def(f),
            Item::BusDef(b) => self.render_bus_def(b),
            Item::MainComponent(m) => self.render_main_component(m),
        }
    }

    fn render_pragma(&mut self, node: &Pragma) {
        match &node.kind {
            PragmaKind::Version(v) => {
                let _ = writeln!(
                    self.buf,
                    "pragma circom {}.{}.{};",
                    v.major, v.minor, v.patch
                );
            }
            PragmaKind::CustomTemplates => self.buf.push_str("pragma custom_templates;\n"),
        }
    }

    fn render_include(&mut self, node: &Include) {
        let _ = writeln!(self.buf, "include \"{}\";", node.path);
    }

    fn render_template_def(&mut self, node: &TemplateDef) {
        let mut header = String::new();
        if node.is_custom {
            header.push_str("custom ");
        }
        header.push_str("template ");
        if node.is_extern {
            header.push_str("extern ");
        }
        if node.is_parallel {
            header.push_str("parallel ");
        }
        header.push_str(&node.name.name);
        header.push('(');
        header.push_str(&comma_sep_idents_inline(&node.params));
        header.push_str(") ");
        self.write_indent();
        self.buf.push_str(&header);
        self.render_block(&node.body);
        self.buf.push('\n');
    }

    fn render_function_def(&mut self, node: &FunctionDef) {
        self.write_indent();
        let _ = write!(
            self.buf,
            "function {}({}) ",
            node.name.name,
            comma_sep_idents_inline(&node.params)
        );
        self.render_block(&node.body);
        self.buf.push('\n');
    }

    fn render_bus_def(&mut self, node: &BusDef) {
        self.write_indent();
        let _ = writeln!(
            self.buf,
            "bus {}({}) {{",
            node.name.name,
            comma_sep_idents_inline(&node.params)
        );
        self.level += 1;
        for member in &node.body {
            let mspan = bus_member_span(member);
            self.flush_leading_comments_before(mspan.start);
            self.write_indent();
            self.render_bus_member(member);
            self.buf.push_str(";\n");
            self.flush_trailing_comments_for(mspan.end);
        }
        self.level -= 1;
        self.write_indent();
        self.buf.push_str("}\n");
    }

    fn render_bus_member(&mut self, node: &BusMember) {
        match node {
            BusMember::Signal(s) => self.render_signal_decl(s),
            BusMember::Bus(b) => self.render_bus_field_decl(b),
        }
    }

    fn render_bus_field_decl(&mut self, node: &BusFieldDecl) {
        let mut head = String::new();
        head.push_str(&render_bus_type(&node.bus_type));
        if !node.tags.is_empty() {
            head.push_str(" {");
            head.push_str(&comma_sep_idents_inline(&node.tags));
            head.push('}');
        }
        head.push(' ');
        head.push_str(&node.name.name);
        for dim in &node.dimensions {
            head.push('[');
            head.push_str(&render_expr_inline(dim));
            head.push(']');
        }
        self.buf.push_str(&head);
    }

    fn render_main_component(&mut self, node: &MainComponent) {
        self.write_indent();
        self.buf.push_str("component main");
        if !node.public_signals.is_empty() {
            self.buf.push_str(" {public [");
            self.buf
                .push_str(&comma_sep_idents_inline(&node.public_signals));
            self.buf.push_str("]}");
        }
        self.buf.push_str(" = ");
        self.render_expr(&node.expr);
        self.buf.push_str(";\n");
    }

    // ── Statements & blocks ───────────────────────────────────────

    fn render_block(&mut self, block: &Block) {
        self.buf.push_str("{\n");
        self.level += 1;
        for stmt in &block.stmts {
            let s = stmt.span;
            self.flush_leading_comments_before(s.start);
            self.render_statement(stmt);
            self.flush_trailing_comments_for(s.end);
        }
        // Flush comments that sit before the closing brace so they are
        // not lost to end-of-file trivia.
        self.flush_leading_comments_before(block.span.end);
        self.level -= 1;
        self.write_indent();
        self.buf.push('}');
    }

    fn render_simple_stmt_body(&mut self, stmt: &Statement) -> bool {
        // Returns true if the statement was handled as a simple ";\n"-terminated form.
        match &stmt.kind {
            StatementKind::VarDecl(n) => self.render_var_decl(n),
            StatementKind::SignalDecl(n) => self.render_signal_decl(n),
            StatementKind::ComponentDecl(n) => self.render_component_decl(n),
            StatementKind::BusDecl(n) => self.render_bus_instance_decl(n),
            StatementKind::Assignment(n) => self.render_assign_stmt(n),
            StatementKind::CompoundAssign(n) => self.render_compound_assign_stmt(n),
            StatementKind::ConstraintEq(n) => self.render_constraint_eq_stmt(n),
            StatementKind::TupleAssign(n) => self.render_tuple_assign_stmt(n),
            StatementKind::Return(n) => {
                self.buf.push_str("return ");
                self.render_expr(&n.value);
            }
            StatementKind::Log(n) => self.render_log_stmt(n),
            StatementKind::Assert(n) => {
                self.buf.push_str("assert(");
                self.render_expr(&n.expr);
                self.buf.push(')');
            }
            StatementKind::Expression(expr) => self.render_expr(expr),
            _ => return false,
        }
        self.buf.push_str(";\n");
        true
    }

    fn render_statement(&mut self, stmt: &Statement) {
        self.write_indent();
        if self.render_simple_stmt_body(stmt) {
            return;
        }
        match &stmt.kind {
            StatementKind::IfElse(n) => {
                self.render_if_else(n);
                self.buf.push('\n');
            }
            StatementKind::For(n) => {
                self.render_for_loop(n);
                self.buf.push('\n');
            }
            StatementKind::While(n) => {
                self.render_while_loop(n);
                self.buf.push('\n');
            }
            StatementKind::Increment(expr) => {
                self.render_expr(expr);
                self.buf.push_str("++;\n");
            }
            StatementKind::Decrement(expr) => {
                self.render_expr(expr);
                self.buf.push_str("--;\n");
            }
            StatementKind::Block(blk) => {
                self.render_block(blk);
                self.buf.push('\n');
            }
            StatementKind::Error => self.buf.push_str("/* error */;\n"),
            _ => {}
        }
    }

    fn render_var_decl(&mut self, node: &VarDecl) {
        self.buf.push_str("var ");
        let entries = node
            .names
            .iter()
            .map(render_var_decl_entry)
            .collect::<Vec<_>>();
        self.emit_wrapped_entries(&entries, "var ");
    }

    fn render_signal_decl(&mut self, node: &SignalDecl) {
        self.buf.push_str("signal ");
        match node.kind {
            SignalKind::Input => self.buf.push_str("input "),
            SignalKind::Output => self.buf.push_str("output "),
            SignalKind::Intermediate => {}
        }
        let mut prefix = String::from("signal ");
        match node.kind {
            SignalKind::Input => prefix.push_str("input "),
            SignalKind::Output => prefix.push_str("output "),
            SignalKind::Intermediate => {}
        }
        if !node.tags.is_empty() {
            let tags = format!("{{{}}} ", comma_sep_idents_inline(&node.tags));
            self.buf.push_str(&tags);
            prefix.push_str(&tags);
        }
        let entries = node
            .names
            .iter()
            .map(render_signal_decl_entry)
            .collect::<Vec<_>>();
        self.emit_wrapped_entries(&entries, &prefix);
    }

    fn render_component_decl(&mut self, node: &ComponentDecl) {
        self.buf.push_str("component ");
        let mut prefix = String::from("component ");
        if node.is_parallel {
            self.buf.push_str("parallel ");
            prefix.push_str("parallel ");
        }
        let entries = node
            .names
            .iter()
            .map(render_component_decl_entry)
            .collect::<Vec<_>>();
        self.emit_wrapped_entries(&entries, &prefix);
    }

    fn render_bus_instance_decl(&mut self, node: &BusInstanceDecl) {
        self.buf.push_str("signal ");
        match node.signal_kind {
            SignalKind::Input => self.buf.push_str("input "),
            SignalKind::Output => self.buf.push_str("output "),
            SignalKind::Intermediate => {}
        }
        self.buf.push_str(&render_bus_type(&node.bus_type));
        self.buf.push(' ');
        if !node.tags.is_empty() {
            self.buf.push('{');
            self.buf.push_str(&comma_sep_idents_inline(&node.tags));
            self.buf.push_str("} ");
        }
        self.buf.push_str(&node.name.name);
        for dim in &node.dimensions {
            self.buf.push('[');
            self.buf.push_str(&render_expr_inline(dim));
            self.buf.push(']');
        }
        if let Some((op, init)) = &node.init {
            self.buf.push_str(match op {
                SignalAssignOp::SafeLeft => " <== ",
                SignalAssignOp::UnsafeLeft => " <-- ",
            });
            self.render_expr(init);
        }
    }

    fn render_assign_stmt(&mut self, node: &AssignStmt) {
        self.render_expr(&node.lhs);
        self.buf.push_str(match node.op {
            AssignOp::Eq => " = ",
            AssignOp::SafeLeft => " <== ",
            AssignOp::SafeRight => " ==> ",
            AssignOp::UnsafeLeft => " <-- ",
            AssignOp::UnsafeRight => " --> ",
        });
        self.render_expr(&node.rhs);
    }

    fn render_compound_assign_stmt(&mut self, node: &CompoundAssignStmt) {
        self.render_expr(&node.lhs);
        self.buf.push_str(compound_op_str(node.op));
        self.render_expr(&node.rhs);
    }

    fn render_constraint_eq_stmt(&mut self, node: &ConstraintEqStmt) {
        self.render_expr(&node.lhs);
        self.buf.push_str(" === ");
        self.render_expr(&node.rhs);
    }

    fn render_tuple_assign_stmt(&mut self, node: &TupleAssignStmt) {
        self.buf.push('(');
        for (i, target) in node.targets.iter().enumerate() {
            if i > 0 {
                self.buf.push_str(", ");
            }
            match target {
                Some(e) => self.buf.push_str(&render_expr_inline(e)),
                None => self.buf.push('_'),
            }
        }
        self.buf.push(')');
        self.buf.push_str(match node.op {
            AssignOp::Eq => " = ",
            AssignOp::SafeLeft => " <== ",
            AssignOp::SafeRight => " ==> ",
            AssignOp::UnsafeLeft => " <-- ",
            AssignOp::UnsafeRight => " --> ",
        });
        self.render_expr(&node.rhs);
    }

    fn render_if_else(&mut self, node: &IfElse) {
        self.buf.push_str("if (");
        self.render_expr(&node.cond);
        self.buf.push_str(") ");
        self.render_block(&node.then_body);
        if let Some(else_body) = &node.else_body {
            if else_body.stmts.len() == 1 {
                if let StatementKind::IfElse(inner) = &else_body.stmts[0].kind {
                    self.buf.push_str(" else ");
                    self.render_if_else(inner);
                    return;
                }
            }
            self.buf.push_str(" else ");
            self.render_block(else_body);
        }
    }

    fn render_for_loop(&mut self, node: &ForLoop) {
        self.buf.push_str("for (");
        self.render_for_init(&node.init);
        self.buf.push_str("; ");
        self.render_expr(&node.cond);
        self.buf.push_str("; ");
        self.render_for_step(&node.step);
        self.buf.push_str(") ");
        self.render_block(&node.body);
    }

    fn render_for_init(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::VarDecl(n) => self.render_var_decl(n),
            StatementKind::Assignment(n) => self.render_assign_stmt(n),
            StatementKind::Expression(e) => self.render_expr(e),
            other => {
                let _ = write!(self.buf, "/* unsupported for-init: {other:?} */");
            }
        }
    }

    fn render_for_step(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::Assignment(n) => self.render_assign_stmt(n),
            StatementKind::CompoundAssign(n) => self.render_compound_assign_stmt(n),
            StatementKind::Increment(e) => {
                self.render_expr(e);
                self.buf.push_str("++");
            }
            StatementKind::Decrement(e) => {
                self.render_expr(e);
                self.buf.push_str("--");
            }
            StatementKind::Expression(e) => self.render_expr(e),
            other => {
                let _ = write!(self.buf, "/* unsupported for-step: {other:?} */");
            }
        }
    }

    fn render_while_loop(&mut self, node: &WhileLoop) {
        self.buf.push_str("while (");
        self.render_expr(&node.cond);
        self.buf.push_str(") ");
        self.render_block(&node.body);
    }

    fn render_log_stmt(&mut self, node: &LogStmt) {
        self.buf.push_str("log(");
        for (i, arg) in node.args.iter().enumerate() {
            if i > 0 {
                self.buf.push_str(", ");
            }
            match arg {
                LogArg::Expr(e) => self.buf.push_str(&render_expr_inline(e)),
                LogArg::String(s) => {
                    self.buf.push('"');
                    self.buf.push_str(s);
                    self.buf.push('"');
                }
            }
        }
        self.buf.push(')');
    }

    // ── Expression rendering ──────────────────────────────────────

    /// Render an expression, wrapping it onto multiple lines when the
    /// resulting text would overflow the configured max line length.
    fn render_expr(&mut self, expr: &Expression) {
        let inline = render_expr_inline(expr);
        let col = self.current_column();
        if col + inline.len() <= self.config.max_line_length {
            self.buf.push_str(&inline);
            return;
        }
        self.render_expr_wrapped(expr);
    }

    fn render_expr_wrapped(&mut self, expr: &Expression) {
        match expr.kind.as_ref() {
            ExpressionKind::Call(callee, args) if !args.is_empty() => {
                self.buf.push_str(&render_expr_inline(callee));
                self.emit_wrapped_args(args.iter().map(render_expr_inline).collect::<Vec<_>>());
            }
            ExpressionKind::Binary(_, _, _) => {
                let chain = flatten_left_binary(expr);
                self.emit_wrapped_binary(&chain);
            }
            ExpressionKind::Paren(inner) => {
                self.buf.push('(');
                self.render_expr(inner);
                self.buf.push(')');
            }
            ExpressionKind::AnonymousComp(comp) => {
                self.render_anonymous_comp_wrapped(comp);
            }
            _ => {
                // Fallback: emit as inline even if too long. We never
                // break inside strings, identifiers, or comments.
                self.buf.push_str(&render_expr_inline(expr));
            }
        }
    }

    fn render_anonymous_comp_wrapped(&mut self, node: &AnonymousComp) {
        self.buf.push_str(&render_expr_inline(&node.template));
        self.emit_wrapped_args(
            node.template_args
                .iter()
                .map(render_expr_inline)
                .collect::<Vec<_>>(),
        );
        let inputs = node
            .inputs
            .iter()
            .map(|i| match i {
                AnonCompInput::Positional(e) => render_expr_inline(e),
                AnonCompInput::Named(id, e) => format!("{} <== {}", id.name, render_expr_inline(e)),
            })
            .collect::<Vec<_>>();
        self.emit_wrapped_args(inputs);
    }

    /// Emit `(arg0, arg1, …)` either on one line (fits) or split with
    /// one argument per indented line.
    fn emit_wrapped_args(&mut self, args: Vec<String>) {
        let col = self.current_column();
        // "(" + joined + ")" inline length
        let joined = args.join(", ");
        if col + joined.len() + 2 <= self.config.max_line_length {
            self.buf.push('(');
            self.buf.push_str(&joined);
            self.buf.push(')');
            return;
        }
        self.buf.push('(');
        self.buf.push('\n');
        self.level += 1;
        for (i, a) in args.iter().enumerate() {
            self.write_indent();
            self.buf.push_str(a);
            if i + 1 != args.len() {
                self.buf.push(',');
            }
            self.buf.push('\n');
        }
        self.level -= 1;
        self.write_indent();
        self.buf.push(')');
    }

    /// Emit a list of already-rendered entry strings (e.g. signal
    /// declaration entries) comma-separated. If the combined length
    /// exceeds the line budget we break after each comma and align to
    /// the length of `prefix`.
    fn emit_wrapped_entries(&mut self, entries: &[String], prefix: &str) {
        let col = self.current_column();
        let joined = entries.join(", ");
        if col + joined.len() <= self.config.max_line_length {
            self.buf.push_str(&joined);
            return;
        }
        // Break after each comma, aligning continuation lines to the
        // column after the prefix.
        let align = self.level * self.config.indent_width + prefix.len();
        let align_str = " ".repeat(align);
        for (i, entry) in entries.iter().enumerate() {
            if i == 0 {
                self.buf.push_str(entry);
            } else {
                self.buf.push_str(",\n");
                self.buf.push_str(&align_str);
                self.buf.push_str(entry);
            }
        }
    }

    /// Emit a left-associative binary chain (`a op b op c op …`) with
    /// each continuation line starting with the operator.
    fn emit_wrapped_binary(&mut self, chain: &BinaryChain) {
        // Try inline first.
        let inline = chain.render_inline();
        let col = self.current_column();
        if col + inline.len() <= self.config.max_line_length {
            self.buf.push_str(&inline);
            return;
        }
        let align = self.current_column();
        let align_str = " ".repeat(align);
        self.buf.push_str(&render_expr_inline(&chain.head));
        for (op, rhs) in &chain.tail {
            self.buf.push('\n');
            self.buf.push_str(&align_str);
            self.buf.push_str(binary_op_str(*op).trim_start());
            self.buf.push(' ');
            self.buf.push_str(&render_expr_inline(rhs));
        }
        // avail unused for this simple layout; keep for future heuristics.
        let _ = self.avail();
    }

    fn current_column(&self) -> usize {
        match self.buf.rfind('\n') {
            Some(i) => self.buf.len() - i - 1,
            None => self.buf.len(),
        }
    }
}

// ── Inline (single-line) rendering helpers ──────────────────────────

fn render_var_decl_entry(entry: &VarDeclEntry) -> String {
    let mut s = entry.name.name.clone();
    for dim in &entry.dimensions {
        s.push('[');
        s.push_str(&render_expr_inline(dim));
        s.push(']');
    }
    if let Some(init) = &entry.init {
        s.push_str(" = ");
        s.push_str(&render_expr_inline(init));
    }
    s
}

fn render_signal_decl_entry(entry: &SignalDeclEntry) -> String {
    let mut s = entry.name.name.clone();
    for dim in &entry.dimensions {
        s.push('[');
        s.push_str(&render_expr_inline(dim));
        s.push(']');
    }
    if let Some((op, init)) = &entry.init {
        s.push_str(match op {
            SignalAssignOp::SafeLeft => " <== ",
            SignalAssignOp::UnsafeLeft => " <-- ",
        });
        s.push_str(&render_expr_inline(init));
    }
    s
}

fn render_component_decl_entry(entry: &ComponentDeclEntry) -> String {
    let mut s = entry.name.name.clone();
    for dim in &entry.dimensions {
        s.push('[');
        s.push_str(&render_expr_inline(dim));
        s.push(']');
    }
    if let Some(init) = &entry.init {
        s.push_str(" = ");
        s.push_str(&render_expr_inline(init));
    }
    s
}

fn render_bus_type(node: &BusType) -> String {
    let mut s = node.name.name.clone();
    s.push('(');
    for (i, arg) in node.args.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&render_expr_inline(arg));
    }
    s.push(')');
    s
}

fn render_expr_inline(expr: &Expression) -> String {
    let mut s = String::new();
    write_expr_inline(&mut s, expr);
    s
}

fn write_expr_inline(s: &mut String, expr: &Expression) {
    match expr.kind.as_ref() {
        ExpressionKind::Number(n) => s.push_str(n),
        ExpressionKind::Ident(name) => s.push_str(name),
        ExpressionKind::Unary(op, e) => {
            s.push_str(match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
                UnaryOp::BitNot => "~",
            });
            write_expr_inline(s, e);
        }
        ExpressionKind::Binary(lhs, op, rhs) => {
            write_expr_inline(s, lhs);
            s.push_str(binary_op_str(*op));
            write_expr_inline(s, rhs);
        }
        ExpressionKind::Ternary(c, t, e) => {
            write_expr_inline(s, c);
            s.push_str(" ? ");
            write_expr_inline(s, t);
            s.push_str(" : ");
            write_expr_inline(s, e);
        }
        ExpressionKind::Index(base, idx) => {
            write_expr_inline(s, base);
            s.push('[');
            write_expr_inline(s, idx);
            s.push(']');
        }
        ExpressionKind::Member(base, ident) => {
            write_expr_inline(s, base);
            s.push('.');
            s.push_str(&ident.name);
        }
        ExpressionKind::Call(callee, args) => write_call_inline(s, callee, args),
        ExpressionKind::AnonymousComp(comp) => write_anon_comp_inline(s, comp),
        ExpressionKind::ArrayLit(elems) => write_array_lit_inline(s, elems),
        ExpressionKind::Paren(e) => {
            s.push('(');
            write_expr_inline(s, e);
            s.push(')');
        }
        ExpressionKind::Parallel(e) => {
            s.push_str("parallel ");
            write_expr_inline(s, e);
        }
        ExpressionKind::Underscore => s.push('_'),
        ExpressionKind::Error => s.push_str("/* error */"),
    }
}

fn write_call_inline(s: &mut String, callee: &Expression, args: &[Expression]) {
    write_expr_inline(s, callee);
    s.push('(');
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        write_expr_inline(s, a);
    }
    s.push(')');
}

fn write_anon_comp_inline(s: &mut String, comp: &AnonymousComp) {
    write_expr_inline(s, &comp.template);
    s.push('(');
    for (i, a) in comp.template_args.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        write_expr_inline(s, a);
    }
    s.push_str(")(");
    for (i, input) in comp.inputs.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        match input {
            AnonCompInput::Positional(e) => write_expr_inline(s, e),
            AnonCompInput::Named(id, e) => {
                s.push_str(&id.name);
                s.push_str(" <== ");
                write_expr_inline(s, e);
            }
        }
    }
    s.push(')');
}

fn write_array_lit_inline(s: &mut String, elems: &[Expression]) {
    s.push('[');
    for (i, e) in elems.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        write_expr_inline(s, e);
    }
    s.push(']');
}

fn binary_op_str(op: BinaryOp) -> &'static str {
    match op {
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
    }
}

fn compound_op_str(op: CompoundOp) -> &'static str {
    match op {
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
    }
}

fn comma_sep_idents_inline(idents: &[Identifier]) -> String {
    idents
        .iter()
        .map(|i| i.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

// ── Binary chain flattening ─────────────────────────────────────────

/// A flattened left-associative binary chain `head op1 rhs1 op2 rhs2
/// …`. Only same-precedence chains are flattened; when we hit a
/// sub-expression that isn't a left-associated binary we stop.
struct BinaryChain {
    head: Expression,
    tail: Vec<(BinaryOp, Expression)>,
}

impl BinaryChain {
    fn render_inline(&self) -> String {
        let mut s = render_expr_inline(&self.head);
        for (op, rhs) in &self.tail {
            s.push_str(binary_op_str(*op));
            s.push_str(&render_expr_inline(rhs));
        }
        s
    }
}

fn flatten_left_binary(expr: &Expression) -> BinaryChain {
    let mut tail = Vec::new();
    let mut cur = expr.clone();
    while let ExpressionKind::Binary(lhs, op, rhs) = cur.kind.as_ref() {
        tail.push((*op, rhs.clone()));
        cur = lhs.clone();
    }
    tail.reverse();
    BinaryChain { head: cur, tail }
}

// ── Span helpers ────────────────────────────────────────────────────

fn item_span(item: &Item) -> crate::span::Span {
    match item {
        Item::Pragma(p) => p.span,
        Item::Include(i) => i.span,
        Item::TemplateDef(t) => t.span,
        Item::FunctionDef(f) => f.span,
        Item::BusDef(b) => b.span,
        Item::MainComponent(m) => m.span,
    }
}

fn bus_member_span(member: &BusMember) -> crate::span::Span {
    match member {
        BusMember::Signal(s) => s.span,
        BusMember::Bus(b) => b.span,
    }
}

// ── Top-level entry ────────────────────────────────────────────────

fn render(source: &str, file: &File, comments: &[Comment], config: &FormatConfig) -> String {
    let mut r = Renderer::new(source, comments, config);
    r.render_file(file);
    // Ensure the output ends with exactly one newline.
    while r.buf.ends_with("\n\n") {
        r.buf.pop();
    }
    if !r.buf.ends_with('\n') {
        r.buf.push('\n');
    }
    r.buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> FormatConfig {
        FormatConfig::default()
    }

    #[test]
    fn formats_simple_template() {
        let src = "pragma circom 2.0.0;\ntemplate T(){signal input x;signal output y;y<==x;}\n";
        let out = format_source(src, &cfg()).unwrap();
        assert!(out.contains("pragma circom 2.0.0;"));
        assert!(out.contains("template T() {"));
        assert!(out.contains("    signal input x;"));
        assert!(out.contains("    y <== x;"));
    }

    #[test]
    fn idempotent_on_simple_template() {
        let src = "pragma circom 2.0.0;\ntemplate T() {\n    signal input x;\n    signal output y;\n    y <== x;\n}\n";
        let once = format_source(src, &cfg()).unwrap();
        let twice = format_source(&once, &cfg()).unwrap();
        assert_eq!(once, twice, "formatter should be idempotent");
    }

    #[test]
    fn preserves_line_comment_before_statement() {
        let src = "template T() {\n    // doc for x\n    signal input x;\n}\n";
        let out = format_source(src, &cfg()).unwrap();
        assert!(out.contains("// doc for x"), "missing comment: {out}");
        // Comment must appear before the signal decl.
        let comment_pos = out.find("// doc for x").unwrap();
        let signal_pos = out.find("signal input x").unwrap();
        assert!(comment_pos < signal_pos, "comment after signal:\n{out}");
    }

    #[test]
    fn preserves_trailing_line_comment() {
        let src = "template T() {\n    signal input x; // inline\n}\n";
        let out = format_source(src, &cfg()).unwrap();
        // The comment should stay on the same line as the signal decl.
        let line = out
            .lines()
            .find(|l| l.contains("signal input x"))
            .expect("signal line");
        assert!(line.contains("// inline"), "line was: {line}\n{out}");
    }

    #[test]
    fn preserves_block_comment() {
        let src = "/* header */\ntemplate T() {\n    signal input x;\n}\n";
        let out = format_source(src, &cfg()).unwrap();
        assert!(out.contains("/* header */"));
    }

    #[test]
    fn preserves_eof_comment() {
        let src = "template T() {\n    signal input x;\n}\n// end\n";
        let out = format_source(src, &cfg()).unwrap();
        assert!(out.contains("// end"));
    }

    #[test]
    fn wraps_long_call_arguments() {
        let src = "template T() {\n    signal output z;\n    z <== SomeReallyLongFunctionName(aaaaaaaa, bbbbbbbb, cccccccc, dddddddd, eeeeeeee, ffffffff);\n}\n";
        let cfg = FormatConfig {
            max_line_length: 60,
            indent_width: 4,
        };
        let out = format_source(src, &cfg).unwrap();
        // At least one line should break the args across lines.
        let has_wrapped = out
            .lines()
            .any(|l| l.trim() == "aaaaaaaa," || l.trim().starts_with("aaaaaaaa,"));
        assert!(has_wrapped, "expected wrapped args:\n{out}");
        for line in out.lines() {
            assert!(
                line.len() <= 80,
                "line still too long after wrap: {line:?}\n{out}"
            );
        }
    }

    #[test]
    fn respects_custom_max_line_length() {
        let src = "template T() {\n    signal output z;\n    z <== Foo(aaaa, bbbb, cccc);\n}\n";
        let wide = format_source(
            src,
            &FormatConfig {
                max_line_length: 200,
                indent_width: 4,
            },
        )
        .unwrap();
        assert!(wide.contains("z <== Foo(aaaa, bbbb, cccc)"));

        let narrow = format_source(
            src,
            &FormatConfig {
                max_line_length: 24,
                indent_width: 4,
            },
        )
        .unwrap();
        // Narrow config must break the Foo call.
        let any_short_arg_line = narrow.lines().any(|l| l.trim() == "aaaa,");
        assert!(any_short_arg_line, "expected wrap:\n{narrow}");
    }

    #[test]
    fn wrap_does_not_break_strings() {
        let src = "template T() {\n    log(\"this is a string that should never be split even if the line is very long\");\n}\n";
        let cfg = FormatConfig {
            max_line_length: 40,
            indent_width: 4,
        };
        let out = format_source(src, &cfg).unwrap();
        // The log arg string must remain intact on one line.
        let has_full = out
            .lines()
            .any(|l| l.contains("this is a string that should never be split"));
        assert!(has_full, "string was split:\n{out}");
    }

    #[test]
    fn reformat_is_idempotent_on_fixtures() {
        use std::fs;
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
        let mut checked = 0;
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("circom") {
                continue;
            }
            let src = fs::read_to_string(&path).unwrap();
            let once = match format_source(&src, &cfg()) {
                Ok(s) => s,
                Err(_) => continue, // Skip files with parse errors.
            };
            let twice = format_source(&once, &cfg()).unwrap();
            assert_eq!(
                once,
                twice,
                "formatter not idempotent on {}",
                path.display()
            );
            checked += 1;
        }
        assert!(checked > 0, "no fixtures were checked");
    }
}
