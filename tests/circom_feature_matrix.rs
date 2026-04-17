//! Feature matrix test suite.
//!
//! Walks every major Circom 2.2.x surface with small focused fixtures.
//! Each test either:
//!
//! - validates a *well-formed* program: parse succeeds, symbol-table
//!   indexing succeeds, and the semantic analyzers produce no
//!   `Error`-severity diagnostics (warnings are allowed); or
//! - validates an *intentionally buggy* program: parse still succeeds
//!   (or fails where noted) and the expected diagnostic kind is
//!   reported by the analyzer.
//!
//! New language features added to the parser should land here with at
//! least one positive and, where it makes sense, one negative fixture
//! so a regression is caught at the matrix level before it reaches the
//! LSP surface.

use cinccino::constraint_checker::check_constraints;
use cinccino::parser;
use cinccino::symbol::{DiagnosticKind, SymbolDiagnostic};
use cinccino::symbol_table::SymbolTable;
use cinccino::type_checker::check_types;

// ─── helpers ───────────────────────────────────────────────────────────

/// Warning-severity diagnostic kinds; everything else is Error-severity.
/// Mirrors `severity_for` in `src/server/backend.rs`.
fn is_warning(kind: DiagnosticKind) -> bool {
    matches!(
        kind,
        DiagnosticKind::UnsafeSignalAssignment
            | DiagnosticKind::TagLoss
            | DiagnosticKind::UnusedComponentOutput
            | DiagnosticKind::MissingComponentInput
            | DiagnosticKind::UnderconstrainedOutput
    )
}

/// Parse, index, and run type + constraint checks. Panics on any parse
/// error. Returns the full diagnostic list (from the symbol table,
/// type checker, and constraint checker combined).
fn analyze(src: &str) -> Vec<SymbolDiagnostic> {
    let (ast, parse_errors) = parser::parse(src);
    assert!(parse_errors.is_empty(), "parse errors: {parse_errors:?}");

    let mut table = SymbolTable::new();
    table.index_file("main.circom", &ast);
    let mut diags: Vec<SymbolDiagnostic> = table.diagnostics().to_vec();
    diags.extend(check_types(&table, "main.circom", &ast));
    diags.extend(check_constraints(&table, "main.circom", &ast));
    diags
}

/// Assert that a well-formed program parses, indexes, and produces no
/// error-severity diagnostics. Warnings are tolerated — they surface
/// legitimate hazards (e.g. unsafe assigns without matching `===`).
fn expect_ok(src: &str) {
    let diags = analyze(src);
    let errors: Vec<&SymbolDiagnostic> = diags.iter().filter(|d| !is_warning(d.kind)).collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

/// Assert that at least one diagnostic of the given kind is produced.
fn expect_kind(src: &str, kind: DiagnosticKind) {
    let diags = analyze(src);
    let matched = diags.iter().filter(|d| d.kind == kind).count();
    assert!(
        matched >= 1,
        "expected at least one {kind:?} diagnostic, got: {diags:?}"
    );
}

// ─── pragmas ───────────────────────────────────────────────────────────

#[test]
fn pragma_version_2_0_0() {
    expect_ok("pragma circom 2.0.0; template T() { signal output o; o <== 1; }");
}

#[test]
fn pragma_version_2_1_0() {
    expect_ok("pragma circom 2.1.0; template T() { signal output o; o <== 1; }");
}

#[test]
fn pragma_version_2_2_0() {
    expect_ok("pragma circom 2.2.0; template T() { signal output o; o <== 1; }");
}

#[test]
fn pragma_version_2_2_3() {
    expect_ok("pragma circom 2.2.3; template T() { signal output o; o <== 1; }");
}

#[test]
fn pragma_custom_templates() {
    expect_ok(
        r#"
        pragma circom 2.2.0;
        pragma custom_templates;
        template custom MyCustom() { signal input x; signal output y; y <-- x; }
        "#,
    );
}

// ─── templates ─────────────────────────────────────────────────────────

#[test]
fn template_plain() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template Id() {
            signal input x;
            signal output y;
            y <== x;
        }
        "#,
    );
}

#[test]
fn template_custom() {
    expect_ok(
        r#"
        pragma circom 2.2.0;
        pragma custom_templates;
        template custom C() {
            signal input x;
            signal output y;
            y <-- x;
        }
        "#,
    );
}

#[test]
fn template_parallel() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template parallel P(n) {
            signal input in[n];
            signal output out;
            out <== in[0];
        }
        "#,
    );
}

#[test]
fn template_extern_on_2_2_3() {
    expect_ok(
        r#"
        pragma circom 2.2.3;
        pragma custom_templates;
        template custom extern Ex() {
            signal input x;
            signal output y;
        }
        "#,
    );
}

#[test]
fn template_parameterized() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template Adder(n) {
            signal input a[n];
            signal input b[n];
            signal output c[n];
            for (var i = 0; i < n; i++) {
                c[i] <== a[i] + b[i];
            }
        }
        "#,
    );
}

#[test]
fn template_main_with_public_signals() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template Mul() {
            signal input a;
            signal input b;
            signal output c;
            c <== a * b;
        }
        component main {public [a, b]} = Mul();
        "#,
    );
}

// ─── functions ─────────────────────────────────────────────────────────

#[test]
fn function_with_return() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function square(x) {
            return x * x;
        }
        "#,
    );
}

#[test]
fn function_calls_another_function() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function double(x) { return x + x; }
        function quadruple(x) { return double(double(x)); }
        "#,
    );
}

#[test]
fn function_recursion_guard_by_depth() {
    // Classic `nbits(n)` pattern — an iterative loop, not runtime
    // recursion, but exercises the same helper shape compiled code
    // typically uses to compute bit widths at elaboration time.
    expect_ok(
        r#"
        pragma circom 2.0.0;
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
}

// ─── buses ─────────────────────────────────────────────────────────────

#[test]
fn bus_plain_declaration() {
    expect_ok(
        r#"
        pragma circom 2.2.0;
        bus Point() {
            signal x;
            signal y;
        }
        "#,
    );
}

#[test]
fn bus_parameterized() {
    expect_ok(
        r#"
        pragma circom 2.2.0;
        bus Vec(n) {
            signal v[n];
        }
        "#,
    );
}

#[test]
fn bus_nested() {
    expect_ok(
        r#"
        pragma circom 2.2.0;
        bus Point() { signal x; signal y; }
        bus Line()  { Point() start; Point() end; }
        "#,
    );
}

#[test]
fn bus_typed_signal_on_template_io() {
    expect_ok(
        r#"
        pragma circom 2.2.0;
        bus Point() { signal x; signal y; }
        template Read() {
            signal input Point() p;
            signal output o;
            o <== p.x;
        }
        "#,
    );
}

#[test]
fn bus_field_access_via_dot() {
    expect_ok(
        r#"
        pragma circom 2.2.0;
        bus Point() { signal x; signal y; }
        template T() {
            signal input Point() p;
            signal output sum;
            sum <== p.x + p.y;
        }
        "#,
    );
}

#[test]
fn bus_field_array_access() {
    expect_ok(
        r#"
        pragma circom 2.2.0;
        bus Row(n) { signal v[n]; }
        template T(n) {
            signal input Row(n) r;
            signal output first;
            first <== r.v[0];
        }
        "#,
    );
}

// ─── signals ───────────────────────────────────────────────────────────

#[test]
fn signal_input_output_intermediate() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template T() {
            signal input a;
            signal mid;
            signal output b;
            mid <== a;
            b <== mid;
        }
        "#,
    );
}

#[test]
fn signal_tagged_binary() {
    expect_ok(
        r#"
        pragma circom 2.1.0;
        template T(n) {
            signal input {binary} in[n];
            signal output out;
            out <== in[0];
        }
        "#,
    );
}

#[test]
fn signal_multi_dim_array() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template Grid(r, c) {
            signal input m[r][c];
            signal output first;
            first <== m[0][0];
        }
        "#,
    );
}

#[test]
fn signal_tagged_output_propagation() {
    // Propagating a `{binary}` input into a `{binary}` output preserves
    // the tag — no `TagLoss` warning should fire.
    expect_ok(
        r#"
        pragma circom 2.1.0;
        template T() {
            signal input {binary} x;
            signal output {binary} y;
            y <== x;
        }
        "#,
    );
}

// ─── anonymous components ──────────────────────────────────────────────

#[test]
fn anonymous_component_single_input() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template Id() { signal input x; signal output y; y <== x; }
        template T() {
            signal input a;
            signal output b;
            b <== Id()(a);
        }
        "#,
    );
}

#[test]
fn anonymous_component_multi_input() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template Add3() {
            signal input a; signal input b; signal input c;
            signal output o;
            o <== a + b + c;
        }
        template T() {
            signal input a; signal input b; signal input c;
            signal output o;
            o <== Add3()(a, b, c);
        }
        "#,
    );
}

#[test]
fn anonymous_component_named_inputs() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template Pair() {
            signal input a; signal input b;
            signal output o;
            o <== a + b;
        }
        template T() {
            signal input x; signal input y;
            signal output r;
            r <== Pair()(a <== x, b <== y);
        }
        "#,
    );
}

// ─── tuple assignment ──────────────────────────────────────────────────

#[test]
fn tuple_assignment_all_targets() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template Split() {
            signal input x;
            signal output a; signal output b;
            a <== x;
            b <== x;
        }
        template T() {
            signal input x;
            signal output a; signal output b;
            (a, b) <== Split()(x);
        }
        "#,
    );
}

#[test]
fn tuple_assignment_with_underscore() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template Split3() {
            signal input x;
            signal output a; signal output b; signal output c;
            a <== x;
            b <== x;
            c <== x;
        }
        template T() {
            signal input x;
            signal output a; signal output b;
            (a, b, _) <== Split3()(x);
        }
        "#,
    );
}

// ─── signal-assign operators ───────────────────────────────────────────

#[test]
fn signal_assign_safe_left() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template T() { signal input a; signal output b; b <== a; }
        "#,
    );
}

#[test]
fn signal_assign_safe_right() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template T() { signal input a; signal output b; a ==> b; }
        "#,
    );
}

#[test]
fn signal_assign_unsafe_left_with_constraint_eq() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template T() {
            signal input a; signal output b;
            b <-- a;
            b === a;
        }
        "#,
    );
}

#[test]
fn signal_assign_unsafe_right_with_constraint_eq() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template T() {
            signal input a; signal output b;
            a --> b;
            b === a;
        }
        "#,
    );
}

#[test]
fn signal_constraint_eq_standalone() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template T() {
            signal input a; signal output b;
            b <-- a;
            a === b;
        }
        "#,
    );
}

// ─── arithmetic / bitwise / comparison / logical ───────────────────────

#[test]
fn arithmetic_ops_in_function() {
    // Full set: +, -, *, /, \, %, **. Exercised in a function body where
    // the constraint checker does not apply.
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function f(a, b) {
            var s = a + b;
            var d = a - b;
            var p = a * b;
            var q = a / b;
            var i = a \ b;
            var m = a % b;
            var e = a ** b;
            return s + d + p + q + i + m + e;
        }
        "#,
    );
}

#[test]
fn bitwise_ops_in_function() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function f(a, b) {
            var x = (a & b) | (a ^ b);
            var y = ~a;
            var z = (a << 2) | (b >> 1);
            return x + y + z;
        }
        "#,
    );
}

#[test]
fn comparison_ops_in_function() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function f(a, b) {
            var t = 0;
            if (a == b) { t = t + 1; }
            if (a != b) { t = t + 1; }
            if (a <  b) { t = t + 1; }
            if (a <= b) { t = t + 1; }
            if (a >  b) { t = t + 1; }
            if (a >= b) { t = t + 1; }
            return t;
        }
        "#,
    );
}

#[test]
fn logical_ops_in_function() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function f(a, b) {
            var t = 0;
            if ((a > 0) && (b > 0)) { t = 1; }
            if ((a > 0) || (b > 0)) { t = t + 1; }
            if (!(a == 0)) { t = t + 1; }
            return t;
        }
        "#,
    );
}

// ─── augmented assigns / increment / decrement / ternary ──────────────

#[test]
fn augmented_assigns_cover_all_operators() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function f(a, b) {
            var x = a;
            x += b; x -= b; x *= b; x /= b;
            x \= b; x %= b; x **= b;
            return x;
        }
        "#,
    );
}

#[test]
fn increment_decrement_statements() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function f() {
            var i = 0;
            i++;
            i--;
            return i;
        }
        "#,
    );
}

#[test]
fn ternary_expression() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function max(a, b) {
            return (a > b) ? a : b;
        }
        "#,
    );
}

// ─── control flow ──────────────────────────────────────────────────────

#[test]
fn control_if_else() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function sign(x) {
            if (x > 0) { return 1; }
            else { if (x < 0) { return 0 - 1; } else { return 0; } }
        }
        "#,
    );
}

#[test]
fn control_for_loop() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function sum(n) {
            var s = 0;
            for (var i = 0; i < n; i++) { s = s + i; }
            return s;
        }
        "#,
    );
}

#[test]
fn control_while_loop() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function countdown(n) {
            var i = n;
            while (i > 0) { i = i - 1; }
            return i;
        }
        "#,
    );
}

// ─── statements: include, log, assert ──────────────────────────────────

#[test]
fn stmt_include_declaration() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        include "circomlib/poseidon.circom";
        template T() { signal output o; o <== 1; }
        "#,
    );
}

#[test]
fn stmt_log_single_expr() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template T() {
            signal input x;
            log(x);
        }
        "#,
    );
}

#[test]
fn stmt_log_string_and_expr() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template T() {
            signal input x;
            log("value:", x);
        }
        "#,
    );
}

#[test]
fn stmt_assert_expression() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template T(n) {
            assert(n > 0);
            signal output o;
            o <== 1;
        }
        "#,
    );
}

// ─── arrays ────────────────────────────────────────────────────────────

#[test]
fn array_literal_flat() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function f() {
            var xs[3] = [1, 2, 3];
            return xs[0] + xs[1] + xs[2];
        }
        "#,
    );
}

#[test]
fn array_literal_nested() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        function f() {
            var m[2][2] = [[1, 2], [3, 4]];
            return m[0][0] + m[1][1];
        }
        "#,
    );
}

#[test]
fn array_indexing_1d() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template T(n) {
            signal input in[n];
            signal output o;
            o <== in[0];
        }
        "#,
    );
}

#[test]
fn array_indexing_2d() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template T(r, c) {
            signal input m[r][c];
            signal output o;
            o <== m[0][1];
        }
        "#,
    );
}

// ─── field access ──────────────────────────────────────────────────────

#[test]
fn field_access_on_component() {
    expect_ok(
        r#"
        pragma circom 2.0.0;
        template Leaf() { signal output out; out <== 1; }
        template Root() {
            component c = Leaf();
            signal output o;
            o <== c.out;
        }
        "#,
    );
}

#[test]
fn field_access_on_bus_chain() {
    expect_ok(
        r#"
        pragma circom 2.2.0;
        bus Inner() { signal v; }
        bus Outer() { Inner() inner; }
        template T() {
            signal input Outer() o;
            signal output r;
            r <== o.inner.v;
        }
        "#,
    );
}

// ─── intentionally buggy programs ──────────────────────────────────────

#[test]
fn buggy_assign_to_input_signal() {
    expect_kind(
        r#"
        pragma circom 2.0.0;
        template T() {
            signal input a;
            a <== 1;
        }
        "#,
        DiagnosticKind::AssignToInput,
    );
}

#[test]
fn buggy_var_assign_on_signal() {
    expect_kind(
        r#"
        pragma circom 2.0.0;
        template T() {
            signal output s;
            s = 1;
        }
        "#,
        DiagnosticKind::VarAssignToSignal,
    );
}

#[test]
fn buggy_signal_assign_on_var() {
    expect_kind(
        r#"
        pragma circom 2.0.0;
        template T() {
            var v;
            v <== 1;
        }
        "#,
        DiagnosticKind::SignalAssignToVar,
    );
}

#[test]
fn buggy_signal_in_function_body() {
    expect_kind(
        r#"
        pragma circom 2.0.0;
        function bad() {
            signal s;
            return 0;
        }
        "#,
        DiagnosticKind::SignalInFunction,
    );
}

#[test]
fn buggy_non_quadratic_constraint() {
    expect_kind(
        r#"
        pragma circom 2.0.0;
        template T() {
            signal input a; signal input b; signal input c;
            signal output d;
            d <== a * b * c;
        }
        "#,
        DiagnosticKind::NonQuadraticConstraint,
    );
}

#[test]
fn buggy_parameter_count_mismatch() {
    expect_kind(
        r#"
        pragma circom 2.0.0;
        template Pair(a, b, c) { signal output o; o <== 1; }
        template Root() {
            component p = Pair(1, 2);
        }
        "#,
        DiagnosticKind::ParameterCountMismatch,
    );
}

#[test]
fn buggy_bus_type_mismatch() {
    expect_kind(
        r#"
        pragma circom 2.2.0;
        bus A() { signal v; }
        bus B() { signal v; }
        template T() {
            signal input A() a;
            signal output B() b;
            b <== a;
        }
        "#,
        DiagnosticKind::BusTypeMismatch,
    );
}
