//! Comprehensive parser test suite covering every Circom language construct,
//! operator precedence, edge cases, error recovery, and version-specific features.
//!
//! This test file serves as both a regression suite and documentation of
//! supported Circom v2.2.3 syntax.

use cinccino::ast::*;
use cinccino::parser::{parse, ParseError};

// ── Helpers ────────────────────────────────────────────────────────────

fn parse_ok(src: &str) -> File {
    let (file, errors) = parse(src);
    assert!(errors.is_empty(), "unexpected errors: {:#?}", errors);
    file
}

fn parse_with_errors(src: &str) -> (File, Vec<ParseError>) {
    parse(src)
}

/// Extract the return expression from a single-function file.
fn return_expr(src: &str) -> Expression {
    let file = parse_ok(src);
    match &file.items[0] {
        Item::FunctionDef(f) => match &f.body.stmts[0].kind {
            StatementKind::Return(r) => r.value.clone(),
            other => panic!("expected return, got {:?}", other),
        },
        other => panic!("expected function, got {:?}", other),
    }
}

/// Extract statements from a single-template file.
fn template_stmts(src: &str) -> Vec<Statement> {
    let file = parse_ok(src);
    match &file.items[0] {
        Item::TemplateDef(t) => t.body.stmts.clone(),
        other => panic!("expected template, got {:?}", other),
    }
}

/// Extract statements from a single-function file.
fn function_stmts(src: &str) -> Vec<Statement> {
    let file = parse_ok(src);
    match &file.items[0] {
        Item::FunctionDef(f) => f.body.stmts.clone(),
        other => panic!("expected function, got {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// LANGUAGE CONSTRUCT FIXTURES
// ═══════════════════════════════════════════════════════════════════════

mod pragma {
    use super::*;

    #[test]
    fn version_2_0_0() {
        let file = parse_ok("pragma circom 2.0.0;");
        match &file.items[0] {
            Item::Pragma(p) => match &p.kind {
                PragmaKind::Version(v) => {
                    assert_eq!((v.major, v.minor, v.patch), (2, 0, 0));
                }
                _ => panic!("expected version pragma"),
            },
            _ => panic!("expected pragma"),
        }
    }

    #[test]
    fn version_2_1_0() {
        let file = parse_ok("pragma circom 2.1.0;");
        match &file.items[0] {
            Item::Pragma(p) => match &p.kind {
                PragmaKind::Version(v) => {
                    assert_eq!((v.major, v.minor, v.patch), (2, 1, 0));
                }
                _ => panic!("expected version pragma"),
            },
            _ => panic!("expected pragma"),
        }
    }

    #[test]
    fn version_2_2_0() {
        let file = parse_ok("pragma circom 2.2.0;");
        match &file.items[0] {
            Item::Pragma(p) => match &p.kind {
                PragmaKind::Version(v) => {
                    assert_eq!((v.major, v.minor, v.patch), (2, 2, 0));
                }
                _ => panic!("expected version pragma"),
            },
            _ => panic!("expected pragma"),
        }
    }

    #[test]
    fn version_2_2_3() {
        let file = parse_ok("pragma circom 2.2.3;");
        match &file.items[0] {
            Item::Pragma(p) => match &p.kind {
                PragmaKind::Version(v) => {
                    assert_eq!((v.major, v.minor, v.patch), (2, 2, 3));
                }
                _ => panic!("expected version pragma"),
            },
            _ => panic!("expected pragma"),
        }
    }

    #[test]
    fn custom_templates() {
        let file = parse_ok("pragma custom_templates;");
        match &file.items[0] {
            Item::Pragma(p) => assert_eq!(p.kind, PragmaKind::CustomTemplates),
            _ => panic!("expected pragma"),
        }
    }

    #[test]
    fn version_overflow() {
        let (_, errors) = parse_with_errors("pragma circom 99999999999.0.0;");
        assert!(
            errors.iter().any(|e| e.message.contains("overflows")),
            "expected overflow error, got: {:?}",
            errors
        );
    }
}

mod include {
    use super::*;

    #[test]
    fn relative_path() {
        let file = parse_ok(r#"include "../utils/helpers.circom";"#);
        match &file.items[0] {
            Item::Include(i) => assert_eq!(i.path, "../utils/helpers.circom"),
            _ => panic!("expected include"),
        }
    }

    #[test]
    fn absolute_path() {
        let file = parse_ok(r#"include "/home/user/circuits/main.circom";"#);
        match &file.items[0] {
            Item::Include(i) => assert_eq!(i.path, "/home/user/circuits/main.circom"),
            _ => panic!("expected include"),
        }
    }

    #[test]
    fn library_path() {
        let file = parse_ok(r#"include "circomlib/poseidon.circom";"#);
        match &file.items[0] {
            Item::Include(i) => assert_eq!(i.path, "circomlib/poseidon.circom"),
            _ => panic!("expected include"),
        }
    }
}

mod template {
    use super::*;

    #[test]
    fn no_params() {
        let file = parse_ok(
            "template Mul() { signal input a; signal input b; signal output c; c <== a * b; }",
        );
        match &file.items[0] {
            Item::TemplateDef(t) => {
                assert_eq!(t.name.name, "Mul");
                assert!(t.params.is_empty());
                assert!(!t.is_custom);
                assert!(!t.is_parallel);
                assert!(!t.is_extern);
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn with_params() {
        let file = parse_ok("template Bits2Num(n) { signal input in[n]; signal output out; }");
        match &file.items[0] {
            Item::TemplateDef(t) => {
                assert_eq!(t.params.len(), 1);
                assert_eq!(t.params[0].name, "n");
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn multiple_params() {
        let file = parse_ok("template T(a, b, c) { signal input x; }");
        match &file.items[0] {
            Item::TemplateDef(t) => {
                assert_eq!(t.params.len(), 3);
                assert_eq!(t.params[0].name, "a");
                assert_eq!(t.params[1].name, "b");
                assert_eq!(t.params[2].name, "c");
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn empty_body() {
        let file = parse_ok("template Empty() {}");
        match &file.items[0] {
            Item::TemplateDef(t) => {
                assert_eq!(t.name.name, "Empty");
                assert!(t.body.stmts.is_empty());
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn complex_body() {
        let src = r#"
            template Complex(n) {
                signal input in[n];
                signal output out;
                var sum = 0;
                for (var i = 0; i < n; i++) {
                    sum += in[i];
                }
                out <== sum;
            }
        "#;
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => {
                assert_eq!(t.name.name, "Complex");
                assert_eq!(t.body.stmts.len(), 5);
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn parallel() {
        let file = parse_ok("template parallel ParMul() {}");
        match &file.items[0] {
            Item::TemplateDef(t) => {
                assert!(t.is_parallel);
                assert!(!t.is_custom);
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn custom() {
        let file = parse_ok("pragma custom_templates; template custom MyC() {}");
        match &file.items[1] {
            Item::TemplateDef(t) => {
                assert!(t.is_custom);
                assert!(!t.is_parallel);
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn extern_custom() {
        let src = "pragma custom_templates; template custom extern Ext() { signal input in; signal output out; }";
        let file = parse_ok(src);
        match &file.items[1] {
            Item::TemplateDef(t) => {
                assert!(t.is_custom);
                assert!(t.is_extern);
            }
            _ => panic!("expected template"),
        }
    }
}

mod custom_templates_pragma {
    use super::*;

    #[test]
    fn pragma_enables_custom() {
        let src = "pragma custom_templates; template custom T() {}";
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 2);
        assert!(matches!(&file.items[0], Item::Pragma(p) if p.kind == PragmaKind::CustomTemplates));
    }
}

mod function {
    use super::*;

    #[test]
    fn with_params() {
        let file = parse_ok("function add(a, b) { return a + b; }");
        match &file.items[0] {
            Item::FunctionDef(f) => {
                assert_eq!(f.name.name, "add");
                assert_eq!(f.params.len(), 2);
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn return_statement() {
        let file = parse_ok("function f() { return 42; }");
        match &file.items[0] {
            Item::FunctionDef(f) => match &f.body.stmts[0].kind {
                StatementKind::Return(r) => {
                    assert!(matches!(*r.value.kind, ExpressionKind::Number(_)));
                }
                _ => panic!("expected return"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn recursive_call() {
        let src = "function factorial(n) { if (n <= 1) { return 1; } else { return n * factorial(n - 1); } }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::FunctionDef(f) => {
                assert_eq!(f.name.name, "factorial");
                // The body has an if/else
                match &f.body.stmts[0].kind {
                    StatementKind::IfElse(ie) => {
                        assert!(ie.else_body.is_some());
                        // Else body should contain a return with n * factorial(...)
                        let else_body = ie.else_body.as_ref().unwrap();
                        match &else_body.stmts[0].kind {
                            StatementKind::Return(r) => match r.value.kind.as_ref() {
                                ExpressionKind::Binary(_, BinaryOp::Mul, rhs) => {
                                    assert!(
                                        matches!(*rhs.kind, ExpressionKind::Call(_, _)),
                                        "expected recursive call"
                                    );
                                }
                                _ => panic!("expected multiplication"),
                            },
                            _ => panic!("expected return"),
                        }
                    }
                    _ => panic!("expected if/else"),
                }
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn no_params() {
        let file = parse_ok("function f() { return 0; }");
        match &file.items[0] {
            Item::FunctionDef(f) => {
                assert!(f.params.is_empty());
            }
            _ => panic!("expected function"),
        }
    }
}

mod bus {
    use super::*;

    #[test]
    fn simple() {
        let file = parse_ok("bus Point() { signal x; signal y; }");
        match &file.items[0] {
            Item::BusDef(b) => {
                assert_eq!(b.name.name, "Point");
                assert_eq!(b.body.len(), 2);
                assert!(b.params.is_empty());
            }
            _ => panic!("expected bus"),
        }
    }

    #[test]
    fn with_params() {
        let file = parse_ok("bus PointN(dim) { signal x[dim]; }");
        match &file.items[0] {
            Item::BusDef(b) => {
                assert_eq!(b.params.len(), 1);
                assert_eq!(b.params[0].name, "dim");
            }
            _ => panic!("expected bus"),
        }
    }

    #[test]
    fn nested() {
        let src = "bus Point() { signal x; signal y; } bus Line() { Point() start; Point() end; }";
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 2);
        match &file.items[1] {
            Item::BusDef(b) => {
                assert_eq!(b.name.name, "Line");
                assert!(matches!(&b.body[0], BusMember::Bus(_)));
                assert!(matches!(&b.body[1], BusMember::Bus(_)));
            }
            _ => panic!("expected bus"),
        }
    }

    #[test]
    fn with_tagged_signals() {
        let src = "bus Book() { signal {maxvalue} title[50]; signal {maxvalue} year; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::BusDef(b) => {
                match &b.body[0] {
                    BusMember::Signal(s) => {
                        assert_eq!(s.tags.len(), 1);
                        assert_eq!(s.tags[0].name, "maxvalue");
                        assert_eq!(s.names[0].dimensions.len(), 1);
                    }
                    _ => panic!("expected signal"),
                }
                match &b.body[1] {
                    BusMember::Signal(s) => {
                        assert_eq!(s.tags[0].name, "maxvalue");
                        assert!(s.names[0].dimensions.is_empty());
                    }
                    _ => panic!("expected signal"),
                }
            }
            _ => panic!("expected bus"),
        }
    }
}

mod signal_decl {
    use super::*;

    #[test]
    fn input() {
        let stmts = template_stmts("template T() { signal input a; }");
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                assert_eq!(s.kind, SignalKind::Input);
                assert_eq!(s.names[0].name.name, "a");
            }
            _ => panic!("expected signal decl"),
        }
    }

    #[test]
    fn output() {
        let stmts = template_stmts("template T() { signal output b; }");
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                assert_eq!(s.kind, SignalKind::Output);
                assert_eq!(s.names[0].name.name, "b");
            }
            _ => panic!("expected signal decl"),
        }
    }

    #[test]
    fn intermediate() {
        let stmts = template_stmts("template T() { signal c; }");
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                assert_eq!(s.kind, SignalKind::Intermediate);
                assert_eq!(s.names[0].name.name, "c");
            }
            _ => panic!("expected signal decl"),
        }
    }

    #[test]
    fn with_single_tag() {
        let stmts = template_stmts("template T() { signal input {binary} in[n]; }");
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                assert_eq!(s.tags.len(), 1);
                assert_eq!(s.tags[0].name, "binary");
            }
            _ => panic!("expected signal decl"),
        }
    }

    #[test]
    fn with_multiple_tags() {
        let stmts = template_stmts("template T() { signal input {binary, maxbit} in[n]; }");
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                assert_eq!(s.tags.len(), 2);
                assert_eq!(s.tags[0].name, "binary");
                assert_eq!(s.tags[1].name, "maxbit");
            }
            _ => panic!("expected signal decl"),
        }
    }

    #[test]
    fn array() {
        let stmts = template_stmts("template T() { signal input in[8]; }");
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                assert_eq!(s.names[0].dimensions.len(), 1);
            }
            _ => panic!("expected signal decl"),
        }
    }

    #[test]
    fn multidimensional_array() {
        let stmts = template_stmts("template T() { signal input matrix[3][4]; }");
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                assert_eq!(s.names[0].dimensions.len(), 2);
            }
            _ => panic!("expected signal decl"),
        }
    }

    #[test]
    fn init_on_decl_safe() {
        let stmts = template_stmts("template T() { signal output out <== in1 * in2; }");
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                let (op, _) = s.names[0].init.as_ref().unwrap();
                assert_eq!(*op, SignalAssignOp::SafeLeft);
            }
            _ => panic!("expected signal decl"),
        }
    }

    #[test]
    fn init_on_decl_unsafe() {
        let stmts = template_stmts("template T() { signal output out <-- in1 * in2; }");
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                let (op, _) = s.names[0].init.as_ref().unwrap();
                assert_eq!(*op, SignalAssignOp::UnsafeLeft);
            }
            _ => panic!("expected signal decl"),
        }
    }
}

mod var_decl {
    use super::*;

    #[test]
    fn simple() {
        let stmts = function_stmts("function f() { var x = 5; return x; }");
        match &stmts[0].kind {
            StatementKind::VarDecl(v) => {
                assert_eq!(v.names[0].name.name, "x");
                assert!(v.names[0].init.is_some());
            }
            _ => panic!("expected var decl"),
        }
    }

    #[test]
    fn array() {
        let stmts = function_stmts("function f() { var x[3]; return x; }");
        match &stmts[0].kind {
            StatementKind::VarDecl(v) => {
                assert_eq!(v.names[0].dimensions.len(), 1);
            }
            _ => panic!("expected var decl"),
        }
    }

    #[test]
    fn multidimensional() {
        let stmts = function_stmts("function f() { var grid[3][4]; return grid; }");
        match &stmts[0].kind {
            StatementKind::VarDecl(v) => {
                assert_eq!(v.names[0].dimensions.len(), 2);
            }
            _ => panic!("expected var decl"),
        }
    }

    #[test]
    fn multiple() {
        let stmts = function_stmts("function f() { var a, b, c; return a; }");
        match &stmts[0].kind {
            StatementKind::VarDecl(v) => {
                assert_eq!(v.names.len(), 3);
            }
            _ => panic!("expected var decl"),
        }
    }

    #[test]
    fn per_variable_init() {
        let stmts = function_stmts("function f() { var a = 1, b = 2; return a + b; }");
        match &stmts[0].kind {
            StatementKind::VarDecl(v) => {
                assert_eq!(v.names.len(), 2);
                assert!(v.names[0].init.is_some());
                assert!(v.names[1].init.is_some());
            }
            _ => panic!("expected var decl"),
        }
    }

    #[test]
    fn no_init() {
        let stmts = function_stmts("function f() { var x; return x; }");
        match &stmts[0].kind {
            StatementKind::VarDecl(v) => {
                assert!(v.names[0].init.is_none());
            }
            _ => panic!("expected var decl"),
        }
    }
}

mod operators {
    use super::*;

    // ── Signal assignment operators ────────────────────────────────

    #[test]
    fn safe_left_assign() {
        let stmts = template_stmts("template T() { signal input a; signal output b; b <== a; }");
        match &stmts[2].kind {
            StatementKind::Assignment(a) => assert_eq!(a.op, AssignOp::SafeLeft),
            other => panic!("expected assignment, got {:?}", other),
        }
    }

    #[test]
    fn safe_right_assign() {
        let stmts = template_stmts("template T() { signal input a; signal output b; a ==> b; }");
        match &stmts[2].kind {
            StatementKind::Assignment(a) => assert_eq!(a.op, AssignOp::SafeRight),
            other => panic!("expected assignment, got {:?}", other),
        }
    }

    #[test]
    fn unsafe_left_assign() {
        let stmts = template_stmts("template T() { signal input a; signal output b; b <-- a; }");
        match &stmts[2].kind {
            StatementKind::Assignment(a) => assert_eq!(a.op, AssignOp::UnsafeLeft),
            other => panic!("expected assignment, got {:?}", other),
        }
    }

    #[test]
    fn unsafe_right_assign() {
        let stmts = template_stmts("template T() { signal input a; signal output b; a --> b; }");
        match &stmts[2].kind {
            StatementKind::Assignment(a) => assert_eq!(a.op, AssignOp::UnsafeRight),
            other => panic!("expected assignment, got {:?}", other),
        }
    }

    #[test]
    fn constraint_eq() {
        let stmts = template_stmts("template T() { signal a; signal b; a === b; }");
        match &stmts[2].kind {
            StatementKind::ConstraintEq(ceq) => {
                assert!(matches!(*ceq.lhs.kind, ExpressionKind::Ident(ref name) if name == "a"));
                assert!(matches!(*ceq.rhs.kind, ExpressionKind::Ident(ref name) if name == "b"));
            }
            other => panic!("expected constraint eq, got {:?}", other),
        }
    }

    #[test]
    fn plain_eq_assign() {
        let stmts = function_stmts("function f() { var x; x = 5; return x; }");
        match &stmts[1].kind {
            StatementKind::Assignment(a) => assert_eq!(a.op, AssignOp::Eq),
            other => panic!("expected assignment, got {:?}", other),
        }
    }

    // ── Compound assignment operators ──────────────────────────────

    #[test]
    fn add_assign() {
        let stmts = function_stmts("function f() { var x = 0; x += 1; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::AddAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }

    #[test]
    fn sub_assign() {
        let stmts = function_stmts("function f() { var x = 10; x -= 1; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::SubAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }

    #[test]
    fn mul_assign() {
        let stmts = function_stmts("function f() { var x = 1; x *= 2; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::MulAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }

    #[test]
    fn pow_assign() {
        let stmts = function_stmts("function f() { var x = 2; x **= 3; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::PowAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }

    #[test]
    fn div_assign() {
        let stmts = function_stmts("function f() { var x = 10; x /= 2; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::DivAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }

    #[test]
    fn intdiv_assign() {
        let stmts = function_stmts(r"function f() { var x = 10; x \= 3; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::IntDivAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }

    #[test]
    fn mod_assign() {
        let stmts = function_stmts("function f() { var x = 10; x %= 3; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::ModAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }

    #[test]
    fn shl_assign() {
        let stmts = function_stmts("function f() { var x = 1; x <<= 3; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::ShlAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }

    #[test]
    fn shr_assign() {
        let stmts = function_stmts("function f() { var x = 8; x >>= 2; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::ShrAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }

    #[test]
    fn bitand_assign() {
        let stmts = function_stmts("function f() { var x = 7; x &= 3; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::BitAndAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }

    #[test]
    fn bitor_assign() {
        let stmts = function_stmts("function f() { var x = 4; x |= 3; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::BitOrAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }

    #[test]
    fn bitxor_assign() {
        let stmts = function_stmts("function f() { var x = 5; x ^= 3; return x; }");
        match &stmts[1].kind {
            StatementKind::CompoundAssign(c) => assert_eq!(c.op, CompoundOp::BitXorAssign),
            other => panic!("expected compound assign, got {:?}", other),
        }
    }
}

mod control_flow {
    use super::*;

    #[test]
    fn if_only() {
        let stmts = function_stmts("function f(x) { if (x > 0) { return x; } return 0; }");
        match &stmts[0].kind {
            StatementKind::IfElse(ie) => {
                assert!(ie.else_body.is_none());
            }
            _ => panic!("expected if"),
        }
    }

    #[test]
    fn if_else() {
        let stmts = function_stmts("function f(x) { if (x > 0) { return x; } else { return 0; } }");
        match &stmts[0].kind {
            StatementKind::IfElse(ie) => {
                assert!(ie.else_body.is_some());
            }
            _ => panic!("expected if/else"),
        }
    }

    #[test]
    fn nested_if_else() {
        let src = "function f(x) { if (x > 10) { return 2; } else { if (x > 0) { return 1; } else { return 0; } } }";
        let stmts = function_stmts(src);
        match &stmts[0].kind {
            StatementKind::IfElse(ie) => {
                let else_body = ie.else_body.as_ref().unwrap();
                assert!(matches!(&else_body.stmts[0].kind, StatementKind::IfElse(_)));
            }
            _ => panic!("expected if/else"),
        }
    }

    #[test]
    fn for_loop() {
        let stmts = function_stmts(
            "function f() { var y = 0; for (var i = 0; i < 100; i++) { y++; } return y; }",
        );
        match &stmts[1].kind {
            StatementKind::For(f) => {
                assert_eq!(
                    f.body.stmts.len(),
                    1,
                    "for-loop body should have 1 statement"
                );
            }
            other => panic!("expected for loop, got {:?}", other),
        }
    }

    #[test]
    fn while_loop() {
        let stmts =
            function_stmts("function f() { var i = 0; while (i < 100) { i++; } return i; }");
        match &stmts[1].kind {
            StatementKind::While(w) => {
                assert_eq!(
                    w.body.stmts.len(),
                    1,
                    "while-loop body should have 1 statement"
                );
            }
            other => panic!("expected while loop, got {:?}", other),
        }
    }

    #[test]
    fn return_value() {
        let stmts = function_stmts("function f() { return 42; }");
        match &stmts[0].kind {
            StatementKind::Return(r) => {
                assert!(matches!(*r.value.kind, ExpressionKind::Number(_)));
            }
            _ => panic!("expected return"),
        }
    }

    #[test]
    fn return_expression() {
        let stmts = function_stmts("function f(a, b) { return a + b * 2; }");
        match &stmts[0].kind {
            StatementKind::Return(r) => {
                assert!(matches!(
                    *r.value.kind,
                    ExpressionKind::Binary(_, BinaryOp::Add, _)
                ));
            }
            _ => panic!("expected return"),
        }
    }
}

mod component {
    use super::*;

    #[test]
    fn basic_instantiation() {
        let stmts = template_stmts("template T() { component c = Multiplier2(); }");
        match &stmts[0].kind {
            StatementKind::ComponentDecl(c) => {
                assert_eq!(c.names[0].name.name, "c");
                assert!(c.names[0].init.is_some());
                assert!(!c.is_parallel);
            }
            _ => panic!("expected component decl"),
        }
    }

    #[test]
    fn array_declaration() {
        let stmts = template_stmts("template T() { component ands[2]; }");
        match &stmts[0].kind {
            StatementKind::ComponentDecl(c) => {
                assert_eq!(c.names[0].dimensions.len(), 1);
                assert!(c.names[0].init.is_none());
            }
            _ => panic!("expected component decl"),
        }
    }

    #[test]
    fn parallel_instantiation() {
        let stmts = template_stmts("template T() { component c = parallel Heavy(); }");
        match &stmts[0].kind {
            StatementKind::ComponentDecl(c) => {
                assert!(c.is_parallel);
            }
            _ => panic!("expected component decl"),
        }
    }

    #[test]
    fn anonymous() {
        let stmts =
            template_stmts("template T() { signal output out; out <== Multiplier2()(a, b); }");
        match &stmts[1].kind {
            StatementKind::Assignment(a) => {
                assert!(matches!(*a.rhs.kind, ExpressionKind::AnonymousComp(_)));
            }
            _ => panic!("expected assignment"),
        }
    }

    #[test]
    fn anonymous_named_inputs() {
        let stmts = template_stmts(
            "template T() { signal output out; out <== A(n)(b <== in1, a <== in0); }",
        );
        match &stmts[1].kind {
            StatementKind::Assignment(a) => match a.rhs.kind.as_ref() {
                ExpressionKind::AnonymousComp(ac) => {
                    assert_eq!(ac.inputs.len(), 2);
                    assert!(matches!(&ac.inputs[0], AnonCompInput::Named(n, _) if n.name == "b"));
                    assert!(matches!(&ac.inputs[1], AnonCompInput::Named(n, _) if n.name == "a"));
                }
                _ => panic!("expected anonymous comp"),
            },
            _ => panic!("expected assignment"),
        }
    }

    #[test]
    fn anonymous_positional_inputs() {
        let stmts = template_stmts("template T() { signal output out; out <== A()(x, y, z); }");
        match &stmts[1].kind {
            StatementKind::Assignment(a) => match a.rhs.kind.as_ref() {
                ExpressionKind::AnonymousComp(ac) => {
                    assert_eq!(ac.inputs.len(), 3);
                    assert!(matches!(&ac.inputs[0], AnonCompInput::Positional(_)));
                }
                _ => panic!("expected anonymous comp"),
            },
            _ => panic!("expected assignment"),
        }
    }

    #[test]
    fn dot_access() {
        let stmts = template_stmts("template T() { component c = Mul(); c.a <== 5; }");
        match &stmts[1].kind {
            StatementKind::Assignment(a) => {
                assert!(matches!(*a.lhs.kind, ExpressionKind::Member(_, _)));
            }
            other => panic!("expected assignment, got {:?}", other),
        }
    }
}

mod tuple {
    use super::*;

    #[test]
    fn basic_assignment() {
        let stmts = template_stmts(
            "template T() { signal output a; signal output b; (a, b) <== SomeT()(inp); }",
        );
        match &stmts[2].kind {
            StatementKind::TupleAssign(ta) => {
                assert_eq!(ta.targets.len(), 2);
                assert!(ta.targets[0].is_some());
                assert!(ta.targets[1].is_some());
                assert_eq!(ta.op, AssignOp::SafeLeft);
            }
            other => panic!("expected tuple assign, got {:?}", other),
        }
    }

    #[test]
    fn with_underscore() {
        let stmts = template_stmts("template T() { signal output a; (_, a) <== SomeT()(inp); }");
        match &stmts[1].kind {
            StatementKind::TupleAssign(ta) => {
                assert!(ta.targets[0].is_none());
                assert!(ta.targets[1].is_some());
            }
            _ => panic!("expected tuple assign"),
        }
    }

    #[test]
    fn return_tuple_like() {
        // Tuple-like in function (multiple values via anonymous component)
        let stmts = template_stmts("template T() { signal output a; signal output b; signal output c; (a, _, c) <== Multi()(inp); }");
        match &stmts[3].kind {
            StatementKind::TupleAssign(ta) => {
                assert_eq!(ta.targets.len(), 3);
                assert!(ta.targets[0].is_some());
                assert!(ta.targets[1].is_none());
                assert!(ta.targets[2].is_some());
            }
            _ => panic!("expected tuple assign"),
        }
    }
}

mod array_access {
    use super::*;

    #[test]
    fn single_index() {
        let e = return_expr("function f() { return x[0]; }");
        assert!(matches!(*e.kind, ExpressionKind::Index(_, _)));
    }

    #[test]
    fn multi_dimensional() {
        let e = return_expr("function f() { return x[0][1]; }");
        // x[0][1] should be (x[0])[1]
        match *e.kind {
            ExpressionKind::Index(inner, _) => {
                assert!(matches!(*inner.kind, ExpressionKind::Index(_, _)));
            }
            _ => panic!("expected nested index"),
        }
    }

    #[test]
    fn computed_index() {
        let e = return_expr("function f() { return x[i + 1]; }");
        match *e.kind {
            ExpressionKind::Index(_, idx) => {
                assert!(matches!(
                    *idx.kind,
                    ExpressionKind::Binary(_, BinaryOp::Add, _)
                ));
            }
            _ => panic!("expected index with computed expr"),
        }
    }
}

mod tags {
    use super::*;

    #[test]
    fn declaration() {
        let stmts = template_stmts("template T() { signal input {binary} in[n]; }");
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                assert_eq!(s.tags.len(), 1);
                assert_eq!(s.tags[0].name, "binary");
            }
            _ => panic!("expected signal decl"),
        }
    }

    #[test]
    fn multiple_tags() {
        let stmts = template_stmts("template T() { signal output {binary, maxbit} out; }");
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                assert_eq!(s.tags.len(), 2);
            }
            _ => panic!("expected signal decl"),
        }
    }

    #[test]
    fn tag_access() {
        let stmts = template_stmts("template T() { signal output {maxbit} out; out.maxbit = n; }");
        match &stmts[1].kind {
            StatementKind::Assignment(a) => match a.lhs.kind.as_ref() {
                ExpressionKind::Member(base, field) => {
                    assert!(matches!(*base.kind, ExpressionKind::Ident(ref s) if s == "out"));
                    assert_eq!(field.name, "maxbit");
                }
                _ => panic!("expected member access"),
            },
            other => panic!("expected assignment, got {:?}", other),
        }
    }
}

mod builtins {
    use super::*;

    #[test]
    fn log_empty() {
        let stmts = template_stmts("template T() { log(); }");
        match &stmts[0].kind {
            StatementKind::Log(l) => {
                assert!(l.args.is_empty());
            }
            _ => panic!("expected log"),
        }
    }

    #[test]
    fn log_string_and_expr() {
        let stmts = template_stmts(r#"template T() { log("value:", x); }"#);
        match &stmts[0].kind {
            StatementKind::Log(l) => {
                assert_eq!(l.args.len(), 2);
                assert!(matches!(&l.args[0], LogArg::String(s) if s == "value:"));
                assert!(matches!(&l.args[1], LogArg::Expr(_)));
            }
            _ => panic!("expected log"),
        }
    }

    #[test]
    fn log_multiple_exprs() {
        let stmts = template_stmts("template T() { log(a, b, c); }");
        match &stmts[0].kind {
            StatementKind::Log(l) => {
                assert_eq!(l.args.len(), 3);
            }
            _ => panic!("expected log"),
        }
    }

    #[test]
    fn assert_expr() {
        let stmts = template_stmts("template T() { assert(x > 0); }");
        assert!(matches!(&stmts[0].kind, StatementKind::Assert(_)));
    }

    #[test]
    fn assert_complex() {
        let stmts = template_stmts("template T() { assert(x > 0 && y < 100); }");
        match &stmts[0].kind {
            StatementKind::Assert(a) => {
                assert!(matches!(
                    *a.expr.kind,
                    ExpressionKind::Binary(_, BinaryOp::And, _)
                ));
            }
            _ => panic!("expected assert"),
        }
    }
}

mod main_component {
    use super::*;

    #[test]
    fn no_public_inputs() {
        let file = parse_ok("component main = Multiplier2();");
        match &file.items[0] {
            Item::MainComponent(m) => {
                assert!(m.public_signals.is_empty());
            }
            _ => panic!("expected main component"),
        }
    }

    #[test]
    fn with_public_inputs() {
        let file = parse_ok("component main {public [in1, in2]} = Multiplier2();");
        match &file.items[0] {
            Item::MainComponent(m) => {
                assert_eq!(m.public_signals.len(), 2);
                assert_eq!(m.public_signals[0].name, "in1");
                assert_eq!(m.public_signals[1].name, "in2");
            }
            _ => panic!("expected main component"),
        }
    }

    #[test]
    fn single_public_input() {
        let file = parse_ok("component main {public [a]} = T();");
        match &file.items[0] {
            Item::MainComponent(m) => {
                assert_eq!(m.public_signals.len(), 1);
                assert_eq!(m.public_signals[0].name, "a");
            }
            _ => panic!("expected main component"),
        }
    }
}

mod increment_decrement {
    use super::*;

    #[test]
    fn increment() {
        let stmts = function_stmts("function f() { var i = 0; i++; return i; }");
        assert!(matches!(&stmts[1].kind, StatementKind::Increment(_)));
    }

    #[test]
    fn decrement() {
        let stmts = function_stmts("function f() { var i = 10; i--; return i; }");
        assert!(matches!(&stmts[1].kind, StatementKind::Decrement(_)));
    }

    #[test]
    fn in_for_loop_step() {
        let stmts = function_stmts("function f() { for (var i = 0; i < 10; i++) {} return 0; }");
        match &stmts[0].kind {
            StatementKind::For(f) => {
                assert!(matches!(&f.step.kind, StatementKind::Increment(_)));
            }
            _ => panic!("expected for loop"),
        }
    }
}

mod extern_c {
    use super::*;

    #[test]
    fn extern_custom_template() {
        let src = r#"
            pragma circom 2.2.3;
            pragma custom_templates;

            template custom extern A() {
                signal input in;
                signal output out;
            }
        "#;
        let file = parse_ok(src);
        match &file.items[2] {
            Item::TemplateDef(t) => {
                assert!(t.is_custom);
                assert!(t.is_extern);
                assert_eq!(t.name.name, "A");
            }
            _ => panic!("expected template"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OPERATOR PRECEDENCE
// ═══════════════════════════════════════════════════════════════════════

mod precedence {
    use super::*;

    #[test]
    fn mul_over_add() {
        // a + b * c => a + (b * c)
        let e = return_expr("function f() { return a + b * c; }");
        match *e.kind {
            ExpressionKind::Binary(_, BinaryOp::Add, ref rhs) => {
                assert!(matches!(
                    *rhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::Mul, _)
                ));
            }
            ref other => panic!("expected Add at top, got {:?}", other),
        }
    }

    #[test]
    fn mul_binds_tighter_than_sub() {
        // a * b - c => (a * b) - c
        let e = return_expr("function f() { return a * b - c; }");
        match *e.kind {
            ExpressionKind::Binary(ref lhs, BinaryOp::Sub, _) => {
                assert!(matches!(
                    *lhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::Mul, _)
                ));
            }
            ref other => panic!("expected Sub at top, got {:?}", other),
        }
    }

    #[test]
    fn div_same_as_mul() {
        // a / b * c => (a / b) * c  (left-to-right)
        let e = return_expr("function f() { return a / b * c; }");
        match *e.kind {
            ExpressionKind::Binary(ref lhs, BinaryOp::Mul, _) => {
                assert!(matches!(
                    *lhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::Div, _)
                ));
            }
            ref other => panic!("expected Mul at top, got {:?}", other),
        }
    }

    #[test]
    fn mod_same_as_mul() {
        // a % b + c => (a % b) + c
        let e = return_expr("function f() { return a % b + c; }");
        match *e.kind {
            ExpressionKind::Binary(ref lhs, BinaryOp::Add, _) => {
                assert!(matches!(
                    *lhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::Mod, _)
                ));
            }
            ref other => panic!("expected Add at top, got {:?}", other),
        }
    }

    #[test]
    fn intdiv_same_as_mul() {
        // a \ b + c => (a \ b) + c
        let e = return_expr(r"function f() { return a \ b + c; }");
        match *e.kind {
            ExpressionKind::Binary(ref lhs, BinaryOp::Add, _) => {
                assert!(matches!(
                    *lhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::IntDiv, _)
                ));
            }
            ref other => panic!("expected Add at top, got {:?}", other),
        }
    }

    #[test]
    fn power_right_associative() {
        // a ** b ** c => a ** (b ** c)
        let e = return_expr("function f() { return a ** b ** c; }");
        match *e.kind {
            ExpressionKind::Binary(_, BinaryOp::Pow, ref rhs) => {
                assert!(matches!(
                    *rhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::Pow, _)
                ));
            }
            ref other => panic!("expected Pow at top, got {:?}", other),
        }
    }

    #[test]
    fn power_over_mul() {
        // a * b ** c => a * (b ** c)
        let e = return_expr("function f() { return a * b ** c; }");
        match *e.kind {
            ExpressionKind::Binary(_, BinaryOp::Mul, ref rhs) => {
                assert!(matches!(
                    *rhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::Pow, _)
                ));
            }
            ref other => panic!("expected Mul at top, got {:?}", other),
        }
    }

    #[test]
    fn shift_over_relational() {
        // a < b << c => a < (b << c)
        let e = return_expr("function f() { return a < b << c; }");
        match *e.kind {
            ExpressionKind::Binary(_, BinaryOp::Lt, ref rhs) => {
                assert!(matches!(
                    *rhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::Shl, _)
                ));
            }
            ref other => panic!("expected Lt at top, got {:?}", other),
        }
    }

    #[test]
    fn shr_precedence() {
        // a >> b + c => a >> (b + c)
        let e = return_expr("function f() { return a >> b + c; }");
        match *e.kind {
            ExpressionKind::Binary(_, BinaryOp::Shr, ref rhs) => {
                assert!(matches!(
                    *rhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::Add, _)
                ));
            }
            ref other => panic!("expected Shr at top, got {:?}", other),
        }
    }

    #[test]
    fn relational_over_equality() {
        // a == b < c => a == (b < c)
        let e = return_expr("function f() { return a == b < c; }");
        match *e.kind {
            ExpressionKind::Binary(_, BinaryOp::Eq, ref rhs) => {
                assert!(matches!(
                    *rhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::Lt, _)
                ));
            }
            ref other => panic!("expected Eq at top, got {:?}", other),
        }
    }

    #[test]
    fn equality_ne() {
        let e = return_expr("function f() { return a != b; }");
        assert!(matches!(
            *e.kind,
            ExpressionKind::Binary(_, BinaryOp::Ne, _)
        ));
    }

    #[test]
    fn relational_le() {
        let e = return_expr("function f() { return a <= b; }");
        assert!(matches!(
            *e.kind,
            ExpressionKind::Binary(_, BinaryOp::Le, _)
        ));
    }

    #[test]
    fn relational_ge() {
        let e = return_expr("function f() { return a >= b; }");
        assert!(matches!(
            *e.kind,
            ExpressionKind::Binary(_, BinaryOp::Ge, _)
        ));
    }

    #[test]
    fn relational_gt() {
        let e = return_expr("function f() { return a > b; }");
        assert!(matches!(
            *e.kind,
            ExpressionKind::Binary(_, BinaryOp::Gt, _)
        ));
    }

    #[test]
    fn bitand_over_bitor() {
        // a | b & c => a | (b & c)
        let e = return_expr("function f() { return a | b & c; }");
        match *e.kind {
            ExpressionKind::Binary(_, BinaryOp::BitOr, ref rhs) => {
                assert!(matches!(
                    *rhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::BitAnd, _)
                ));
            }
            ref other => panic!("expected BitOr at top, got {:?}", other),
        }
    }

    #[test]
    fn bitxor_between_bitand_and_bitor() {
        // a | b ^ c => a | (b ^ c)
        let e = return_expr("function f() { return a | b ^ c; }");
        match *e.kind {
            ExpressionKind::Binary(_, BinaryOp::BitOr, ref rhs) => {
                assert!(matches!(
                    *rhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::BitXor, _)
                ));
            }
            ref other => panic!("expected BitOr at top, got {:?}", other),
        }
    }

    #[test]
    fn and_over_or() {
        // a && b || c => (a && b) || c
        let e = return_expr("function f() { return a && b || c; }");
        match *e.kind {
            ExpressionKind::Binary(ref lhs, BinaryOp::Or, _) => {
                assert!(matches!(
                    *lhs.kind,
                    ExpressionKind::Binary(_, BinaryOp::And, _)
                ));
            }
            ref other => panic!("expected Or at top, got {:?}", other),
        }
    }

    #[test]
    fn ternary_lower_than_or() {
        // a || b ? c : d => (a || b) ? c : d
        let e = return_expr("function f() { return a || b ? c : d; }");
        match *e.kind {
            ExpressionKind::Ternary(ref cond, _, _) => {
                assert!(matches!(
                    *cond.kind,
                    ExpressionKind::Binary(_, BinaryOp::Or, _)
                ));
            }
            ref other => panic!("expected Ternary at top, got {:?}", other),
        }
    }

    #[test]
    fn ternary_right_associative() {
        // a ? b : c ? d : e => a ? b : (c ? d : e)
        let e = return_expr("function f() { return a ? b : c ? d : e; }");
        match *e.kind {
            ExpressionKind::Ternary(_, _, ref else_expr) => {
                assert!(
                    matches!(*else_expr.kind, ExpressionKind::Ternary(_, _, _)),
                    "expected nested ternary in else branch"
                );
            }
            ref other => panic!("expected Ternary at top, got {:?}", other),
        }
    }

    #[test]
    fn parens_override_precedence() {
        // (a + b) * c => Mul(Paren(Add(a, b)), c)
        let e = return_expr("function f() { return (a + b) * c; }");
        match *e.kind {
            ExpressionKind::Binary(ref lhs, BinaryOp::Mul, _) => match *lhs.kind {
                ExpressionKind::Paren(ref inner) => {
                    assert!(matches!(
                        *inner.kind,
                        ExpressionKind::Binary(_, BinaryOp::Add, _)
                    ));
                }
                ref other => panic!("expected Paren, got {:?}", other),
            },
            ref other => panic!("expected Mul at top, got {:?}", other),
        }
    }

    // ── Unary operator precedence ──────────────────────────────────

    #[test]
    fn unary_neg() {
        let e = return_expr("function f() { return -x; }");
        assert!(matches!(*e.kind, ExpressionKind::Unary(UnaryOp::Neg, _)));
    }

    #[test]
    fn unary_not() {
        let e = return_expr("function f() { return !x; }");
        assert!(matches!(*e.kind, ExpressionKind::Unary(UnaryOp::Not, _)));
    }

    #[test]
    fn unary_bitnot() {
        let e = return_expr("function f() { return ~x; }");
        assert!(matches!(*e.kind, ExpressionKind::Unary(UnaryOp::BitNot, _)));
    }

    #[test]
    fn unary_over_binary() {
        // -a + b => (-a) + b
        let e = return_expr("function f() { return -a + b; }");
        match *e.kind {
            ExpressionKind::Binary(ref lhs, BinaryOp::Add, _) => {
                assert!(matches!(*lhs.kind, ExpressionKind::Unary(UnaryOp::Neg, _)));
            }
            ref other => panic!("expected Add at top, got {:?}", other),
        }
    }

    #[test]
    fn double_negation() {
        // --x should not parse as decrement in expression context; we wrap in return
        // The parser might handle this differently; let's test - -x with a space
        let e = return_expr("function f() { return -(-x); }");
        match *e.kind {
            ExpressionKind::Unary(UnaryOp::Neg, ref inner) => match *inner.kind {
                ExpressionKind::Paren(ref p) => {
                    assert!(matches!(*p.kind, ExpressionKind::Unary(UnaryOp::Neg, _)));
                }
                _ => panic!("expected Paren"),
            },
            ref other => panic!("expected Neg, got {:?}", other),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// EDGE CASES
// ═══════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn deeply_nested_parens() {
        // Build (((((((x)))))))
        let depth = 50;
        let open: String = "(".repeat(depth);
        let close: String = ")".repeat(depth);
        let src = format!("function f() {{ return {}x{}; }}", open, close);
        let mut e = return_expr(&src);
        // Unwrap all Paren layers to reach the inner expression
        for _ in 0..depth {
            e = match *e.kind {
                ExpressionKind::Paren(ref inner) => inner.clone(),
                _ => panic!("expected Paren wrapper at this depth"),
            };
        }
        assert!(
            matches!(*e.kind, ExpressionKind::Ident(ref name) if name == "x"),
            "innermost expression should be identifier 'x'"
        );
    }

    #[test]
    fn deeply_nested_binary() {
        // a + a + a + ... (100 terms, left-associative)
        let terms = vec!["a"; 100];
        let expr = terms.join(" + ");
        let src = format!("function f() {{ return {}; }}", expr);
        let e = return_expr(&src);
        // Walk the left spine: for left-associative addition,
        // the RHS at each level should be a leaf `a`, and the LHS
        // should be another Binary(Add) — except at the deepest level.
        let mut current = e;
        for _ in 0..99 {
            match *current.kind {
                ExpressionKind::Binary(ref lhs, BinaryOp::Add, ref rhs) => {
                    assert!(
                        matches!(*rhs.kind, ExpressionKind::Ident(ref name) if name == "a"),
                        "RHS should be leaf 'a'"
                    );
                    current = lhs.clone();
                }
                _ => panic!("expected Binary(Add) at each level of left-associative tree"),
            }
        }
        // The deepest node should be the leftmost leaf `a`
        assert!(
            matches!(*current.kind, ExpressionKind::Ident(ref name) if name == "a"),
            "leftmost leaf should be 'a'"
        );
    }

    #[test]
    fn large_signal_array() {
        let src = "template T() { signal input x[1000]; }";
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[0].kind {
                StatementKind::SignalDecl(s) => {
                    assert_eq!(s.names[0].dimensions.len(), 1);
                }
                _ => panic!("expected signal decl"),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn unicode_in_line_comment() {
        let src = "// こんにちは 🎉\nfunction f() { return 0; }";
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 1);
    }

    #[test]
    fn unicode_in_block_comment() {
        let src = "/* 日本語のコメント */ function f() { return 0; }";
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 1);
    }

    #[test]
    fn multiple_items_in_file() {
        let src = r#"
            pragma circom 2.0.0;
            include "circomlib/poseidon.circom";

            template Multiplier2() {
                signal input a;
                signal input b;
                signal output c;
                c <== a * b;
            }

            function nbits(a) {
                var n = 1;
                var r = 0;
                while (n - 1 < a) { r++; n *= 2; }
                return r;
            }

            component main {public [a]} = Multiplier2();
        "#;
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 5); // pragma, include, template, function, main
    }

    #[test]
    fn number_literals() {
        let e = return_expr("function f() { return 0; }");
        match *e.kind {
            ExpressionKind::Number(ref n) => assert_eq!(n, "0"),
            _ => panic!("expected number"),
        }
    }

    #[test]
    fn large_number_literal() {
        let e = return_expr("function f() { return 21888242871839275222246405745257275088548364400416034343698204186575808495617; }");
        match *e.kind {
            ExpressionKind::Number(ref n) => {
                assert!(n.starts_with("21888242871839275222246405"));
            }
            _ => panic!("expected number"),
        }
    }

    #[test]
    fn array_literal() {
        let e = return_expr("function f() { return [1, 2, 3]; }");
        match *e.kind {
            ExpressionKind::ArrayLit(ref elems) => assert_eq!(elems.len(), 3),
            _ => panic!("expected array literal"),
        }
    }

    #[test]
    fn empty_array_literal() {
        let e = return_expr("function f() { return []; }");
        match *e.kind {
            ExpressionKind::ArrayLit(ref elems) => assert!(elems.is_empty()),
            _ => panic!("expected array literal"),
        }
    }

    #[test]
    fn nested_array_literal() {
        let e = return_expr("function f() { return [[1, 2], [3, 4]]; }");
        match *e.kind {
            ExpressionKind::ArrayLit(ref elems) => {
                assert_eq!(elems.len(), 2);
                assert!(matches!(*elems[0].kind, ExpressionKind::ArrayLit(_)));
            }
            _ => panic!("expected array literal"),
        }
    }

    #[test]
    fn member_access_chain() {
        let e = return_expr("function f() { return a.b.c; }");
        // a.b.c => (a.b).c
        match *e.kind {
            ExpressionKind::Member(ref inner, ref field) => {
                assert_eq!(field.name, "c");
                assert!(matches!(*inner.kind, ExpressionKind::Member(_, _)));
            }
            _ => panic!("expected member access"),
        }
    }

    #[test]
    fn index_then_member() {
        let e = return_expr("function f() { return a[0].b; }");
        match *e.kind {
            ExpressionKind::Member(ref inner, ref field) => {
                assert_eq!(field.name, "b");
                assert!(matches!(*inner.kind, ExpressionKind::Index(_, _)));
            }
            _ => panic!("expected member"),
        }
    }

    #[test]
    fn function_call_with_no_args() {
        let e = return_expr("function f() { return g(); }");
        match *e.kind {
            ExpressionKind::Call(_, ref args) => assert!(args.is_empty()),
            _ => panic!("expected call"),
        }
    }

    #[test]
    fn function_call_with_multiple_args() {
        let e = return_expr("function f() { return g(a, b, c); }");
        match *e.kind {
            ExpressionKind::Call(_, ref args) => assert_eq!(args.len(), 3),
            _ => panic!("expected call"),
        }
    }

    #[test]
    fn block_statement() {
        let stmts = function_stmts("function f() { { var x = 1; } return 0; }");
        assert!(matches!(&stmts[0].kind, StatementKind::Block(_)));
    }

    #[test]
    fn for_loop_with_decrement() {
        let stmts = function_stmts("function f() { for (var i = 10; i > 0; i--) {} return 0; }");
        match &stmts[0].kind {
            StatementKind::For(f) => {
                assert!(matches!(&f.step.kind, StatementKind::Decrement(_)));
            }
            _ => panic!("expected for loop"),
        }
    }

    #[test]
    fn for_loop_with_compound_step() {
        let stmts =
            function_stmts("function f() { for (var i = 0; i < 100; i += 2) {} return 0; }");
        match &stmts[0].kind {
            StatementKind::For(f) => {
                assert!(matches!(&f.step.kind, StatementKind::CompoundAssign(_)));
            }
            _ => panic!("expected for loop"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// ERROR RECOVERY
// ═══════════════════════════════════════════════════════════════════════

mod error_recovery {
    use super::*;

    #[test]
    fn missing_semicolon_continues() {
        let (file, errors) = parse_with_errors("template T() { signal input a signal output b; }");
        assert!(!errors.is_empty(), "should report missing semicolon");
        assert_eq!(file.items.len(), 1, "template should still be parsed");
    }

    #[test]
    fn unclosed_brace_reports_error() {
        let (_, errors) = parse_with_errors("template T() { signal input a; ");
        assert!(!errors.is_empty(), "should report unclosed brace");
    }

    #[test]
    fn multiple_errors_reported() {
        let (_, errors) = parse_with_errors("template T() { signal a signal b signal c; }");
        assert_eq!(
            errors.len(),
            2,
            "should report exactly 2 errors for 2 missing semicolons"
        );
    }

    #[test]
    fn invalid_at_top_level() {
        let (_, errors) = parse_with_errors("+ + +");
        assert!(!errors.is_empty(), "should report error for invalid input");
    }

    #[test]
    fn recovery_after_bad_statement() {
        // Bad statement followed by good one
        let (file, errors) = parse_with_errors("function f() { ??? ; return 0; }");
        assert!(!errors.is_empty());
        // The function should still be parsed
        assert_eq!(file.items.len(), 1);
    }

    #[test]
    fn missing_closing_paren() {
        let (_, errors) = parse_with_errors("function f() { return (a + b; }");
        assert!(!errors.is_empty(), "should report missing closing paren");
    }

    #[test]
    fn missing_semicolons_all_reported() {
        // Multiple missing semicolons — parser should continue and report all
        // 3 missing semicolons: after `a`, after `b`, and after `signal output c`
        let (file, errors) = parse_with_errors(
            "template T() { signal input a signal input b signal output c c <== a; }",
        );
        assert_eq!(errors.len(), 3, "expected 3 errors, got {}", errors.len());
        assert_eq!(file.items.len(), 1);
    }

    #[test]
    fn multiple_pragmas_error() {
        let (_, errors) = parse_with_errors("pragma circom 2.0.0; pragma circom 2.1.0;");
        assert!(
            !errors.is_empty(),
            "should report error for multiple pragmas"
        );
    }

    #[test]
    #[ignore = "parser allows keywords as identifiers — #83"]
    fn signal_named_as_keyword() {
        let (_, errors) = parse_with_errors("template T() { signal input template; }");
        assert!(!errors.is_empty(), "should reject keyword as signal name");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// VERSION-SPECIFIC FEATURES
// ═══════════════════════════════════════════════════════════════════════

mod version_specific {
    use super::*;

    #[test]
    fn tags_parse_with_2_1_0() {
        let src = r#"
            pragma circom 2.1.0;
            template T() {
                signal input {binary} in[8];
                signal output {maxbit} out;
            }
        "#;
        let file = parse_ok(src);
        match &file.items[1] {
            Item::TemplateDef(t) => {
                match &t.body.stmts[0].kind {
                    StatementKind::SignalDecl(s) => {
                        assert_eq!(s.tags.len(), 1);
                        assert_eq!(s.tags[0].name, "binary");
                    }
                    _ => panic!("expected signal decl"),
                }
                match &t.body.stmts[1].kind {
                    StatementKind::SignalDecl(s) => {
                        assert_eq!(s.tags.len(), 1);
                        assert_eq!(s.tags[0].name, "maxbit");
                    }
                    _ => panic!("expected signal decl"),
                }
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn buses_parse_with_2_2_0() {
        let src = r#"
            pragma circom 2.2.0;
            bus Point() { signal x; signal y; }
            template T() { signal input a; }
        "#;
        let file = parse_ok(src);
        assert!(matches!(&file.items[1], Item::BusDef(_)));
    }

    #[test]
    fn extern_c_parse_with_2_2_3() {
        let src = r#"
            pragma circom 2.2.3;
            pragma custom_templates;
            template custom extern SHA256() {
                signal input in[512];
                signal output out[256];
            }
        "#;
        let file = parse_ok(src);
        match &file.items[2] {
            Item::TemplateDef(t) => {
                assert!(t.is_custom);
                assert!(t.is_extern);
                assert_eq!(t.name.name, "SHA256");
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn full_v2_2_3_file() {
        let src = r#"
            pragma circom 2.2.3;
            pragma custom_templates;

            bus MyBus() {
                signal x;
                signal y;
            }

            template custom extern FastHash() {
                signal input in;
                signal output out;
            }

            template T(n) {
                signal input {binary} in[n];
                signal output out;

                component hasher = FastHash();
                hasher.in <== in[0];
                out <== hasher.out;
            }

            component main {public [in]} = T(8);
        "#;
        let file = parse_ok(src);
        // pragma, custom_templates pragma, bus, extern template, template, main
        assert_eq!(file.items.len(), 6);
    }

    // Negative tests: features used with older pragma versions should produce errors
    #[test]
    fn tags_rejected_before_2_1_0() {
        let src = r#"
            pragma circom 2.0.0;
            template T() {
                signal input {binary} in[8];
            }
        "#;
        let (_, errors) = parse_with_errors(src);
        assert!(!errors.is_empty(), "tags should be rejected before 2.1.0");
    }

    #[test]
    fn buses_rejected_before_2_2_0() {
        let src = r#"
            pragma circom 2.1.0;
            bus Point() { signal x; signal y; }
        "#;
        let (_, errors) = parse_with_errors(src);
        assert!(!errors.is_empty(), "buses should be rejected before 2.2.0");
    }

    #[test]
    #[ignore = "extern version-gating not yet implemented — #110"]
    fn extern_rejected_before_2_2_3() {
        let src = r#"
            pragma circom 2.2.0;
            pragma custom_templates;
            template custom extern SHA256() {
                signal input in[512];
                signal output out[256];
            }
        "#;
        let (_, errors) = parse_with_errors(src);
        assert!(
            !errors.is_empty(),
            "extern templates should be rejected before 2.2.3"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// COMPLEX INTEGRATION TESTS
// ═══════════════════════════════════════════════════════════════════════

mod integration {
    use super::*;

    #[test]
    fn full_multiplier_circuit() {
        let src = r#"
            pragma circom 2.0.0;

            template Multiplier2() {
                signal input a;
                signal input b;
                signal output c;

                c <== a * b;
            }

            component main {public [a]} = Multiplier2();
        "#;
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 3);
    }

    #[test]
    fn bits2num_with_tags() {
        let src = r#"
            pragma circom 2.1.0;

            template Bits2Num(n) {
                signal input {binary} in[n];
                signal output {maxbit} out;

                var lc1 = 0;
                var e2 = 1;
                for (var i = 0; i < n; i++) {
                    lc1 += in[i] * e2;
                    e2 = e2 + e2;
                }

                out.maxbit = n;
                lc1 ==> out;
            }
        "#;
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 2);

        // Verify tags were actually parsed on the signal declarations
        let stmts = match &file.items[1] {
            Item::TemplateDef(t) => &t.body.stmts,
            _ => panic!("expected template"),
        };
        // First statement: signal input {binary} in[n]
        match &stmts[0].kind {
            StatementKind::SignalDecl(s) => {
                assert_eq!(s.tags.len(), 1, "input signal should have one tag");
                assert_eq!(s.tags[0].name, "binary");
            }
            _ => panic!("expected signal decl"),
        }
        // Second statement: signal output {maxbit} out
        match &stmts[1].kind {
            StatementKind::SignalDecl(s) => {
                assert_eq!(s.tags.len(), 1, "output signal should have one tag");
                assert_eq!(s.tags[0].name, "maxbit");
            }
            _ => panic!("expected signal decl"),
        }
    }

    #[test]
    fn multiple_templates_and_functions() {
        let src = r#"
            pragma circom 2.0.0;

            function nbits(a) {
                var n = 1;
                var r = 0;
                while (n - 1 < a) {
                    r++;
                    n *= 2;
                }
                return r;
            }

            template IsZero() {
                signal input in;
                signal output out;
                signal inv;
                inv <-- in != 0 ? 1 / in : 0;
                out <== -in * inv + 1;
                in * out === 0;
            }

            template LessThan(n) {
                assert(n <= 252);
                signal input in[2];
                signal output out;

                component n2b = Num2Bits(n + 1);
                n2b.in <== in[0] + (1 << n) - in[1];
                out <== 1 - n2b.out[n];
            }

            component main {public [in]} = LessThan(32);
        "#;
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 5); // pragma + function + 2 templates + main
    }

    #[test]
    fn component_wiring_pattern() {
        let src = r#"
            template T() {
                signal input a;
                signal input b;
                signal output out;

                component mul = Multiplier2();
                mul.a <== a;
                mul.b <== b;

                component add = Adder();
                add.a <== mul.c;
                add.b <== 1;

                out <== add.c;
            }
        "#;
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => {
                // 3 signals + 2 components + 5 assignments = 10 statements
                assert_eq!(t.body.stmts.len(), 10);
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn complex_for_loop_with_arrays() {
        let src = r#"
            template Sum(n) {
                signal input in[n];
                signal output out;

                var total = 0;
                for (var i = 0; i < n; i++) {
                    total += in[i];
                }
                out <== total;
            }
        "#;
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 1);
    }

    #[test]
    fn anonymous_comp_in_tuple() {
        let src = r#"
            template T() {
                signal output a;
                signal output b;
                (a, b) <== SplitTemplate()(combined_input);
            }
        "#;
        let file = parse_ok(src);
        match &file.items[0] {
            Item::TemplateDef(t) => match &t.body.stmts[2].kind {
                StatementKind::TupleAssign(ta) => {
                    assert_eq!(ta.targets.len(), 2);
                }
                other => panic!("expected tuple assign, got {:?}", other),
            },
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn bus_instance_in_template() {
        let src = r#"
            bus Point() { signal x; signal y; }
            template T() {
                signal input Point() p;
            }
        "#;
        let file = parse_ok(src);
        assert_eq!(file.items.len(), 2);
    }

    #[test]
    fn nested_function_calls() {
        let e = return_expr("function f() { return g(h(x), k(y, z)); }");
        match *e.kind {
            ExpressionKind::Call(_, ref args) => {
                assert_eq!(args.len(), 2);
                assert!(matches!(*args[0].kind, ExpressionKind::Call(_, _)));
                assert!(matches!(*args[1].kind, ExpressionKind::Call(_, _)));
            }
            _ => panic!("expected call"),
        }
    }

    #[test]
    fn chained_operations() {
        // (a + b) * c ** 2 - d / e => Sub at top, Mul on LHS, Div on RHS
        let e = return_expr("function f() { return (a + b) * c ** 2 - d / e; }");
        match *e.kind {
            ExpressionKind::Binary(ref lhs, BinaryOp::Sub, ref rhs) => {
                assert!(
                    matches!(*lhs.kind, ExpressionKind::Binary(_, BinaryOp::Mul, _)),
                    "LHS of Sub should be Mul"
                );
                assert!(
                    matches!(*rhs.kind, ExpressionKind::Binary(_, BinaryOp::Div, _)),
                    "RHS of Sub should be Div"
                );
            }
            ref other => panic!("expected Sub at top, got {:?}", other),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// ALL BINARY OPERATORS
// ═══════════════════════════════════════════════════════════════════════

mod all_binary_ops {
    use super::*;

    macro_rules! test_binop {
        ($name:ident, $op_str:expr, $expected:expr) => {
            #[test]
            fn $name() {
                let src = format!("function f() {{ return a {} b; }}", $op_str);
                let e = return_expr(&src);
                match *e.kind {
                    ExpressionKind::Binary(_, ref op, _) => assert_eq!(*op, $expected),
                    ref other => panic!("expected binary op, got {:?}", other),
                }
            }
        };
    }

    test_binop!(add, "+", BinaryOp::Add);
    test_binop!(sub, "-", BinaryOp::Sub);
    test_binop!(mul, "*", BinaryOp::Mul);
    test_binop!(div, "/", BinaryOp::Div);
    test_binop!(intdiv, r"\", BinaryOp::IntDiv);
    test_binop!(modulo, "%", BinaryOp::Mod);
    test_binop!(pow, "**", BinaryOp::Pow);
    test_binop!(shl, "<<", BinaryOp::Shl);
    test_binop!(shr, ">>", BinaryOp::Shr);
    test_binop!(bitand, "&", BinaryOp::BitAnd);
    test_binop!(bitor, "|", BinaryOp::BitOr);
    test_binop!(bitxor, "^", BinaryOp::BitXor);
    test_binop!(and, "&&", BinaryOp::And);
    test_binop!(or, "||", BinaryOp::Or);
    test_binop!(eq, "==", BinaryOp::Eq);
    test_binop!(ne, "!=", BinaryOp::Ne);
    test_binop!(lt, "<", BinaryOp::Lt);
    test_binop!(gt, ">", BinaryOp::Gt);
    test_binop!(le, "<=", BinaryOp::Le);
    test_binop!(ge, ">=", BinaryOp::Ge);
}
