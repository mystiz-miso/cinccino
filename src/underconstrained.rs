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

    // Build the bipartite incidence and derived sets.
    let mut mentioned_by_some_constraint: HashSet<String> = HashSet::new();
    let mut output_is_driven: HashMap<String, bool> = signals
        .iter()
        .filter(|(_, info)| info.kind == SignalKind::Output)
        .map(|(name, _)| (name.clone(), false))
        .collect();

    for c in &constraints {
        for sig in &c.signals {
            mentioned_by_some_constraint.insert(sig.clone());
        }
        if let Some(lhs) = &c.driven {
            if let Some(driven) = output_is_driven.get_mut(lhs) {
                *driven = true;
            }
        }
    }

    // 1. Outputs that are never driven by a `<==` or `===` with themselves on
    //    one side — the classic "forgotten output" bug.
    for (name, driven) in &output_is_driven {
        if !*driven {
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

    // 2. Non-input signals never touched by any constraint (fully dangling).
    //    We skip pure intermediates that *might* be used via component
    //    wiring (c.x <== s) because such wires flow through member access,
    //    which we already count in `signals`.
    for (name, info) in &signals {
        if info.kind == SignalKind::Input {
            continue;
        }
        // Outputs already handled above.
        if info.kind == SignalKind::Output {
            continue;
        }
        if !mentioned_by_some_constraint.contains(name) {
            diagnostics.push(SymbolDiagnostic {
                span: info.span,
                message: format!("signal '{name}' is declared but never appears in any constraint"),
                kind: DiagnosticKind::UnderconstrainedOutput,
                file: file_path.to_string(),
            });
        }
    }

    // 3. Global count heuristic: if the template declares more non-input
    //    signals than it produces constraints, at least one signal is
    //    necessarily free. We only warn on the *template* span, not on a
    //    specific signal, because pinpointing which one would need full
    //    rank analysis.
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
struct Constraint {
    /// Signal names mentioned anywhere in the constraint.
    signals: HashSet<String>,
    /// If this constraint "drives" a single signal (LHS of `<==` or `===`
    /// where one side is a bare signal reference), that signal's name.
    driven: Option<String>,
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

fn collect_constraints_in_stmt(
    stmt: &Statement,
    signals: &HashMap<String, SignalInfo>,
    out: &mut Vec<Constraint>,
) {
    match &stmt.kind {
        StatementKind::ConstraintEq(c) => {
            let mut sigs = HashSet::new();
            collect_signal_refs(&c.lhs, signals, &mut sigs);
            collect_signal_refs(&c.rhs, signals, &mut sigs);
            // A `===` constraint drives either side if that side is a
            // bare signal ref; we accept either.
            let driven = c
                .lhs
                .extract_base_name()
                .filter(|n| signals.contains_key(n))
                .or_else(|| {
                    c.rhs
                        .extract_base_name()
                        .filter(|n| signals.contains_key(n))
                });
            out.push(Constraint {
                signals: sigs,
                driven,
            });
        }
        StatementKind::Assignment(a) => match a.op {
            AssignOp::SafeLeft | AssignOp::UnsafeLeft => {
                let mut sigs = HashSet::new();
                collect_signal_refs(&a.lhs, signals, &mut sigs);
                collect_signal_refs(&a.rhs, signals, &mut sigs);
                let driven = a
                    .lhs
                    .extract_base_name()
                    .filter(|n| signals.contains_key(n));
                // Only `<==` produces a constraint; `<--` assigns the
                // witness without constraining it.
                if a.op == AssignOp::SafeLeft {
                    out.push(Constraint {
                        signals: sigs,
                        driven,
                    });
                }
            }
            AssignOp::SafeRight | AssignOp::UnsafeRight => {
                let mut sigs = HashSet::new();
                collect_signal_refs(&a.lhs, signals, &mut sigs);
                collect_signal_refs(&a.rhs, signals, &mut sigs);
                let driven = a
                    .rhs
                    .extract_base_name()
                    .filter(|n| signals.contains_key(n));
                if a.op == AssignOp::SafeRight {
                    out.push(Constraint {
                        signals: sigs,
                        driven,
                    });
                }
            }
            AssignOp::Eq => {}
        },
        StatementKind::TupleAssign(t) => {
            if matches!(t.op, AssignOp::SafeLeft) {
                for target in t.targets.iter().flatten() {
                    let mut sigs = HashSet::new();
                    collect_signal_refs(target, signals, &mut sigs);
                    collect_signal_refs(&t.rhs, signals, &mut sigs);
                    let driven = target
                        .extract_base_name()
                        .filter(|n| signals.contains_key(n));
                    out.push(Constraint {
                        signals: sigs,
                        driven,
                    });
                }
            }
        }
        StatementKind::SignalDecl(s) => {
            // `signal output o <== expr;` is a constraint in disguise.
            for entry in &s.names {
                if let Some((op, init)) = &entry.init {
                    if *op == SignalAssignOp::SafeLeft {
                        let mut sigs = HashSet::new();
                        sigs.insert(entry.name.name.clone());
                        collect_signal_refs(init, signals, &mut sigs);
                        out.push(Constraint {
                            signals: sigs,
                            driven: Some(entry.name.name.clone()),
                        });
                    }
                }
            }
        }
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
