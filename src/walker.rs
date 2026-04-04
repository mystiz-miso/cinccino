//! Depth-first AST walker with pre-order (enter) and post-order (leave) hooks.
//!
//! The [`Walker`] trait provides `enter_*` / `leave_*` method pairs for every
//! AST node type. `enter_*` returns a `bool`: `true` to descend into children,
//! `false` to skip the subtree. `leave_*` is always called if `enter_*` was
//! called, regardless of the return value.
//!
//! # Example
//!
//! ```
//! use cinccino::walker::{self, Walker};
//! use cinccino::ast::*;
//!
//! struct DepthTracker { depth: usize, max_depth: usize }
//!
//! impl Walker for DepthTracker {
//!     fn enter_block(&mut self, _node: &Block) -> bool {
//!         self.depth += 1;
//!         self.max_depth = self.max_depth.max(self.depth);
//!         true
//!     }
//!     fn leave_block(&mut self, _node: &Block) {
//!         self.depth -= 1;
//!     }
//! }
//! ```

use crate::ast::*;

/// Trait for walking the AST with enter/leave hooks.
///
/// Each `enter_*` method is called before descending into children. Returning
/// `false` skips child traversal but `leave_*` is still called.
pub trait Walker {
    fn enter_file(&mut self, _node: &File) -> bool {
        true
    }
    fn leave_file(&mut self, _node: &File) {}

    fn enter_item(&mut self, _node: &Item) -> bool {
        true
    }
    fn leave_item(&mut self, _node: &Item) {}

    fn enter_pragma(&mut self, _node: &Pragma) -> bool {
        true
    }
    fn leave_pragma(&mut self, _node: &Pragma) {}

    fn enter_include(&mut self, _node: &Include) -> bool {
        true
    }
    fn leave_include(&mut self, _node: &Include) {}

    fn enter_template_def(&mut self, _node: &TemplateDef) -> bool {
        true
    }
    fn leave_template_def(&mut self, _node: &TemplateDef) {}

    fn enter_function_def(&mut self, _node: &FunctionDef) -> bool {
        true
    }
    fn leave_function_def(&mut self, _node: &FunctionDef) {}

    fn enter_bus_def(&mut self, _node: &BusDef) -> bool {
        true
    }
    fn leave_bus_def(&mut self, _node: &BusDef) {}

    fn enter_bus_member(&mut self, _node: &BusMember) -> bool {
        true
    }
    fn leave_bus_member(&mut self, _node: &BusMember) {}

    fn enter_bus_field_decl(&mut self, _node: &BusFieldDecl) -> bool {
        true
    }
    fn leave_bus_field_decl(&mut self, _node: &BusFieldDecl) {}

    fn enter_main_component(&mut self, _node: &MainComponent) -> bool {
        true
    }
    fn leave_main_component(&mut self, _node: &MainComponent) {}

    fn enter_block(&mut self, _node: &Block) -> bool {
        true
    }
    fn leave_block(&mut self, _node: &Block) {}

    fn enter_statement(&mut self, _node: &Statement) -> bool {
        true
    }
    fn leave_statement(&mut self, _node: &Statement) {}

    fn enter_var_decl(&mut self, _node: &VarDecl) -> bool {
        true
    }
    fn leave_var_decl(&mut self, _node: &VarDecl) {}

    fn enter_var_decl_entry(&mut self, _node: &VarDeclEntry) -> bool {
        true
    }
    fn leave_var_decl_entry(&mut self, _node: &VarDeclEntry) {}

    fn enter_signal_decl(&mut self, _node: &SignalDecl) -> bool {
        true
    }
    fn leave_signal_decl(&mut self, _node: &SignalDecl) {}

    fn enter_signal_decl_entry(&mut self, _node: &SignalDeclEntry) -> bool {
        true
    }
    fn leave_signal_decl_entry(&mut self, _node: &SignalDeclEntry) {}

    fn enter_component_decl(&mut self, _node: &ComponentDecl) -> bool {
        true
    }
    fn leave_component_decl(&mut self, _node: &ComponentDecl) {}

    fn enter_component_decl_entry(&mut self, _node: &ComponentDeclEntry) -> bool {
        true
    }
    fn leave_component_decl_entry(&mut self, _node: &ComponentDeclEntry) {}

    fn enter_bus_instance_decl(&mut self, _node: &BusInstanceDecl) -> bool {
        true
    }
    fn leave_bus_instance_decl(&mut self, _node: &BusInstanceDecl) {}

    fn enter_bus_type(&mut self, _node: &BusType) -> bool {
        true
    }
    fn leave_bus_type(&mut self, _node: &BusType) {}

    fn enter_assign_stmt(&mut self, _node: &AssignStmt) -> bool {
        true
    }
    fn leave_assign_stmt(&mut self, _node: &AssignStmt) {}

    fn enter_compound_assign_stmt(&mut self, _node: &CompoundAssignStmt) -> bool {
        true
    }
    fn leave_compound_assign_stmt(&mut self, _node: &CompoundAssignStmt) {}

    fn enter_constraint_eq_stmt(&mut self, _node: &ConstraintEqStmt) -> bool {
        true
    }
    fn leave_constraint_eq_stmt(&mut self, _node: &ConstraintEqStmt) {}

    fn enter_tuple_assign_stmt(&mut self, _node: &TupleAssignStmt) -> bool {
        true
    }
    fn leave_tuple_assign_stmt(&mut self, _node: &TupleAssignStmt) {}

    fn enter_if_else(&mut self, _node: &IfElse) -> bool {
        true
    }
    fn leave_if_else(&mut self, _node: &IfElse) {}

    fn enter_for_loop(&mut self, _node: &ForLoop) -> bool {
        true
    }
    fn leave_for_loop(&mut self, _node: &ForLoop) {}

    fn enter_while_loop(&mut self, _node: &WhileLoop) -> bool {
        true
    }
    fn leave_while_loop(&mut self, _node: &WhileLoop) {}

    fn enter_return_stmt(&mut self, _node: &ReturnStmt) -> bool {
        true
    }
    fn leave_return_stmt(&mut self, _node: &ReturnStmt) {}

    fn enter_log_stmt(&mut self, _node: &LogStmt) -> bool {
        true
    }
    fn leave_log_stmt(&mut self, _node: &LogStmt) {}

    fn enter_log_arg(&mut self, _node: &LogArg) -> bool {
        true
    }
    fn leave_log_arg(&mut self, _node: &LogArg) {}

    fn enter_assert_stmt(&mut self, _node: &AssertStmt) -> bool {
        true
    }
    fn leave_assert_stmt(&mut self, _node: &AssertStmt) {}

    fn enter_increment(&mut self, _node: &Expression) -> bool {
        true
    }
    fn leave_increment(&mut self, _node: &Expression) {}

    fn enter_decrement(&mut self, _node: &Expression) -> bool {
        true
    }
    fn leave_decrement(&mut self, _node: &Expression) {}

    fn enter_expression(&mut self, _node: &Expression) -> bool {
        true
    }
    fn leave_expression(&mut self, _node: &Expression) {}

    fn enter_anonymous_comp(&mut self, _node: &AnonymousComp) -> bool {
        true
    }
    fn leave_anonymous_comp(&mut self, _node: &AnonymousComp) {}

    fn enter_anon_comp_input(&mut self, _node: &AnonCompInput) -> bool {
        true
    }
    fn leave_anon_comp_input(&mut self, _node: &AnonCompInput) {}

    fn enter_identifier(&mut self, _node: &Identifier) -> bool {
        true
    }
    fn leave_identifier(&mut self, _node: &Identifier) {}
}

// ── Walk functions ─────────────────────────────────────────────────────

pub fn walk_file<W: Walker + ?Sized>(w: &mut W, node: &File) {
    if !w.enter_file(node) {
        w.leave_file(node);
        return;
    }
    for item in &node.items {
        walk_item(w, item);
    }
    w.leave_file(node);
}

pub fn walk_item<W: Walker + ?Sized>(w: &mut W, node: &Item) {
    if !w.enter_item(node) {
        w.leave_item(node);
        return;
    }
    match node {
        Item::Pragma(n) => walk_pragma(w, n),
        Item::Include(n) => walk_include(w, n),
        Item::TemplateDef(n) => walk_template_def(w, n),
        Item::FunctionDef(n) => walk_function_def(w, n),
        Item::BusDef(n) => walk_bus_def(w, n),
        Item::MainComponent(n) => walk_main_component(w, n),
    }
    w.leave_item(node);
}

pub fn walk_pragma<W: Walker + ?Sized>(w: &mut W, node: &Pragma) {
    if !w.enter_pragma(node) {
        w.leave_pragma(node);
        return;
    }
    w.leave_pragma(node);
}

pub fn walk_include<W: Walker + ?Sized>(w: &mut W, node: &Include) {
    if !w.enter_include(node) {
        w.leave_include(node);
        return;
    }
    w.leave_include(node);
}

pub fn walk_template_def<W: Walker + ?Sized>(w: &mut W, node: &TemplateDef) {
    if !w.enter_template_def(node) {
        w.leave_template_def(node);
        return;
    }
    walk_identifier(w, &node.name);
    for param in &node.params {
        walk_identifier(w, param);
    }
    walk_block(w, &node.body);
    w.leave_template_def(node);
}

pub fn walk_function_def<W: Walker + ?Sized>(w: &mut W, node: &FunctionDef) {
    if !w.enter_function_def(node) {
        w.leave_function_def(node);
        return;
    }
    walk_identifier(w, &node.name);
    for param in &node.params {
        walk_identifier(w, param);
    }
    walk_block(w, &node.body);
    w.leave_function_def(node);
}

pub fn walk_bus_def<W: Walker + ?Sized>(w: &mut W, node: &BusDef) {
    if !w.enter_bus_def(node) {
        w.leave_bus_def(node);
        return;
    }
    walk_identifier(w, &node.name);
    for param in &node.params {
        walk_identifier(w, param);
    }
    for member in &node.body {
        walk_bus_member(w, member);
    }
    w.leave_bus_def(node);
}

pub fn walk_bus_member<W: Walker + ?Sized>(w: &mut W, node: &BusMember) {
    if !w.enter_bus_member(node) {
        w.leave_bus_member(node);
        return;
    }
    match node {
        BusMember::Signal(n) => walk_signal_decl(w, n),
        BusMember::Bus(n) => walk_bus_field_decl(w, n),
    }
    w.leave_bus_member(node);
}

pub fn walk_bus_field_decl<W: Walker + ?Sized>(w: &mut W, node: &BusFieldDecl) {
    if !w.enter_bus_field_decl(node) {
        w.leave_bus_field_decl(node);
        return;
    }
    walk_bus_type(w, &node.bus_type);
    for tag in &node.tags {
        walk_identifier(w, tag);
    }
    walk_identifier(w, &node.name);
    for dim in &node.dimensions {
        walk_expression(w, dim);
    }
    w.leave_bus_field_decl(node);
}

pub fn walk_main_component<W: Walker + ?Sized>(w: &mut W, node: &MainComponent) {
    if !w.enter_main_component(node) {
        w.leave_main_component(node);
        return;
    }
    for sig in &node.public_signals {
        walk_identifier(w, sig);
    }
    walk_expression(w, &node.expr);
    w.leave_main_component(node);
}

pub fn walk_block<W: Walker + ?Sized>(w: &mut W, node: &Block) {
    if !w.enter_block(node) {
        w.leave_block(node);
        return;
    }
    for stmt in &node.stmts {
        walk_statement(w, stmt);
    }
    w.leave_block(node);
}

pub fn walk_statement<W: Walker + ?Sized>(w: &mut W, node: &Statement) {
    if !w.enter_statement(node) {
        w.leave_statement(node);
        return;
    }
    match &node.kind {
        StatementKind::VarDecl(n) => walk_var_decl(w, n),
        StatementKind::SignalDecl(n) => walk_signal_decl(w, n),
        StatementKind::ComponentDecl(n) => walk_component_decl(w, n),
        StatementKind::BusDecl(n) => walk_bus_instance_decl(w, n),
        StatementKind::Assignment(n) => walk_assign_stmt(w, n),
        StatementKind::CompoundAssign(n) => walk_compound_assign_stmt(w, n),
        StatementKind::ConstraintEq(n) => walk_constraint_eq_stmt(w, n),
        StatementKind::TupleAssign(n) => walk_tuple_assign_stmt(w, n),
        StatementKind::IfElse(n) => walk_if_else(w, n),
        StatementKind::For(n) => walk_for_loop(w, n),
        StatementKind::While(n) => walk_while_loop(w, n),
        StatementKind::Return(n) => walk_return_stmt(w, n),
        StatementKind::Log(n) => walk_log_stmt(w, n),
        StatementKind::Assert(n) => walk_assert_stmt(w, n),
        StatementKind::Increment(expr) => walk_increment(w, expr),
        StatementKind::Decrement(expr) => walk_decrement(w, expr),
        StatementKind::Expression(expr) => walk_expression(w, expr),
        StatementKind::Block(blk) => walk_block(w, blk),
        StatementKind::Error => {}
    }
    w.leave_statement(node);
}

pub fn walk_increment<W: Walker + ?Sized>(w: &mut W, expr: &Expression) {
    if w.enter_increment(expr) {
        walk_expression(w, expr);
    }
    w.leave_increment(expr);
}

pub fn walk_decrement<W: Walker + ?Sized>(w: &mut W, expr: &Expression) {
    if w.enter_decrement(expr) {
        walk_expression(w, expr);
    }
    w.leave_decrement(expr);
}

pub fn walk_var_decl<W: Walker + ?Sized>(w: &mut W, node: &VarDecl) {
    if !w.enter_var_decl(node) {
        w.leave_var_decl(node);
        return;
    }
    for entry in &node.names {
        walk_var_decl_entry(w, entry);
    }
    w.leave_var_decl(node);
}

pub fn walk_var_decl_entry<W: Walker + ?Sized>(w: &mut W, node: &VarDeclEntry) {
    if !w.enter_var_decl_entry(node) {
        w.leave_var_decl_entry(node);
        return;
    }
    walk_identifier(w, &node.name);
    for dim in &node.dimensions {
        walk_expression(w, dim);
    }
    if let Some(init) = &node.init {
        walk_expression(w, init);
    }
    w.leave_var_decl_entry(node);
}

pub fn walk_signal_decl<W: Walker + ?Sized>(w: &mut W, node: &SignalDecl) {
    if !w.enter_signal_decl(node) {
        w.leave_signal_decl(node);
        return;
    }
    for tag in &node.tags {
        walk_identifier(w, tag);
    }
    for entry in &node.names {
        walk_signal_decl_entry(w, entry);
    }
    w.leave_signal_decl(node);
}

pub fn walk_signal_decl_entry<W: Walker + ?Sized>(w: &mut W, node: &SignalDeclEntry) {
    if !w.enter_signal_decl_entry(node) {
        w.leave_signal_decl_entry(node);
        return;
    }
    walk_identifier(w, &node.name);
    for dim in &node.dimensions {
        walk_expression(w, dim);
    }
    if let Some((_, init)) = &node.init {
        walk_expression(w, init);
    }
    w.leave_signal_decl_entry(node);
}

pub fn walk_component_decl<W: Walker + ?Sized>(w: &mut W, node: &ComponentDecl) {
    if !w.enter_component_decl(node) {
        w.leave_component_decl(node);
        return;
    }
    for entry in &node.names {
        walk_component_decl_entry(w, entry);
    }
    w.leave_component_decl(node);
}

pub fn walk_component_decl_entry<W: Walker + ?Sized>(w: &mut W, node: &ComponentDeclEntry) {
    if !w.enter_component_decl_entry(node) {
        w.leave_component_decl_entry(node);
        return;
    }
    walk_identifier(w, &node.name);
    for dim in &node.dimensions {
        walk_expression(w, dim);
    }
    if let Some(init) = &node.init {
        walk_expression(w, init);
    }
    w.leave_component_decl_entry(node);
}

pub fn walk_bus_instance_decl<W: Walker + ?Sized>(w: &mut W, node: &BusInstanceDecl) {
    if !w.enter_bus_instance_decl(node) {
        w.leave_bus_instance_decl(node);
        return;
    }
    walk_bus_type(w, &node.bus_type);
    for tag in &node.tags {
        walk_identifier(w, tag);
    }
    walk_identifier(w, &node.name);
    for dim in &node.dimensions {
        walk_expression(w, dim);
    }
    if let Some((_, init)) = &node.init {
        walk_expression(w, init);
    }
    w.leave_bus_instance_decl(node);
}

pub fn walk_bus_type<W: Walker + ?Sized>(w: &mut W, node: &BusType) {
    if !w.enter_bus_type(node) {
        w.leave_bus_type(node);
        return;
    }
    walk_identifier(w, &node.name);
    for arg in &node.args {
        walk_expression(w, arg);
    }
    w.leave_bus_type(node);
}

pub fn walk_assign_stmt<W: Walker + ?Sized>(w: &mut W, node: &AssignStmt) {
    if !w.enter_assign_stmt(node) {
        w.leave_assign_stmt(node);
        return;
    }
    walk_expression(w, &node.lhs);
    walk_expression(w, &node.rhs);
    w.leave_assign_stmt(node);
}

pub fn walk_compound_assign_stmt<W: Walker + ?Sized>(w: &mut W, node: &CompoundAssignStmt) {
    if !w.enter_compound_assign_stmt(node) {
        w.leave_compound_assign_stmt(node);
        return;
    }
    walk_expression(w, &node.lhs);
    walk_expression(w, &node.rhs);
    w.leave_compound_assign_stmt(node);
}

pub fn walk_constraint_eq_stmt<W: Walker + ?Sized>(w: &mut W, node: &ConstraintEqStmt) {
    if !w.enter_constraint_eq_stmt(node) {
        w.leave_constraint_eq_stmt(node);
        return;
    }
    walk_expression(w, &node.lhs);
    walk_expression(w, &node.rhs);
    w.leave_constraint_eq_stmt(node);
}

pub fn walk_tuple_assign_stmt<W: Walker + ?Sized>(w: &mut W, node: &TupleAssignStmt) {
    if !w.enter_tuple_assign_stmt(node) {
        w.leave_tuple_assign_stmt(node);
        return;
    }
    for expr in node.targets.iter().flatten() {
        walk_expression(w, expr);
    }
    walk_expression(w, &node.rhs);
    w.leave_tuple_assign_stmt(node);
}

pub fn walk_if_else<W: Walker + ?Sized>(w: &mut W, node: &IfElse) {
    if !w.enter_if_else(node) {
        w.leave_if_else(node);
        return;
    }
    walk_expression(w, &node.cond);
    walk_block(w, &node.then_body);
    if let Some(else_body) = &node.else_body {
        walk_block(w, else_body);
    }
    w.leave_if_else(node);
}

pub fn walk_for_loop<W: Walker + ?Sized>(w: &mut W, node: &ForLoop) {
    if !w.enter_for_loop(node) {
        w.leave_for_loop(node);
        return;
    }
    walk_statement(w, &node.init);
    walk_expression(w, &node.cond);
    walk_statement(w, &node.step);
    walk_block(w, &node.body);
    w.leave_for_loop(node);
}

pub fn walk_while_loop<W: Walker + ?Sized>(w: &mut W, node: &WhileLoop) {
    if !w.enter_while_loop(node) {
        w.leave_while_loop(node);
        return;
    }
    walk_expression(w, &node.cond);
    walk_block(w, &node.body);
    w.leave_while_loop(node);
}

pub fn walk_return_stmt<W: Walker + ?Sized>(w: &mut W, node: &ReturnStmt) {
    if !w.enter_return_stmt(node) {
        w.leave_return_stmt(node);
        return;
    }
    walk_expression(w, &node.value);
    w.leave_return_stmt(node);
}

pub fn walk_log_stmt<W: Walker + ?Sized>(w: &mut W, node: &LogStmt) {
    if !w.enter_log_stmt(node) {
        w.leave_log_stmt(node);
        return;
    }
    for arg in &node.args {
        walk_log_arg(w, arg);
    }
    w.leave_log_stmt(node);
}

pub fn walk_log_arg<W: Walker + ?Sized>(w: &mut W, node: &LogArg) {
    if !w.enter_log_arg(node) {
        w.leave_log_arg(node);
        return;
    }
    match node {
        LogArg::Expr(expr) => walk_expression(w, expr),
        LogArg::String(_) => {}
    }
    w.leave_log_arg(node);
}

pub fn walk_assert_stmt<W: Walker + ?Sized>(w: &mut W, node: &AssertStmt) {
    if !w.enter_assert_stmt(node) {
        w.leave_assert_stmt(node);
        return;
    }
    walk_expression(w, &node.expr);
    w.leave_assert_stmt(node);
}

pub fn walk_expression<W: Walker + ?Sized>(w: &mut W, node: &Expression) {
    if !w.enter_expression(node) {
        w.leave_expression(node);
        return;
    }
    match node.kind.as_ref() {
        ExpressionKind::Number(_) | ExpressionKind::Underscore | ExpressionKind::Error => {}
        ExpressionKind::Ident(_) => {}
        ExpressionKind::Unary(_, expr) => walk_expression(w, expr),
        ExpressionKind::Binary(lhs, _, rhs) => {
            walk_expression(w, lhs);
            walk_expression(w, rhs);
        }
        ExpressionKind::Ternary(cond, then_expr, else_expr) => {
            walk_expression(w, cond);
            walk_expression(w, then_expr);
            walk_expression(w, else_expr);
        }
        ExpressionKind::Index(expr, index) => {
            walk_expression(w, expr);
            walk_expression(w, index);
        }
        ExpressionKind::Member(expr, ident) => {
            walk_expression(w, expr);
            walk_identifier(w, ident);
        }
        ExpressionKind::Call(callee, args) => {
            walk_expression(w, callee);
            for arg in args {
                walk_expression(w, arg);
            }
        }
        ExpressionKind::AnonymousComp(comp) => walk_anonymous_comp(w, comp),
        ExpressionKind::ArrayLit(elems) => {
            for elem in elems {
                walk_expression(w, elem);
            }
        }
        ExpressionKind::Paren(expr) => walk_expression(w, expr),
        ExpressionKind::Parallel(expr) => walk_expression(w, expr),
    }
    w.leave_expression(node);
}

pub fn walk_anonymous_comp<W: Walker + ?Sized>(w: &mut W, node: &AnonymousComp) {
    if !w.enter_anonymous_comp(node) {
        w.leave_anonymous_comp(node);
        return;
    }
    walk_expression(w, &node.template);
    for arg in &node.template_args {
        walk_expression(w, arg);
    }
    for input in &node.inputs {
        walk_anon_comp_input(w, input);
    }
    w.leave_anonymous_comp(node);
}

pub fn walk_anon_comp_input<W: Walker + ?Sized>(w: &mut W, node: &AnonCompInput) {
    if !w.enter_anon_comp_input(node) {
        w.leave_anon_comp_input(node);
        return;
    }
    match node {
        AnonCompInput::Positional(expr) => walk_expression(w, expr),
        AnonCompInput::Named(ident, expr) => {
            walk_identifier(w, ident);
            walk_expression(w, expr);
        }
    }
    w.leave_anon_comp_input(node);
}

pub fn walk_identifier<W: Walker + ?Sized>(w: &mut W, node: &Identifier) {
    if !w.enter_identifier(node) {
        w.leave_identifier(node);
        return;
    }
    w.leave_identifier(node);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[derive(Debug, PartialEq)]
    enum Event {
        Enter(&'static str),
        Leave(&'static str),
    }

    /// A no-op walker that uses all default trait implementations.
    struct DefaultWalker;
    impl Walker for DefaultWalker {}

    #[test]
    fn walker_default_impls_bus_def() {
        let src = r#"
            bus MyBus(n) {
                signal input x[n];
                signal output y;
                Inner() inner;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut w = DefaultWalker;
        walk_file(&mut w, &file);
    }

    #[test]
    fn walker_default_impls_all_statements() {
        let src = r#"
            pragma circom 2.2.0;
            include "other.circom";
            template T() {
                signal input a;
                signal output b;
                var x = 0;
                component c = OtherTemplate();
                signal output MyBus() myBus;
                b <== a;
                x += 1;
                a === b;
                (a, b) <== SomeTemplate()();
                if (x) { x = 1; } else { x = 2; }
                for (var i = 0; i < 10; i++) { x += i; }
                while (x) { x = x - 1; }
                log("msg: ", x);
                assert(x);
                x++;
                x--;
                { var y = 1; }
                b <== Multiplier(x)(a, b);
                b <== A(x)(p <== a, q <== b);
            }
            function f(a, b) {
                return a + b;
            }
            component main {public [a]} = T();
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut w = DefaultWalker;
        walk_file(&mut w, &file);
    }

    #[test]
    fn walker_default_impls_expressions() {
        let src = r#"
            template T() {
                var x = -1;
                var y = !x;
                var z = ~x;
                var a = x + y * z;
                var b = x ? y : z;
                signal input arr[3];
                signal output out;
                component c = OtherTemplate();
                c.inp <== 1;
                out <== arr[0];
                var d = [1, 2, 3];
                out <== (x + y);
                out <== parallel a;
                out <== Multiplier(1)(a, b);
                out <== A(1)(p <== a);
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut w = DefaultWalker;
        walk_file(&mut w, &file);
    }

    /// Walker that returns false from all enter_* methods, exercising the
    /// "skip children" early-return paths in every walk_* function.
    struct SkipAllWalker;

    impl Walker for SkipAllWalker {
        fn enter_file(&mut self, _: &File) -> bool {
            false
        }
        fn enter_item(&mut self, _: &Item) -> bool {
            false
        }
        fn enter_pragma(&mut self, _: &Pragma) -> bool {
            false
        }
        fn enter_include(&mut self, _: &Include) -> bool {
            false
        }
        fn enter_template_def(&mut self, _: &TemplateDef) -> bool {
            false
        }
        fn enter_function_def(&mut self, _: &FunctionDef) -> bool {
            false
        }
        fn enter_bus_def(&mut self, _: &BusDef) -> bool {
            false
        }
        fn enter_bus_member(&mut self, _: &BusMember) -> bool {
            false
        }
        fn enter_bus_field_decl(&mut self, _: &BusFieldDecl) -> bool {
            false
        }
        fn enter_main_component(&mut self, _: &MainComponent) -> bool {
            false
        }
        fn enter_block(&mut self, _: &Block) -> bool {
            false
        }
        fn enter_statement(&mut self, _: &Statement) -> bool {
            false
        }
        fn enter_var_decl(&mut self, _: &VarDecl) -> bool {
            false
        }
        fn enter_var_decl_entry(&mut self, _: &VarDeclEntry) -> bool {
            false
        }
        fn enter_signal_decl(&mut self, _: &SignalDecl) -> bool {
            false
        }
        fn enter_signal_decl_entry(&mut self, _: &SignalDeclEntry) -> bool {
            false
        }
        fn enter_component_decl(&mut self, _: &ComponentDecl) -> bool {
            false
        }
        fn enter_component_decl_entry(&mut self, _: &ComponentDeclEntry) -> bool {
            false
        }
        fn enter_bus_instance_decl(&mut self, _: &BusInstanceDecl) -> bool {
            false
        }
        fn enter_bus_type(&mut self, _: &BusType) -> bool {
            false
        }
        fn enter_assign_stmt(&mut self, _: &AssignStmt) -> bool {
            false
        }
        fn enter_compound_assign_stmt(&mut self, _: &CompoundAssignStmt) -> bool {
            false
        }
        fn enter_constraint_eq_stmt(&mut self, _: &ConstraintEqStmt) -> bool {
            false
        }
        fn enter_tuple_assign_stmt(&mut self, _: &TupleAssignStmt) -> bool {
            false
        }
        fn enter_if_else(&mut self, _: &IfElse) -> bool {
            false
        }
        fn enter_for_loop(&mut self, _: &ForLoop) -> bool {
            false
        }
        fn enter_while_loop(&mut self, _: &WhileLoop) -> bool {
            false
        }
        fn enter_return_stmt(&mut self, _: &ReturnStmt) -> bool {
            false
        }
        fn enter_log_stmt(&mut self, _: &LogStmt) -> bool {
            false
        }
        fn enter_log_arg(&mut self, _: &LogArg) -> bool {
            false
        }
        fn enter_assert_stmt(&mut self, _: &AssertStmt) -> bool {
            false
        }
        fn enter_expression(&mut self, _: &Expression) -> bool {
            false
        }
        fn enter_anonymous_comp(&mut self, _: &AnonymousComp) -> bool {
            false
        }
        fn enter_anon_comp_input(&mut self, _: &AnonCompInput) -> bool {
            false
        }
        fn enter_identifier(&mut self, _: &Identifier) -> bool {
            false
        }
    }

    /// Exercises the "enter returns false" early-return path in every
    /// walk_* function by parsing a comprehensive source and calling
    /// each walk function with SkipAllWalker directly on extracted nodes.
    #[test]
    fn walker_skip_all_exercises_every_walk_fn() {
        let src = r#"
            pragma circom 2.2.0;
            include "other.circom";
            bus MyBus() {
                signal input x;
                Inner() inner;
            }
            template T() {
                signal input {tag1} a;
                signal output b;
                var x[3] = [1, 2, 3];
                component c = OtherTemplate();
                signal output MyBus() myBus;
                b <== a;
                x[0] += 1;
                a === b;
                (a, b) <== SomeTemplate()();
                if (x[0]) { x[0] = 1; } else { x[0] = 2; }
                for (var i = 0; i < 10; i++) { x[0] += i; }
                while (x[0]) { x[0] = x[0] - 1; }
                log("msg: ", x[0]);
                assert(x[0]);
                x[0]++;
                x[0]--;
                { var y = 1; }
                b <== Multiplier(1)(a, b);
                b <== A(1)(p <== a, q <== b);
            }
            function f(a, b) {
                return a + b;
            }
            component main {public [a]} = T();
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        // walk_file skip
        let mut s = SkipAllWalker;
        walk_file(&mut s, &file);

        // walk_item skip (for each item)
        for item in &file.items {
            let mut s = SkipAllWalker;
            walk_item(&mut s, item);
        }

        // walk into specific item types
        for item in &file.items {
            let mut s = SkipAllWalker;
            match item {
                Item::Pragma(n) => walk_pragma(&mut s, n),
                Item::Include(n) => walk_include(&mut s, n),
                Item::TemplateDef(n) => {
                    walk_template_def(&mut s, n);
                    walk_block(&mut s, &n.body);
                    for stmt in &n.body.stmts {
                        walk_statement(&mut s, stmt);
                        // Walk into specific statement types
                        match &stmt.kind {
                            StatementKind::VarDecl(n) => {
                                walk_var_decl(&mut s, n);
                                for e in &n.names {
                                    walk_var_decl_entry(&mut s, e);
                                }
                            }
                            StatementKind::SignalDecl(n) => {
                                walk_signal_decl(&mut s, n);
                                for e in &n.names {
                                    walk_signal_decl_entry(&mut s, e);
                                }
                            }
                            StatementKind::ComponentDecl(n) => {
                                walk_component_decl(&mut s, n);
                                for e in &n.names {
                                    walk_component_decl_entry(&mut s, e);
                                }
                            }
                            StatementKind::BusDecl(n) => {
                                walk_bus_instance_decl(&mut s, n);
                                walk_bus_type(&mut s, &n.bus_type);
                            }
                            StatementKind::Assignment(n) => walk_assign_stmt(&mut s, n),
                            StatementKind::CompoundAssign(n) => {
                                walk_compound_assign_stmt(&mut s, n)
                            }
                            StatementKind::ConstraintEq(n) => walk_constraint_eq_stmt(&mut s, n),
                            StatementKind::TupleAssign(n) => walk_tuple_assign_stmt(&mut s, n),
                            StatementKind::IfElse(n) => walk_if_else(&mut s, n),
                            StatementKind::For(n) => walk_for_loop(&mut s, n),
                            StatementKind::While(n) => walk_while_loop(&mut s, n),
                            StatementKind::Log(n) => {
                                walk_log_stmt(&mut s, n);
                                for arg in &n.args {
                                    walk_log_arg(&mut s, arg);
                                }
                            }
                            StatementKind::Assert(n) => walk_assert_stmt(&mut s, n),
                            StatementKind::Return(n) => walk_return_stmt(&mut s, n),
                            StatementKind::Increment(e)
                            | StatementKind::Decrement(e)
                            | StatementKind::Expression(e) => walk_expression(&mut s, e),
                            StatementKind::Block(blk) => walk_block(&mut s, blk),
                            StatementKind::Error => {}
                        }
                    }
                }
                Item::FunctionDef(n) => {
                    walk_function_def(&mut s, n);
                    for stmt in &n.body.stmts {
                        walk_statement(&mut s, stmt);
                        if let StatementKind::Return(r) = &stmt.kind {
                            walk_return_stmt(&mut s, r);
                        }
                    }
                }
                Item::BusDef(n) => {
                    walk_bus_def(&mut s, n);
                    for member in &n.body {
                        walk_bus_member(&mut s, member);
                        if let BusMember::Bus(f) = member {
                            walk_bus_field_decl(&mut s, f);
                        }
                    }
                }
                Item::MainComponent(n) => walk_main_component(&mut s, n),
            }
        }

        // Walk anonymous comp and its inputs
        for item in &file.items {
            if let Item::TemplateDef(t) = item {
                for stmt in &t.body.stmts {
                    if let StatementKind::Assignment(a) = &stmt.kind {
                        if let ExpressionKind::AnonymousComp(comp) = a.rhs.kind.as_ref() {
                            let mut s = SkipAllWalker;
                            walk_anonymous_comp(&mut s, comp);
                            for input in &comp.inputs {
                                walk_anon_comp_input(&mut s, input);
                            }
                        }
                    }
                }
            }
        }

        // Walk identifiers
        for item in &file.items {
            if let Item::TemplateDef(t) = item {
                let mut s = SkipAllWalker;
                walk_identifier(&mut s, &t.name);
            }
        }
    }

    struct EventCollector {
        events: Vec<Event>,
    }

    impl EventCollector {
        fn new() -> Self {
            Self { events: vec![] }
        }
    }

    impl Walker for EventCollector {
        fn enter_file(&mut self, _: &File) -> bool {
            self.events.push(Event::Enter("File"));
            true
        }
        fn leave_file(&mut self, _: &File) {
            self.events.push(Event::Leave("File"));
        }
        fn enter_item(&mut self, _: &Item) -> bool {
            self.events.push(Event::Enter("Item"));
            true
        }
        fn leave_item(&mut self, _: &Item) {
            self.events.push(Event::Leave("Item"));
        }
        fn enter_template_def(&mut self, _: &TemplateDef) -> bool {
            self.events.push(Event::Enter("TemplateDef"));
            true
        }
        fn leave_template_def(&mut self, _: &TemplateDef) {
            self.events.push(Event::Leave("TemplateDef"));
        }
        fn enter_block(&mut self, _: &Block) -> bool {
            self.events.push(Event::Enter("Block"));
            true
        }
        fn leave_block(&mut self, _: &Block) {
            self.events.push(Event::Leave("Block"));
        }
        fn enter_statement(&mut self, _: &Statement) -> bool {
            self.events.push(Event::Enter("Statement"));
            true
        }
        fn leave_statement(&mut self, _: &Statement) {
            self.events.push(Event::Leave("Statement"));
        }
        fn enter_var_decl(&mut self, _: &VarDecl) -> bool {
            self.events.push(Event::Enter("VarDecl"));
            true
        }
        fn leave_var_decl(&mut self, _: &VarDecl) {
            self.events.push(Event::Leave("VarDecl"));
        }
        fn enter_signal_decl(&mut self, _: &SignalDecl) -> bool {
            self.events.push(Event::Enter("SignalDecl"));
            true
        }
        fn leave_signal_decl(&mut self, _: &SignalDecl) {
            self.events.push(Event::Leave("SignalDecl"));
        }
        fn enter_expression(&mut self, _: &Expression) -> bool {
            self.events.push(Event::Enter("Expression"));
            true
        }
        fn leave_expression(&mut self, _: &Expression) {
            self.events.push(Event::Leave("Expression"));
        }
        fn enter_for_loop(&mut self, _: &ForLoop) -> bool {
            self.events.push(Event::Enter("ForLoop"));
            true
        }
        fn leave_for_loop(&mut self, _: &ForLoop) {
            self.events.push(Event::Leave("ForLoop"));
        }
        fn enter_if_else(&mut self, _: &IfElse) -> bool {
            self.events.push(Event::Enter("IfElse"));
            true
        }
        fn leave_if_else(&mut self, _: &IfElse) {
            self.events.push(Event::Leave("IfElse"));
        }
        fn enter_identifier(&mut self, _: &Identifier) -> bool {
            self.events.push(Event::Enter("Identifier"));
            true
        }
        fn leave_identifier(&mut self, _: &Identifier) {
            self.events.push(Event::Leave("Identifier"));
        }
    }

    #[test]
    fn walker_enter_leave_pairs() {
        let src = r#"
            template T() {
                var x = 1;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut collector = EventCollector::new();
        walk_file(&mut collector, &file);

        // Every Enter must have a matching Leave in reverse order
        let enters: Vec<_> = collector
            .events
            .iter()
            .filter(|e| matches!(e, Event::Enter(_)))
            .collect();
        let leaves: Vec<_> = collector
            .events
            .iter()
            .filter(|e| matches!(e, Event::Leave(_)))
            .collect();
        assert_eq!(enters.len(), leaves.len());

        // File is entered first and left last
        assert_eq!(collector.events.first(), Some(&Event::Enter("File")));
        assert_eq!(collector.events.last(), Some(&Event::Leave("File")));
    }

    #[test]
    fn walker_preorder_postorder() {
        let src = r#"
            template T() {
                var x = 1;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut collector = EventCollector::new();
        walk_file(&mut collector, &file);

        // TemplateDef is entered before Block
        let enter_template = collector
            .events
            .iter()
            .position(|e| *e == Event::Enter("TemplateDef"))
            .unwrap();
        let enter_block = collector
            .events
            .iter()
            .position(|e| *e == Event::Enter("Block"))
            .unwrap();
        let leave_block = collector
            .events
            .iter()
            .position(|e| *e == Event::Leave("Block"))
            .unwrap();
        let leave_template = collector
            .events
            .iter()
            .position(|e| *e == Event::Leave("TemplateDef"))
            .unwrap();

        // Pre-order: Enter(TemplateDef) < Enter(Block)
        assert!(enter_template < enter_block);
        // Post-order: Leave(Block) < Leave(TemplateDef)
        assert!(leave_block < leave_template);
    }

    #[test]
    fn walker_skip_children() {
        let src = r#"
            template T() {
                var x = 1;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        /// Walker that skips Block children
        struct SkipBlock {
            events: Vec<Event>,
        }

        impl Walker for SkipBlock {
            fn enter_block(&mut self, _: &Block) -> bool {
                self.events.push(Event::Enter("Block"));
                false // skip children
            }
            fn leave_block(&mut self, _: &Block) {
                self.events.push(Event::Leave("Block"));
            }
            fn enter_statement(&mut self, _: &Statement) -> bool {
                self.events.push(Event::Enter("Statement"));
                true
            }
            fn leave_statement(&mut self, _: &Statement) {
                self.events.push(Event::Leave("Statement"));
            }
        }

        let mut w = SkipBlock { events: vec![] };
        walk_file(&mut w, &file);

        // Block should be entered and left, but no Statement events
        assert!(w.events.contains(&Event::Enter("Block")));
        assert!(w.events.contains(&Event::Leave("Block")));
        assert!(!w.events.contains(&Event::Enter("Statement")));
    }

    #[test]
    fn walker_deeply_nested() {
        // Deeply nested for loops
        let src = r#"
            template T() {
                for (var i = 0; i < 10; i++) {
                    for (var j = 0; j < 10; j++) {
                        for (var k = 0; k < 10; k++) {
                            if (i) {
                                if (j) {
                                    var x = k;
                                }
                            }
                        }
                    }
                }
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct DepthTracker {
            depth: usize,
            max_depth: usize,
        }

        impl Walker for DepthTracker {
            fn enter_block(&mut self, _: &Block) -> bool {
                self.depth += 1;
                self.max_depth = self.max_depth.max(self.depth);
                true
            }
            fn leave_block(&mut self, _: &Block) {
                self.depth -= 1;
            }
        }

        let mut tracker = DepthTracker {
            depth: 0,
            max_depth: 0,
        };
        walk_file(&mut tracker, &file);

        // template body + 3 for bodies + 2 if bodies = 6 levels
        assert!(tracker.max_depth >= 6);
        assert_eq!(tracker.depth, 0); // balanced
    }

    #[test]
    fn walker_visits_bus_def() {
        let src = r#"
            bus MyBus() {
                signal input x;
                signal output y;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct BusWalker {
            events: Vec<Event>,
        }

        impl Walker for BusWalker {
            fn enter_bus_def(&mut self, _: &BusDef) -> bool {
                self.events.push(Event::Enter("BusDef"));
                true
            }
            fn leave_bus_def(&mut self, _: &BusDef) {
                self.events.push(Event::Leave("BusDef"));
            }
            fn enter_bus_member(&mut self, _: &BusMember) -> bool {
                self.events.push(Event::Enter("BusMember"));
                true
            }
            fn leave_bus_member(&mut self, _: &BusMember) {
                self.events.push(Event::Leave("BusMember"));
            }
            fn enter_signal_decl(&mut self, _: &SignalDecl) -> bool {
                self.events.push(Event::Enter("SignalDecl"));
                true
            }
            fn leave_signal_decl(&mut self, _: &SignalDecl) {
                self.events.push(Event::Leave("SignalDecl"));
            }
            fn enter_identifier(&mut self, _: &Identifier) -> bool {
                self.events.push(Event::Enter("Identifier"));
                true
            }
            fn leave_identifier(&mut self, _: &Identifier) {
                self.events.push(Event::Leave("Identifier"));
            }
        }

        let mut w = BusWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("BusDef")));
        assert!(w.events.contains(&Event::Leave("BusDef")));
        assert_eq!(
            w.events
                .iter()
                .filter(|e| **e == Event::Enter("BusMember"))
                .count(),
            2
        );
        // Verify enter/leave pairing
        let enters = w
            .events
            .iter()
            .filter(|e| matches!(e, Event::Enter(_)))
            .count();
        let leaves = w
            .events
            .iter()
            .filter(|e| matches!(e, Event::Leave(_)))
            .count();
        assert_eq!(enters, leaves);
    }

    #[test]
    fn walker_visits_bus_field_decl() {
        let src = r#"
            bus Outer() {
                Inner() inner_field;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct BusFieldWalker {
            events: Vec<Event>,
        }

        impl Walker for BusFieldWalker {
            fn enter_bus_field_decl(&mut self, _: &BusFieldDecl) -> bool {
                self.events.push(Event::Enter("BusFieldDecl"));
                true
            }
            fn leave_bus_field_decl(&mut self, _: &BusFieldDecl) {
                self.events.push(Event::Leave("BusFieldDecl"));
            }
            fn enter_bus_type(&mut self, _: &BusType) -> bool {
                self.events.push(Event::Enter("BusType"));
                true
            }
            fn leave_bus_type(&mut self, _: &BusType) {
                self.events.push(Event::Leave("BusType"));
            }
            fn enter_identifier(&mut self, _: &Identifier) -> bool {
                self.events.push(Event::Enter("Identifier"));
                true
            }
            fn leave_identifier(&mut self, _: &Identifier) {
                self.events.push(Event::Leave("Identifier"));
            }
        }

        let mut w = BusFieldWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("BusFieldDecl")));
        assert!(w.events.contains(&Event::Leave("BusFieldDecl")));
        assert!(w.events.contains(&Event::Enter("BusType")));
        assert!(w.events.contains(&Event::Leave("BusType")));
    }

    #[test]
    fn walker_visits_bus_instance_decl() {
        let src = r#"
            template T() {
                signal output MyBus() myBus;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct BusInstWalker {
            events: Vec<Event>,
        }

        impl Walker for BusInstWalker {
            fn enter_bus_instance_decl(&mut self, _: &BusInstanceDecl) -> bool {
                self.events.push(Event::Enter("BusInstanceDecl"));
                true
            }
            fn leave_bus_instance_decl(&mut self, _: &BusInstanceDecl) {
                self.events.push(Event::Leave("BusInstanceDecl"));
            }
            fn enter_bus_type(&mut self, _: &BusType) -> bool {
                self.events.push(Event::Enter("BusType"));
                true
            }
            fn leave_bus_type(&mut self, _: &BusType) {
                self.events.push(Event::Leave("BusType"));
            }
            fn enter_identifier(&mut self, _: &Identifier) -> bool {
                self.events.push(Event::Enter("Identifier"));
                true
            }
            fn leave_identifier(&mut self, _: &Identifier) {
                self.events.push(Event::Leave("Identifier"));
            }
        }

        let mut w = BusInstWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("BusInstanceDecl")));
        assert!(w.events.contains(&Event::Leave("BusInstanceDecl")));
        assert!(w.events.contains(&Event::Enter("BusType")));
    }

    #[test]
    fn walker_visits_tuple_assign() {
        let src = r#"
            template T() {
                var a;
                var b;
                (a, b) <== SomeTemplate()();
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct TupleWalker {
            events: Vec<Event>,
        }

        impl Walker for TupleWalker {
            fn enter_tuple_assign_stmt(&mut self, _: &TupleAssignStmt) -> bool {
                self.events.push(Event::Enter("TupleAssign"));
                true
            }
            fn leave_tuple_assign_stmt(&mut self, _: &TupleAssignStmt) {
                self.events.push(Event::Leave("TupleAssign"));
            }
            fn enter_expression(&mut self, _: &Expression) -> bool {
                self.events.push(Event::Enter("Expression"));
                true
            }
            fn leave_expression(&mut self, _: &Expression) {
                self.events.push(Event::Leave("Expression"));
            }
        }

        let mut w = TupleWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("TupleAssign")));
        assert!(w.events.contains(&Event::Leave("TupleAssign")));
    }

    #[test]
    fn walker_visits_compound_assign() {
        let src = r#"
            template T() {
                var x = 0;
                x += 1;
                x -= 2;
                x *= 3;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct CompoundWalker {
            count: usize,
        }

        impl Walker for CompoundWalker {
            fn enter_compound_assign_stmt(&mut self, _: &CompoundAssignStmt) -> bool {
                self.count += 1;
                true
            }
        }

        let mut w = CompoundWalker { count: 0 };
        walk_file(&mut w, &file);
        assert_eq!(w.count, 3);
    }

    #[test]
    fn walker_visits_log_stmt_with_mixed_args() {
        let src = r#"
            template T() {
                signal input x;
                log("value: ", x, " done");
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct LogWalker {
            events: Vec<Event>,
        }

        impl Walker for LogWalker {
            fn enter_log_stmt(&mut self, _: &LogStmt) -> bool {
                self.events.push(Event::Enter("LogStmt"));
                true
            }
            fn leave_log_stmt(&mut self, _: &LogStmt) {
                self.events.push(Event::Leave("LogStmt"));
            }
            fn enter_log_arg(&mut self, _: &LogArg) -> bool {
                self.events.push(Event::Enter("LogArg"));
                true
            }
            fn leave_log_arg(&mut self, _: &LogArg) {
                self.events.push(Event::Leave("LogArg"));
            }
            fn enter_expression(&mut self, _: &Expression) -> bool {
                self.events.push(Event::Enter("Expression"));
                true
            }
            fn leave_expression(&mut self, _: &Expression) {
                self.events.push(Event::Leave("Expression"));
            }
        }

        let mut w = LogWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("LogStmt")));
        assert!(w.events.contains(&Event::Leave("LogStmt")));
        // 3 log args: string, expr, string
        assert_eq!(
            w.events
                .iter()
                .filter(|e| **e == Event::Enter("LogArg"))
                .count(),
            3
        );
    }

    #[test]
    fn walker_visits_assert_stmt() {
        let src = r#"
            template T() {
                signal input x;
                assert(x);
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct AssertWalker {
            events: Vec<Event>,
        }

        impl Walker for AssertWalker {
            fn enter_assert_stmt(&mut self, _: &AssertStmt) -> bool {
                self.events.push(Event::Enter("AssertStmt"));
                true
            }
            fn leave_assert_stmt(&mut self, _: &AssertStmt) {
                self.events.push(Event::Leave("AssertStmt"));
            }
            fn enter_expression(&mut self, _: &Expression) -> bool {
                self.events.push(Event::Enter("Expression"));
                true
            }
            fn leave_expression(&mut self, _: &Expression) {
                self.events.push(Event::Leave("Expression"));
            }
        }

        let mut w = AssertWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("AssertStmt")));
        assert!(w.events.contains(&Event::Leave("AssertStmt")));
    }

    #[test]
    fn walker_visits_anonymous_comp() {
        let src = r#"
            template T() {
                signal output out;
                out <== Multiplier(n)(a, b);
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct AnonWalker {
            events: Vec<Event>,
        }

        impl Walker for AnonWalker {
            fn enter_anonymous_comp(&mut self, _: &AnonymousComp) -> bool {
                self.events.push(Event::Enter("AnonymousComp"));
                true
            }
            fn leave_anonymous_comp(&mut self, _: &AnonymousComp) {
                self.events.push(Event::Leave("AnonymousComp"));
            }
            fn enter_anon_comp_input(&mut self, _: &AnonCompInput) -> bool {
                self.events.push(Event::Enter("AnonCompInput"));
                true
            }
            fn leave_anon_comp_input(&mut self, _: &AnonCompInput) {
                self.events.push(Event::Leave("AnonCompInput"));
            }
            fn enter_expression(&mut self, _: &Expression) -> bool {
                self.events.push(Event::Enter("Expression"));
                true
            }
            fn leave_expression(&mut self, _: &Expression) {
                self.events.push(Event::Leave("Expression"));
            }
        }

        let mut w = AnonWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("AnonymousComp")));
        assert!(w.events.contains(&Event::Leave("AnonymousComp")));
        assert_eq!(
            w.events
                .iter()
                .filter(|e| **e == Event::Enter("AnonCompInput"))
                .count(),
            2
        );
    }

    #[test]
    fn walker_visits_anonymous_comp_named_inputs() {
        let src = r#"
            template T() {
                signal output out;
                out <== A(n)(x <== in1, y <== in2);
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct NamedAnonWalker {
            events: Vec<Event>,
        }

        impl Walker for NamedAnonWalker {
            fn enter_anon_comp_input(&mut self, _: &AnonCompInput) -> bool {
                self.events.push(Event::Enter("AnonCompInput"));
                true
            }
            fn leave_anon_comp_input(&mut self, _: &AnonCompInput) {
                self.events.push(Event::Leave("AnonCompInput"));
            }
            fn enter_identifier(&mut self, _: &Identifier) -> bool {
                self.events.push(Event::Enter("Identifier"));
                true
            }
            fn leave_identifier(&mut self, _: &Identifier) {
                self.events.push(Event::Leave("Identifier"));
            }
        }

        let mut w = NamedAnonWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert_eq!(
            w.events
                .iter()
                .filter(|e| **e == Event::Enter("AnonCompInput"))
                .count(),
            2
        );
        // Named inputs have identifier children
        assert!(w.events.contains(&Event::Enter("Identifier")));
    }

    #[test]
    fn walker_skip_bus_def_children() {
        let src = r#"
            bus MyBus() {
                signal input x;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct SkipBus {
            events: Vec<Event>,
        }

        impl Walker for SkipBus {
            fn enter_bus_def(&mut self, _: &BusDef) -> bool {
                self.events.push(Event::Enter("BusDef"));
                false // skip children
            }
            fn leave_bus_def(&mut self, _: &BusDef) {
                self.events.push(Event::Leave("BusDef"));
            }
            fn enter_bus_member(&mut self, _: &BusMember) -> bool {
                self.events.push(Event::Enter("BusMember"));
                true
            }
            fn leave_bus_member(&mut self, _: &BusMember) {
                self.events.push(Event::Leave("BusMember"));
            }
        }

        let mut w = SkipBus { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("BusDef")));
        assert!(w.events.contains(&Event::Leave("BusDef")));
        // Members should be skipped
        assert!(!w.events.contains(&Event::Enter("BusMember")));
    }

    #[test]
    fn walker_visits_main_component() {
        let src = r#"
            template T() { signal input x; }
            component main {public [x]} = T();
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct MainWalker {
            events: Vec<Event>,
        }

        impl Walker for MainWalker {
            fn enter_main_component(&mut self, _: &MainComponent) -> bool {
                self.events.push(Event::Enter("MainComponent"));
                true
            }
            fn leave_main_component(&mut self, _: &MainComponent) {
                self.events.push(Event::Leave("MainComponent"));
            }
            fn enter_identifier(&mut self, _: &Identifier) -> bool {
                self.events.push(Event::Enter("Identifier"));
                true
            }
            fn leave_identifier(&mut self, _: &Identifier) {
                self.events.push(Event::Leave("Identifier"));
            }
        }

        let mut w = MainWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("MainComponent")));
        assert!(w.events.contains(&Event::Leave("MainComponent")));
    }

    #[test]
    fn walker_visits_constraint_eq() {
        let src = r#"
            template T() {
                signal input a;
                signal input b;
                a === b;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct ConstraintWalker {
            count: usize,
        }

        impl Walker for ConstraintWalker {
            fn enter_constraint_eq_stmt(&mut self, _: &ConstraintEqStmt) -> bool {
                self.count += 1;
                true
            }
        }

        let mut w = ConstraintWalker { count: 0 };
        walk_file(&mut w, &file);
        assert_eq!(w.count, 1);
    }

    #[test]
    fn walker_visits_return_stmt() {
        let src = r#"
            function f() {
                return 42;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct ReturnWalker {
            events: Vec<Event>,
        }

        impl Walker for ReturnWalker {
            fn enter_return_stmt(&mut self, _: &ReturnStmt) -> bool {
                self.events.push(Event::Enter("ReturnStmt"));
                true
            }
            fn leave_return_stmt(&mut self, _: &ReturnStmt) {
                self.events.push(Event::Leave("ReturnStmt"));
            }
        }

        let mut w = ReturnWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("ReturnStmt")));
        assert!(w.events.contains(&Event::Leave("ReturnStmt")));
    }

    #[test]
    fn walker_visits_while_loop() {
        let src = r#"
            template T() {
                var x = 10;
                while (x) {
                    x = x - 1;
                }
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct WhileWalker {
            events: Vec<Event>,
        }

        impl Walker for WhileWalker {
            fn enter_while_loop(&mut self, _: &WhileLoop) -> bool {
                self.events.push(Event::Enter("WhileLoop"));
                true
            }
            fn leave_while_loop(&mut self, _: &WhileLoop) {
                self.events.push(Event::Leave("WhileLoop"));
            }
        }

        let mut w = WhileWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("WhileLoop")));
        assert!(w.events.contains(&Event::Leave("WhileLoop")));
    }

    #[test]
    fn walker_visits_component_decl() {
        let src = r#"
            template T() {
                component c = OtherTemplate();
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct CompDeclWalker {
            events: Vec<Event>,
        }

        impl Walker for CompDeclWalker {
            fn enter_component_decl(&mut self, _: &ComponentDecl) -> bool {
                self.events.push(Event::Enter("ComponentDecl"));
                true
            }
            fn leave_component_decl(&mut self, _: &ComponentDecl) {
                self.events.push(Event::Leave("ComponentDecl"));
            }
            fn enter_component_decl_entry(&mut self, _: &ComponentDeclEntry) -> bool {
                self.events.push(Event::Enter("ComponentDeclEntry"));
                true
            }
            fn leave_component_decl_entry(&mut self, _: &ComponentDeclEntry) {
                self.events.push(Event::Leave("ComponentDeclEntry"));
            }
        }

        let mut w = CompDeclWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("ComponentDecl")));
        assert!(w.events.contains(&Event::Leave("ComponentDecl")));
        assert!(w.events.contains(&Event::Enter("ComponentDeclEntry")));
        assert!(w.events.contains(&Event::Leave("ComponentDeclEntry")));
    }

    #[test]
    fn walker_visits_var_decl_entry() {
        let src = r#"
            template T() {
                var x[3] = [1, 2, 3];
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct VarDeclWalker {
            events: Vec<Event>,
        }

        impl Walker for VarDeclWalker {
            fn enter_var_decl(&mut self, _: &VarDecl) -> bool {
                self.events.push(Event::Enter("VarDecl"));
                true
            }
            fn leave_var_decl(&mut self, _: &VarDecl) {
                self.events.push(Event::Leave("VarDecl"));
            }
            fn enter_var_decl_entry(&mut self, _: &VarDeclEntry) -> bool {
                self.events.push(Event::Enter("VarDeclEntry"));
                true
            }
            fn leave_var_decl_entry(&mut self, _: &VarDeclEntry) {
                self.events.push(Event::Leave("VarDeclEntry"));
            }
        }

        let mut w = VarDeclWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("VarDecl")));
        assert!(w.events.contains(&Event::Enter("VarDeclEntry")));
    }

    #[test]
    fn walker_visits_signal_decl_entry() {
        let src = r#"
            template T() {
                signal input a[2];
                signal output b <== 1;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct SigDeclWalker {
            events: Vec<Event>,
        }

        impl Walker for SigDeclWalker {
            fn enter_signal_decl(&mut self, _: &SignalDecl) -> bool {
                self.events.push(Event::Enter("SignalDecl"));
                true
            }
            fn leave_signal_decl(&mut self, _: &SignalDecl) {
                self.events.push(Event::Leave("SignalDecl"));
            }
            fn enter_signal_decl_entry(&mut self, _: &SignalDeclEntry) -> bool {
                self.events.push(Event::Enter("SignalDeclEntry"));
                true
            }
            fn leave_signal_decl_entry(&mut self, _: &SignalDeclEntry) {
                self.events.push(Event::Leave("SignalDeclEntry"));
            }
        }

        let mut w = SigDeclWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert_eq!(
            w.events
                .iter()
                .filter(|e| **e == Event::Enter("SignalDecl"))
                .count(),
            2
        );
        assert_eq!(
            w.events
                .iter()
                .filter(|e| **e == Event::Enter("SignalDeclEntry"))
                .count(),
            2
        );
    }

    #[test]
    fn walker_visits_pragma_and_include() {
        let src = r#"
            pragma circom 2.0.0;
            include "other.circom";
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct PragmaIncludeWalker {
            events: Vec<Event>,
        }

        impl Walker for PragmaIncludeWalker {
            fn enter_pragma(&mut self, _: &Pragma) -> bool {
                self.events.push(Event::Enter("Pragma"));
                true
            }
            fn leave_pragma(&mut self, _: &Pragma) {
                self.events.push(Event::Leave("Pragma"));
            }
            fn enter_include(&mut self, _: &Include) -> bool {
                self.events.push(Event::Enter("Include"));
                true
            }
            fn leave_include(&mut self, _: &Include) {
                self.events.push(Event::Leave("Include"));
            }
        }

        let mut w = PragmaIncludeWalker { events: vec![] };
        walk_file(&mut w, &file);

        assert!(w.events.contains(&Event::Enter("Pragma")));
        assert!(w.events.contains(&Event::Leave("Pragma")));
        assert!(w.events.contains(&Event::Enter("Include")));
        assert!(w.events.contains(&Event::Leave("Include")));
    }

    #[test]
    fn walker_traverses_all_circomlib_fixtures() {
        use std::fs;

        let fixtures_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
        let mut total_nodes = 0;

        for entry in fs::read_dir(fixtures_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().map(|e| e == "circom").unwrap_or(false) {
                let src = fs::read_to_string(&path).unwrap();
                let (file, _) = parser::parse(&src);

                struct Counter(usize);
                impl Walker for Counter {
                    fn enter_expression(&mut self, _: &Expression) -> bool {
                        self.0 += 1;
                        true
                    }
                }

                let mut counter = Counter(0);
                walk_file(&mut counter, &file);
                total_nodes += counter.0;
            }
        }

        assert!(
            total_nodes > 100,
            "should traverse many expression nodes across all fixtures"
        );
    }
}
