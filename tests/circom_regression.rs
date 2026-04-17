//! Regression tests against the circom compiler.
//!
//! For each fixture in `tests/fixtures/circom_regression/`, this harness:
//!
//! 1. Parses the `*.circom` source with cinccino's parser.
//! 2. Runs cinccino's full analyzer stack (symbol table + type checker
//!    + constraint checker + undeclared-symbol check).
//! 3. Compares the resulting diagnostics against the `.expected_diagnostics.json`
//!    sibling file.
//! 4. If the `circom` compiler is available on `$PATH`, also invokes it on
//!    the fixture and asserts cinccino's diagnostics are a *superset* of
//!    the compiler's (i.e. cinccino may produce additional warnings, but
//!    it must not miss a real compiler error).
//!
//! When `circom` is absent, step 4 is skipped — the test still pins
//! cinccino's own diagnostics to the expected JSON so regressions are
//! caught across refactors.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use cinccino::constraint_checker::check_constraints;
use cinccino::parser;
use cinccino::symbol::DiagnosticKind;
use cinccino::symbol_table::SymbolTable;
use cinccino::type_checker::check_types;
use serde_json::Value;

fn regression_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("circom_regression")
}

fn circom_available() -> bool {
    Command::new("circom")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn kind_name(kind: DiagnosticKind) -> &'static str {
    match kind {
        DiagnosticKind::DuplicateSymbol => "DuplicateSymbol",
        DiagnosticKind::UndeclaredSymbol => "UndeclaredSymbol",
        DiagnosticKind::AssignToInput => "AssignToInput",
        DiagnosticKind::VarAssignToSignal => "VarAssignToSignal",
        DiagnosticKind::SignalAssignToVar => "SignalAssignToVar",
        DiagnosticKind::ParameterCountMismatch => "ParameterCountMismatch",
        DiagnosticKind::NonQuadraticConstraint => "NonQuadraticConstraint",
        DiagnosticKind::UnsafeSignalAssignment => "UnsafeSignalAssignment",
        DiagnosticKind::SignalInFunction => "SignalInFunction",
        DiagnosticKind::TagLoss => "TagLoss",
        DiagnosticKind::MissingRequiredTag => "MissingRequiredTag",
        DiagnosticKind::UnknownComponentSignal => "UnknownComponentSignal",
        DiagnosticKind::UnusedComponentOutput => "UnusedComponentOutput",
        DiagnosticKind::MissingComponentInput => "MissingComponentInput",
        DiagnosticKind::UnderconstrainedOutput => "UnderconstrainedOutput",
    }
}

/// Run cinccino's full analyzer stack on the given source. Returns the
/// set of diagnostic-kind names produced.
fn analyze_kinds(src: &str) -> (BTreeSet<String>, Vec<String>) {
    let (ast, parse_errors) = parser::parse(src);
    let mut kinds = BTreeSet::new();
    let mut messages: Vec<String> = parse_errors
        .iter()
        .map(|e| format!("ParseError: {e:?}"))
        .collect();
    if !parse_errors.is_empty() {
        kinds.insert("ParseError".to_string());
    }

    let mut table = SymbolTable::new();
    table.index_file("fixture.circom", &ast);
    for d in table.diagnostics() {
        kinds.insert(kind_name(d.kind).to_string());
        messages.push(format!("{}: {}", kind_name(d.kind), d.message));
    }

    let type_diags = check_types(&table, "fixture.circom", &ast);
    for d in &type_diags {
        kinds.insert(kind_name(d.kind).to_string());
        messages.push(format!("{}: {}", kind_name(d.kind), d.message));
    }

    let constraint_diags = check_constraints(&table, "fixture.circom", &ast);
    for d in &constraint_diags {
        kinds.insert(kind_name(d.kind).to_string());
        messages.push(format!("{}: {}", kind_name(d.kind), d.message));
    }

    // Also run undeclared-symbol check against a fresh table (since
    // check_undeclared borrows the table mutably).
    let mut table2 = SymbolTable::new();
    table2.index_file("fixture.circom", &ast);
    table2.check_undeclared("fixture.circom", &ast);
    for d in table2.diagnostics() {
        if d.kind == DiagnosticKind::UndeclaredSymbol {
            kinds.insert(kind_name(d.kind).to_string());
            messages.push(format!("{}: {}", kind_name(d.kind), d.message));
        }
    }

    (kinds, messages)
}

fn load_expected(fixture: &PathBuf) -> Value {
    let json_path = fixture.with_extension("expected_diagnostics.json");
    let raw = fs::read_to_string(&json_path)
        .unwrap_or_else(|_| panic!("missing expected diagnostics for {fixture:?}"));
    serde_json::from_str(&raw).expect("invalid expected diagnostics JSON")
}

fn expected_kinds(expected: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(arr) = expected["diagnostics"].as_array() {
        for d in arr {
            if let Some(k) = d["kind"].as_str() {
                out.insert(k.to_string());
            }
        }
    }
    out
}

/// Collect `(path, stem)` pairs for all fixtures in the regression dir.
fn fixtures() -> Vec<(PathBuf, String)> {
    let dir = regression_dir();
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).expect("regression fixture dir missing") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) == Some("circom") {
            let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
            out.push((path, stem));
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Parse circom's stderr output for error / warning tokens and
/// convert them into our diagnostic-kind names heuristically. This is
/// intentionally loose: we only care about "did the compiler find
/// *something* broken" granularity, and that we see at least the same
/// set of issues.
fn circom_error_kinds(stderr: &str) -> BTreeSet<String> {
    let mut kinds = BTreeSet::new();
    let lower = stderr.to_lowercase();
    if lower.contains("non quadratic") || lower.contains("not quadratic") {
        kinds.insert("NonQuadraticConstraint".to_string());
    }
    if lower.contains("input signal") && lower.contains("assigned") {
        kinds.insert("AssignToInput".to_string());
    }
    if lower.contains("number of parameters") {
        kinds.insert("ParameterCountMismatch".to_string());
    }
    if lower.contains("undeclared") {
        kinds.insert("UndeclaredSymbol".to_string());
    }
    if lower.contains("duplicated") {
        kinds.insert("DuplicateSymbol".to_string());
    }
    kinds
}

fn run_circom(path: &PathBuf) -> Option<BTreeSet<String>> {
    if !circom_available() {
        return None;
    }
    let output = Command::new("circom")
        .arg("--no_init")
        .arg(path)
        .output()
        .ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    Some(circom_error_kinds(&stderr))
}

// ─── the test matrix ───────────────────────────────────────────────────

/// Assert cinccino's analyzer is a superset of `expected_diagnostics.json`
/// for every fixture: every expected kind must be present, but cinccino
/// may report additional warnings (e.g., component-level lints). Runs as
/// a single test so a failure surfaces the specific fixture by name.
#[test]
fn cinccino_matches_expected_for_all_fixtures() {
    let mut failures = Vec::new();
    for (path, stem) in fixtures() {
        let src = fs::read_to_string(&path).unwrap();
        let (actual, messages) = analyze_kinds(&src);
        let expected = load_expected(&path);
        let expected_set = expected_kinds(&expected);

        let missing: BTreeSet<_> = expected_set.difference(&actual).cloned().collect();
        if !missing.is_empty() {
            failures.push(format!(
                "{stem}: missing {missing:?} (expected {expected_set:?}, got {actual:?})\n  messages: {messages:#?}"
            ));
        }
        // For fixtures that expect no diagnostics, be strict: the file
        // should be clean modulo component-level lints we consider
        // acceptable additions only for buggy fixtures.
        if expected_set.is_empty() && !actual.is_empty() {
            // Allow component-level warnings for the `valid_component`
            // fixture (which would trigger MissingComponentInput /
            // UnusedComponentOutput for Main's own input/output only if
            // they're never driven — which they are here).
            let acceptable: BTreeSet<&str> = BTreeSet::new();
            let unexpected: BTreeSet<_> = actual
                .iter()
                .filter(|k| !acceptable.contains(k.as_str()))
                .cloned()
                .collect();
            if !unexpected.is_empty() {
                failures.push(format!(
                    "{stem}: expected clean file but got {unexpected:?}\n  messages: {messages:#?}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "fixture mismatches:\n{}",
        failures.join("\n")
    );
}

/// Assert cinccino's diagnostics are a *superset* of circom's, when the
/// compiler is available. No-op in environments without circom.
#[test]
fn cinccino_superset_of_circom_compiler() {
    if !circom_available() {
        eprintln!("circom compiler not on PATH — skipping superset check");
        return;
    }
    let mut failures = Vec::new();
    for (path, stem) in fixtures() {
        let src = fs::read_to_string(&path).unwrap();
        let (actual, _) = analyze_kinds(&src);
        if let Some(circom_kinds) = run_circom(&path) {
            let missing: Vec<_> = circom_kinds.difference(&actual).cloned().collect();
            if !missing.is_empty() {
                failures.push(format!(
                    "{stem}: cinccino missed {missing:?} (circom saw {circom_kinds:?})"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "superset failures:\n{}",
        failures.join("\n")
    );
}

/// Every `.circom` fixture must have a matching `.expected_diagnostics.json`.
#[test]
fn every_fixture_has_expected_json() {
    let dir = regression_dir();
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) == Some("circom") {
            let json = path.with_extension("expected_diagnostics.json");
            assert!(json.exists(), "missing {json:?}");
        }
    }
}

/// Smoke test: we expect at least 10 regression fixtures.
#[test]
fn regression_fixture_count_at_least_ten() {
    let n = fixtures().len();
    assert!(n >= 10, "only {n} fixtures (need >= 10)");
}
