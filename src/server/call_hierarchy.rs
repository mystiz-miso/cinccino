//! Call hierarchy support for `textDocument/prepareCallHierarchy`
//! and `callHierarchy/outgoingCalls`.
//!
//! Walks the parsed AST to answer "what does this template / function
//! call?" — used by the indexer (via its generic LSP call-hierarchy
//! path) to build call-graph edges for circom sources, and by editor
//! UIs that want a structural view of which callees a symbol invokes.
//!
//! Incoming calls (who calls me?) is symmetric but currently out of
//! scope — the indexer only needs outgoing, and adding incoming would
//! require a workspace-wide scan on every request. Follow-up if a
//! consumer asks for it.
//!
//! The module is LSP-agnostic at its core: public functions accept a
//! parsed `ast::File` + source text and return plain structs the
//! backend converts into `tower_lsp::lsp_types::*`.

use tower_lsp::lsp_types::{
    CallHierarchyItem, CallHierarchyOutgoingCall, Position, Range, SymbolKind, Url,
};

use crate::ast::*;
use crate::span::{LineIndex, Span};

/// A resolved caller used by both `prepare_call_hierarchy` and
/// `outgoing_calls`. `range` covers the full definition (template /
/// function body); `selection_range` covers just the name — so
/// clicking jumps to the identifier.
#[derive(Debug, Clone)]
pub struct Caller {
    pub name: String,
    pub kind: CallerKind,
    pub range: Span,
    pub selection_range: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerKind {
    Template,
    Function,
}

/// A resolved outgoing call. `from_ranges` are every call-site span
/// in the caller's body — the LSP protocol lets multiple call sites
/// share the same callee, and editors highlight each site.
#[derive(Debug, Clone)]
pub struct Outgoing {
    pub callee_name: String,
    pub from_ranges: Vec<Span>,
}

/// Return the enclosing caller at `offset`, if any — supports the
/// prepareCallHierarchy request. Offset is a byte position in `source`.
pub fn caller_at_offset(file: &File, offset: usize) -> Option<Caller> {
    for item in &file.items {
        match item {
            Item::TemplateDef(t) if span_contains(t.span, offset) => {
                return Some(Caller {
                    name: t.name.name.clone(),
                    kind: CallerKind::Template,
                    range: t.span,
                    selection_range: t.name.span,
                });
            }
            Item::FunctionDef(f) if span_contains(f.span, offset) => {
                return Some(Caller {
                    name: f.name.name.clone(),
                    kind: CallerKind::Function,
                    range: f.span,
                    selection_range: f.name.span,
                });
            }
            _ => {}
        }
    }
    None
}

/// Return every outgoing call from the caller whose name + kind match
/// `item`. Callee ranges are the call-site positions in the caller's
/// body; callee name is bare (the downstream resolver binds cross-
/// file). Duplicates from multiple call sites collapse into one
/// entry with a list of `from_ranges`.
pub fn outgoing_calls_for(file: &File, caller_name: &str, kind: CallerKind) -> Vec<Outgoing> {
    for item in &file.items {
        match item {
            Item::TemplateDef(t) if kind == CallerKind::Template && t.name.name == caller_name => {
                return collect_outgoing(&t.body);
            }
            Item::FunctionDef(f) if kind == CallerKind::Function && f.name.name == caller_name => {
                return collect_outgoing(&f.body);
            }
            _ => {}
        }
    }
    Vec::new()
}

/// Convert a [`Caller`] to an LSP `CallHierarchyItem`.
pub fn caller_to_item(caller: &Caller, uri: Url, line_index: &LineIndex) -> CallHierarchyItem {
    CallHierarchyItem {
        name: caller.name.clone(),
        kind: match caller.kind {
            CallerKind::Template => SymbolKind::CLASS,
            CallerKind::Function => SymbolKind::FUNCTION,
        },
        tags: None,
        detail: None,
        uri,
        range: span_to_range(caller.range, line_index),
        selection_range: span_to_range(caller.selection_range, line_index),
        data: None,
    }
}

/// Convert an [`Outgoing`] to an LSP `CallHierarchyOutgoingCall`.
///
/// Callee bare names that don't resolve locally still produce an item
/// pointing at the current URI with a zero-length range — the
/// downstream indexer's bare-name resolver binds them. This matches
/// how the template component-instantiation path has always worked.
pub fn outgoing_to_call(
    outgoing: &Outgoing,
    caller_uri: Url,
    line_index: &LineIndex,
    callee_range: Option<Span>,
) -> CallHierarchyOutgoingCall {
    let callee_range = callee_range.unwrap_or(Span { start: 0, end: 0 });
    CallHierarchyOutgoingCall {
        to: CallHierarchyItem {
            name: outgoing.callee_name.clone(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            detail: None,
            uri: caller_uri,
            range: span_to_range(callee_range, line_index),
            selection_range: span_to_range(callee_range, line_index),
            data: None,
        },
        from_ranges: outgoing
            .from_ranges
            .iter()
            .map(|s| span_to_range(*s, line_index))
            .collect(),
    }
}

// ── Walker ──────────────────────────────────────────────────────────

fn collect_outgoing(block: &Block) -> Vec<Outgoing> {
    let mut visitor = CallVisitor::default();
    visitor.visit_block(block);
    visitor.into_result()
}

#[derive(Default)]
struct CallVisitor {
    /// Insertion-order map from bare callee name → call-site spans.
    calls: Vec<(String, Vec<Span>)>,
}

impl CallVisitor {
    fn record(&mut self, name: String, span: Span) {
        if let Some((_, spans)) = self.calls.iter_mut().find(|(n, _)| n == &name) {
            spans.push(span);
            return;
        }
        self.calls.push((name, vec![span]));
    }

    fn into_result(self) -> Vec<Outgoing> {
        self.calls
            .into_iter()
            .map(|(callee_name, from_ranges)| Outgoing {
                callee_name,
                from_ranges,
            })
            .collect()
    }

    fn visit_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.visit_stmt(stmt);
        }
    }

    fn visit_var_decl(&mut self, d: &VarDecl) {
        for entry in &d.names {
            for dim in &entry.dimensions {
                self.visit_expr(dim);
            }
            if let Some(init) = &entry.init {
                self.visit_expr(init);
            }
        }
    }

    fn visit_signal_decl(&mut self, d: &SignalDecl) {
        for entry in &d.names {
            for dim in &entry.dimensions {
                self.visit_expr(dim);
            }
            if let Some((_, e)) = &entry.init {
                self.visit_expr(e);
            }
        }
    }

    fn visit_component_decl(&mut self, d: &ComponentDecl) {
        for entry in &d.names {
            for dim in &entry.dimensions {
                self.visit_expr(dim);
            }
            if let Some(init) = &entry.init {
                self.visit_expr(init);
            }
        }
    }

    fn visit_bus_decl(&mut self, d: &BusInstanceDecl) {
        for arg in &d.bus_type.args {
            self.visit_expr(arg);
        }
        for dim in &d.dimensions {
            self.visit_expr(dim);
        }
        if let Some((_, e)) = &d.init {
            self.visit_expr(e);
        }
    }

    fn visit_stmt(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::VarDecl(d) => self.visit_var_decl(d),
            StatementKind::SignalDecl(d) => self.visit_signal_decl(d),
            StatementKind::ComponentDecl(d) => self.visit_component_decl(d),
            StatementKind::BusDecl(d) => self.visit_bus_decl(d),
            StatementKind::Assignment(a) => self.visit_binary_expr(&a.lhs, &a.rhs),
            StatementKind::CompoundAssign(a) => self.visit_binary_expr(&a.lhs, &a.rhs),
            StatementKind::ConstraintEq(c) => self.visit_binary_expr(&c.lhs, &c.rhs),
            StatementKind::TupleAssign(t) => self.visit_tuple_assign(t),
            StatementKind::IfElse(ie) => self.visit_if_else(ie),
            StatementKind::For(f) => self.visit_for(f),
            StatementKind::While(w) => self.visit_while(w),
            StatementKind::Return(r) => self.visit_expr(&r.value),
            StatementKind::Log(l) => self.visit_log(l),
            StatementKind::Assert(a) => self.visit_expr(&a.expr),
            StatementKind::Increment(e) | StatementKind::Decrement(e) => self.visit_expr(e),
            StatementKind::Expression(e) => self.visit_expr(e),
            StatementKind::Block(b) => self.visit_block(b),
            StatementKind::Error => {}
        }
    }

    fn visit_binary_expr(&mut self, lhs: &Expression, rhs: &Expression) {
        self.visit_expr(lhs);
        self.visit_expr(rhs);
    }

    fn visit_tuple_assign(&mut self, t: &TupleAssignStmt) {
        for e in t.targets.iter().flatten() {
            self.visit_expr(e);
        }
        self.visit_expr(&t.rhs);
    }

    fn visit_if_else(&mut self, ie: &IfElse) {
        self.visit_expr(&ie.cond);
        self.visit_block(&ie.then_body);
        if let Some(else_body) = &ie.else_body {
            self.visit_block(else_body);
        }
    }

    fn visit_for(&mut self, f: &ForLoop) {
        self.visit_stmt(&f.init);
        self.visit_expr(&f.cond);
        self.visit_stmt(&f.step);
        self.visit_block(&f.body);
    }

    fn visit_while(&mut self, w: &WhileLoop) {
        self.visit_expr(&w.cond);
        self.visit_block(&w.body);
    }

    fn visit_log(&mut self, l: &LogStmt) {
        for arg in &l.args {
            if let LogArg::Expr(e) = arg {
                self.visit_expr(e);
            }
        }
    }

    fn visit_expr(&mut self, expr: &Expression) {
        match expr.kind.as_ref() {
            // A plain `Name(args)` call — template or function call.
            ExpressionKind::Call(callee, args) => {
                if let ExpressionKind::Ident(name) = callee.kind.as_ref() {
                    self.record(name.clone(), callee.span);
                }
                // Still recurse into the callee expression in case
                // it's e.g. a member access on a call result, and
                // into every argument.
                self.visit_expr(callee);
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            // Anonymous component: `Template(params)(inputs)`. The
            // `template` expression here is either an Ident or a Call
            // — either way we want to record the outermost template
            // name the user wrote.
            ExpressionKind::AnonymousComp(anon) => {
                self.record_anon_template(&anon.template);
                for arg in &anon.template_args {
                    self.visit_expr(arg);
                }
                for input in &anon.inputs {
                    match input {
                        AnonCompInput::Positional(e) => self.visit_expr(e),
                        AnonCompInput::Named(_, e) => self.visit_expr(e),
                    }
                }
            }
            ExpressionKind::Unary(_, e) => self.visit_expr(e),
            ExpressionKind::Binary(l, _, r) => {
                self.visit_expr(l);
                self.visit_expr(r);
            }
            ExpressionKind::Ternary(c, t, e) => {
                self.visit_expr(c);
                self.visit_expr(t);
                self.visit_expr(e);
            }
            ExpressionKind::Index(base, idx) => {
                self.visit_expr(base);
                self.visit_expr(idx);
            }
            ExpressionKind::Member(base, _) => {
                self.visit_expr(base);
            }
            ExpressionKind::ArrayLit(items) => {
                for e in items {
                    self.visit_expr(e);
                }
            }
            ExpressionKind::Paren(e) | ExpressionKind::Parallel(e) => self.visit_expr(e),
            ExpressionKind::Number(_)
            | ExpressionKind::Ident(_)
            | ExpressionKind::Underscore
            | ExpressionKind::Error => {}
        }
    }

    fn record_anon_template(&mut self, expr: &Expression) {
        match expr.kind.as_ref() {
            ExpressionKind::Ident(name) => self.record(name.clone(), expr.span),
            ExpressionKind::Call(callee, _) => {
                if let ExpressionKind::Ident(name) = callee.kind.as_ref() {
                    self.record(name.clone(), callee.span);
                }
            }
            _ => self.visit_expr(expr),
        }
    }
}

fn span_contains(span: Span, offset: usize) -> bool {
    offset >= span.start && offset <= span.end
}

fn span_to_range(span: Span, line_index: &LineIndex) -> Range {
    let start = line_index
        .line_col(span.start)
        .unwrap_or(crate::span::LineCol { line: 0, col: 0 });
    let end = line_index
        .line_col(span.end)
        .unwrap_or(crate::span::LineCol { line: 0, col: 0 });
    Range {
        start: Position {
            line: start.line,
            character: start.col,
        },
        end: Position {
            line: end.line,
            character: end.col,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    fn outgoing_names(src: &str, caller: &str, kind: CallerKind) -> Vec<String> {
        let (file, _errors) = parser::parse(src);
        outgoing_calls_for(&file, caller, kind)
            .iter()
            .map(|o| o.callee_name.clone())
            .collect()
    }

    #[test]
    fn template_component_instantiation_yields_callee() {
        let src =
            "template Circuit() {\n    component h = Poseidon(2);\n    component v = MiMC();\n}";
        let callees = outgoing_names(src, "Circuit", CallerKind::Template);
        assert_eq!(callees, vec!["Poseidon".to_string(), "MiMC".to_string()]);
    }

    #[test]
    fn function_body_direct_calls_yield_edges() {
        // Mirrors the sha256compression_function.circom shape that
        // motivated #383.
        let src = "function sha256compression(hin, inp) {\n\
            for (var i=0; i<64; i++) {\n\
                var T1 = (h + bsigma1(e) + Ch(e,f,g) + sha256K(i));\n\
                var T2 = (bsigma0(a) + Maj(a,b,c));\n\
            }\n\
            return bsigma1(e);\n\
        }";
        let callees = outgoing_names(src, "sha256compression", CallerKind::Function);
        assert_eq!(
            callees,
            vec![
                "bsigma1".to_string(),
                "Ch".to_string(),
                "sha256K".to_string(),
                "bsigma0".to_string(),
                "Maj".to_string(),
            ]
        );
    }

    #[test]
    fn repeated_calls_dedupe_to_one_entry_with_multiple_ranges() {
        let src = "function g(x) {\n\
            var a = Foo(x);\n\
            var b = Foo(x+1);\n\
            return Foo(a+b);\n\
        }";
        let (file, _errors) = parser::parse(src);
        let outgoing = outgoing_calls_for(&file, "g", CallerKind::Function);
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].callee_name, "Foo");
        assert_eq!(outgoing[0].from_ranges.len(), 3);
    }

    #[test]
    fn anonymous_component_call_is_recorded() {
        // Poseidon(2)(x) — template instantiated as an anonymous
        // component. Record "Poseidon" once.
        let src = "template Outer() {\n\
            signal input x;\n\
            signal output y;\n\
            y <== Poseidon(2)(x);\n\
        }";
        let callees = outgoing_names(src, "Outer", CallerKind::Template);
        assert!(callees.contains(&"Poseidon".to_string()));
    }

    #[test]
    fn caller_at_offset_returns_enclosing_template() {
        let src = "template A() {\n    signal x;\n}\nfunction f() {\n    return 1;\n}";
        let (file, _errors) = parser::parse(src);
        // Offset inside template A's body.
        let caller = caller_at_offset(&file, 15).expect("no caller found inside A");
        assert_eq!(caller.name, "A");
        assert_eq!(caller.kind, CallerKind::Template);
        // Offset inside function f's body.
        let caller = caller_at_offset(&file, 55).expect("no caller found inside f");
        assert_eq!(caller.name, "f");
        assert_eq!(caller.kind, CallerKind::Function);
    }

    #[test]
    fn no_calls_means_no_outgoing() {
        let src = "function f() {\n    return 1;\n}";
        let callees = outgoing_names(src, "f", CallerKind::Function);
        assert!(callees.is_empty());
    }
}
