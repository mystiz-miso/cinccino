//! Visitor pattern for Circom AST traversal.
//!
//! The [`Visitor`] trait provides a `visit_*` method for every AST node type.
//! Default implementations call the corresponding `walk_*` free function to
//! traverse children. Override a method to add custom behavior, then
//! optionally call the `walk_*` function to continue into children.
//!
//! # Example
//!
//! ```
//! use cinccino::visitor::{self, Visitor};
//! use cinccino::ast::*;
//!
//! struct TemplateCounter(usize);
//!
//! impl Visitor for TemplateCounter {
//!     fn visit_template_def(&mut self, node: &TemplateDef) {
//!         self.0 += 1;
//!         visitor::walk_template_def(self, node);
//!     }
//! }
//! ```

use crate::ast::*;

/// Trait for visiting AST nodes. Each method has a default implementation
/// that traverses into child nodes via the corresponding `walk_*` function.
pub trait Visitor {
    fn visit_file(&mut self, node: &File) {
        walk_file(self, node);
    }
    fn visit_item(&mut self, node: &Item) {
        walk_item(self, node);
    }
    fn visit_pragma(&mut self, node: &Pragma) {
        walk_pragma(self, node);
    }
    fn visit_include(&mut self, node: &Include) {
        walk_include(self, node);
    }
    fn visit_template_def(&mut self, node: &TemplateDef) {
        walk_template_def(self, node);
    }
    fn visit_function_def(&mut self, node: &FunctionDef) {
        walk_function_def(self, node);
    }
    fn visit_bus_def(&mut self, node: &BusDef) {
        walk_bus_def(self, node);
    }
    fn visit_bus_member(&mut self, node: &BusMember) {
        walk_bus_member(self, node);
    }
    fn visit_bus_field_decl(&mut self, node: &BusFieldDecl) {
        walk_bus_field_decl(self, node);
    }
    fn visit_main_component(&mut self, node: &MainComponent) {
        walk_main_component(self, node);
    }
    fn visit_block(&mut self, node: &Block) {
        walk_block(self, node);
    }
    fn visit_statement(&mut self, node: &Statement) {
        walk_statement(self, node);
    }
    fn visit_var_decl(&mut self, node: &VarDecl) {
        walk_var_decl(self, node);
    }
    fn visit_var_decl_entry(&mut self, node: &VarDeclEntry) {
        walk_var_decl_entry(self, node);
    }
    fn visit_signal_decl(&mut self, node: &SignalDecl) {
        walk_signal_decl(self, node);
    }
    fn visit_signal_decl_entry(&mut self, node: &SignalDeclEntry) {
        walk_signal_decl_entry(self, node);
    }
    fn visit_component_decl(&mut self, node: &ComponentDecl) {
        walk_component_decl(self, node);
    }
    fn visit_component_decl_entry(&mut self, node: &ComponentDeclEntry) {
        walk_component_decl_entry(self, node);
    }
    fn visit_bus_instance_decl(&mut self, node: &BusInstanceDecl) {
        walk_bus_instance_decl(self, node);
    }
    fn visit_bus_type(&mut self, node: &BusType) {
        walk_bus_type(self, node);
    }
    fn visit_assign_stmt(&mut self, node: &AssignStmt) {
        walk_assign_stmt(self, node);
    }
    fn visit_compound_assign_stmt(&mut self, node: &CompoundAssignStmt) {
        walk_compound_assign_stmt(self, node);
    }
    fn visit_constraint_eq_stmt(&mut self, node: &ConstraintEqStmt) {
        walk_constraint_eq_stmt(self, node);
    }
    fn visit_tuple_assign_stmt(&mut self, node: &TupleAssignStmt) {
        walk_tuple_assign_stmt(self, node);
    }
    fn visit_if_else(&mut self, node: &IfElse) {
        walk_if_else(self, node);
    }
    fn visit_for_loop(&mut self, node: &ForLoop) {
        walk_for_loop(self, node);
    }
    fn visit_while_loop(&mut self, node: &WhileLoop) {
        walk_while_loop(self, node);
    }
    fn visit_return_stmt(&mut self, node: &ReturnStmt) {
        walk_return_stmt(self, node);
    }
    fn visit_log_stmt(&mut self, node: &LogStmt) {
        walk_log_stmt(self, node);
    }
    fn visit_log_arg(&mut self, node: &LogArg) {
        walk_log_arg(self, node);
    }
    fn visit_assert_stmt(&mut self, node: &AssertStmt) {
        walk_assert_stmt(self, node);
    }
    fn visit_expression(&mut self, node: &Expression) {
        walk_expression(self, node);
    }
    fn visit_anonymous_comp(&mut self, node: &AnonymousComp) {
        walk_anonymous_comp(self, node);
    }
    fn visit_anon_comp_input(&mut self, node: &AnonCompInput) {
        walk_anon_comp_input(self, node);
    }
    fn visit_increment(&mut self, node: &Expression) {
        walk_increment(self, node);
    }
    fn visit_decrement(&mut self, node: &Expression) {
        walk_decrement(self, node);
    }
    fn visit_identifier(&mut self, node: &Identifier) {
        walk_identifier(self, node);
    }
}

// ── Walk functions ─────────────────────────────────────────────────────

pub fn walk_file<V: Visitor + ?Sized>(v: &mut V, node: &File) {
    for item in &node.items {
        v.visit_item(item);
    }
}

pub fn walk_item<V: Visitor + ?Sized>(v: &mut V, node: &Item) {
    match node {
        Item::Pragma(n) => v.visit_pragma(n),
        Item::Include(n) => v.visit_include(n),
        Item::TemplateDef(n) => v.visit_template_def(n),
        Item::FunctionDef(n) => v.visit_function_def(n),
        Item::BusDef(n) => v.visit_bus_def(n),
        Item::MainComponent(n) => v.visit_main_component(n),
    }
}

pub fn walk_pragma<V: Visitor + ?Sized>(_v: &mut V, _node: &Pragma) {
    // Pragma has no child AST nodes to traverse
}

pub fn walk_include<V: Visitor + ?Sized>(_v: &mut V, _node: &Include) {
    // Include has no child AST nodes to traverse
}

pub fn walk_identifier<V: Visitor + ?Sized>(_v: &mut V, _node: &Identifier) {
    // Identifier is a leaf node
}

pub fn walk_template_def<V: Visitor + ?Sized>(v: &mut V, node: &TemplateDef) {
    v.visit_identifier(&node.name);
    for param in &node.params {
        v.visit_identifier(param);
    }
    v.visit_block(&node.body);
}

pub fn walk_function_def<V: Visitor + ?Sized>(v: &mut V, node: &FunctionDef) {
    v.visit_identifier(&node.name);
    for param in &node.params {
        v.visit_identifier(param);
    }
    v.visit_block(&node.body);
}

pub fn walk_bus_def<V: Visitor + ?Sized>(v: &mut V, node: &BusDef) {
    v.visit_identifier(&node.name);
    for param in &node.params {
        v.visit_identifier(param);
    }
    for member in &node.body {
        v.visit_bus_member(member);
    }
}

pub fn walk_bus_member<V: Visitor + ?Sized>(v: &mut V, node: &BusMember) {
    match node {
        BusMember::Signal(n) => v.visit_signal_decl(n),
        BusMember::Bus(n) => v.visit_bus_field_decl(n),
    }
}

pub fn walk_bus_field_decl<V: Visitor + ?Sized>(v: &mut V, node: &BusFieldDecl) {
    v.visit_bus_type(&node.bus_type);
    for tag in &node.tags {
        v.visit_identifier(tag);
    }
    v.visit_identifier(&node.name);
    for dim in &node.dimensions {
        v.visit_expression(dim);
    }
}

pub fn walk_main_component<V: Visitor + ?Sized>(v: &mut V, node: &MainComponent) {
    for sig in &node.public_signals {
        v.visit_identifier(sig);
    }
    v.visit_expression(&node.expr);
}

pub fn walk_block<V: Visitor + ?Sized>(v: &mut V, node: &Block) {
    for stmt in &node.stmts {
        v.visit_statement(stmt);
    }
}

pub fn walk_statement<V: Visitor + ?Sized>(v: &mut V, node: &Statement) {
    match &node.kind {
        StatementKind::VarDecl(n) => v.visit_var_decl(n),
        StatementKind::SignalDecl(n) => v.visit_signal_decl(n),
        StatementKind::ComponentDecl(n) => v.visit_component_decl(n),
        StatementKind::BusDecl(n) => v.visit_bus_instance_decl(n),
        StatementKind::Assignment(n) => v.visit_assign_stmt(n),
        StatementKind::CompoundAssign(n) => v.visit_compound_assign_stmt(n),
        StatementKind::ConstraintEq(n) => v.visit_constraint_eq_stmt(n),
        StatementKind::TupleAssign(n) => v.visit_tuple_assign_stmt(n),
        StatementKind::IfElse(n) => v.visit_if_else(n),
        StatementKind::For(n) => v.visit_for_loop(n),
        StatementKind::While(n) => v.visit_while_loop(n),
        StatementKind::Return(n) => v.visit_return_stmt(n),
        StatementKind::Log(n) => v.visit_log_stmt(n),
        StatementKind::Assert(n) => v.visit_assert_stmt(n),
        StatementKind::Increment(expr) => v.visit_increment(expr),
        StatementKind::Decrement(expr) => v.visit_decrement(expr),
        StatementKind::Expression(expr) => v.visit_expression(expr),
        StatementKind::Block(blk) => v.visit_block(blk),
        StatementKind::Error => {}
    }
}

pub fn walk_var_decl<V: Visitor + ?Sized>(v: &mut V, node: &VarDecl) {
    for entry in &node.names {
        v.visit_var_decl_entry(entry);
    }
}

pub fn walk_var_decl_entry<V: Visitor + ?Sized>(v: &mut V, node: &VarDeclEntry) {
    v.visit_identifier(&node.name);
    for dim in &node.dimensions {
        v.visit_expression(dim);
    }
    if let Some(init) = &node.init {
        v.visit_expression(init);
    }
}

pub fn walk_signal_decl<V: Visitor + ?Sized>(v: &mut V, node: &SignalDecl) {
    for tag in &node.tags {
        v.visit_identifier(tag);
    }
    for entry in &node.names {
        v.visit_signal_decl_entry(entry);
    }
}

pub fn walk_signal_decl_entry<V: Visitor + ?Sized>(v: &mut V, node: &SignalDeclEntry) {
    v.visit_identifier(&node.name);
    for dim in &node.dimensions {
        v.visit_expression(dim);
    }
    if let Some((_, init)) = &node.init {
        v.visit_expression(init);
    }
}

pub fn walk_component_decl<V: Visitor + ?Sized>(v: &mut V, node: &ComponentDecl) {
    for entry in &node.names {
        v.visit_component_decl_entry(entry);
    }
}

pub fn walk_component_decl_entry<V: Visitor + ?Sized>(v: &mut V, node: &ComponentDeclEntry) {
    v.visit_identifier(&node.name);
    for dim in &node.dimensions {
        v.visit_expression(dim);
    }
    if let Some(init) = &node.init {
        v.visit_expression(init);
    }
}

pub fn walk_bus_instance_decl<V: Visitor + ?Sized>(v: &mut V, node: &BusInstanceDecl) {
    v.visit_bus_type(&node.bus_type);
    for tag in &node.tags {
        v.visit_identifier(tag);
    }
    v.visit_identifier(&node.name);
    for dim in &node.dimensions {
        v.visit_expression(dim);
    }
    if let Some((_, init)) = &node.init {
        v.visit_expression(init);
    }
}

pub fn walk_bus_type<V: Visitor + ?Sized>(v: &mut V, node: &BusType) {
    v.visit_identifier(&node.name);
    for arg in &node.args {
        v.visit_expression(arg);
    }
}

pub fn walk_assign_stmt<V: Visitor + ?Sized>(v: &mut V, node: &AssignStmt) {
    v.visit_expression(&node.lhs);
    v.visit_expression(&node.rhs);
}

pub fn walk_compound_assign_stmt<V: Visitor + ?Sized>(v: &mut V, node: &CompoundAssignStmt) {
    v.visit_expression(&node.lhs);
    v.visit_expression(&node.rhs);
}

pub fn walk_constraint_eq_stmt<V: Visitor + ?Sized>(v: &mut V, node: &ConstraintEqStmt) {
    v.visit_expression(&node.lhs);
    v.visit_expression(&node.rhs);
}

pub fn walk_tuple_assign_stmt<V: Visitor + ?Sized>(v: &mut V, node: &TupleAssignStmt) {
    for expr in node.targets.iter().flatten() {
        v.visit_expression(expr);
    }
    v.visit_expression(&node.rhs);
}

pub fn walk_if_else<V: Visitor + ?Sized>(v: &mut V, node: &IfElse) {
    v.visit_expression(&node.cond);
    v.visit_block(&node.then_body);
    if let Some(else_body) = &node.else_body {
        v.visit_block(else_body);
    }
}

pub fn walk_for_loop<V: Visitor + ?Sized>(v: &mut V, node: &ForLoop) {
    v.visit_statement(&node.init);
    v.visit_expression(&node.cond);
    v.visit_statement(&node.step);
    v.visit_block(&node.body);
}

pub fn walk_while_loop<V: Visitor + ?Sized>(v: &mut V, node: &WhileLoop) {
    v.visit_expression(&node.cond);
    v.visit_block(&node.body);
}

pub fn walk_return_stmt<V: Visitor + ?Sized>(v: &mut V, node: &ReturnStmt) {
    v.visit_expression(&node.value);
}

pub fn walk_log_stmt<V: Visitor + ?Sized>(v: &mut V, node: &LogStmt) {
    for arg in &node.args {
        v.visit_log_arg(arg);
    }
}

pub fn walk_log_arg<V: Visitor + ?Sized>(v: &mut V, node: &LogArg) {
    match node {
        LogArg::Expr(expr) => v.visit_expression(expr),
        LogArg::String(_) => {}
    }
}

pub fn walk_assert_stmt<V: Visitor + ?Sized>(v: &mut V, node: &AssertStmt) {
    v.visit_expression(&node.expr);
}

pub fn walk_increment<V: Visitor + ?Sized>(v: &mut V, node: &Expression) {
    v.visit_expression(node);
}

pub fn walk_decrement<V: Visitor + ?Sized>(v: &mut V, node: &Expression) {
    v.visit_expression(node);
}

pub fn walk_expression<V: Visitor + ?Sized>(v: &mut V, node: &Expression) {
    match node.kind.as_ref() {
        ExpressionKind::Number(_) | ExpressionKind::Underscore | ExpressionKind::Error => {}
        ExpressionKind::Ident(_) => {}
        ExpressionKind::Unary(_, expr) => v.visit_expression(expr),
        ExpressionKind::Binary(lhs, _, rhs) => {
            v.visit_expression(lhs);
            v.visit_expression(rhs);
        }
        ExpressionKind::Ternary(cond, then_expr, else_expr) => {
            v.visit_expression(cond);
            v.visit_expression(then_expr);
            v.visit_expression(else_expr);
        }
        ExpressionKind::Index(expr, index) => {
            v.visit_expression(expr);
            v.visit_expression(index);
        }
        ExpressionKind::Member(expr, ident) => {
            v.visit_expression(expr);
            v.visit_identifier(ident);
        }
        ExpressionKind::Call(callee, args) => {
            v.visit_expression(callee);
            for arg in args {
                v.visit_expression(arg);
            }
        }
        ExpressionKind::AnonymousComp(comp) => v.visit_anonymous_comp(comp),
        ExpressionKind::ArrayLit(elems) => {
            for elem in elems {
                v.visit_expression(elem);
            }
        }
        ExpressionKind::Paren(expr) => v.visit_expression(expr),
        ExpressionKind::Parallel(expr) => v.visit_expression(expr),
    }
}

pub fn walk_anonymous_comp<V: Visitor + ?Sized>(v: &mut V, node: &AnonymousComp) {
    v.visit_expression(&node.template);
    for arg in &node.template_args {
        v.visit_expression(arg);
    }
    for input in &node.inputs {
        v.visit_anon_comp_input(input);
    }
}

pub fn walk_anon_comp_input<V: Visitor + ?Sized>(v: &mut V, node: &AnonCompInput) {
    match node {
        AnonCompInput::Positional(expr) => v.visit_expression(expr),
        AnonCompInput::Named(ident, expr) => {
            v.visit_identifier(ident);
            v.visit_expression(expr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    /// Collects all visited node types in order.
    struct NodeCollector {
        events: Vec<&'static str>,
    }

    impl NodeCollector {
        fn new() -> Self {
            Self { events: vec![] }
        }
    }

    impl Visitor for NodeCollector {
        fn visit_file(&mut self, node: &File) {
            self.events.push("File");
            walk_file(self, node);
        }
        fn visit_item(&mut self, node: &Item) {
            self.events.push("Item");
            walk_item(self, node);
        }
        fn visit_pragma(&mut self, node: &Pragma) {
            self.events.push("Pragma");
            walk_pragma(self, node);
        }
        fn visit_include(&mut self, node: &Include) {
            self.events.push("Include");
            let _ = node;
        }
        fn visit_template_def(&mut self, node: &TemplateDef) {
            self.events.push("TemplateDef");
            walk_template_def(self, node);
        }
        fn visit_function_def(&mut self, node: &FunctionDef) {
            self.events.push("FunctionDef");
            walk_function_def(self, node);
        }
        fn visit_block(&mut self, node: &Block) {
            self.events.push("Block");
            walk_block(self, node);
        }
        fn visit_statement(&mut self, node: &Statement) {
            self.events.push("Statement");
            walk_statement(self, node);
        }
        fn visit_var_decl(&mut self, node: &VarDecl) {
            self.events.push("VarDecl");
            walk_var_decl(self, node);
        }
        fn visit_signal_decl(&mut self, node: &SignalDecl) {
            self.events.push("SignalDecl");
            walk_signal_decl(self, node);
        }
        fn visit_component_decl(&mut self, node: &ComponentDecl) {
            self.events.push("ComponentDecl");
            walk_component_decl(self, node);
        }
        fn visit_assign_stmt(&mut self, node: &AssignStmt) {
            self.events.push("AssignStmt");
            walk_assign_stmt(self, node);
        }
        fn visit_constraint_eq_stmt(&mut self, node: &ConstraintEqStmt) {
            self.events.push("ConstraintEq");
            walk_constraint_eq_stmt(self, node);
        }
        fn visit_compound_assign_stmt(&mut self, node: &CompoundAssignStmt) {
            self.events.push("CompoundAssign");
            walk_compound_assign_stmt(self, node);
        }
        fn visit_if_else(&mut self, node: &IfElse) {
            self.events.push("IfElse");
            walk_if_else(self, node);
        }
        fn visit_for_loop(&mut self, node: &ForLoop) {
            self.events.push("ForLoop");
            walk_for_loop(self, node);
        }
        fn visit_while_loop(&mut self, node: &WhileLoop) {
            self.events.push("WhileLoop");
            walk_while_loop(self, node);
        }
        fn visit_return_stmt(&mut self, node: &ReturnStmt) {
            self.events.push("ReturnStmt");
            walk_return_stmt(self, node);
        }
        fn visit_log_stmt(&mut self, node: &LogStmt) {
            self.events.push("LogStmt");
            walk_log_stmt(self, node);
        }
        fn visit_assert_stmt(&mut self, node: &AssertStmt) {
            self.events.push("AssertStmt");
            walk_assert_stmt(self, node);
        }
        fn visit_expression(&mut self, node: &Expression) {
            self.events.push("Expression");
            walk_expression(self, node);
        }
        fn visit_identifier(&mut self, node: &Identifier) {
            self.events.push("Identifier");
            let _ = node;
        }
        fn visit_main_component(&mut self, node: &MainComponent) {
            self.events.push("MainComponent");
            walk_main_component(self, node);
        }
    }

    #[test]
    fn visitor_visits_simple_template() {
        let src = r#"
            pragma circom 2.0.0;
            template Foo(n) {
                signal input a;
                signal output b;
                b <== a;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut collector = NodeCollector::new();
        collector.visit_file(&file);

        assert_eq!(collector.events[0], "File");
        assert!(collector.events.contains(&"Pragma"));
        assert!(collector.events.contains(&"TemplateDef"));
        assert!(collector.events.contains(&"SignalDecl"));
        assert!(collector.events.contains(&"AssignStmt"));
    }

    #[test]
    fn visitor_visits_function_def() {
        let src = r#"
            function add(a, b) {
                return a + b;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut collector = NodeCollector::new();
        collector.visit_file(&file);

        assert!(collector.events.contains(&"FunctionDef"));
        assert!(collector.events.contains(&"ReturnStmt"));
        assert!(collector.events.contains(&"Expression")); // a + b
    }

    #[test]
    fn visitor_visits_control_flow() {
        let src = r#"
            template T() {
                var x = 0;
                if (x) { x = 1; }
                for (var i = 0; i < 10; i++) { x += i; }
                while (x) { x = x - 1; }
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut collector = NodeCollector::new();
        collector.visit_file(&file);

        assert!(collector.events.contains(&"IfElse"));
        assert!(collector.events.contains(&"ForLoop"));
        assert!(collector.events.contains(&"WhileLoop"));
        assert!(collector.events.contains(&"VarDecl"));
        assert!(collector.events.contains(&"CompoundAssign"));
    }

    #[test]
    fn visitor_visits_include() {
        let src = r#"
            pragma circom 2.0.0;
            include "other.circom";
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut collector = NodeCollector::new();
        collector.visit_file(&file);

        assert!(collector.events.contains(&"Include"));
    }

    #[test]
    fn visitor_order_is_preorder() {
        let src = r#"
            template T() {
                var x = 1;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut collector = NodeCollector::new();
        collector.visit_file(&file);

        // File > Item > TemplateDef > ... > Block > Statement > VarDecl
        let file_pos = collector.events.iter().position(|e| *e == "File").unwrap();
        let template_pos = collector
            .events
            .iter()
            .position(|e| *e == "TemplateDef")
            .unwrap();
        let block_pos = collector.events.iter().position(|e| *e == "Block").unwrap();
        let var_pos = collector
            .events
            .iter()
            .position(|e| *e == "VarDecl")
            .unwrap();

        assert!(file_pos < template_pos);
        assert!(template_pos < block_pos);
        assert!(block_pos < var_pos);
    }

    /// Count templates using the visitor — verifies the visitor pattern works
    /// without requiring the user to call walk functions manually.
    struct TemplateCounter(usize);

    impl Visitor for TemplateCounter {
        fn visit_template_def(&mut self, node: &TemplateDef) {
            self.0 += 1;
            walk_template_def(self, node);
        }
    }

    #[test]
    fn visitor_can_count_templates() {
        let src = r#"
            pragma circom 2.0.0;
            template A() { signal input x; }
            template B() { signal output y; }
            function f() { return 1; }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut counter = TemplateCounter(0);
        counter.visit_file(&file);
        assert_eq!(counter.0, 2);
    }

    /// A no-op visitor that uses all default trait implementations.
    /// Walking with this exercises every default `visit_*` method.
    struct DefaultVisitor;
    impl Visitor for DefaultVisitor {}

    #[test]
    fn visitor_default_impls_bus_def() {
        let src = r#"
            bus MyBus(n) {
                signal input x[n];
                signal output y;
                Inner() inner;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut v = DefaultVisitor;
        v.visit_file(&file);
    }

    #[test]
    fn visitor_default_impls_all_statements() {
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
        let mut v = DefaultVisitor;
        v.visit_file(&file);
    }

    #[test]
    fn visitor_default_impls_expressions() {
        // Exercise expression kinds: unary, binary, ternary, index, member, call,
        // array literal, paren, parallel, anonymous comp
        let src = r#"
            template T() {
                var x = -1;
                var y = !x;
                var z = ~x;
                var a = x + y * z;
                var b = x ? y : z;
                var c = x;
                c = c + 1;
                signal input arr[3];
                signal output out;
                component comp = OtherTemplate();
                comp.inp <== 1;
                out <== arr[0];
                var d = [1, 2, 3];
                out <== (x + y);
                out <== parallel c;
                out <== Multiplier(1)(a, b);
                out <== A(1)(p <== a);
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut v = DefaultVisitor;
        v.visit_file(&file);
    }

    #[test]
    fn visitor_visits_bus_def() {
        let src = r#"
            bus MyBus() {
                signal input x;
                signal output y;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        /// Visitor that tracks bus-related node visits.
        struct BusCollector {
            events: Vec<&'static str>,
        }

        impl Visitor for BusCollector {
            fn visit_bus_def(&mut self, node: &BusDef) {
                self.events.push("BusDef");
                walk_bus_def(self, node);
            }
            fn visit_bus_member(&mut self, node: &BusMember) {
                self.events.push("BusMember");
                walk_bus_member(self, node);
            }
            fn visit_signal_decl(&mut self, node: &SignalDecl) {
                self.events.push("SignalDecl");
                walk_signal_decl(self, node);
            }
            fn visit_identifier(&mut self, node: &Identifier) {
                self.events.push("Identifier");
                let _ = node;
            }
        }

        let mut c = BusCollector { events: vec![] };
        c.visit_file(&file);
        assert!(c.events.contains(&"BusDef"));
        assert!(c.events.contains(&"BusMember"));
        assert!(c.events.contains(&"SignalDecl"));
        assert!(c.events.contains(&"Identifier"));
        assert_eq!(c.events.iter().filter(|e| **e == "BusMember").count(), 2);
    }

    #[test]
    fn visitor_visits_bus_field_decl() {
        let src = r#"
            bus Outer() {
                Inner() inner_field;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct FieldCollector {
            events: Vec<&'static str>,
        }

        impl Visitor for FieldCollector {
            fn visit_bus_def(&mut self, node: &BusDef) {
                self.events.push("BusDef");
                walk_bus_def(self, node);
            }
            fn visit_bus_member(&mut self, node: &BusMember) {
                self.events.push("BusMember");
                walk_bus_member(self, node);
            }
            fn visit_bus_field_decl(&mut self, node: &BusFieldDecl) {
                self.events.push("BusFieldDecl");
                walk_bus_field_decl(self, node);
            }
            fn visit_bus_type(&mut self, node: &BusType) {
                self.events.push("BusType");
                walk_bus_type(self, node);
            }
            fn visit_identifier(&mut self, node: &Identifier) {
                self.events.push("Identifier");
                let _ = node;
            }
        }

        let mut c = FieldCollector { events: vec![] };
        c.visit_file(&file);
        assert!(c.events.contains(&"BusDef"));
        assert!(c.events.contains(&"BusMember"));
        assert!(c.events.contains(&"BusFieldDecl"));
        assert!(c.events.contains(&"BusType"));
    }

    #[test]
    fn visitor_visits_bus_instance_decl() {
        let src = r#"
            template T() {
                signal output MyBus() myBus;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct BusInstanceCollector {
            events: Vec<&'static str>,
        }

        impl Visitor for BusInstanceCollector {
            fn visit_bus_instance_decl(&mut self, node: &BusInstanceDecl) {
                self.events.push("BusInstanceDecl");
                walk_bus_instance_decl(self, node);
            }
            fn visit_bus_type(&mut self, node: &BusType) {
                self.events.push("BusType");
                walk_bus_type(self, node);
            }
            fn visit_identifier(&mut self, node: &Identifier) {
                self.events.push("Identifier");
                let _ = node;
            }
        }

        let mut c = BusInstanceCollector { events: vec![] };
        c.visit_file(&file);
        assert!(c.events.contains(&"BusInstanceDecl"));
        assert!(c.events.contains(&"BusType"));
    }

    #[test]
    fn visitor_visits_log_stmt_with_mixed_args() {
        let src = r#"
            template T() {
                signal input x;
                log("value: ", x, " done");
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct LogCollector {
            events: Vec<&'static str>,
        }

        impl Visitor for LogCollector {
            fn visit_log_stmt(&mut self, node: &LogStmt) {
                self.events.push("LogStmt");
                walk_log_stmt(self, node);
            }
            fn visit_log_arg(&mut self, node: &LogArg) {
                match node {
                    LogArg::String(_) => self.events.push("LogArg::String"),
                    LogArg::Expr(_) => self.events.push("LogArg::Expr"),
                }
                walk_log_arg(self, node);
            }
            fn visit_expression(&mut self, node: &Expression) {
                self.events.push("Expression");
                walk_expression(self, node);
            }
        }

        let mut c = LogCollector { events: vec![] };
        c.visit_file(&file);
        assert!(c.events.contains(&"LogStmt"));
        assert_eq!(
            c.events.iter().filter(|e| **e == "LogArg::String").count(),
            2
        );
        assert_eq!(c.events.iter().filter(|e| **e == "LogArg::Expr").count(), 1);
    }

    #[test]
    fn visitor_visits_assert_stmt() {
        let src = r#"
            template T() {
                signal input x;
                assert(x);
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut collector = NodeCollector::new();
        collector.visit_file(&file);
        assert!(collector.events.contains(&"AssertStmt"));
        assert!(collector.events.contains(&"Expression"));
    }

    #[test]
    fn visitor_visits_tuple_assign() {
        let src = r#"
            template T() {
                var a;
                var b;
                (a, b) <== SomeTemplate()();
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct TupleCollector {
            events: Vec<&'static str>,
        }

        impl Visitor for TupleCollector {
            fn visit_tuple_assign_stmt(&mut self, node: &TupleAssignStmt) {
                self.events.push("TupleAssign");
                walk_tuple_assign_stmt(self, node);
            }
            fn visit_expression(&mut self, node: &Expression) {
                self.events.push("Expression");
                walk_expression(self, node);
            }
        }

        let mut c = TupleCollector { events: vec![] };
        c.visit_file(&file);
        assert!(c.events.contains(&"TupleAssign"));
        // targets (a, b) + rhs expression
        assert!(c.events.iter().filter(|e| **e == "Expression").count() >= 3);
    }

    #[test]
    fn visitor_visits_anonymous_comp() {
        let src = r#"
            template T() {
                signal output out;
                out <== Multiplier(n)(a, b);
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct AnonCollector {
            events: Vec<&'static str>,
        }

        impl Visitor for AnonCollector {
            fn visit_anonymous_comp(&mut self, node: &AnonymousComp) {
                self.events.push("AnonymousComp");
                walk_anonymous_comp(self, node);
            }
            fn visit_anon_comp_input(&mut self, node: &AnonCompInput) {
                self.events.push("AnonCompInput");
                walk_anon_comp_input(self, node);
            }
            fn visit_expression(&mut self, node: &Expression) {
                self.events.push("Expression");
                walk_expression(self, node);
            }
        }

        let mut c = AnonCollector { events: vec![] };
        c.visit_file(&file);
        assert!(c.events.contains(&"AnonymousComp"));
        assert_eq!(
            c.events.iter().filter(|e| **e == "AnonCompInput").count(),
            2
        );
    }

    #[test]
    fn visitor_visits_anonymous_comp_named_inputs() {
        let src = r#"
            template T() {
                signal output out;
                out <== A(n)(x <== in1, y <== in2);
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        struct NamedInputCollector {
            events: Vec<&'static str>,
        }

        impl Visitor for NamedInputCollector {
            fn visit_anonymous_comp(&mut self, node: &AnonymousComp) {
                self.events.push("AnonymousComp");
                walk_anonymous_comp(self, node);
            }
            fn visit_anon_comp_input(&mut self, node: &AnonCompInput) {
                match node {
                    AnonCompInput::Positional(_) => self.events.push("Positional"),
                    AnonCompInput::Named(_, _) => self.events.push("Named"),
                }
                walk_anon_comp_input(self, node);
            }
            fn visit_identifier(&mut self, node: &Identifier) {
                self.events.push("Identifier");
                let _ = node;
            }
        }

        let mut c = NamedInputCollector { events: vec![] };
        c.visit_file(&file);
        assert!(c.events.contains(&"AnonymousComp"));
        assert_eq!(c.events.iter().filter(|e| **e == "Named").count(), 2);
    }

    #[test]
    fn visitor_visits_main_component() {
        let src = r#"
            pragma circom 2.0.0;
            template T() { signal input x; }
            component main {public [x]} = T();
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut collector = NodeCollector::new();
        collector.visit_file(&file);
        assert!(collector.events.contains(&"MainComponent"));
    }

    #[test]
    fn visitor_visits_increment_decrement() {
        let src = r#"
            template T() {
                var x = 0;
                x++;
                x--;
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut collector = NodeCollector::new();
        collector.visit_file(&file);
        // increment and decrement dispatch to visit_expression
        assert!(
            collector
                .events
                .iter()
                .filter(|e| **e == "Expression")
                .count()
                >= 2
        );
    }

    #[test]
    fn visitor_visits_block_statement() {
        let src = r#"
            template T() {
                {
                    var x = 1;
                }
            }
        "#;
        let (file, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");

        let mut collector = NodeCollector::new();
        collector.visit_file(&file);
        // Should have at least 2 blocks: template body + nested block
        assert!(collector.events.iter().filter(|e| **e == "Block").count() >= 2);
    }
}
