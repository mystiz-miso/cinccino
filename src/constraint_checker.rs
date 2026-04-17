//! Constraint checking for Circom semantic analysis.
//!
//! Validates that constraints (`===`, `<==`) produce valid R1CS quadratic form
//! (A * B + C = 0) and warns on unsafe signal assignments (`<--`) without
//! a corresponding constraint (`===`).

use std::collections::HashSet;

use crate::ast::*;
use crate::span::Span;
use crate::symbol::*;
use crate::symbol_table::SymbolTable;

/// Run constraint checks on a file's AST using the populated symbol table.
///
/// Returns diagnostics for any constraint errors or warnings found.
pub fn check_constraints(
    table: &SymbolTable,
    file_path: &str,
    ast: &File,
) -> Vec<SymbolDiagnostic> {
    let file_scope = match table.file_scope(file_path) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut checker = ConstraintChecker {
        table,
        file: file_path.to_string(),
        current_scope: file_scope,
        diagnostics: Vec::new(),
    };
    checker.check_file(ast);
    checker.diagnostics
}

struct ConstraintChecker<'a> {
    table: &'a SymbolTable,
    file: String,
    current_scope: ScopeId,
    diagnostics: Vec<SymbolDiagnostic>,
}

/// Represents the "degree" of an expression in the signal domain.
///
/// In R1CS, constraints must be at most quadratic (degree 2) in signals.
/// - Constants and variables have degree 0.
/// - A single signal reference has degree 1.
/// - signal * signal has degree 2 (quadratic — allowed).
/// - signal * signal * signal has degree 3+ (not allowed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Degree {
    /// Constant or variable expression (degree 0).
    Constant,
    /// Linear in signals (degree 1).
    Linear,
    /// Quadratic in signals (degree 2) — maximum for R1CS.
    Quadratic,
    /// Cubic or higher (degree 3+) — invalid for R1CS.
    NonQuadratic,
}

impl Degree {
    /// Combine degrees under multiplication.
    fn mul(self, other: Degree) -> Degree {
        let sum = self.as_u32() + other.as_u32();
        Degree::from_u32(sum)
    }

    /// Combine degrees under addition (max of the two).
    fn combined(self, other: Degree) -> Degree {
        if self >= other {
            self
        } else {
            other
        }
    }

    fn as_u32(self) -> u32 {
        match self {
            Degree::Constant => 0,
            Degree::Linear => 1,
            Degree::Quadratic => 2,
            Degree::NonQuadratic => 3,
        }
    }

    fn from_u32(n: u32) -> Degree {
        match n {
            0 => Degree::Constant,
            1 => Degree::Linear,
            2 => Degree::Quadratic,
            _ => Degree::NonQuadratic,
        }
    }
}

impl<'a> ConstraintChecker<'a> {
    fn check_file(&mut self, ast: &File) {
        for item in &ast.items {
            if let Item::TemplateDef(t) = item {
                self.check_template(t);
            }
        }
    }

    fn check_template(&mut self, node: &TemplateDef) {
        if let Some(sym) = self.table.lookup(self.current_scope, &node.name.name) {
            if let SymbolKind::Template(ref t) = sym.kind {
                let outer_scope = self.current_scope;
                self.current_scope = t.body_scope;

                // Collect signals that have a `===` constraint.
                let constrained = self.collect_constrained_signals(&node.body);

                self.check_block_constraints(&node.body, &constrained);

                self.current_scope = outer_scope;
            }
        }
    }

    /// Collect all signal names on the LHS of `===` constraints in a block.
    fn collect_constrained_signals(&self, block: &Block) -> HashSet<String> {
        let mut constrained = HashSet::new();
        self.collect_constrained_in_block(block, &mut constrained);
        constrained
    }

    fn collect_constrained_in_block(&self, block: &Block, constrained: &mut HashSet<String>) {
        for stmt in &block.stmts {
            self.collect_constrained_in_stmt(stmt, constrained);
        }
    }

    fn collect_constrained_in_stmt(&self, stmt: &Statement, constrained: &mut HashSet<String>) {
        match &stmt.kind {
            StatementKind::ConstraintEq(c) => {
                if let Some(name) = c.lhs.extract_base_name() {
                    constrained.insert(name);
                }
                if let Some(name) = c.rhs.extract_base_name() {
                    constrained.insert(name);
                }
            }
            StatementKind::Assignment(a) => {
                // <== also generates a constraint
                if a.op == AssignOp::SafeLeft {
                    if let Some(name) = a.lhs.extract_base_name() {
                        constrained.insert(name);
                    }
                } else if a.op == AssignOp::SafeRight {
                    if let Some(name) = a.rhs.extract_base_name() {
                        constrained.insert(name);
                    }
                }
            }
            StatementKind::For(f) => {
                self.collect_constrained_in_block(&f.body, constrained);
            }
            StatementKind::While(w) => {
                self.collect_constrained_in_block(&w.body, constrained);
            }
            StatementKind::IfElse(ie) => {
                self.collect_constrained_in_block(&ie.then_body, constrained);
                if let Some(eb) = &ie.else_body {
                    self.collect_constrained_in_block(eb, constrained);
                }
            }
            StatementKind::Block(b) => {
                self.collect_constrained_in_block(b, constrained);
            }
            _ => {}
        }
    }

    fn check_block_constraints(&mut self, block: &Block, constrained: &HashSet<String>) {
        for stmt in &block.stmts {
            self.check_stmt_constraints(stmt, constrained);
        }
    }

    fn warn_unsafe_assignment(
        &mut self,
        target: &Expression,
        constrained: &HashSet<String>,
        span: Span,
        op_str: &str,
    ) {
        if let Some(name) = target.extract_base_name() {
            if !constrained.contains(&name) && self.is_signal(&name) {
                self.diagnostics.push(SymbolDiagnostic {
                    span,
                    message: format!(
                        "signal '{name}' assigned with '{op_str}' without a corresponding '===' constraint"
                    ),
                    kind: DiagnosticKind::UnsafeSignalAssignment,
                    file: self.file.clone(),
                });
            }
        }
    }

    fn check_assignment_constraint(
        &mut self,
        a: &AssignStmt,
        constrained: &HashSet<String>,
        span: Span,
    ) {
        match a.op {
            AssignOp::SafeLeft => {
                // <== generates a constraint: check quadratic form
                self.check_quadratic_constraint(&a.lhs, &a.rhs, span);
            }
            AssignOp::SafeRight => {
                self.check_quadratic_constraint(&a.rhs, &a.lhs, span);
            }
            AssignOp::UnsafeLeft => {
                self.warn_unsafe_assignment(&a.lhs, constrained, span, "<--");
            }
            AssignOp::UnsafeRight => {
                self.warn_unsafe_assignment(&a.rhs, constrained, span, "-->");
            }
            _ => {}
        }
    }

    fn check_stmt_constraints(&mut self, stmt: &Statement, constrained: &HashSet<String>) {
        match &stmt.kind {
            StatementKind::ConstraintEq(c) => {
                self.check_quadratic_constraint(&c.lhs, &c.rhs, stmt.span);
            }
            StatementKind::Assignment(a) => {
                self.check_assignment_constraint(a, constrained, stmt.span);
            }
            StatementKind::For(f) => {
                self.check_block_constraints(&f.body, constrained);
            }
            StatementKind::While(w) => {
                self.check_block_constraints(&w.body, constrained);
            }
            StatementKind::IfElse(ie) => {
                self.check_block_constraints(&ie.then_body, constrained);
                if let Some(eb) = &ie.else_body {
                    self.check_block_constraints(eb, constrained);
                }
            }
            StatementKind::Block(b) => {
                self.check_block_constraints(b, constrained);
            }
            _ => {}
        }
    }

    /// Check that a constraint (lhs === rhs or lhs <== rhs) is quadratic.
    ///
    /// The constraint `lhs - rhs = 0` must be expressible in R1CS form:
    /// A * B + C = 0 where A, B, C are linear combinations of signals.
    /// This means the overall expression degree must be at most 2.
    fn check_quadratic_constraint(&mut self, lhs: &Expression, rhs: &Expression, span: Span) {
        let lhs_deg = self.expr_degree(lhs);
        let rhs_deg = self.expr_degree(rhs);
        let overall = lhs_deg.combined(rhs_deg);

        if overall == Degree::NonQuadratic {
            self.diagnostics.push(SymbolDiagnostic {
                span,
                message: "constraint is not quadratic; R1CS requires at most degree-2 expressions"
                    .to_string(),
                kind: DiagnosticKind::NonQuadraticConstraint,
                file: self.file.clone(),
            });
        }
    }

    fn pow_degree(&self, ld: Degree, r: &Expression, rd: Degree) -> Degree {
        // signal ** constant: degree = signal_degree * constant_value
        // For simplicity, if LHS is a signal and RHS is constant,
        // we conservatively check: if LHS has signals and RHS > 2, non-quadratic.
        if ld > Degree::Constant && rd == Degree::Constant {
            // If exponent is a known number, compute degree
            if let ExpressionKind::Number(n) = r.kind.as_ref() {
                if let Ok(exp) = n.parse::<u32>() {
                    return Degree::from_u32(ld.as_u32().saturating_mul(exp));
                }
            }
            // Unknown exponent with signal base — assume non-quadratic
            Degree::NonQuadratic
        } else if ld > Degree::Constant && rd > Degree::Constant {
            Degree::NonQuadratic
        } else {
            ld.combined(rd)
        }
    }

    fn binary_degree(&self, l: &Expression, op: BinaryOp, r: &Expression) -> Degree {
        let ld = self.expr_degree(l);
        let rd = self.expr_degree(r);
        match op {
            BinaryOp::Mul => ld.mul(rd),
            BinaryOp::Pow => self.pow_degree(ld, r, rd),
            BinaryOp::Add | BinaryOp::Sub => ld.combined(rd),
            BinaryOp::Div | BinaryOp::IntDiv | BinaryOp::Mod => {
                // Division by a signal is non-linear
                if rd > Degree::Constant {
                    Degree::NonQuadratic
                } else {
                    ld
                }
            }
            // Comparison and logical ops produce constants
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::Le
            | BinaryOp::Ge
            | BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                if ld > Degree::Constant || rd > Degree::Constant {
                    Degree::NonQuadratic
                } else {
                    Degree::Constant
                }
            }
        }
    }

    /// Compute the signal degree of an expression.
    fn expr_degree(&self, expr: &Expression) -> Degree {
        match expr.kind.as_ref() {
            ExpressionKind::Number(_) => Degree::Constant,
            ExpressionKind::Ident(name) => {
                if self.is_signal(name) {
                    Degree::Linear
                } else {
                    Degree::Constant
                }
            }
            ExpressionKind::Unary(_, e) => self.expr_degree(e),
            ExpressionKind::Binary(l, op, r) => self.binary_degree(l, *op, r),
            ExpressionKind::Ternary(cond, then_expr, else_expr) => {
                let cd = self.expr_degree(cond);
                let td = self.expr_degree(then_expr);
                let ed = self.expr_degree(else_expr);
                if cd == Degree::Constant && td == Degree::Constant && ed == Degree::Constant {
                    Degree::Constant
                } else {
                    Degree::NonQuadratic
                }
            }
            ExpressionKind::Index(base, _) => self.expr_degree(base),
            ExpressionKind::Member(base, _) => {
                // Member access on a component signal — treat as linear
                if let Some(name) = expr.extract_base_name() {
                    if self.is_signal_or_component(&name) {
                        Degree::Linear
                    } else {
                        Degree::Constant
                    }
                } else {
                    self.expr_degree(base)
                }
            }
            ExpressionKind::Call(_, _) => {
                // Function calls return runtime values (degree 0 for constraint purposes)
                Degree::Constant
            }
            ExpressionKind::Paren(e) => self.expr_degree(e),
            ExpressionKind::ArrayLit(_)
            | ExpressionKind::AnonymousComp(_)
            | ExpressionKind::Parallel(_)
            | ExpressionKind::Underscore
            | ExpressionKind::Error => Degree::Constant,
        }
    }

    fn is_signal(&self, name: &str) -> bool {
        self.table
            .lookup_with_includes(self.current_scope, name, &self.file)
            .map(|s| matches!(s.kind, SymbolKind::Signal(_)))
            .unwrap_or(false)
    }

    fn is_signal_or_component(&self, name: &str) -> bool {
        self.table
            .lookup_with_includes(self.current_scope, name, &self.file)
            .map(|s| matches!(s.kind, SymbolKind::Signal(_) | SymbolKind::Component(_)))
            .unwrap_or(false)
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
        check_constraints(&table, "main.circom", &ast)
    }

    fn diags_of_kind(diags: &[SymbolDiagnostic], kind: DiagnosticKind) -> Vec<&SymbolDiagnostic> {
        diags.iter().filter(|d| d.kind == kind).collect()
    }

    // ── Valid constraint forms ──────────────────────────────────────

    #[test]
    fn allows_linear_constraint() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <== a + 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::NonQuadraticConstraint);
        assert!(errors.is_empty());
    }

    #[test]
    fn allows_quadratic_constraint() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal input b;
                signal output c;
                c <== a * b;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::NonQuadraticConstraint);
        assert!(errors.is_empty());
    }

    #[test]
    fn allows_quadratic_constraint_eq() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal input b;
                signal output c;
                c <-- a * b;
                c === a * b;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::NonQuadraticConstraint);
        assert!(errors.is_empty());
    }

    // ── Invalid constraint forms ───────────────────────────────────

    #[test]
    fn detects_cubic_constraint() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal input b;
                signal input c;
                signal output d;
                d <== a * b * c;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::NonQuadraticConstraint);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("not quadratic"));
    }

    #[test]
    fn detects_cubic_via_power() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <== a ** 3;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::NonQuadraticConstraint);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn allows_quadratic_via_power() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <== a ** 2;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::NonQuadraticConstraint);
        assert!(errors.is_empty());
    }

    #[test]
    fn detects_signal_division() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal input b;
                signal output c;
                c <== a / b;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::NonQuadraticConstraint);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn allows_constant_division() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <== a / 2;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::NonQuadraticConstraint);
        assert!(errors.is_empty());
    }

    // ── Unsafe signal assignment warnings ──────────────────────────

    #[test]
    fn warns_unsafe_assign_without_constraint() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <-- a;
            }
            "#,
        );
        let warnings = diags_of_kind(&diags, DiagnosticKind::UnsafeSignalAssignment);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("'b'"));
        assert!(warnings[0].message.contains("<--"));
    }

    #[test]
    fn no_warning_when_constraint_exists() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <-- a;
                b === a;
            }
            "#,
        );
        let warnings = diags_of_kind(&diags, DiagnosticKind::UnsafeSignalAssignment);
        assert!(warnings.is_empty());
    }

    #[test]
    fn no_warning_for_safe_assign() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <== a;
            }
            "#,
        );
        let warnings = diags_of_kind(&diags, DiagnosticKind::UnsafeSignalAssignment);
        assert!(warnings.is_empty());
    }

    // ── Constraint with variables (valid — degree 0) ───────────────

    #[test]
    fn allows_variable_in_constraint() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                var x = 5;
                b <== a * x;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::NonQuadraticConstraint);
        assert!(errors.is_empty());
    }

    // ── Bitwise ops on signals are non-linear ──────────────────────

    #[test]
    fn detects_bitwise_on_signals() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal input b;
                signal output c;
                c <== a & b;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::NonQuadraticConstraint);
        assert_eq!(errors.len(), 1);
    }
}
