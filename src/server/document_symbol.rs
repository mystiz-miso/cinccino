//! Document symbol extraction for `textDocument/documentSymbol`.
//!
//! Walks the Circom AST and produces a hierarchical list of
//! [`DocumentSymbol`] nodes that editors use for the outline view.

use tower_lsp::lsp_types::{DocumentSymbol, Position, Range, SymbolKind, SymbolTag};

use crate::ast::*;
use crate::span::{LineIndex, Span};

/// Extract document symbols from a parsed Circom AST.
pub fn document_symbols(file: &File, source: &str) -> Vec<DocumentSymbol> {
    let line_index = LineIndex::new(source);
    let mut visitor = SymbolVisitor::new(&line_index);
    visitor.collect_file(file)
}

struct SymbolVisitor<'a> {
    line_index: &'a LineIndex,
}

impl<'a> SymbolVisitor<'a> {
    fn new(line_index: &'a LineIndex) -> Self {
        Self { line_index }
    }

    fn span_to_range(&self, span: Span) -> Range {
        let start = self
            .line_index
            .line_col(span.start)
            .unwrap_or(crate::span::LineCol { line: 0, col: 0 });
        let end = self
            .line_index
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

    fn make_symbol(
        &self,
        name: &str,
        detail: Option<String>,
        kind: SymbolKind,
        range: Span,
        selection_range: Span,
        children: Vec<DocumentSymbol>,
    ) -> DocumentSymbol {
        #[allow(deprecated)]
        DocumentSymbol {
            name: name.to_string(),
            detail,
            kind,
            tags: Some(Vec::<SymbolTag>::new()),
            deprecated: None,
            range: self.span_to_range(range),
            selection_range: self.span_to_range(selection_range),
            children: if children.is_empty() {
                None
            } else {
                Some(children)
            },
        }
    }

    fn collect_file(&mut self, file: &File) -> Vec<DocumentSymbol> {
        let mut symbols = Vec::new();
        for item in &file.items {
            if let Some(sym) = self.collect_item(item) {
                symbols.push(sym);
            }
        }
        symbols
    }

    fn collect_item(&mut self, item: &Item) -> Option<DocumentSymbol> {
        match item {
            Item::TemplateDef(t) => Some(self.collect_template(t)),
            Item::FunctionDef(f) => Some(self.collect_function(f)),
            Item::BusDef(b) => Some(self.collect_bus(b)),
            Item::MainComponent(m) => Some(self.collect_main_component(m)),
            Item::Pragma(_) | Item::Include(_) => None,
        }
    }

    fn collect_template(&mut self, t: &TemplateDef) -> DocumentSymbol {
        let mut children = Vec::new();

        // Collect parameters as variables
        for param in &t.params {
            children.push(self.make_symbol(
                &param.name,
                Some("parameter".to_string()),
                SymbolKind::VARIABLE,
                param.span,
                param.span,
                Vec::new(),
            ));
        }

        // Collect declarations from body
        self.collect_block_declarations(&t.body, &mut children);

        let detail = if t.is_custom {
            Some("custom template".to_string())
        } else if t.is_parallel {
            Some("parallel template".to_string())
        } else {
            Some("template".to_string())
        };

        self.make_symbol(
            &t.name.name,
            detail,
            SymbolKind::CLASS,
            t.span,
            t.name.span,
            children,
        )
    }

    fn collect_function(&mut self, f: &FunctionDef) -> DocumentSymbol {
        let mut children = Vec::new();

        // Collect parameters
        for param in &f.params {
            children.push(self.make_symbol(
                &param.name,
                Some("parameter".to_string()),
                SymbolKind::VARIABLE,
                param.span,
                param.span,
                Vec::new(),
            ));
        }

        // Collect var declarations from body
        self.collect_block_declarations(&f.body, &mut children);

        self.make_symbol(
            &f.name.name,
            Some("function".to_string()),
            SymbolKind::FUNCTION,
            f.span,
            f.name.span,
            children,
        )
    }

    fn collect_bus(&mut self, b: &BusDef) -> DocumentSymbol {
        let mut children = Vec::new();

        for member in &b.body {
            match member {
                BusMember::Signal(sig) => {
                    self.collect_signal_decl_entries(sig, &mut children);
                }
                BusMember::Bus(field) => {
                    children.push(self.make_symbol(
                        &field.name.name,
                        Some(format!("bus {}", field.bus_type.name.name)),
                        SymbolKind::FIELD,
                        field.span,
                        field.name.span,
                        Vec::new(),
                    ));
                }
            }
        }

        self.make_symbol(
            &b.name.name,
            Some("bus".to_string()),
            SymbolKind::STRUCT,
            b.span,
            b.name.span,
            children,
        )
    }

    fn collect_main_component(&mut self, m: &MainComponent) -> DocumentSymbol {
        let detail = if m.public_signals.is_empty() {
            "main component".to_string()
        } else {
            let signals: Vec<&str> = m.public_signals.iter().map(|s| s.name.as_str()).collect();
            format!("main component (public: {})", signals.join(", "))
        };

        self.make_symbol(
            "main",
            Some(detail),
            SymbolKind::CONSTANT,
            m.span,
            m.span,
            Vec::new(),
        )
    }

    fn collect_block_declarations(&mut self, block: &Block, out: &mut Vec<DocumentSymbol>) {
        for stmt in &block.stmts {
            match &stmt.kind {
                StatementKind::VarDecl(v) => {
                    for entry in &v.names {
                        // Check if the var init is a component instantiation
                        if let Some(ref init) = entry.init {
                            if let Some(name) = extract_call_name(init) {
                                out.push(self.make_symbol(
                                    &entry.name.name,
                                    Some(format!("component {name}")),
                                    SymbolKind::OBJECT,
                                    entry.name.span,
                                    entry.name.span,
                                    Vec::new(),
                                ));
                                continue;
                            }
                        }
                        out.push(self.make_symbol(
                            &entry.name.name,
                            Some("var".to_string()),
                            SymbolKind::VARIABLE,
                            entry.name.span,
                            entry.name.span,
                            Vec::new(),
                        ));
                    }
                }
                StatementKind::SignalDecl(sig) => {
                    // Check each signal: if init is a component instantiation,
                    // emit as component; otherwise emit as signal.
                    for entry in &sig.names {
                        if let Some((_, ref init_expr)) = entry.init {
                            if let Some(name) = extract_call_name(init_expr) {
                                out.push(self.make_symbol(
                                    &entry.name.name,
                                    Some(format!("component {name}")),
                                    SymbolKind::OBJECT,
                                    entry.name.span,
                                    entry.name.span,
                                    Vec::new(),
                                ));
                                continue;
                            }
                        }
                        // Not a component — emit as signal
                        let kind_str = match sig.kind {
                            SignalKind::Input => "input",
                            SignalKind::Output => "output",
                            SignalKind::Intermediate => "intermediate",
                        };
                        let detail = if sig.tags.is_empty() {
                            format!("signal {kind_str}")
                        } else {
                            let tags: Vec<&str> =
                                sig.tags.iter().map(|t| t.name.as_str()).collect();
                            format!("signal {kind_str} {{{}}}", tags.join(", "))
                        };
                        out.push(self.make_symbol(
                            &entry.name.name,
                            Some(detail),
                            SymbolKind::FIELD,
                            entry.name.span,
                            entry.name.span,
                            Vec::new(),
                        ));
                    }
                }
                StatementKind::ComponentDecl(c) => {
                    for entry in &c.names {
                        let detail = entry
                            .init
                            .as_ref()
                            .and_then(|init| {
                                // Try to extract template name from init expression
                                extract_call_name(init).map(|name| format!("component {name}"))
                            })
                            .unwrap_or_else(|| "component".to_string());
                        out.push(self.make_symbol(
                            &entry.name.name,
                            Some(detail),
                            SymbolKind::OBJECT,
                            entry.name.span,
                            entry.name.span,
                            Vec::new(),
                        ));
                    }
                }
                StatementKind::BusDecl(b) => {
                    out.push(self.make_symbol(
                        &b.name.name,
                        Some(format!("bus {}", b.bus_type.name.name)),
                        SymbolKind::FIELD,
                        b.name.span,
                        b.name.span,
                        Vec::new(),
                    ));
                }
                // Detect component instantiation in assignment statements:
                // `comp = TemplateName(args)` or `comp[i] = TemplateName(args)`
                StatementKind::Assignment(assign) => {
                    if let Some(name) = extract_call_name(&assign.rhs) {
                        let lhs_name = extract_lhs_name(&assign.lhs);
                        out.push(self.make_symbol(
                            &lhs_name,
                            Some(format!("component {name}")),
                            SymbolKind::OBJECT,
                            stmt.span,
                            stmt.span,
                            Vec::new(),
                        ));
                    }
                    // Also extract nested calls from the RHS
                    let mut calls = Vec::new();
                    extract_all_call_names(&assign.rhs, &mut calls);
                    // Skip the first one if it was already emitted above
                    let top_name = extract_call_name(&assign.rhs);
                    for name in calls {
                        if top_name == Some(name) {
                            continue;
                        }
                        out.push(self.make_symbol(
                            name,
                            Some(format!("component {name}")),
                            SymbolKind::OBJECT,
                            stmt.span,
                            stmt.span,
                            Vec::new(),
                        ));
                    }
                }
                // Tuple assignment: `(a, b) <== Template(args)(inputs)`
                StatementKind::TupleAssign(ta) => {
                    let mut calls = Vec::new();
                    extract_all_call_names(&ta.rhs, &mut calls);
                    for name in calls {
                        out.push(self.make_symbol(
                            name,
                            Some(format!("component {name}")),
                            SymbolKind::OBJECT,
                            stmt.span,
                            stmt.span,
                            Vec::new(),
                        ));
                    }
                }
                // Constraint equality: `expr === expr`
                StatementKind::ConstraintEq(ceq) => {
                    let mut calls = Vec::new();
                    extract_all_call_names(&ceq.lhs, &mut calls);
                    extract_all_call_names(&ceq.rhs, &mut calls);
                    for name in calls {
                        out.push(self.make_symbol(
                            name,
                            Some(format!("component {name}")),
                            SymbolKind::OBJECT,
                            stmt.span,
                            stmt.span,
                            Vec::new(),
                        ));
                    }
                }
                // Bare expression statement: `Must()(IsNonZero()(x));`
                StatementKind::Expression(expr) => {
                    let mut calls = Vec::new();
                    extract_all_call_names(expr, &mut calls);
                    for name in calls {
                        out.push(self.make_symbol(
                            name,
                            Some(format!("component {name}")),
                            SymbolKind::OBJECT,
                            stmt.span,
                            stmt.span,
                            Vec::new(),
                        ));
                    }
                }
                // Recurse into control flow blocks to find nested assignments
                StatementKind::For(f) => {
                    self.collect_block_declarations(&f.body, out);
                }
                StatementKind::While(w) => {
                    self.collect_block_declarations(&w.body, out);
                }
                StatementKind::IfElse(ie) => {
                    self.collect_block_declarations(&ie.then_body, out);
                    if let Some(ref else_body) = ie.else_body {
                        self.collect_block_declarations(else_body, out);
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_signal_decl_entries(&self, sig: &SignalDecl, out: &mut Vec<DocumentSymbol>) {
        let kind_str = match sig.kind {
            SignalKind::Input => "input",
            SignalKind::Output => "output",
            SignalKind::Intermediate => "intermediate",
        };
        let detail = if sig.tags.is_empty() {
            format!("signal {kind_str}")
        } else {
            let tags: Vec<&str> = sig.tags.iter().map(|t| t.name.as_str()).collect();
            format!("signal {kind_str} {{{}}}", tags.join(", "))
        };
        for entry in &sig.names {
            out.push(self.make_symbol(
                &entry.name.name,
                Some(detail.clone()),
                SymbolKind::FIELD,
                entry.name.span,
                entry.name.span,
                Vec::new(),
            ));
        }
    }
}

/// Extract the base name from an LHS expression (stripping array indices and member access).
fn extract_lhs_name(expr: &Expression) -> String {
    match expr.kind.as_ref() {
        ExpressionKind::Ident(name) => name.clone(),
        ExpressionKind::Index(base, _) => extract_lhs_name(base),
        ExpressionKind::Member(base, field) => {
            format!("{}.{}", extract_lhs_name(base), field.name)
        }
        _ => "<component>".to_string(),
    }
}

/// Try to extract a function/template name from a call or anonymous component expression.
fn extract_call_name(expr: &Expression) -> Option<&str> {
    match expr.kind.as_ref() {
        ExpressionKind::Call(callee, _) => match callee.kind.as_ref() {
            ExpressionKind::Ident(name) => Some(name),
            _ => None,
        },
        // Anonymous component: Template(params)(inputs) — the outer "call" has
        // a Call as callee: Call(Call(Ident("Template"), params), inputs)
        ExpressionKind::AnonymousComp(anon) => match anon.template.kind.as_ref() {
            ExpressionKind::Ident(name) => Some(name),
            _ => None,
        },
        _ => None,
    }
}

/// Recursively extract ALL call/anonymous-component names from an expression tree.
/// This catches nested calls like `Must()(IsNonZero()(x))`.
fn extract_all_call_names<'a>(expr: &'a Expression, out: &mut Vec<&'a str>) {
    match expr.kind.as_ref() {
        ExpressionKind::Call(callee, args) => {
            if let ExpressionKind::Ident(name) = callee.kind.as_ref() {
                out.push(name);
            }
            extract_all_call_names(callee, out);
            for arg in args {
                extract_all_call_names(arg, out);
            }
        }
        ExpressionKind::AnonymousComp(anon) => {
            if let ExpressionKind::Ident(name) = anon.template.kind.as_ref() {
                out.push(name);
            }
            extract_all_call_names(&anon.template, out);
            for input in &anon.inputs {
                match input {
                    AnonCompInput::Positional(e) => extract_all_call_names(e, out),
                    AnonCompInput::Named(_, e) => extract_all_call_names(e, out),
                }
            }
            for arg in &anon.template_args {
                extract_all_call_names(arg, out);
            }
        }
        ExpressionKind::Binary(lhs, _, rhs) => {
            extract_all_call_names(lhs, out);
            extract_all_call_names(rhs, out);
        }
        ExpressionKind::Unary(_, inner) => {
            extract_all_call_names(inner, out);
        }
        ExpressionKind::Ternary(cond, then_e, else_e) => {
            extract_all_call_names(cond, out);
            extract_all_call_names(then_e, out);
            extract_all_call_names(else_e, out);
        }
        ExpressionKind::Index(arr, idx) => {
            extract_all_call_names(arr, out);
            extract_all_call_names(idx, out);
        }
        ExpressionKind::Member(base, _) => {
            extract_all_call_names(base, out);
        }
        ExpressionKind::ArrayLit(elems) => {
            for e in elems {
                extract_all_call_names(e, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    fn symbols_for(src: &str) -> Vec<DocumentSymbol> {
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        document_symbols(&file, src)
    }

    #[test]
    fn template_with_nested_signals() {
        let symbols = symbols_for(
            r#"
            template Adder(n) {
                signal input a;
                signal input b;
                signal output c;
                c <== a + b;
            }
        "#,
        );

        assert_eq!(symbols.len(), 1);
        let adder = &symbols[0];
        assert_eq!(adder.name, "Adder");
        assert_eq!(adder.kind, SymbolKind::CLASS);
        assert_eq!(adder.detail.as_deref(), Some("template"));

        let children = adder.children.as_ref().unwrap();
        // parameter n + 3 signals = 4
        assert_eq!(children.len(), 4);

        assert_eq!(children[0].name, "n");
        assert_eq!(children[0].detail.as_deref(), Some("parameter"));
        assert_eq!(children[0].kind, SymbolKind::VARIABLE);

        assert_eq!(children[1].name, "a");
        assert_eq!(children[1].detail.as_deref(), Some("signal input"));
        assert_eq!(children[1].kind, SymbolKind::FIELD);

        assert_eq!(children[2].name, "b");
        assert_eq!(children[2].detail.as_deref(), Some("signal input"));

        assert_eq!(children[3].name, "c");
        assert_eq!(children[3].detail.as_deref(), Some("signal output"));
    }

    #[test]
    fn function_with_parameters() {
        let symbols = symbols_for(
            r#"
            function add(a, b) {
                var result;
                return a + b;
            }
        "#,
        );

        assert_eq!(symbols.len(), 1);
        let func = &symbols[0];
        assert_eq!(func.name, "add");
        assert_eq!(func.kind, SymbolKind::FUNCTION);
        assert_eq!(func.detail.as_deref(), Some("function"));

        let children = func.children.as_ref().unwrap();
        // 2 params + 1 var = 3
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].name, "a");
        assert_eq!(children[0].detail.as_deref(), Some("parameter"));
        assert_eq!(children[1].name, "b");
        assert_eq!(children[1].detail.as_deref(), Some("parameter"));
        assert_eq!(children[2].name, "result");
        assert_eq!(children[2].detail.as_deref(), Some("var"));
    }

    #[test]
    fn bus_with_fields() {
        let symbols = symbols_for(
            r#"
            pragma circom 2.2.0;
            bus Point() {
                signal input x;
                signal input y;
            }
        "#,
        );

        // pragma is skipped, only bus
        assert_eq!(symbols.len(), 1);
        let bus = &symbols[0];
        assert_eq!(bus.name, "Point");
        assert_eq!(bus.kind, SymbolKind::STRUCT);
        assert_eq!(bus.detail.as_deref(), Some("bus"));

        let children = bus.children.as_ref().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name, "x");
        assert_eq!(children[0].detail.as_deref(), Some("signal input"));
        assert_eq!(children[1].name, "y");
        assert_eq!(children[1].detail.as_deref(), Some("signal input"));
    }

    #[test]
    fn multiple_templates() {
        let symbols = symbols_for(
            r#"
            pragma circom 2.0.0;
            template A() { signal input x; }
            template B() { signal output y; }
            function f() { return 1; }
        "#,
        );

        assert_eq!(symbols.len(), 3);
        assert_eq!(symbols[0].name, "A");
        assert_eq!(symbols[0].kind, SymbolKind::CLASS);
        assert_eq!(symbols[1].name, "B");
        assert_eq!(symbols[1].kind, SymbolKind::CLASS);
        assert_eq!(symbols[2].name, "f");
        assert_eq!(symbols[2].kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn empty_file() {
        let symbols = symbols_for("");
        assert!(symbols.is_empty());
    }

    #[test]
    fn main_component_shown() {
        let symbols = symbols_for(
            r#"
            pragma circom 2.0.0;
            template Foo() { signal input x; }
            component main { public [x] } = Foo();
        "#,
        );

        assert_eq!(symbols.len(), 2);
        let main = &symbols[1];
        assert_eq!(main.name, "main");
        assert_eq!(main.kind, SymbolKind::CONSTANT);
        assert!(main.detail.as_deref().unwrap().contains("public: x"));
    }

    #[test]
    fn template_with_components() {
        let symbols = symbols_for(
            r#"
            template Circuit() {
                signal input in;
                signal output out;
                component hasher = Poseidon(2);
                out <== hasher.out;
            }
        "#,
        );

        let circuit = &symbols[0];
        let children = circuit.children.as_ref().unwrap();
        // 2 signals + 1 component
        assert_eq!(children.len(), 3);

        let hasher = &children[2];
        assert_eq!(hasher.name, "hasher");
        assert_eq!(hasher.kind, SymbolKind::OBJECT);
        assert_eq!(hasher.detail.as_deref(), Some("component Poseidon"));
    }

    #[test]
    fn template_with_variables() {
        let symbols = symbols_for(
            r#"
            template T() {
                var x = 0;
                var y;
                signal input a;
            }
        "#,
        );

        let t = &symbols[0];
        let children = t.children.as_ref().unwrap();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].name, "x");
        assert_eq!(children[0].detail.as_deref(), Some("var"));
        assert_eq!(children[0].kind, SymbolKind::VARIABLE);
        assert_eq!(children[1].name, "y");
        assert_eq!(children[1].detail.as_deref(), Some("var"));
        assert_eq!(children[2].name, "a");
        assert_eq!(children[2].kind, SymbolKind::FIELD);
    }

    #[test]
    fn intermediate_signal() {
        let symbols = symbols_for(
            r#"
            template T() {
                signal x;
            }
        "#,
        );

        let children = symbols[0].children.as_ref().unwrap();
        assert_eq!(children[0].detail.as_deref(), Some("signal intermediate"));
    }

    #[test]
    fn pragma_and_include_are_skipped() {
        let symbols = symbols_for(
            r#"
            pragma circom 2.0.0;
            include "other.circom";
        "#,
        );
        assert!(symbols.is_empty());
    }

    #[test]
    fn component_assignment_in_loop() {
        let symbols = symbols_for(
            r#"
            template Circuit(n) {
                signal input in;
                signal output out;
                component mux[n];
                for (var i = 0; i < n; i++) {
                    mux[i] = MultiMux1(2);
                }
                out <== in;
            }
        "#,
        );

        let circuit = &symbols[0];
        let children = circuit.children.as_ref().unwrap();

        // Find all OBJECT children (component decl + assignment)
        let components: Vec<&DocumentSymbol> = children
            .iter()
            .filter(|c| c.kind == SymbolKind::OBJECT)
            .collect();

        // Should have the declaration (mux) AND the assignment (mux from loop)
        assert!(
            components.len() >= 2,
            "expected at least 2 component symbols, got {}",
            components.len()
        );

        // The loop assignment should have detail "component MultiMux1"
        let has_mux1 = components
            .iter()
            .any(|c| c.detail.as_deref() == Some("component MultiMux1"));
        assert!(
            has_mux1,
            "expected a component with detail 'component MultiMux1'"
        );
    }

    #[test]
    fn component_assignment_in_if() {
        let symbols = symbols_for(
            r#"
            template T() {
                signal input sel;
                component h;
                if (sel == 1) {
                    h = Poseidon(2);
                }
            }
        "#,
        );

        let t = &symbols[0];
        let children = t.children.as_ref().unwrap();
        let components: Vec<&DocumentSymbol> = children
            .iter()
            .filter(|c| c.kind == SymbolKind::OBJECT)
            .collect();

        let has_poseidon = components
            .iter()
            .any(|c| c.detail.as_deref() == Some("component Poseidon"));
        assert!(has_poseidon, "expected component Poseidon from if body");
    }
}
