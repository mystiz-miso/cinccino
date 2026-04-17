//! Semantic analysis test suite for cinccino's analyzers.
//!
//! Covers the three semantic-analysis passes:
//! - `type_checker`: arrays, signal kinds, function vs template context
//! - `constraint_checker`: quadratic-form validation, === in functions
//! - `symbol_table`: scoping, shadowing, cross-file resolution via include
//!
//! Each test is small and focused on a single behaviour so that
//! regressions point at a specific rule.

use cinccino::ast::SignalKind;
use cinccino::constraint_checker::check_constraints;
use cinccino::parser;
use cinccino::symbol::{DiagnosticKind, SymbolDiagnostic, SymbolKind};
use cinccino::symbol_table::SymbolTable;
use cinccino::type_checker::check_types;

// ─── helpers ───────────────────────────────────────────────────────────

fn parse_ok(src: &str) -> cinccino::ast::File {
    let (ast, errors) = parser::parse(src);
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    ast
}

fn index(src: &str) -> SymbolTable {
    let ast = parse_ok(src);
    let mut table = SymbolTable::new();
    table.index_file("main.circom", &ast);
    table
}

fn types_of(src: &str) -> Vec<SymbolDiagnostic> {
    let ast = parse_ok(src);
    let mut table = SymbolTable::new();
    table.index_file("main.circom", &ast);
    check_types(&table, "main.circom", &ast)
}

fn constraints_of(src: &str) -> Vec<SymbolDiagnostic> {
    let ast = parse_ok(src);
    let mut table = SymbolTable::new();
    table.index_file("main.circom", &ast);
    check_constraints(&table, "main.circom", &ast)
}

fn kind_count(diags: &[SymbolDiagnostic], kind: DiagnosticKind) -> usize {
    diags.iter().filter(|d| d.kind == kind).count()
}

// ─── type_checker: arrays ──────────────────────────────────────────────

#[test]
fn type_array_element_assignment_to_output_is_ok() {
    let diags = types_of(
        r#"
        template T(n) {
            signal input a[n];
            signal output b[n];
            b[0] <== a[0];
        }
        "#,
    );
    assert_eq!(kind_count(&diags, DiagnosticKind::AssignToInput), 0);
}

#[test]
fn type_array_input_element_assignment_is_error() {
    let diags = types_of(
        r#"
        template T(n) {
            signal input a[n];
            signal output b[n];
            a[0] <== 1;
        }
        "#,
    );
    assert_eq!(kind_count(&diags, DiagnosticKind::AssignToInput), 1);
}

#[test]
fn type_nested_array_input_assignment_is_error() {
    let diags = types_of(
        r#"
        template T(n, m) {
            signal input a[n][m];
            a[0][1] <== 1;
        }
        "#,
    );
    assert_eq!(kind_count(&diags, DiagnosticKind::AssignToInput), 1);
}

// ─── type_checker: signal kinds ────────────────────────────────────────

#[test]
fn type_intermediate_signal_ok_with_safe_assign() {
    let diags = types_of(
        r#"
        template T() {
            signal input x;
            signal mid;
            signal output y;
            mid <== x;
            y <== mid;
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected: {diags:?}");
}

#[test]
fn type_input_cannot_be_assigned_via_right_arrow() {
    // ==> is the right-hand form of <== and targets the RHS.
    let diags = types_of(
        r#"
        template T() {
            signal input a;
            1 ==> a;
        }
        "#,
    );
    assert_eq!(kind_count(&diags, DiagnosticKind::AssignToInput), 1);
}

#[test]
fn type_var_assigned_with_signal_op_is_flagged() {
    let diags = types_of(
        r#"
        template T() {
            var v;
            v <== 1;
        }
        "#,
    );
    assert_eq!(kind_count(&diags, DiagnosticKind::SignalAssignToVar), 1);
}

#[test]
fn type_signal_assigned_with_var_op_is_flagged() {
    let diags = types_of(
        r#"
        template T() {
            signal output s;
            s = 1;
        }
        "#,
    );
    assert_eq!(kind_count(&diags, DiagnosticKind::VarAssignToSignal), 1);
}

#[test]
fn type_parameter_count_matches_for_zero_arg_template() {
    let diags = types_of(
        r#"
        template Leaf() {
            signal output x;
            x <== 1;
        }
        template Root() {
            component c = Leaf();
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::ParameterCountMismatch),
        0
    );
}

#[test]
fn type_parameter_count_mismatch_multi_params() {
    let diags = types_of(
        r#"
        template Pair(a, b, c) {
            signal output o;
            o <== 1;
        }
        template Root() {
            component c = Pair(1, 2);
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::ParameterCountMismatch),
        1
    );
}

// ─── type_checker: function vs template ────────────────────────────────

#[test]
fn type_function_allows_plain_variable_work() {
    let diags = types_of(
        r#"
        function nbits(a) {
            var n = 0;
            while (a > 0) {
                n = n + 1;
                a = a \ 2;
            }
            return n;
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected: {diags:?}");
}

#[test]
fn type_function_forbids_signal_decl() {
    let diags = types_of(
        r#"
        function bad() {
            signal s;
            return 0;
        }
        "#,
    );
    assert_eq!(kind_count(&diags, DiagnosticKind::SignalInFunction), 1);
}

#[test]
fn type_function_forbids_constraint_eq() {
    let diags = types_of(
        r#"
        function bad(a, b) {
            a === b;
            return a;
        }
        "#,
    );
    assert_eq!(kind_count(&diags, DiagnosticKind::SignalInFunction), 1);
}

#[test]
fn type_template_allows_constraint_eq() {
    let diags = types_of(
        r#"
        template T() {
            signal input a;
            signal output b;
            b <-- a;
            b === a;
        }
        "#,
    );
    // Any SignalInFunction would be wrong.
    assert_eq!(kind_count(&diags, DiagnosticKind::SignalInFunction), 0);
}

// ─── constraint_checker: quadratic form ────────────────────────────────

#[test]
fn constraint_linear_sum_is_quadratic_ok() {
    let diags = constraints_of(
        r#"
        template T() {
            signal input a;
            signal input b;
            signal output c;
            c <== a + b + 5;
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::NonQuadraticConstraint),
        0
    );
}

#[test]
fn constraint_signal_times_signal_is_ok() {
    let diags = constraints_of(
        r#"
        template T() {
            signal input a;
            signal input b;
            signal output c;
            c <== a * b;
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::NonQuadraticConstraint),
        0
    );
}

#[test]
fn constraint_three_signal_product_is_cubic_error() {
    let diags = constraints_of(
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
    assert_eq!(
        kind_count(&diags, DiagnosticKind::NonQuadraticConstraint),
        1
    );
}

#[test]
fn constraint_power_of_two_is_ok() {
    let diags = constraints_of(
        r#"
        template T() {
            signal input a;
            signal output b;
            b <== a ** 2;
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::NonQuadraticConstraint),
        0
    );
}

#[test]
fn constraint_power_of_three_is_not_quadratic() {
    let diags = constraints_of(
        r#"
        template T() {
            signal input a;
            signal output b;
            b <== a ** 3;
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::NonQuadraticConstraint),
        1
    );
}

#[test]
fn constraint_division_by_constant_is_ok() {
    let diags = constraints_of(
        r#"
        template T() {
            signal input a;
            signal output b;
            b <== a / 2;
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::NonQuadraticConstraint),
        0
    );
}

#[test]
fn constraint_division_by_signal_is_non_linear() {
    let diags = constraints_of(
        r#"
        template T() {
            signal input a;
            signal input b;
            signal output c;
            c <== a / b;
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::NonQuadraticConstraint),
        1
    );
}

#[test]
fn constraint_unsafe_assign_without_constraint_warns() {
    let diags = constraints_of(
        r#"
        template T() {
            signal input a;
            signal output b;
            b <-- a + 1;
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::UnsafeSignalAssignment),
        1
    );
}

#[test]
fn constraint_unsafe_then_constraint_eq_no_warning() {
    let diags = constraints_of(
        r#"
        template T() {
            signal input a;
            signal output b;
            b <-- a;
            b === a;
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::UnsafeSignalAssignment),
        0
    );
}

#[test]
fn constraint_eq_in_function_reports_signal_in_function() {
    // === in a function is a semantic (type) error, surfaced by the type
    // checker. The constraint checker walks templates only, so it does
    // not produce a NonQuadraticConstraint diagnostic here.
    let diags = types_of(
        r#"
        function bad(a, b) {
            a === b;
            return a;
        }
        "#,
    );
    assert_eq!(kind_count(&diags, DiagnosticKind::SignalInFunction), 1);
}

#[test]
fn constraint_bitwise_on_signals_non_quadratic() {
    let diags = constraints_of(
        r#"
        template T() {
            signal input a;
            signal input b;
            signal output c;
            c <== a | b;
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::NonQuadraticConstraint),
        1
    );
}

#[test]
fn constraint_quadratic_constraint_eq_direct() {
    // `a * b === c` is quadratic and valid R1CS.
    let diags = constraints_of(
        r#"
        template T() {
            signal input a;
            signal input b;
            signal output c;
            c <-- a * b;
            a * b === c;
        }
        "#,
    );
    assert_eq!(
        kind_count(&diags, DiagnosticKind::NonQuadraticConstraint),
        0
    );
}

// ─── symbol_table: scoping ─────────────────────────────────────────────

#[test]
fn symbol_table_template_parameter_in_body_scope() {
    let table = index(
        r#"
        template T(n) {
            signal input x;
        }
        "#,
    );
    let file_scope = table.file_scope("main.circom").unwrap();
    let tmpl = table.lookup(file_scope, "T").unwrap();
    let body_scope = match &tmpl.kind {
        SymbolKind::Template(t) => t.body_scope,
        _ => panic!("expected template"),
    };
    let n = table.lookup(body_scope, "n").unwrap();
    assert!(matches!(n.kind, SymbolKind::Parameter));
}

#[test]
fn symbol_table_function_parameters_resolve_in_body() {
    let table = index(
        r#"
        function add(a, b) {
            return a + b;
        }
        "#,
    );
    let file_scope = table.file_scope("main.circom").unwrap();
    let f = table.lookup(file_scope, "add").unwrap();
    let body_scope = match &f.kind {
        SymbolKind::Function(fs) => fs.body_scope,
        _ => panic!("expected function"),
    };
    assert!(table.lookup(body_scope, "a").is_some());
    assert!(table.lookup(body_scope, "b").is_some());
}

#[test]
fn symbol_table_output_signal_kind_is_output() {
    let table = index(
        r#"
        template T() {
            signal output b;
        }
        "#,
    );
    let file_scope = table.file_scope("main.circom").unwrap();
    let tmpl = table.lookup(file_scope, "T").unwrap();
    let body_scope = match &tmpl.kind {
        SymbolKind::Template(t) => t.body_scope,
        _ => panic!("expected template"),
    };
    let b = table.lookup(body_scope, "b").unwrap();
    match &b.kind {
        SymbolKind::Signal(s) => assert_eq!(s.kind, SignalKind::Output),
        _ => panic!("expected signal"),
    }
}

// ─── symbol_table: shadowing ───────────────────────────────────────────

#[test]
fn symbol_table_for_loop_variable_is_scoped() {
    let table = index(
        r#"
        template T() {
            for (var i = 0; i < 3; i++) {
                var j = i;
            }
        }
        "#,
    );
    let file_scope = table.file_scope("main.circom").unwrap();
    let tmpl = table.lookup(file_scope, "T").unwrap();
    let body_scope = match &tmpl.kind {
        SymbolKind::Template(t) => t.body_scope,
        _ => panic!("expected template"),
    };
    // `i` and `j` are in the for-body scope, not the template scope.
    assert!(table.scopes.lookup_local(body_scope, "i").is_none());
    assert!(table.scopes.lookup_local(body_scope, "j").is_none());
}

#[test]
fn symbol_table_nested_block_does_not_leak() {
    let table = index(
        r#"
        template T(n) {
            if (n > 0) {
                var local = 1;
            }
        }
        "#,
    );
    let file_scope = table.file_scope("main.circom").unwrap();
    let tmpl = table.lookup(file_scope, "T").unwrap();
    let body_scope = match &tmpl.kind {
        SymbolKind::Template(t) => t.body_scope,
        _ => panic!("expected template"),
    };
    assert!(table.scopes.lookup_local(body_scope, "local").is_none());
}

#[test]
fn symbol_table_duplicate_function_flagged() {
    let table = index(
        r#"
        function f() { return 0; }
        function f() { return 1; }
        "#,
    );
    let dups = table
        .diagnostics()
        .iter()
        .filter(|d| d.kind == DiagnosticKind::DuplicateSymbol)
        .count();
    assert_eq!(dups, 1);
}

// ─── symbol_table: cross-file include resolution ───────────────────────

#[test]
fn symbol_table_include_resolves_template() {
    let mut table = SymbolTable::new();

    let (lib, _) = parser::parse(r#"template Lib() { signal input x; }"#);
    table.index_file("lib.circom", &lib);

    let (main, _) = parser::parse(
        r#"
        include "lib.circom";
        template Root() {
            component c = Lib();
        }
        "#,
    );
    table.index_file("main.circom", &main);

    let scope = table.file_scope("main.circom").unwrap();
    assert!(table
        .lookup_with_includes(scope, "Lib", "main.circom")
        .is_some());
}

#[test]
fn symbol_table_include_resolves_transitive_template() {
    let mut table = SymbolTable::new();
    let (leaf, _) = parser::parse(r#"template Leaf() { signal input x; }"#);
    table.index_file("leaf.circom", &leaf);

    let (mid, _) = parser::parse(
        r#"
        include "leaf.circom";
        template Mid() { signal input y; }
        "#,
    );
    table.index_file("mid.circom", &mid);

    let (top, _) = parser::parse(
        r#"
        include "mid.circom";
        template Top() { signal input z; }
        "#,
    );
    table.index_file("top.circom", &top);

    let scope = table.file_scope("top.circom").unwrap();
    assert!(table
        .lookup_with_includes(scope, "Leaf", "top.circom")
        .is_some());
    assert!(table
        .lookup_with_includes(scope, "Mid", "top.circom")
        .is_some());
}

#[test]
fn symbol_table_include_missing_symbol_returns_none() {
    let mut table = SymbolTable::new();
    let (lib, _) = parser::parse(r#"template Lib() { signal input x; }"#);
    table.index_file("lib.circom", &lib);

    let (main, _) = parser::parse(
        r#"
        include "lib.circom";
        template Root() { signal input a; }
        "#,
    );
    table.index_file("main.circom", &main);

    let scope = table.file_scope("main.circom").unwrap();
    assert!(table
        .lookup_with_includes(scope, "NotDefined", "main.circom")
        .is_none());
}

#[test]
fn symbol_table_include_cycle_tolerated() {
    let mut table = SymbolTable::new();
    let (a, _) = parser::parse(
        r#"
        include "b.circom";
        template A() { signal input x; }
        "#,
    );
    table.index_file("a.circom", &a);

    let (b, _) = parser::parse(
        r#"
        include "a.circom";
        template B() { signal input y; }
        "#,
    );
    table.index_file("b.circom", &b);

    let scope = table.file_scope("a.circom").unwrap();
    // Lookup from a.circom sees B via direct include, even in a cycle.
    assert!(table.lookup_with_includes(scope, "B", "a.circom").is_some());
}

#[test]
fn symbol_table_remove_file_drops_symbols() {
    let mut table = SymbolTable::new();
    let (a, _) = parser::parse(r#"template A() { signal input x; }"#);
    table.index_file("a.circom", &a);
    table.remove_file("a.circom");
    assert!(table.file_scope("a.circom").is_none());
}

// ─── symbol_table: qualified resolution ────────────────────────────────

#[test]
fn symbol_table_qualified_resolution_across_include() {
    let mut table = SymbolTable::new();
    let (lib, _) = parser::parse(
        r#"
        template Leaf() {
            signal input x;
            signal output y;
        }
        "#,
    );
    table.index_file("lib.circom", &lib);

    let (main, _) = parser::parse(
        r#"
        include "lib.circom";
        template Root() {
            component c = Leaf();
        }
        "#,
    );
    table.index_file("main.circom", &main);

    let main_scope = table.file_scope("main.circom").unwrap();
    let root = table.lookup(main_scope, "Root").unwrap();
    let root_body = match &root.kind {
        SymbolKind::Template(t) => t.body_scope,
        _ => panic!("expected template"),
    };
    let resolved = table
        .resolve_qualified(root_body, &["c", "y"], "main.circom")
        .unwrap();
    assert_eq!(resolved.name, "y");
}
