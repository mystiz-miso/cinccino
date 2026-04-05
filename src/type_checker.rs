//! Type checking for Circom semantic analysis.
//!
//! Validates:
//! - Signal direction (cannot assign to input signals inside a template)
//! - Assignment operator correctness (`=` for variables, `<==`/`<--` for signals)
//! - Template parameter count on component instantiation
//! - Signals cannot appear in function bodies

use crate::ast::*;
use crate::span::Span;
use crate::symbol::*;
use crate::symbol_table::SymbolTable;

/// Run type checks on a file's AST using the populated symbol table.
///
/// Returns diagnostics for any type errors found.
pub fn check_types(table: &SymbolTable, file_path: &str, ast: &File) -> Vec<SymbolDiagnostic> {
    let file_scope = match table.file_scope(file_path) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut checker = TypeChecker {
        table,
        file: file_path.to_string(),
        current_scope: file_scope,
        diagnostics: Vec::new(),
        context: CheckContext::File,
    };
    checker.check_file(ast);
    checker.diagnostics
}

/// Tracks whether we're inside a template or function body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckContext {
    File,
    Template,
    Function,
}

struct TypeChecker<'a> {
    table: &'a SymbolTable,
    file: String,
    current_scope: ScopeId,
    diagnostics: Vec<SymbolDiagnostic>,
    context: CheckContext,
}

impl<'a> TypeChecker<'a> {
    fn check_file(&mut self, ast: &File) {
        for item in &ast.items {
            match item {
                Item::TemplateDef(t) => self.check_template(t),
                Item::FunctionDef(f) => self.check_function(f),
                _ => {}
            }
        }
    }

    fn check_template(&mut self, node: &TemplateDef) {
        if let Some(sym) = self.table.lookup(self.current_scope, &node.name.name) {
            if let SymbolKind::Template(ref t) = sym.kind {
                let outer_scope = self.current_scope;
                let outer_context = self.context;
                self.current_scope = t.body_scope;
                self.context = CheckContext::Template;
                self.check_block(&node.body);
                self.current_scope = outer_scope;
                self.context = outer_context;
            }
        }
    }

    fn check_function(&mut self, node: &FunctionDef) {
        if let Some(sym) = self.table.lookup(self.current_scope, &node.name.name) {
            if let SymbolKind::Function(ref f) = sym.kind {
                let outer_scope = self.current_scope;
                let outer_context = self.context;
                self.current_scope = f.body_scope;
                self.context = CheckContext::Function;
                self.check_block(&node.body);
                self.current_scope = outer_scope;
                self.context = outer_context;
            }
        }
    }

    fn check_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.check_statement(stmt);
        }
    }

    fn check_statement(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::Assignment(a) => {
                self.check_assignment(a, stmt.span);
            }
            StatementKind::ConstraintEq(_) => {
                // === is only valid in templates, not functions.
                if self.context == CheckContext::Function {
                    self.diagnostics.push(SymbolDiagnostic {
                        span: stmt.span,
                        message: "constraint equality '===' cannot be used in a function"
                            .to_string(),
                        kind: DiagnosticKind::SignalInFunction,
                        file: self.file.clone(),
                    });
                }
                // Constraint equality operates on signal expressions — type
                // validation is handled by the constraint checker (quadratic
                // form). No additional checks needed here beyond the
                // function-context check above.
            }
            StatementKind::SignalDecl(s) => {
                if self.context == CheckContext::Function {
                    self.diagnostics.push(SymbolDiagnostic {
                        span: s.span,
                        message: "signal declarations are not allowed in functions".to_string(),
                        kind: DiagnosticKind::SignalInFunction,
                        file: self.file.clone(),
                    });
                }
            }
            StatementKind::ComponentDecl(c) => {
                for entry in &c.names {
                    if let Some(init) = &entry.init {
                        self.check_component_init(init);
                    }
                }
            }
            StatementKind::For(f) => {
                self.check_statement(&f.init);
                self.check_block(&f.body);
                self.check_statement(&f.step);
            }
            StatementKind::While(w) => {
                self.check_block(&w.body);
            }
            StatementKind::IfElse(ie) => {
                self.check_block(&ie.then_body);
                if let Some(eb) = &ie.else_body {
                    self.check_block(eb);
                }
            }
            StatementKind::Block(b) => {
                self.check_block(b);
            }
            StatementKind::TupleAssign(t) => {
                self.check_tuple_assign(t, stmt.span);
            }
            _ => {}
        }
    }

    fn check_assignment(&mut self, assign: &AssignStmt, span: Span) {
        match assign.op {
            AssignOp::SafeLeft | AssignOp::UnsafeLeft => {
                // <== or <-- : LHS should be a signal (output or intermediate)
                self.check_signal_assign_target(&assign.lhs, assign.op);
            }
            AssignOp::SafeRight | AssignOp::UnsafeRight => {
                // ==> or --> : RHS is the signal being assigned
                self.check_signal_assign_target(&assign.rhs, assign.op);
            }
            AssignOp::Eq => {
                // = : should be used for variables, not signals
                self.check_var_assign_target(&assign.lhs, span);
            }
        }
    }

    fn check_tuple_assign(&mut self, assign: &TupleAssignStmt, _span: Span) {
        match assign.op {
            AssignOp::SafeLeft | AssignOp::UnsafeLeft => {
                for target in assign.targets.iter().flatten() {
                    self.check_signal_assign_target(target, assign.op);
                }
            }
            _ => {}
        }
    }

    /// Check that the target of a signal assignment (<== or <--) is valid:
    /// - Must be a signal (not a variable)
    /// - Cannot be an input signal (inside a template)
    fn check_signal_assign_target(&mut self, expr: &Expression, op: AssignOp) {
        let name = match expr.extract_base_name() {
            Some(n) => n,
            None => return,
        };

        if let Some(sym) = self
            .table
            .lookup_with_includes(self.current_scope, &name, &self.file)
        {
            match &sym.kind {
                SymbolKind::Signal(sig) => {
                    if sig.kind == SignalKind::Input && self.context == CheckContext::Template {
                        let op_str = match op {
                            AssignOp::SafeLeft => "<==",
                            AssignOp::UnsafeLeft => "<--",
                            AssignOp::SafeRight => "==>",
                            AssignOp::UnsafeRight => "-->",
                            _ => "signal assign",
                        };
                        self.diagnostics.push(SymbolDiagnostic {
                            span: expr.span,
                            message: format!(
                                "cannot assign to input signal '{name}' with '{op_str}'"
                            ),
                            kind: DiagnosticKind::AssignToInput,
                            file: self.file.clone(),
                        });
                    }
                }
                SymbolKind::Variable | SymbolKind::Parameter => {
                    let op_str = match op {
                        AssignOp::SafeLeft => "<==",
                        AssignOp::UnsafeLeft => "<--",
                        AssignOp::SafeRight => "==>",
                        AssignOp::UnsafeRight => "-->",
                        _ => "signal assign",
                    };
                    self.diagnostics.push(SymbolDiagnostic {
                        span: expr.span,
                        message: format!("signal operator '{op_str}' used on variable '{name}'"),
                        kind: DiagnosticKind::SignalAssignToVar,
                        file: self.file.clone(),
                    });
                }
                _ => {}
            }
        }
    }

    /// Check that `=` is not used on a signal.
    fn check_var_assign_target(&mut self, expr: &Expression, span: Span) {
        let name = match expr.extract_base_name() {
            Some(n) => n,
            None => return,
        };

        if let Some(sym) = self
            .table
            .lookup_with_includes(self.current_scope, &name, &self.file)
        {
            if let SymbolKind::Signal(_) = &sym.kind {
                self.diagnostics.push(SymbolDiagnostic {
                    span,
                    message: format!(
                        "cannot use '=' to assign to signal '{name}'; use '<==' or '<--'"
                    ),
                    kind: DiagnosticKind::VarAssignToSignal,
                    file: self.file.clone(),
                });
            }
        }
    }

    /// Check that a component initialization call matches the template parameter count.
    fn check_component_init(&mut self, expr: &Expression) {
        if let ExpressionKind::Call(callee, args) = expr.kind.as_ref() {
            if let ExpressionKind::Ident(name) = callee.kind.as_ref() {
                if let Some(sym) =
                    self.table
                        .lookup_with_includes(self.current_scope, name, &self.file)
                {
                    if let SymbolKind::Template(tmpl) = &sym.kind {
                        if args.len() != tmpl.params.len() {
                            self.diagnostics.push(SymbolDiagnostic {
                                span: expr.span,
                                message: format!(
                                    "template '{}' expects {} parameter(s), but {} provided",
                                    name,
                                    tmpl.params.len(),
                                    args.len()
                                ),
                                kind: DiagnosticKind::ParameterCountMismatch,
                                file: self.file.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    fn parse_and_check(src: &str) -> Vec<SymbolDiagnostic> {
        let (ast, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut table = SymbolTable::new();
        table.index_file("main.circom", &ast);
        check_types(&table, "main.circom", &ast)
    }

    fn diags_of_kind(diags: &[SymbolDiagnostic], kind: DiagnosticKind) -> Vec<&SymbolDiagnostic> {
        diags.iter().filter(|d| d.kind == kind).collect()
    }

    // ── Signal direction ───────────────────────────────────────────

    #[test]
    fn detects_assign_to_input_signal() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                a <== 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::AssignToInput);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("'a'"));
    }

    #[test]
    fn allows_assign_to_output_signal() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <== a;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::AssignToInput);
        assert!(errors.is_empty());
    }

    #[test]
    fn allows_assign_to_intermediate_signal() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal s;
                signal output b;
                s <== a;
                b <== s;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::AssignToInput);
        assert!(errors.is_empty());
    }

    #[test]
    fn detects_assign_to_input_with_unsafe_op() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                a <-- 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::AssignToInput);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("<--"));
    }

    // ── Assignment operator validation ─────────────────────────────

    #[test]
    fn detects_var_assign_to_signal() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal output b;
                b = 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::VarAssignToSignal);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("'b'"));
    }

    #[test]
    fn detects_signal_assign_to_var() {
        let diags = parse_and_check(
            r#"
            template T() {
                var x;
                x <== 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::SignalAssignToVar);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("'x'"));
    }

    #[test]
    fn allows_var_assign_to_var() {
        let diags = parse_and_check(
            r#"
            template T() {
                var x;
                x = 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::VarAssignToSignal);
        assert!(errors.is_empty());
        let errors = diags_of_kind(&diags, DiagnosticKind::SignalAssignToVar);
        assert!(errors.is_empty());
    }

    // ── Template parameter count ───────────────────────────────────

    #[test]
    fn detects_parameter_count_mismatch() {
        let diags = parse_and_check(
            r#"
            template Adder(n) {
                signal input a;
                signal output b;
                b <== a;
            }
            template Main() {
                component c = Adder(1, 2);
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::ParameterCountMismatch);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("expects 1"));
        assert!(errors[0].message.contains("2 provided"));
    }

    #[test]
    fn allows_correct_parameter_count() {
        let diags = parse_and_check(
            r#"
            template Adder(n) {
                signal input a;
                signal output b;
                b <== a;
            }
            template Main() {
                component c = Adder(4);
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::ParameterCountMismatch);
        assert!(errors.is_empty());
    }

    #[test]
    fn detects_zero_args_for_parameterized_template() {
        let diags = parse_and_check(
            r#"
            template Adder(n) {
                signal input a;
                signal output b;
                b <== a;
            }
            template Main() {
                component c = Adder();
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::ParameterCountMismatch);
        assert_eq!(errors.len(), 1);
    }

    // ── Signals in functions ───────────────────────────────────────

    #[test]
    fn detects_signal_decl_in_function() {
        let diags = parse_and_check(
            r#"
            function foo() {
                signal input x;
                return 0;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::SignalInFunction);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("signal"));
    }

    #[test]
    fn detects_constraint_eq_in_function() {
        let diags = parse_and_check(
            r#"
            function foo(a, b) {
                a === b;
                return 0;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::SignalInFunction);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("==="));
    }

    #[test]
    fn allows_signal_decl_in_template() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input x;
                signal output y;
                y <== x;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::SignalInFunction);
        assert!(errors.is_empty());
    }

    // ── Indexed signal access ──────────────────────────────────────

    #[test]
    fn detects_assign_to_input_array_element() {
        let diags = parse_and_check(
            r#"
            template T(n) {
                signal input a[n];
                signal output b;
                a[0] <== 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::AssignToInput);
        assert_eq!(errors.len(), 1);
    }
}
