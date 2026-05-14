//! Underconstrained-signal detection.
//!
//! A Circom circuit is "underconstrained" when the R1CS system has more
//! unknown signals than independent constraints to determine them — so a
//! prover can produce multiple valid witnesses for the same public input.
//!
//! Full R1CS-rank analysis is well outside the scope of a local static
//! analyzer (it requires constant-folding, Gaussian elimination and
//! witness-generation reasoning). What this module does instead is a
//! bipartite-graph heuristic that catches the overwhelming majority of
//! real-world bugs: *for every template, every output signal must be
//! referenced by at least one constraint, and the total number of
//! constraints referencing signals in the template body must be at least
//! as large as the number of distinct non-input signals those constraints
//! mention.*
//!
//! Concretely we build, for each template:
//! - A set of **signals** declared inside the template body (inputs are
//!   excluded because the caller constrains them).
//! - A set of **constraints**: every `===` constraint statement and every
//!   `<==` assignment (both generate an R1CS row).
//! - A bipartite incidence: which signals each constraint references.
//!
//! Then we emit a warning when:
//! - A non-input signal is not referenced by any constraint at all
//!   (dangling signal).
//! - An `output` signal is not the left-hand side of any `<==` / `===`
//!   constraint that directly involves it (classic missing-output bug).
//! - The total signal count exceeds the constraint count: the system is
//!   definitely underdetermined.
//!
//! These heuristics are conservative: false positives can happen when
//! constraints live inside `for` loops whose bounds depend on template
//! parameters (we include them once per loop regardless). The goal is to
//! flag suspicious templates at edit time, not to replace formal tooling
//! such as `circomspect` or `picus`.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::symbol::*;
use crate::symbol_table::SymbolTable;

/// Run underconstrained-signal detection against a file's AST.
#[tracing::instrument(level = "debug", skip(table, ast), fields(file = %file_path))]
pub fn analyze(table: &SymbolTable, file_path: &str, ast: &File) -> Vec<SymbolDiagnostic> {
    let file_scope = match table.file_scope(file_path) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut diagnostics = Vec::new();
    for item in &ast.items {
        if let Item::TemplateDef(tpl) = item {
            analyze_template(table, file_path, file_scope, tpl, &mut diagnostics);
        }
    }
    diagnostics
}

fn compute_incidence(
    signals: &HashMap<String, SignalInfo>,
    constraints: &[Constraint],
) -> (HashSet<String>, HashMap<String, bool>) {
    let mut mentioned_by_some_constraint: HashSet<String> = HashSet::new();
    let mut output_is_driven: HashMap<String, bool> = signals
        .iter()
        .filter(|(_, info)| info.kind == SignalKind::Output)
        .map(|(name, _)| (name.clone(), false))
        .collect();

    for c in constraints {
        for sig in &c.signals {
            mentioned_by_some_constraint.insert(sig.clone());
            // An output is "driven" if it appears in any constraint —
            // not only as a bare LHS/RHS. This is the common circomlib
            // pattern: `out <-- expr;` paired with a separate `===`
            // that pins `out`, where `out` is buried inside a product
            // expression (e.g. `out * x === y`). The previous bare-ref
            // rule missed this and produced false-positive
            // "never assigned" warnings.
            if let Some(driven) = output_is_driven.get_mut(sig) {
                *driven = true;
            }
        }
    }
    (mentioned_by_some_constraint, output_is_driven)
}

fn warn_undriven_outputs(
    output_is_driven: &HashMap<String, bool>,
    signals: &HashMap<String, SignalInfo>,
    file_path: &str,
    diagnostics: &mut Vec<SymbolDiagnostic>,
) {
    for (name, driven) in output_is_driven {
        if *driven {
            continue;
        }
        if let Some(info) = signals.get(name) {
            diagnostics.push(SymbolDiagnostic {
                span: info.span,
                message: format!(
                    "output signal '{name}' is never assigned by a '<==' or '===' constraint"
                ),
                kind: DiagnosticKind::UnderconstrainedOutput,
                file: file_path.to_string(),
            });
        }
    }
}

fn warn_dangling_signals(
    signals: &HashMap<String, SignalInfo>,
    mentioned: &HashSet<String>,
    file_path: &str,
    diagnostics: &mut Vec<SymbolDiagnostic>,
) {
    for (name, info) in signals {
        // Inputs are never under-constrained; outputs are handled elsewhere.
        if matches!(info.kind, SignalKind::Input | SignalKind::Output) {
            continue;
        }
        if !mentioned.contains(name) {
            diagnostics.push(SymbolDiagnostic {
                span: info.span,
                message: format!("signal '{name}' is declared but never appears in any constraint"),
                kind: DiagnosticKind::UnderconstrainedOutput,
                file: file_path.to_string(),
            });
        }
    }
}

fn warn_global_count(
    signals: &HashMap<String, SignalInfo>,
    constraints: &[Constraint],
    tpl: &TemplateDef,
    file_path: &str,
    diagnostics: &mut Vec<SymbolDiagnostic>,
) {
    let non_input_count = signals
        .values()
        .filter(|info| info.kind != SignalKind::Input)
        .count();
    if non_input_count > constraints.len() && !constraints.is_empty() {
        diagnostics.push(SymbolDiagnostic {
            span: tpl.name.span,
            message: format!(
                "template '{}' has {} non-input signals but only {} constraint(s); some signals may be underconstrained",
                tpl.name.name,
                non_input_count,
                constraints.len()
            ),
            kind: DiagnosticKind::UnderconstrainedOutput,
            file: file_path.to_string(),
        });
    }
}

fn analyze_template(
    table: &SymbolTable,
    file_path: &str,
    file_scope: ScopeId,
    tpl: &TemplateDef,
    diagnostics: &mut Vec<SymbolDiagnostic>,
) {
    // Collect every signal symbol in the template body scope + all
    // nested block scopes belonging to it.
    let body_scope = match table.lookup(file_scope, &tpl.name.name) {
        Some(sym) => match &sym.kind {
            SymbolKind::Template(t) => t.body_scope,
            _ => return,
        },
        None => return,
    };

    let mut signals: HashMap<String, SignalInfo> = HashMap::new();
    collect_signals_in_scope(table, body_scope, &mut signals);

    let mut constraints: Vec<Constraint> = Vec::new();
    collect_constraints_in_block(&tpl.body, &signals, &mut constraints);

    let (mentioned, output_is_driven) = compute_incidence(&signals, &constraints);
    warn_undriven_outputs(&output_is_driven, &signals, file_path, diagnostics);
    warn_dangling_signals(&signals, &mentioned, file_path, diagnostics);
    warn_global_count(&signals, &constraints, tpl, file_path, diagnostics);
}

/// Info we cache about a signal while walking the template body.
struct SignalInfo {
    kind: SignalKind,
    span: crate::span::Span,
}

fn collect_signals_in_scope(
    table: &SymbolTable,
    scope: ScopeId,
    out: &mut HashMap<String, SignalInfo>,
) {
    // Walk the scope sub-tree collecting signals.
    let scope_node = table.scopes.get(scope);
    for id in scope_node.all_symbols() {
        let sym = table.get_symbol(id);
        if let SymbolKind::Signal(sig) = &sym.kind {
            out.entry(sym.name.clone()).or_insert(SignalInfo {
                kind: sig.kind,
                span: sym.span,
            });
        }
    }
    let children = scope_node.children.clone();
    for child in children {
        collect_signals_in_scope(table, child, out);
    }
}

/// A constraint / assignment that contributes to the R1CS system.
/// We track the set of signal names mentioned anywhere in it; an output
/// is considered "driven" if it appears in *any* such constraint (the
/// `<-- pin + === constraint` idiom relies on this — see
/// `compute_incidence`).
struct Constraint {
    signals: HashSet<String>,
}

fn collect_constraints_in_block(
    block: &Block,
    signals: &HashMap<String, SignalInfo>,
    out: &mut Vec<Constraint>,
) {
    for stmt in &block.stmts {
        collect_constraints_in_stmt(stmt, signals, out);
    }
}

fn constraint_from_eq(c: &ConstraintEqStmt, signals: &HashMap<String, SignalInfo>) -> Constraint {
    let mut sigs = HashSet::new();
    collect_signal_refs(&c.lhs, signals, &mut sigs);
    collect_signal_refs(&c.rhs, signals, &mut sigs);
    Constraint { signals: sigs }
}

fn constraint_from_assignment(
    a: &AssignStmt,
    signals: &HashMap<String, SignalInfo>,
) -> Option<Constraint> {
    // Only `<==` / `==>` add an R1CS row; `<--` / `-->` are witness-
    // only and `===` is handled by `constraint_from_eq`.
    if !matches!(a.op, AssignOp::SafeLeft | AssignOp::SafeRight) {
        return None;
    }
    let mut sigs = HashSet::new();
    collect_signal_refs(&a.lhs, signals, &mut sigs);
    collect_signal_refs(&a.rhs, signals, &mut sigs);
    Some(Constraint { signals: sigs })
}

fn constraints_from_tuple_assign(
    t: &TupleAssignStmt,
    signals: &HashMap<String, SignalInfo>,
    out: &mut Vec<Constraint>,
) {
    if !matches!(t.op, AssignOp::SafeLeft) {
        return;
    }
    for target in t.targets.iter().flatten() {
        let mut sigs = HashSet::new();
        collect_signal_refs(target, signals, &mut sigs);
        collect_signal_refs(&t.rhs, signals, &mut sigs);
        out.push(Constraint { signals: sigs });
    }
}

fn constraints_from_signal_decl(
    s: &SignalDecl,
    signals: &HashMap<String, SignalInfo>,
    out: &mut Vec<Constraint>,
) {
    // `signal output o <== expr;` is a constraint in disguise.
    for entry in &s.names {
        if let Some((op, init)) = &entry.init {
            if *op == SignalAssignOp::SafeLeft {
                let mut sigs = HashSet::new();
                sigs.insert(entry.name.name.clone());
                collect_signal_refs(init, signals, &mut sigs);
                out.push(Constraint { signals: sigs });
            }
        }
    }
}

fn collect_constraints_in_stmt(
    stmt: &Statement,
    signals: &HashMap<String, SignalInfo>,
    out: &mut Vec<Constraint>,
) {
    match &stmt.kind {
        StatementKind::ConstraintEq(c) => out.push(constraint_from_eq(c, signals)),
        StatementKind::Assignment(a) => {
            if let Some(c) = constraint_from_assignment(a, signals) {
                out.push(c);
            }
        }
        StatementKind::TupleAssign(t) => constraints_from_tuple_assign(t, signals, out),
        StatementKind::SignalDecl(s) => constraints_from_signal_decl(s, signals, out),
        StatementKind::For(f) => collect_constraints_in_block(&f.body, signals, out),
        StatementKind::While(w) => collect_constraints_in_block(&w.body, signals, out),
        StatementKind::IfElse(ie) => {
            collect_constraints_in_block(&ie.then_body, signals, out);
            if let Some(eb) = &ie.else_body {
                collect_constraints_in_block(eb, signals, out);
            }
        }
        StatementKind::Block(b) => collect_constraints_in_block(b, signals, out),
        _ => {}
    }
}

/// Collect every signal name referenced by an expression.
fn collect_signal_refs(
    expr: &Expression,
    signals: &HashMap<String, SignalInfo>,
    out: &mut HashSet<String>,
) {
    match expr.kind.as_ref() {
        ExpressionKind::Ident(name) => {
            if signals.contains_key(name) {
                out.insert(name.clone());
            }
        }
        ExpressionKind::Index(base, idx) => {
            collect_signal_refs(base, signals, out);
            collect_signal_refs(idx, signals, out);
        }
        ExpressionKind::Member(base, _) => collect_signal_refs(base, signals, out),
        ExpressionKind::Unary(_, e) => collect_signal_refs(e, signals, out),
        ExpressionKind::Binary(l, _, r) => {
            collect_signal_refs(l, signals, out);
            collect_signal_refs(r, signals, out);
        }
        ExpressionKind::Ternary(c, t, f) => {
            collect_signal_refs(c, signals, out);
            collect_signal_refs(t, signals, out);
            collect_signal_refs(f, signals, out);
        }
        ExpressionKind::Call(callee, args) => {
            collect_signal_refs(callee, signals, out);
            for a in args {
                collect_signal_refs(a, signals, out);
            }
        }
        ExpressionKind::AnonymousComp(ac) => {
            collect_signal_refs(&ac.template, signals, out);
            for a in &ac.template_args {
                collect_signal_refs(a, signals, out);
            }
            for inp in &ac.inputs {
                match inp {
                    AnonCompInput::Positional(e) | AnonCompInput::Named(_, e) => {
                        collect_signal_refs(e, signals, out);
                    }
                }
            }
        }
        ExpressionKind::ArrayLit(elems) => {
            for e in elems {
                collect_signal_refs(e, signals, out);
            }
        }
        ExpressionKind::Paren(e) | ExpressionKind::Parallel(e) => {
            collect_signal_refs(e, signals, out);
        }
        ExpressionKind::Number(_) | ExpressionKind::Underscore | ExpressionKind::Error => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    fn analyze_src(src: &str) -> Vec<SymbolDiagnostic> {
        let (ast, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut table = SymbolTable::new();
        table.index_file("main.circom", &ast);
        analyze(&table, "main.circom", &ast)
    }

    #[test]
    fn warns_on_unassigned_output() {
        let diags = analyze_src(
            r#"
            template T() {
                signal input a;
                signal output b;
            }
            "#,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.kind == DiagnosticKind::UnderconstrainedOutput
                    && d.message.contains("'b'")),
            "expected warning for unassigned output, got: {diags:?}"
        );
    }

    #[test]
    fn no_warning_when_output_is_constrained() {
        let diags = analyze_src(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <== a;
            }
            "#,
        );
        assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    }

    #[test]
    fn no_warning_for_unsafe_assign_paired_with_constraint() {
        // Regression: Montgomery2Edwards pattern — `out <-- expr;` plus
        // a separate `===` where the output is buried inside a product.
        // Pre-fix, `extract_base_name` returned `None` for the LHS of
        // `out * in1 === in2` so the output was treated as undriven.
        let diags = analyze_src(
            r#"
            template Montgomery2EdwardsLike() {
                signal input in1;
                signal input in2;
                signal output out;
                out <-- in2 / in1;
                out * in1 === in2;
            }
            "#,
        );
        let undriven: Vec<&SymbolDiagnostic> = diags
            .iter()
            .filter(|d| {
                d.kind == DiagnosticKind::UnderconstrainedOutput
                    && d.message.contains("'out'")
                    && d.message.contains("never assigned")
            })
            .collect();
        assert!(
            undriven.is_empty(),
            "out is constrained by `out * in1 === in2`; should not warn: {undriven:?}"
        );
    }

    #[test]
    fn no_warning_when_output_initialized_inline() {
        let diags = analyze_src(
            r#"
            template T() {
                signal input a;
                signal output b <== a;
            }
            "#,
        );
        assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    }

    #[test]
    fn warns_on_dangling_intermediate() {
        let diags = analyze_src(
            r#"
            template T() {
                signal input a;
                signal mid;
                signal output b;
                b <== a;
            }
            "#,
        );
        assert!(
            diags.iter().any(|d| d.message.contains("'mid'")),
            "expected warning for dangling intermediate, got: {diags:?}"
        );
    }

    #[test]
    fn no_warning_when_intermediate_is_used() {
        let diags = analyze_src(
            r#"
            template T() {
                signal input a;
                signal mid;
                signal output b;
                mid <== a;
                b <== mid;
            }
            "#,
        );
        assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    }

    #[test]
    fn warns_when_more_signals_than_constraints() {
        // Three non-input signals but only one constraint.
        let diags = analyze_src(
            r#"
            template T() {
                signal input a;
                signal output b;
                signal output c;
                signal output d;
                b <== a;
            }
            "#,
        );
        // Expect at least the per-output warnings (c, d) and the global one.
        let count = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::UnderconstrainedOutput)
            .count();
        assert!(count >= 2, "expected multiple warnings, got: {diags:?}");
    }

    #[test]
    fn constraint_eq_counts_as_constraint() {
        let diags = analyze_src(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <-- a;
                b === a;
            }
            "#,
        );
        // `<--` does not produce a constraint, but `===` does and references `b`.
        assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    }

    #[test]
    fn tolerates_unknown_template() {
        // Template lookup through an included but unknown file should not panic.
        let diags = analyze_src("function f() { return 0; }");
        assert!(diags.is_empty());
    }
}
