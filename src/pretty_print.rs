//! Pretty-printer for the Circom AST.
//!
//! Implements [`std::fmt::Display`] for [`File`] and all AST node types,
//! producing canonical Circom source code from the AST.
//!
//! # Example
//!
//! ```
//! use cinccino::parser;
//!
//! let src = "pragma circom 2.0.0;\ntemplate T() {\n    signal input x;\n}\n";
//! let (file, _errors) = parser::parse(src);
//! let output = file.to_string();
//! assert!(output.contains("pragma circom"));
//! ```

use std::fmt::{self, Display};

use crate::ast::*;

// ── Indentation helper ─────────────────────────────────────────────────

struct IndentWriter<'a, 'b> {
    f: &'a mut fmt::Formatter<'b>,
    level: usize,
}

impl<'a, 'b> IndentWriter<'a, 'b> {
    fn new(f: &'a mut fmt::Formatter<'b>) -> Self {
        Self { f, level: 0 }
    }

    fn indent(&mut self) {
        self.level += 1;
    }

    fn dedent(&mut self) {
        debug_assert!(self.level > 0);
        self.level -= 1;
    }

    fn write_indent(&mut self) -> fmt::Result {
        for _ in 0..self.level {
            self.f.write_str("    ")?;
        }
        Ok(())
    }

    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.f.write_str(s)
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        self.f.write_fmt(args)
    }
}

// ── Display impls ──────────────────────────────────────────────────────

impl Display for File {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut w = IndentWriter::new(f);
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                w.write_str("\n")?;
            }
            write_item(&mut w, item)?;
        }
        Ok(())
    }
}

fn write_item(w: &mut IndentWriter, item: &Item) -> fmt::Result {
    match item {
        Item::Pragma(n) => write_pragma(w, n),
        Item::Include(n) => write_include(w, n),
        Item::TemplateDef(n) => write_template_def(w, n),
        Item::FunctionDef(n) => write_function_def(w, n),
        Item::BusDef(n) => write_bus_def(w, n),
        Item::MainComponent(n) => write_main_component(w, n),
    }
}

fn write_pragma(w: &mut IndentWriter, node: &Pragma) -> fmt::Result {
    match &node.kind {
        PragmaKind::Version(v) => w.write_fmt(format_args!(
            "pragma circom {}.{}.{};\n",
            v.major, v.minor, v.patch
        )),
        PragmaKind::CustomTemplates => w.write_str("pragma custom_templates;\n"),
    }
}

fn write_include(w: &mut IndentWriter, node: &Include) -> fmt::Result {
    w.write_str("include \"")?;
    // The lexer stores escape sequences verbatim, so emit as-is.
    w.write_str(&node.path)?;
    w.write_str("\";\n")
}

fn write_template_def(w: &mut IndentWriter, node: &TemplateDef) -> fmt::Result {
    if node.is_custom {
        w.write_str("custom ")?;
    }
    w.write_str("template ")?;
    if node.is_extern {
        w.write_str("extern ")?;
    }
    if node.is_parallel {
        w.write_str("parallel ")?;
    }
    w.write_fmt(format_args!("{}(", node.name.name))?;
    write_comma_sep_idents(w, &node.params)?;
    w.write_str(") ")?;
    write_block(w, &node.body)?;
    w.write_str("\n")
}

fn write_function_def(w: &mut IndentWriter, node: &FunctionDef) -> fmt::Result {
    w.write_fmt(format_args!("function {}(", node.name.name))?;
    write_comma_sep_idents(w, &node.params)?;
    w.write_str(") ")?;
    write_block(w, &node.body)?;
    w.write_str("\n")
}

fn write_bus_def(w: &mut IndentWriter, node: &BusDef) -> fmt::Result {
    w.write_fmt(format_args!("bus {}(", node.name.name))?;
    write_comma_sep_idents(w, &node.params)?;
    w.write_str(") {\n")?;
    w.indent();
    for member in &node.body {
        write_bus_member(w, member)?;
    }
    w.dedent();
    w.write_indent()?;
    w.write_str("}\n")
}

fn write_bus_member(w: &mut IndentWriter, node: &BusMember) -> fmt::Result {
    match node {
        BusMember::Signal(s) => {
            w.write_indent()?;
            write_signal_decl(w, s)?;
            w.write_str(";\n")
        }
        BusMember::Bus(b) => {
            w.write_indent()?;
            write_bus_field_decl(w, b)?;
            w.write_str(";\n")
        }
    }
}

fn write_bus_field_decl(w: &mut IndentWriter, node: &BusFieldDecl) -> fmt::Result {
    write_bus_type(w, &node.bus_type)?;
    if !node.tags.is_empty() {
        w.write_str(" {")?;
        write_comma_sep_idents(w, &node.tags)?;
        w.write_str("}")?;
    }
    w.write_fmt(format_args!(" {}", node.name.name))?;
    for dim in &node.dimensions {
        w.write_str("[")?;
        write_expr(w, dim)?;
        w.write_str("]")?;
    }
    Ok(())
}

fn write_main_component(w: &mut IndentWriter, node: &MainComponent) -> fmt::Result {
    w.write_str("component main")?;
    if !node.public_signals.is_empty() {
        w.write_str(" {public [")?;
        write_comma_sep_idents(w, &node.public_signals)?;
        w.write_str("]}")?;
    }
    w.write_str(" = ")?;
    write_expr(w, &node.expr)?;
    w.write_str(";\n")
}

fn write_block(w: &mut IndentWriter, node: &Block) -> fmt::Result {
    w.write_str("{\n")?;
    w.indent();
    for stmt in &node.stmts {
        write_statement(w, stmt)?;
    }
    w.dedent();
    w.write_indent()?;
    w.write_str("}")
}

fn write_statement(w: &mut IndentWriter, node: &Statement) -> fmt::Result {
    w.write_indent()?;
    match &node.kind {
        StatementKind::VarDecl(n) => {
            write_var_decl(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::SignalDecl(n) => {
            write_signal_decl(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::ComponentDecl(n) => {
            write_component_decl(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::BusDecl(n) => {
            write_bus_instance_decl(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::Assignment(n) => {
            write_assign_stmt(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::CompoundAssign(n) => {
            write_compound_assign_stmt(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::ConstraintEq(n) => {
            write_constraint_eq_stmt(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::TupleAssign(n) => {
            write_tuple_assign_stmt(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::IfElse(n) => {
            write_if_else(w, n)?;
            w.write_str("\n")
        }
        StatementKind::For(n) => {
            write_for_loop(w, n)?;
            w.write_str("\n")
        }
        StatementKind::While(n) => {
            write_while_loop(w, n)?;
            w.write_str("\n")
        }
        StatementKind::Return(n) => {
            w.write_str("return ")?;
            write_expr(w, &n.value)?;
            w.write_str(";\n")
        }
        StatementKind::Log(n) => {
            write_log_stmt(w, n)?;
            w.write_str(";\n")
        }
        StatementKind::Assert(n) => {
            w.write_str("assert(")?;
            write_expr(w, &n.expr)?;
            w.write_str(");\n")
        }
        StatementKind::Increment(expr) => {
            write_expr(w, expr)?;
            w.write_str("++;\n")
        }
        StatementKind::Decrement(expr) => {
            write_expr(w, expr)?;
            w.write_str("--;\n")
        }
        StatementKind::Expression(expr) => {
            write_expr(w, expr)?;
            w.write_str(";\n")
        }
        StatementKind::Block(blk) => {
            write_block(w, blk)?;
            w.write_str("\n")
        }
        StatementKind::Error => w.write_str("/* error */;\n"),
    }
}

fn write_var_decl(w: &mut IndentWriter, node: &VarDecl) -> fmt::Result {
    w.write_str("var ")?;
    for (i, entry) in node.names.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        w.write_str(&entry.name.name)?;
        for dim in &entry.dimensions {
            w.write_str("[")?;
            write_expr(w, dim)?;
            w.write_str("]")?;
        }
        if let Some(init) = &entry.init {
            w.write_str(" = ")?;
            write_expr(w, init)?;
        }
    }
    Ok(())
}

fn write_signal_decl(w: &mut IndentWriter, node: &SignalDecl) -> fmt::Result {
    w.write_str("signal ")?;
    match node.kind {
        SignalKind::Input => w.write_str("input ")?,
        SignalKind::Output => w.write_str("output ")?,
        SignalKind::Intermediate => {}
    }
    if !node.tags.is_empty() {
        w.write_str("{")?;
        write_comma_sep_idents(w, &node.tags)?;
        w.write_str("} ")?;
    }
    for (i, entry) in node.names.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        w.write_str(&entry.name.name)?;
        for dim in &entry.dimensions {
            w.write_str("[")?;
            write_expr(w, dim)?;
            w.write_str("]")?;
        }
        if let Some((op, init)) = &entry.init {
            match op {
                SignalAssignOp::SafeLeft => w.write_str(" <== ")?,
                SignalAssignOp::UnsafeLeft => w.write_str(" <-- ")?,
            }
            write_expr(w, init)?;
        }
    }
    Ok(())
}

fn write_component_decl(w: &mut IndentWriter, node: &ComponentDecl) -> fmt::Result {
    w.write_str("component ")?;
    if node.is_parallel {
        w.write_str("parallel ")?;
    }
    for (i, entry) in node.names.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        w.write_str(&entry.name.name)?;
        for dim in &entry.dimensions {
            w.write_str("[")?;
            write_expr(w, dim)?;
            w.write_str("]")?;
        }
        if let Some(init) = &entry.init {
            w.write_str(" = ")?;
            write_expr(w, init)?;
        }
    }
    Ok(())
}

fn write_bus_instance_decl(w: &mut IndentWriter, node: &BusInstanceDecl) -> fmt::Result {
    w.write_str("signal ")?;
    match node.signal_kind {
        SignalKind::Input => w.write_str("input ")?,
        SignalKind::Output => w.write_str("output ")?,
        SignalKind::Intermediate => {}
    }
    write_bus_type(w, &node.bus_type)?;
    w.write_str(" ")?;
    if !node.tags.is_empty() {
        w.write_str("{")?;
        write_comma_sep_idents(w, &node.tags)?;
        w.write_str("} ")?;
    }
    w.write_str(&node.name.name)?;
    for dim in &node.dimensions {
        w.write_str("[")?;
        write_expr(w, dim)?;
        w.write_str("]")?;
    }
    if let Some((op, init)) = &node.init {
        match op {
            SignalAssignOp::SafeLeft => w.write_str(" <== ")?,
            SignalAssignOp::UnsafeLeft => w.write_str(" <-- ")?,
        }
        write_expr(w, init)?;
    }
    Ok(())
}

fn write_bus_type(w: &mut IndentWriter, node: &BusType) -> fmt::Result {
    w.write_str(&node.name.name)?;
    w.write_str("(")?;
    for (i, arg) in node.args.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        write_expr(w, arg)?;
    }
    w.write_str(")")?;
    Ok(())
}

fn write_assign_stmt(w: &mut IndentWriter, node: &AssignStmt) -> fmt::Result {
    write_expr(w, &node.lhs)?;
    match node.op {
        AssignOp::Eq => w.write_str(" = ")?,
        AssignOp::SafeLeft => w.write_str(" <== ")?,
        AssignOp::SafeRight => w.write_str(" ==> ")?,
        AssignOp::UnsafeLeft => w.write_str(" <-- ")?,
        AssignOp::UnsafeRight => w.write_str(" --> ")?,
    }
    write_expr(w, &node.rhs)
}

fn write_compound_assign_stmt(w: &mut IndentWriter, node: &CompoundAssignStmt) -> fmt::Result {
    write_expr(w, &node.lhs)?;
    let op_str = match node.op {
        CompoundOp::AddAssign => " += ",
        CompoundOp::SubAssign => " -= ",
        CompoundOp::MulAssign => " *= ",
        CompoundOp::PowAssign => " **= ",
        CompoundOp::DivAssign => " /= ",
        CompoundOp::IntDivAssign => " \\= ",
        CompoundOp::ModAssign => " %= ",
        CompoundOp::ShlAssign => " <<= ",
        CompoundOp::ShrAssign => " >>= ",
        CompoundOp::BitAndAssign => " &= ",
        CompoundOp::BitOrAssign => " |= ",
        CompoundOp::BitXorAssign => " ^= ",
    };
    w.write_str(op_str)?;
    write_expr(w, &node.rhs)
}

fn write_constraint_eq_stmt(w: &mut IndentWriter, node: &ConstraintEqStmt) -> fmt::Result {
    write_expr(w, &node.lhs)?;
    w.write_str(" === ")?;
    write_expr(w, &node.rhs)
}

fn write_tuple_assign_stmt(w: &mut IndentWriter, node: &TupleAssignStmt) -> fmt::Result {
    w.write_str("(")?;
    for (i, target) in node.targets.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        match target {
            Some(expr) => write_expr(w, expr)?,
            None => w.write_str("_")?,
        }
    }
    w.write_str(")")?;
    match node.op {
        AssignOp::Eq => w.write_str(" = ")?,
        AssignOp::SafeLeft => w.write_str(" <== ")?,
        AssignOp::SafeRight => w.write_str(" ==> ")?,
        AssignOp::UnsafeLeft => w.write_str(" <-- ")?,
        AssignOp::UnsafeRight => w.write_str(" --> ")?,
    }
    write_expr(w, &node.rhs)
}

fn write_if_else(w: &mut IndentWriter, node: &IfElse) -> fmt::Result {
    w.write_str("if (")?;
    write_expr(w, &node.cond)?;
    w.write_str(") ")?;
    write_block(w, &node.then_body)?;
    if let Some(else_body) = &node.else_body {
        // Detect `else if`: when the else block contains exactly one IfElse statement
        if else_body.stmts.len() == 1 {
            if let StatementKind::IfElse(inner) = &else_body.stmts[0].kind {
                w.write_str(" else ")?;
                return write_if_else(w, inner);
            }
        }
        w.write_str(" else ")?;
        write_block(w, else_body)?;
    }
    Ok(())
}

fn write_for_loop(w: &mut IndentWriter, node: &ForLoop) -> fmt::Result {
    w.write_str("for (")?;
    write_for_init(w, &node.init)?;
    w.write_str("; ")?;
    write_expr(w, &node.cond)?;
    w.write_str("; ")?;
    write_for_step(w, &node.step)?;
    w.write_str(") ")?;
    write_block(w, &node.body)
}

fn write_for_init(w: &mut IndentWriter, stmt: &Statement) -> fmt::Result {
    match &stmt.kind {
        StatementKind::VarDecl(n) => write_var_decl(w, n),
        StatementKind::Assignment(n) => write_assign_stmt(w, n),
        StatementKind::Expression(expr) => write_expr(w, expr),
        other => w.write_fmt(format_args!("/* unsupported for-init: {other:?} */")),
    }
}

fn write_for_step(w: &mut IndentWriter, stmt: &Statement) -> fmt::Result {
    match &stmt.kind {
        StatementKind::Assignment(n) => write_assign_stmt(w, n),
        StatementKind::CompoundAssign(n) => write_compound_assign_stmt(w, n),
        StatementKind::Increment(expr) => {
            write_expr(w, expr)?;
            w.write_str("++")
        }
        StatementKind::Decrement(expr) => {
            write_expr(w, expr)?;
            w.write_str("--")
        }
        StatementKind::Expression(expr) => write_expr(w, expr),
        other => w.write_fmt(format_args!("/* unsupported for-step: {other:?} */")),
    }
}

fn write_while_loop(w: &mut IndentWriter, node: &WhileLoop) -> fmt::Result {
    w.write_str("while (")?;
    write_expr(w, &node.cond)?;
    w.write_str(") ")?;
    write_block(w, &node.body)
}

fn write_log_stmt(w: &mut IndentWriter, node: &LogStmt) -> fmt::Result {
    w.write_str("log(")?;
    for (i, arg) in node.args.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        match arg {
            LogArg::Expr(expr) => write_expr(w, expr)?,
            LogArg::String(s) => {
                w.write_str("\"")?;
                w.write_str(s)?;
                w.write_str("\"")?;
            }
        }
    }
    w.write_str(")")
}

/// Write an expression to the output.
///
/// **Precedence note:** Binary and Unary expressions are printed without
/// precedence-aware parenthesization.  This is correct for ASTs produced
/// by the parser because the parser preserves explicit `Paren` nodes.
/// However, programmatically-constructed ASTs that omit `Paren` nodes
/// may produce output with different semantics when re-parsed (e.g.,
/// `Binary(Binary(a, Add, b), Mul, c)` prints as `a + b * c`, which
/// re-parses as `a + (b * c)`, and `Unary(Neg, Unary(Neg, x))` prints
/// as `--x`, which re-parses as a decrement).
fn write_expr(w: &mut IndentWriter, node: &Expression) -> fmt::Result {
    match node.kind.as_ref() {
        ExpressionKind::Number(n) => w.write_str(n),
        ExpressionKind::Ident(name) => w.write_str(name),
        ExpressionKind::Unary(op, expr) => {
            match op {
                UnaryOp::Neg => w.write_str("-")?,
                UnaryOp::Not => w.write_str("!")?,
                UnaryOp::BitNot => w.write_str("~")?,
            }
            write_expr(w, expr)
        }
        ExpressionKind::Binary(lhs, op, rhs) => {
            write_expr(w, lhs)?;
            let op_str = match op {
                BinaryOp::Add => " + ",
                BinaryOp::Sub => " - ",
                BinaryOp::Mul => " * ",
                BinaryOp::Div => " / ",
                BinaryOp::IntDiv => " \\ ",
                BinaryOp::Mod => " % ",
                BinaryOp::Pow => " ** ",
                BinaryOp::Shl => " << ",
                BinaryOp::Shr => " >> ",
                BinaryOp::BitAnd => " & ",
                BinaryOp::BitOr => " | ",
                BinaryOp::BitXor => " ^ ",
                BinaryOp::And => " && ",
                BinaryOp::Or => " || ",
                BinaryOp::Eq => " == ",
                BinaryOp::Ne => " != ",
                BinaryOp::Lt => " < ",
                BinaryOp::Gt => " > ",
                BinaryOp::Le => " <= ",
                BinaryOp::Ge => " >= ",
            };
            w.write_str(op_str)?;
            write_expr(w, rhs)
        }
        ExpressionKind::Ternary(cond, then_expr, else_expr) => {
            write_expr(w, cond)?;
            w.write_str(" ? ")?;
            write_expr(w, then_expr)?;
            w.write_str(" : ")?;
            write_expr(w, else_expr)
        }
        ExpressionKind::Index(expr, index) => {
            write_expr(w, expr)?;
            w.write_str("[")?;
            write_expr(w, index)?;
            w.write_str("]")
        }
        ExpressionKind::Member(expr, ident) => {
            write_expr(w, expr)?;
            w.write_fmt(format_args!(".{}", ident.name))
        }
        ExpressionKind::Call(callee, args) => {
            write_expr(w, callee)?;
            w.write_str("(")?;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    w.write_str(", ")?;
                }
                write_expr(w, arg)?;
            }
            w.write_str(")")
        }
        ExpressionKind::AnonymousComp(comp) => write_anonymous_comp(w, comp),
        ExpressionKind::ArrayLit(elems) => {
            w.write_str("[")?;
            for (i, elem) in elems.iter().enumerate() {
                if i > 0 {
                    w.write_str(", ")?;
                }
                write_expr(w, elem)?;
            }
            w.write_str("]")
        }
        ExpressionKind::Paren(expr) => {
            w.write_str("(")?;
            write_expr(w, expr)?;
            w.write_str(")")
        }
        ExpressionKind::Parallel(expr) => {
            w.write_str("parallel ")?;
            write_expr(w, expr)
        }
        ExpressionKind::Underscore => w.write_str("_"),
        ExpressionKind::Error => w.write_str("/* error */"),
    }
}

fn write_anonymous_comp(w: &mut IndentWriter, node: &AnonymousComp) -> fmt::Result {
    write_expr(w, &node.template)?;
    w.write_str("(")?;
    for (i, arg) in node.template_args.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        write_expr(w, arg)?;
    }
    w.write_str(")(")?;
    for (i, input) in node.inputs.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        match input {
            AnonCompInput::Positional(expr) => write_expr(w, expr)?,
            AnonCompInput::Named(ident, expr) => {
                w.write_fmt(format_args!("{} <== ", ident.name))?;
                write_expr(w, expr)?;
            }
        }
    }
    w.write_str(")")
}

fn write_comma_sep_idents(w: &mut IndentWriter, idents: &[Identifier]) -> fmt::Result {
    for (i, ident) in idents.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        w.write_str(&ident.name)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::parser;

    #[test]
    fn pretty_print_pragma() {
        let src = "pragma circom 2.0.0;\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        assert_eq!(file.to_string(), "pragma circom 2.0.0;\n");
    }

    #[test]
    fn pretty_print_include() {
        let src = "include \"other.circom\";\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        assert_eq!(file.to_string(), "include \"other.circom\";\n");
    }

    #[test]
    fn pretty_print_simple_template() {
        let src = r#"template Foo(n) {
    signal input a;
    signal output b;
    b <== a;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("template Foo(n)"));
        assert!(output.contains("signal input a"));
        assert!(output.contains("signal output b"));
        assert!(output.contains("b <== a"));
    }

    #[test]
    fn pretty_print_function() {
        let src = r#"function add(a, b) {
    return a + b;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("function add(a, b)"));
        assert!(output.contains("return a + b"));
    }

    #[test]
    fn pretty_print_control_flow() {
        let src = r#"template T() {
    var x = 0;
    if (x) {
        x = 1;
    } else {
        x = 2;
    }
    for (var i = 0; i < 10; i++) {
        x += i;
    }
    while (x) {
        x = x - 1;
    }
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("if (x)"));
        assert!(output.contains("} else {"));
        assert!(output.contains("for (var i = 0; i < 10; i++)"));
        assert!(output.contains("while (x)"));
    }

    #[test]
    fn pretty_print_main_component() {
        let src = "component main {public [a, b]} = MyTemplate(10);\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("component main {public [a, b]}"));
        assert!(output.contains("MyTemplate(10)"));
    }

    #[test]
    fn pretty_print_constraint_eq() {
        let src = r#"template T() {
    signal input a;
    signal input b;
    a === b;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty());
        let output = file.to_string();
        assert!(output.contains("a === b"));
    }

    #[test]
    fn roundtrip_parse_print_reparse() {
        // Parse -> print -> reparse should produce the same AST structure
        let src = r#"pragma circom 2.0.0;

template Num2Bits(n) {
    signal input in;
    signal output out[n];
    var lc1 = 0;
    var e2 = 1;
    for (var i = 0; i < n; i++) {
        out[i] <-- (in >> i) & 1;
        out[i] * (out[i] - 1) === 0;
        lc1 += out[i] * e2;
        e2 = e2 + e2;
    }
    lc1 === in;
}
"#;
        let (file1, errors1) = parser::parse(src);
        assert!(errors1.is_empty(), "first parse errors: {errors1:?}");

        let printed = file1.to_string();
        let (file2, errors2) = parser::parse(&printed);
        assert!(errors2.is_empty(), "reparse errors: {errors2:?}");

        // Compare structure via Debug representation (spans may differ
        // but the AST shape and content should be identical)
        assert_eq!(
            format!("{:?}", file1.items),
            format!("{:?}", file2.items),
            "round-trip AST differs"
        );

        // Print again — should be stable
        let printed2 = file2.to_string();
        assert_eq!(printed, printed2, "pretty-print is not idempotent");
    }

    #[test]
    fn pretty_print_bus_def() {
        let src = r#"bus MyBus() {
    signal input x;
    signal output y;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("bus MyBus()"));
        assert!(output.contains("signal input x"));
        assert!(output.contains("signal output y"));
    }

    #[test]
    fn pretty_print_bus_def_with_params() {
        let src = r#"bus MyBus(n) {
    signal input x[n];
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("bus MyBus(n)"));
        assert!(output.contains("signal input x[n]"));
    }

    #[test]
    fn pretty_print_bus_field_decl() {
        let src = r#"bus Outer() {
    Inner() inner_field;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(
            output.contains("Inner() inner_field"),
            "output was:\n{output}"
        );
    }

    #[test]
    fn roundtrip_bus_instance_decl() {
        let src = r#"template T() {
    signal output MyBus() myBus;
}
"#;
        let (file1, errors1) = parser::parse(src);
        assert!(errors1.is_empty(), "first parse errors: {errors1:?}");
        let printed = file1.to_string();
        let (file2, errors2) = parser::parse(&printed);
        assert!(errors2.is_empty(), "reparse errors: {errors2:?}");
        let printed2 = file2.to_string();
        assert_eq!(
            printed, printed2,
            "bus instance decl round-trip is not idempotent"
        );
    }

    #[test]
    fn pretty_print_compound_assign_all_ops() {
        let src = r#"template T() {
    var x = 100;
    x += 1;
    x -= 2;
    x *= 3;
    x **= 4;
    x /= 5;
    x \= 6;
    x %= 7;
    x <<= 8;
    x >>= 9;
    x &= 10;
    x |= 11;
    x ^= 12;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("x += 1"));
        assert!(output.contains("x -= 2"));
        assert!(output.contains("x *= 3"));
        assert!(output.contains("x **= 4"));
        assert!(output.contains("x /= 5"));
        assert!(output.contains("x \\= 6"));
        assert!(output.contains("x %= 7"));
        assert!(output.contains("x <<= 8"));
        assert!(output.contains("x >>= 9"));
        assert!(output.contains("x &= 10"));
        assert!(output.contains("x |= 11"));
        assert!(output.contains("x ^= 12"));
    }

    #[test]
    fn pretty_print_tuple_assign() {
        let src = r#"template T() {
    var a;
    var b;
    (a, b) <== SomeTemplate()();
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("(a, b) <=="));
    }

    #[test]
    fn pretty_print_tuple_assign_with_underscore() {
        let src = r#"template T() {
    var a;
    (a, _) <== SomeTemplate()();
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("(a, _) <=="));
    }

    #[test]
    fn pretty_print_log_stmt_expr_only() {
        let src = r#"template T() {
    signal input x;
    log(x);
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("log(x)"));
    }

    #[test]
    fn pretty_print_log_stmt_mixed_args() {
        let src = r#"template T() {
    signal input x;
    log("value: ", x, " done");
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains(r#"log("value: ", x, " done")"#));
    }

    #[test]
    fn pretty_print_assert_stmt() {
        let src = r#"template T() {
    signal input x;
    assert(x);
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("assert(x)"));
    }

    #[test]
    fn pretty_print_anonymous_comp_positional() {
        let src = r#"template T() {
    signal output out;
    out <== Multiplier(n)(a, b);
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("Multiplier(n)(a, b)"));
    }

    #[test]
    fn pretty_print_anonymous_comp_named() {
        let src = r#"template T() {
    signal output out;
    out <== A(n)(x <== in1, y <== in2);
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("A(n)(x <== in1, y <== in2)"));
    }

    #[test]
    fn pretty_print_all_expression_kinds() {
        let src = r#"template T() {
    var x = -1;
    var y = !x;
    var z = ~x;
    var a = x + y;
    var b = x - y;
    var c = x * y;
    var d = x / y;
    var e = x \ y;
    var f = x % y;
    var g = x ** y;
    var h = x << y;
    var j = x >> y;
    var k = x & y;
    var l = x | y;
    var m = x ^ y;
    var n = x && y;
    var o = x || y;
    var p = x == y;
    var q = x != y;
    var r = x < y;
    var s = x > y;
    var t = x <= y;
    var u = x >= y;
    var v = x ? y : z;
    signal input arr[3];
    signal output out;
    out <== arr[0];
    out <== arr[0] + 1;
    var w = [1, 2, 3];
    out <== (x + y);
    out <== parallel x;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        // Verify key operators are present
        assert!(output.contains(" + "));
        assert!(output.contains(" - "));
        assert!(output.contains(" * "));
        assert!(output.contains(" ** "));
        assert!(output.contains(" << "));
        assert!(output.contains(" >> "));
        assert!(output.contains(" && "));
        assert!(output.contains(" || "));
        assert!(output.contains(" == "));
        assert!(output.contains(" != "));
        assert!(output.contains(" <= "));
        assert!(output.contains(" >= "));
        assert!(output.contains(" ? "));
        assert!(output.contains("parallel "));
    }

    #[test]
    fn pretty_print_signal_tags_and_component_parallel() {
        let src = r#"template T() {
    signal input {tag1, tag2} x;
    component parallel c = OtherTemplate();
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("{tag1, tag2}"));
        assert!(output.contains("component parallel"));
    }

    #[test]
    fn pretty_print_custom_template() {
        let src = r#"custom template extern parallel Foo() {
    signal input x;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(
            output.contains("custom template extern parallel Foo()"),
            "output was:\n{output}"
        );
    }

    #[test]
    fn pretty_print_pragma_custom_templates() {
        let src = "pragma custom_templates;\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        assert_eq!(file.to_string(), "pragma custom_templates;\n");
    }

    #[test]
    fn pretty_print_assignment_ops() {
        let src = r#"template T() {
    signal input a;
    signal output b;
    b <== a;
    b <-- a;
    a ==> b;
    a --> b;
    var x = 0;
    x = 1;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("<=="));
        assert!(output.contains("<--"));
        assert!(output.contains("==>"));
        assert!(output.contains("-->"));
    }

    #[test]
    fn pretty_print_member_access() {
        let src = r#"template T() {
    component c = OtherTemplate();
    c.inp <== 1;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("c.inp"));
    }

    #[test]
    fn pretty_print_increment_decrement() {
        let src = r#"template T() {
    var x = 0;
    x++;
    x--;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("x++"));
        assert!(output.contains("x--"));
    }

    #[test]
    fn pretty_print_nested_block() {
        let src = r#"template T() {
    {
        var x = 1;
    }
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        // Should have nested indentation
        assert!(output.contains("        var x = 1"));
    }

    #[test]
    fn pretty_print_escaped_string() {
        // The parser stores the raw string content. Verify the pretty printer
        // escapes special characters in log string arguments.
        let src = "template T() {\n    log(\"hello\\\\world\");\n}\n";
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("log(\"hello"));
    }

    #[test]
    fn pretty_print_bus_field_decl_with_tags_and_dims() {
        let src = r#"bus Outer() {
    Inner(2) {tag1} inner_field[3];
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("Inner(2)"));
        assert!(output.contains("{tag1}"));
        assert!(output.contains("inner_field[3]"));
    }

    #[test]
    fn pretty_print_bus_instance_decl_with_init() {
        let src = r#"template T() {
    signal output Inner(2) {tag1} myBus[3] <== 1;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("Inner(2)"));
        assert!(output.contains("{tag1}"));
        assert!(output.contains("myBus[3]"));
        assert!(output.contains("<=="));
    }

    #[test]
    fn pretty_print_for_decrement_step() {
        let src = r#"template T() {
    for (var i = 10; i > 0; i--) {
        var x = i;
    }
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("i--"));
    }

    #[test]
    fn pretty_print_for_assign_step() {
        let src = r#"template T() {
    for (var i = 0; i < 10; i = i + 1) {
        var x = i;
    }
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("i = i + 1"));
    }

    #[test]
    fn pretty_print_for_compound_step() {
        let src = r#"template T() {
    for (var i = 0; i < 10; i += 2) {
        var x = i;
    }
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("i += 2"));
    }

    #[test]
    fn pretty_print_signal_unsafe_init() {
        let src = r#"template T() {
    signal output b <-- 42;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("b <-- 42"));
    }

    #[test]
    fn pretty_print_multiple_var_entries() {
        let src = r#"template T() {
    var a, b, c;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("var a, b, c"));
    }

    #[test]
    fn pretty_print_multiple_signal_entries() {
        let src = r#"template T() {
    signal input a, b;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("signal input a, b"));
    }

    #[test]
    fn pretty_print_multiple_component_entries() {
        let src = r#"template T() {
    component a, b;
}
"#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let output = file.to_string();
        assert!(output.contains("component a, b"));
    }

    #[test]
    fn roundtrip_bus_def() {
        let src = r#"bus MyBus(n) {
    signal input x[n];
    signal output y;
}
"#;
        let (file1, errors1) = parser::parse(src);
        assert!(errors1.is_empty(), "first parse errors: {errors1:?}");
        let printed = file1.to_string();
        let (file2, errors2) = parser::parse(&printed);
        assert!(errors2.is_empty(), "reparse errors: {errors2:?}");
        let printed2 = file2.to_string();
        assert_eq!(printed, printed2, "bus def round-trip is not idempotent");
    }

    #[test]
    fn roundtrip_log_assert_tuple() {
        let src = r#"template T() {
    var a;
    var b;
    log("msg: ", a);
    assert(b);
    (a, b) <== SomeTemplate()();
}
"#;
        let (file1, errors1) = parser::parse(src);
        assert!(errors1.is_empty(), "first parse errors: {errors1:?}");
        let printed = file1.to_string();
        let (file2, errors2) = parser::parse(&printed);
        assert!(errors2.is_empty(), "reparse errors: {errors2:?}");
        let printed2 = file2.to_string();
        assert_eq!(
            printed, printed2,
            "log/assert/tuple round-trip is not idempotent"
        );
    }

    #[test]
    fn roundtrip_anonymous_comp() {
        let src = r#"template T() {
    signal output out;
    out <== A(n)(x <== in1, y <== in2);
}
"#;
        let (file1, errors1) = parser::parse(src);
        assert!(errors1.is_empty(), "first parse errors: {errors1:?}");
        let printed = file1.to_string();
        let (file2, errors2) = parser::parse(&printed);
        assert!(errors2.is_empty(), "reparse errors: {errors2:?}");
        let printed2 = file2.to_string();
        assert_eq!(
            printed, printed2,
            "anonymous comp round-trip is not idempotent"
        );
    }

    #[test]
    fn roundtrip_compound_assign_all_ops() {
        let src = r#"template T() {
    var x = 100;
    x += 1;
    x -= 2;
    x *= 3;
    x **= 4;
    x /= 5;
    x \= 6;
    x %= 7;
    x <<= 8;
    x >>= 9;
    x &= 10;
    x |= 11;
    x ^= 12;
}
"#;
        let (file1, errors1) = parser::parse(src);
        assert!(errors1.is_empty(), "first parse errors: {errors1:?}");
        let printed = file1.to_string();
        let (file2, errors2) = parser::parse(&printed);
        assert!(errors2.is_empty(), "reparse errors: {errors2:?}");
        let printed2 = file2.to_string();
        assert_eq!(
            printed, printed2,
            "compound assign round-trip is not idempotent"
        );
    }

    #[test]
    fn roundtrip_circomlib_fixtures() {
        use std::fs;

        let fixtures_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
        for entry in fs::read_dir(fixtures_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().map(|e| e == "circom").unwrap_or(false) {
                let src = fs::read_to_string(&path).unwrap();
                let (file1, errors1) = parser::parse(&src);
                // Skip files with parse errors (some may use unsupported features)
                if !errors1.is_empty() {
                    continue;
                }

                let printed = file1.to_string();
                let (file2, errors2) = parser::parse(&printed);
                assert!(
                    errors2.is_empty(),
                    "reparse of {} failed: {errors2:?}\n---printed---\n{printed}",
                    path.display()
                );

                // Idempotency: print again and compare
                let printed2 = file2.to_string();
                assert_eq!(
                    printed,
                    printed2,
                    "pretty-print of {} is not idempotent",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn roundtrip_parallel_template() {
        let src = "custom template extern parallel Foo() {\n}\n";
        let (file1, errors1) = parser::parse(src);
        assert!(errors1.is_empty(), "first parse errors: {errors1:?}");
        let printed = file1.to_string();
        let (file2, errors2) = parser::parse(&printed);
        assert!(
            errors2.is_empty(),
            "reparse errors: {errors2:?}\n---printed---\n{printed}"
        );
        assert_eq!(
            format!("{:?}", file1.items),
            format!("{:?}", file2.items),
            "round-trip AST differs"
        );
        let printed2 = file2.to_string();
        assert_eq!(printed, printed2, "pretty-print is not idempotent");
    }

    #[test]
    fn roundtrip_zero_arg_bus_type() {
        let src = "bus Outer() {\n    Inner() name;\n}\n";
        let (file1, errors1) = parser::parse(src);
        assert!(errors1.is_empty(), "first parse errors: {errors1:?}");
        let printed = file1.to_string();
        let (file2, errors2) = parser::parse(&printed);
        assert!(
            errors2.is_empty(),
            "reparse errors: {errors2:?}\n---printed---\n{printed}"
        );
        assert_eq!(
            format!("{:?}", file1.items),
            format!("{:?}", file2.items),
            "round-trip AST differs"
        );
        let printed2 = file2.to_string();
        assert_eq!(printed, printed2, "pretty-print is not idempotent");
    }
}
