//! Symbol table for Circom semantic analysis.
//!
//! The [`SymbolTable`] is the central data structure for tracking all named
//! entities in a Circom project. It supports:
//!
//! - All Circom symbol types (templates, functions, buses, signals, variables, components)
//! - Scope resolution following Circom scoping rules
//! - Cross-file symbol resolution via includes
//! - Qualified name resolution (dot notation)
//! - Incremental updates (re-parse one file without affecting others)
//! - Duplicate and undeclared symbol detection

use std::collections::HashMap;

use crate::ast::*;
use crate::scope::ScopeTree;
use crate::span::Span;
use crate::symbol::*;
use crate::visitor::{self, Visitor};

/// The symbol table for a Circom project.
#[derive(Debug, Clone)]
pub struct SymbolTable {
    /// All symbols, indexed by their ID.
    symbols: Vec<Symbol>,
    /// The scope tree.
    pub scopes: ScopeTree,
    /// Per-file tracking: file path -> (file scope ID, all scope IDs for that file, all symbol IDs for that file).
    file_scopes: HashMap<String, FileEntry>,
    /// Include graph: file path -> list of included file paths.
    includes: HashMap<String, Vec<String>>,
    /// Diagnostics collected during symbol collection.
    diagnostics: Vec<SymbolDiagnostic>,
}

/// Per-file metadata in the symbol table.
#[derive(Debug, Clone)]
struct FileEntry {
    /// The file-level scope ID.
    root_scope: ScopeId,
    /// All scope IDs created for this file.
    scope_ids: Vec<ScopeId>,
    /// All symbol IDs created for this file.
    symbol_ids: Vec<SymbolId>,
}

impl SymbolTable {
    /// Create a new empty symbol table.
    pub fn new() -> Self {
        Self {
            symbols: Vec::new(),
            scopes: ScopeTree::new(),
            file_scopes: HashMap::new(),
            includes: HashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Add (or update) a file's symbols from its parsed AST.
    ///
    /// If the file was previously indexed, its old symbols and scopes are
    /// removed first, so only that file's data is refreshed.
    pub fn index_file(&mut self, file_path: &str, ast: &File) {
        // Remove old data for this file if it exists.
        self.remove_file(file_path);

        let mut collector = SymbolCollector {
            table: self,
            file: file_path.to_string(),
            current_scope: ScopeId(0), // will be set below
            file_scope_ids: Vec::new(),
            file_symbol_ids: Vec::new(),
        };

        // Create the file-level scope.
        let root_scope = collector.table.scopes.create_root(ScopeKind::File);
        collector.current_scope = root_scope;
        collector.file_scope_ids.push(root_scope);

        // Walk the AST.
        collector.visit_file(ast);

        // Collect includes.
        let mut include_paths = Vec::new();
        for item in &ast.items {
            if let Item::Include(inc) = item {
                include_paths.push(inc.path.clone());
            }
        }

        let scope_ids = collector.file_scope_ids;
        let symbol_ids = collector.file_symbol_ids;

        self.file_scopes.insert(
            file_path.to_string(),
            FileEntry {
                root_scope,
                scope_ids,
                symbol_ids,
            },
        );
        self.includes.insert(file_path.to_string(), include_paths);
    }

    /// Remove all symbols and scopes associated with a file.
    pub fn remove_file(&mut self, file_path: &str) {
        if let Some(entry) = self.file_scopes.remove(file_path) {
            self.scopes.remove_scopes(&entry.scope_ids);
            // Symbols stay in the vec but are unreachable via scopes
            // since the scope was cleared. We don't compact to keep IDs stable.
        }
        self.diagnostics.retain(|d| d.file != file_path);
        self.includes.remove(file_path);
    }

    /// Get a symbol by its ID.
    pub fn get_symbol(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.0 as usize]
    }

    /// Look up a simple name from a given scope, walking up the scope chain.
    pub fn lookup(&self, scope: ScopeId, name: &str) -> Option<&Symbol> {
        self.scopes
            .lookup(scope, name)
            .map(|(_, ids)| &self.symbols[ids[0].0 as usize])
    }

    /// Look up a simple name from a given scope, also searching included files.
    pub fn lookup_with_includes(
        &self,
        scope: ScopeId,
        name: &str,
        file_path: &str,
    ) -> Option<&Symbol> {
        // First try local scope chain.
        if let Some(sym) = self.lookup(scope, name) {
            return Some(sym);
        }

        // Then try included files' file-level scopes.
        // TODO: transitive includes — currently only direct includes are
        // searched. In Circom, if A includes B and B includes C, symbols
        // from C should be visible in A. This needs a BFS/DFS over the
        // include graph.
        if let Some(includes) = self.includes.get(file_path) {
            for inc_path in includes {
                if let Some(entry) = self.file_scopes.get(inc_path) {
                    if let Some(ids) = self.scopes.lookup_local(entry.root_scope, name) {
                        return Some(&self.symbols[ids[0].0 as usize]);
                    }
                }
            }
        }

        None
    }

    /// Resolve a qualified name like `component.signal` or `bus.field`.
    ///
    /// Splits on `.` and resolves each segment: the first segment is looked
    /// up in the scope chain, subsequent segments are looked up in the
    /// body scope of the resolved symbol (template or bus).
    pub fn resolve_qualified(
        &self,
        scope: ScopeId,
        parts: &[&str],
        file_path: &str,
    ) -> Option<&Symbol> {
        if parts.is_empty() {
            return None;
        }

        let first = self.lookup_with_includes(scope, parts[0], file_path)?;

        let mut current = first;
        for &part in &parts[1..] {
            let body_scope = match &current.kind {
                SymbolKind::Component(comp) => {
                    // Look up the component's template, then find its body scope.
                    let tmpl_name = comp.template_name.as_ref()?;
                    let tmpl = self.lookup_with_includes(scope, tmpl_name, file_path)?;
                    match &tmpl.kind {
                        SymbolKind::Template(t) => t.body_scope,
                        _ => return None,
                    }
                }
                SymbolKind::Template(t) => t.body_scope,
                SymbolKind::Bus(b) => b.body_scope,
                _ => return None,
            };
            let ids = self.scopes.lookup_local(body_scope, part)?;
            current = &self.symbols[ids[0].0 as usize];
        }

        Some(current)
    }

    /// Get the file-level scope for a file.
    pub fn file_scope(&self, file_path: &str) -> Option<ScopeId> {
        self.file_scopes.get(file_path).map(|e| e.root_scope)
    }

    /// Get all diagnostics.
    pub fn diagnostics(&self) -> &[SymbolDiagnostic] {
        &self.diagnostics
    }

    /// Get all symbols in the table.
    pub fn all_symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// Get all file-level symbol names for a given file.
    pub fn file_symbols(&self, file_path: &str) -> Vec<&Symbol> {
        match self.file_scopes.get(file_path) {
            Some(entry) => entry
                .symbol_ids
                .iter()
                .map(|id| &self.symbols[id.0 as usize])
                .collect(),
            None => Vec::new(),
        }
    }

    /// Get the include list for a file.
    pub fn includes(&self, file_path: &str) -> Option<&[String]> {
        self.includes.get(file_path).map(|v| v.as_slice())
    }

    /// Check for undeclared symbol usage in a file's AST.
    pub fn check_undeclared(&mut self, file_path: &str, ast: &File) {
        let file_scope = match self.file_scope(file_path) {
            Some(s) => s,
            None => return,
        };

        let mut checker = UndeclaredChecker {
            table: self,
            file: file_path.to_string(),
            current_scope: file_scope,
            new_diagnostics: Vec::new(),
            child_cursor: HashMap::new(),
        };
        checker.check_file(ast);
        let diags = checker.new_diagnostics;
        self.diagnostics.extend(diags);
    }

    /// Allocate a new symbol and return its ID.
    fn alloc_symbol(
        &mut self,
        name: String,
        kind: SymbolKind,
        span: Span,
        scope: ScopeId,
        file: String,
    ) -> SymbolId {
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(Symbol {
            id,
            name,
            kind,
            span,
            scope,
            file,
        });
        id
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

// ── Symbol collector (AST visitor) ──────────────────────────────────

/// Walks the AST and populates the symbol table.
struct SymbolCollector<'a> {
    table: &'a mut SymbolTable,
    file: String,
    current_scope: ScopeId,
    file_scope_ids: Vec<ScopeId>,
    file_symbol_ids: Vec<SymbolId>,
}

impl<'a> SymbolCollector<'a> {
    fn define_symbol(&mut self, name: &str, kind: SymbolKind, span: Span) -> SymbolId {
        let id = self.table.alloc_symbol(
            name.to_string(),
            kind,
            span,
            self.current_scope,
            self.file.clone(),
        );
        self.file_symbol_ids.push(id);

        let is_dup = self
            .table
            .scopes
            .insert_symbol(self.current_scope, name, id);
        if is_dup {
            self.table.diagnostics.push(SymbolDiagnostic {
                span,
                message: format!("duplicate symbol '{name}'"),
                kind: DiagnosticKind::DuplicateSymbol,
                file: self.file.clone(),
            });
        }

        id
    }

    fn push_scope(&mut self, kind: ScopeKind) -> ScopeId {
        let new_scope = self.table.scopes.create_child(self.current_scope, kind);
        self.file_scope_ids.push(new_scope);
        self.current_scope = new_scope;
        new_scope
    }

    fn pop_scope(&mut self) {
        if let Some(parent) = self.table.scopes.get(self.current_scope).parent {
            self.current_scope = parent;
        }
    }
}

impl<'a> Visitor for SymbolCollector<'a> {
    fn visit_template_def(&mut self, node: &TemplateDef) {
        // Create the body scope first so we can reference it in the symbol.
        let body_scope = self
            .table
            .scopes
            .create_child(self.current_scope, ScopeKind::Template);
        self.file_scope_ids.push(body_scope);

        let params: Vec<String> = node.params.iter().map(|p| p.name.clone()).collect();

        self.define_symbol(
            &node.name.name,
            SymbolKind::Template(TemplateSymbol {
                params: params.clone(),
                is_custom: node.is_custom,
                is_parallel: node.is_parallel,
                body_scope,
            }),
            node.name.span,
        );

        // Enter the body scope to define parameters and body symbols.
        let outer_scope = self.current_scope;
        self.current_scope = body_scope;

        for param in &node.params {
            self.define_symbol(&param.name, SymbolKind::Parameter, param.span);
        }

        // Visit the body block's statements (not the block itself, to avoid
        // creating an extra scope).
        for stmt in &node.body.stmts {
            self.visit_statement(stmt);
        }

        self.current_scope = outer_scope;
    }

    fn visit_function_def(&mut self, node: &FunctionDef) {
        let body_scope = self
            .table
            .scopes
            .create_child(self.current_scope, ScopeKind::Function);
        self.file_scope_ids.push(body_scope);

        let params: Vec<String> = node.params.iter().map(|p| p.name.clone()).collect();

        self.define_symbol(
            &node.name.name,
            SymbolKind::Function(FunctionSymbol {
                params: params.clone(),
                body_scope,
            }),
            node.name.span,
        );

        let outer_scope = self.current_scope;
        self.current_scope = body_scope;

        for param in &node.params {
            self.define_symbol(&param.name, SymbolKind::Parameter, param.span);
        }

        for stmt in &node.body.stmts {
            self.visit_statement(stmt);
        }

        self.current_scope = outer_scope;
    }

    fn visit_bus_def(&mut self, node: &BusDef) {
        let body_scope = self
            .table
            .scopes
            .create_child(self.current_scope, ScopeKind::Bus);
        self.file_scope_ids.push(body_scope);

        let params: Vec<String> = node.params.iter().map(|p| p.name.clone()).collect();

        self.define_symbol(
            &node.name.name,
            SymbolKind::Bus(BusSymbol {
                params: params.clone(),
                body_scope,
            }),
            node.name.span,
        );

        let outer_scope = self.current_scope;
        self.current_scope = body_scope;

        for param in &node.params {
            self.define_symbol(&param.name, SymbolKind::Parameter, param.span);
        }

        for member in &node.body {
            match member {
                BusMember::Signal(sig) => {
                    for entry in &sig.names {
                        self.define_symbol(
                            &entry.name.name,
                            SymbolKind::Signal(SignalSymbol {
                                kind: sig.kind,
                                tags: sig.tags.iter().map(|t| t.name.clone()).collect(),
                                bus_type: None,
                                dimensions: entry.dimensions.len(),
                            }),
                            entry.name.span,
                        );
                    }
                }
                BusMember::Bus(field) => {
                    self.define_symbol(
                        &field.name.name,
                        // TODO: BusMember::Bus fields lack an explicit signal kind in
                        // the AST (BusFieldDecl has no `kind` field). We default to
                        // Input, which may misclassify output/intermediate bus fields.
                        // Consider adding a SymbolKind::BusField variant or inferring
                        // direction from context once the Circom spec clarifies this.
                        SymbolKind::Signal(SignalSymbol {
                            kind: SignalKind::Input,
                            tags: field.tags.iter().map(|t| t.name.clone()).collect(),
                            bus_type: Some(field.bus_type.name.name.clone()),
                            dimensions: field.dimensions.len(),
                        }),
                        field.name.span,
                    );
                }
            }
        }

        self.current_scope = outer_scope;
    }

    fn visit_var_decl(&mut self, node: &VarDecl) {
        for entry in &node.names {
            self.define_symbol(&entry.name.name, SymbolKind::Variable, entry.name.span);
            // Visit initializer expressions.
            if let Some(init) = &entry.init {
                self.visit_expression(init);
            }
            for dim in &entry.dimensions {
                self.visit_expression(dim);
            }
        }
    }

    fn visit_signal_decl(&mut self, node: &SignalDecl) {
        for entry in &node.names {
            self.define_symbol(
                &entry.name.name,
                SymbolKind::Signal(SignalSymbol {
                    kind: node.kind,
                    tags: node.tags.iter().map(|t| t.name.clone()).collect(),
                    bus_type: None,
                    dimensions: entry.dimensions.len(),
                }),
                entry.name.span,
            );
            // Visit initializer and dimension expressions.
            if let Some((_, init)) = &entry.init {
                self.visit_expression(init);
            }
            for dim in &entry.dimensions {
                self.visit_expression(dim);
            }
        }
    }

    fn visit_component_decl(&mut self, node: &ComponentDecl) {
        for entry in &node.names {
            let template_name = entry.init.as_ref().and_then(extract_template_name);
            self.define_symbol(
                &entry.name.name,
                SymbolKind::Component(ComponentSymbol { template_name }),
                entry.name.span,
            );
            if let Some(init) = &entry.init {
                self.visit_expression(init);
            }
            for dim in &entry.dimensions {
                self.visit_expression(dim);
            }
        }
    }

    fn visit_bus_instance_decl(&mut self, node: &BusInstanceDecl) {
        self.define_symbol(
            &node.name.name,
            SymbolKind::Signal(SignalSymbol {
                kind: node.signal_kind,
                tags: node.tags.iter().map(|t| t.name.clone()).collect(),
                bus_type: Some(node.bus_type.name.name.clone()),
                dimensions: node.dimensions.len(),
            }),
            node.name.span,
        );
        // Visit init expression.
        if let Some((_, init)) = &node.init {
            self.visit_expression(init);
        }
        for dim in &node.dimensions {
            self.visit_expression(dim);
        }
    }

    fn visit_statement(&mut self, node: &Statement) {
        match &node.kind {
            StatementKind::Block(block) => {
                self.push_scope(ScopeKind::Block);
                for stmt in &block.stmts {
                    self.visit_statement(stmt);
                }
                self.pop_scope();
            }
            StatementKind::For(for_loop) => {
                // For loops create a new block scope.
                self.push_scope(ScopeKind::Block);
                self.visit_statement(&for_loop.init);
                self.visit_expression(&for_loop.cond);
                self.visit_statement(&for_loop.step);
                for stmt in &for_loop.body.stmts {
                    self.visit_statement(stmt);
                }
                self.pop_scope();
            }
            StatementKind::While(while_loop) => {
                self.push_scope(ScopeKind::Block);
                self.visit_expression(&while_loop.cond);
                for stmt in &while_loop.body.stmts {
                    self.visit_statement(stmt);
                }
                self.pop_scope();
            }
            StatementKind::IfElse(if_else) => {
                self.visit_expression(&if_else.cond);
                self.push_scope(ScopeKind::Block);
                for stmt in &if_else.then_body.stmts {
                    self.visit_statement(stmt);
                }
                self.pop_scope();
                if let Some(else_body) = &if_else.else_body {
                    self.push_scope(ScopeKind::Block);
                    for stmt in &else_body.stmts {
                        self.visit_statement(stmt);
                    }
                    self.pop_scope();
                }
            }
            // Delegate to specific visit methods for declarations.
            _ => visitor::walk_statement(self, node),
        }
    }
}

/// Extract a template name from a call expression like `Poseidon(2)`.
fn extract_template_name(expr: &Expression) -> Option<String> {
    match expr.kind.as_ref() {
        ExpressionKind::Call(callee, _) => match callee.kind.as_ref() {
            ExpressionKind::Ident(name) => Some(name.clone()),
            _ => None,
        },
        _ => None,
    }
}

// ── Undeclared symbol checker ───────────────────────────────────────

/// Walks the AST and reports identifiers that are not in scope.
struct UndeclaredChecker<'a> {
    table: &'a SymbolTable,
    file: String,
    current_scope: ScopeId,
    new_diagnostics: Vec<SymbolDiagnostic>,
    /// Tracks the next child scope index to consume for each scope,
    /// so we mirror the same scope nesting order as `SymbolCollector`.
    child_cursor: HashMap<ScopeId, usize>,
}

impl<'a> UndeclaredChecker<'a> {
    /// Enter the next child block scope of the current scope, mirroring
    /// the order in which `SymbolCollector` created them.
    fn enter_child_scope(&mut self) {
        let idx = self.child_cursor.entry(self.current_scope).or_insert(0);
        let children = &self.table.scopes.get(self.current_scope).children;
        debug_assert!(
            *idx < children.len(),
            "UndeclaredChecker: out of child scopes for scope {:?} (cursor={}, len={})",
            self.current_scope,
            *idx,
            children.len()
        );
        if let Some(&child) = children.get(*idx) {
            *idx += 1;
            self.current_scope = child;
        }
    }

    fn leave_scope(&mut self) {
        if let Some(parent) = self.table.scopes.get(self.current_scope).parent {
            self.current_scope = parent;
        }
    }

    fn check_file(&mut self, ast: &File) {
        for item in &ast.items {
            match item {
                Item::TemplateDef(t) => self.check_template(t),
                Item::FunctionDef(f) => self.check_function(f),
                _ => {}
            }
        }
    }

    fn check_template(&mut self, node: &TemplateDef) {
        // Find the template symbol to get its body scope.
        if let Some(sym) = self.table.lookup(self.current_scope, &node.name.name) {
            if let SymbolKind::Template(ref t) = sym.kind {
                let outer = self.current_scope;
                self.current_scope = t.body_scope;
                self.check_block(&node.body);
                self.current_scope = outer;
            }
        }
    }

    fn check_function(&mut self, node: &FunctionDef) {
        if let Some(sym) = self.table.lookup(self.current_scope, &node.name.name) {
            if let SymbolKind::Function(ref f) = sym.kind {
                let outer = self.current_scope;
                self.current_scope = f.body_scope;
                self.check_block(&node.body);
                self.current_scope = outer;
            }
        }
    }

    fn check_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.check_statement(stmt);
        }
    }

    fn check_statement(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::Assignment(a) => {
                self.check_expr(&a.lhs);
                self.check_expr(&a.rhs);
            }
            StatementKind::CompoundAssign(a) => {
                self.check_expr(&a.lhs);
                self.check_expr(&a.rhs);
            }
            StatementKind::ConstraintEq(c) => {
                self.check_expr(&c.lhs);
                self.check_expr(&c.rhs);
            }
            StatementKind::Return(r) => {
                self.check_expr(&r.value);
            }
            StatementKind::Assert(a) => {
                self.check_expr(&a.expr);
            }
            StatementKind::Log(l) => {
                for arg in &l.args {
                    if let LogArg::Expr(e) = arg {
                        self.check_expr(e);
                    }
                }
            }
            StatementKind::Expression(e) => {
                self.check_expr(e);
            }
            StatementKind::Increment(e) | StatementKind::Decrement(e) => {
                self.check_expr(e);
            }
            StatementKind::VarDecl(v) => {
                for entry in &v.names {
                    if let Some(init) = &entry.init {
                        self.check_expr(init);
                    }
                }
            }
            StatementKind::SignalDecl(s) => {
                for entry in &s.names {
                    if let Some((_, init)) = &entry.init {
                        self.check_expr(init);
                    }
                }
            }
            StatementKind::ComponentDecl(c) => {
                for entry in &c.names {
                    if let Some(init) = &entry.init {
                        self.check_expr(init);
                    }
                }
            }
            StatementKind::For(f) => {
                self.enter_child_scope();
                self.check_statement(&f.init);
                self.check_expr(&f.cond);
                self.check_statement(&f.step);
                self.check_block(&f.body);
                self.leave_scope();
            }
            StatementKind::While(w) => {
                self.enter_child_scope();
                self.check_expr(&w.cond);
                self.check_block(&w.body);
                self.leave_scope();
            }
            StatementKind::IfElse(ie) => {
                self.check_expr(&ie.cond);
                self.enter_child_scope();
                self.check_block(&ie.then_body);
                self.leave_scope();
                if let Some(eb) = &ie.else_body {
                    self.enter_child_scope();
                    self.check_block(eb);
                    self.leave_scope();
                }
            }
            StatementKind::Block(b) => {
                self.enter_child_scope();
                self.check_block(b);
                self.leave_scope();
            }
            StatementKind::TupleAssign(t) => {
                for e in t.targets.iter().flatten() {
                    self.check_expr(e);
                }
                self.check_expr(&t.rhs);
            }
            StatementKind::BusDecl(b) => {
                if let Some((_, init)) = &b.init {
                    self.check_expr(init);
                }
            }
            StatementKind::Error => {}
        }
    }

    fn check_expr(&mut self, expr: &Expression) {
        match expr.kind.as_ref() {
            ExpressionKind::Ident(name) => {
                if self
                    .table
                    .lookup_with_includes(self.current_scope, name, &self.file)
                    .is_none()
                {
                    self.new_diagnostics.push(SymbolDiagnostic {
                        span: expr.span,
                        message: format!("undeclared symbol '{name}'"),
                        kind: DiagnosticKind::UndeclaredSymbol,
                        file: self.file.clone(),
                    });
                }
            }
            ExpressionKind::Member(base, _field) => {
                // For member access, we only check the base. Qualified resolution
                // is a separate analysis pass.
                self.check_expr(base);
            }
            ExpressionKind::Unary(_, e) => self.check_expr(e),
            ExpressionKind::Binary(l, _, r) => {
                self.check_expr(l);
                self.check_expr(r);
            }
            ExpressionKind::Ternary(c, t, f) => {
                self.check_expr(c);
                self.check_expr(t);
                self.check_expr(f);
            }
            ExpressionKind::Index(base, idx) => {
                self.check_expr(base);
                self.check_expr(idx);
            }
            ExpressionKind::Call(callee, args) => {
                self.check_expr(callee);
                for arg in args {
                    self.check_expr(arg);
                }
            }
            ExpressionKind::ArrayLit(elems) => {
                for e in elems {
                    self.check_expr(e);
                }
            }
            ExpressionKind::Paren(e) => self.check_expr(e),
            ExpressionKind::Parallel(e) => self.check_expr(e),
            ExpressionKind::AnonymousComp(ac) => {
                self.check_expr(&ac.template);
                for arg in &ac.template_args {
                    self.check_expr(arg);
                }
                for input in &ac.inputs {
                    match input {
                        AnonCompInput::Positional(e) => self.check_expr(e),
                        AnonCompInput::Named(_, e) => self.check_expr(e),
                    }
                }
            }
            ExpressionKind::Number(_) | ExpressionKind::Underscore | ExpressionKind::Error => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    fn parse_and_index(src: &str, file_path: &str) -> SymbolTable {
        let (ast, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut table = SymbolTable::new();
        table.index_file(file_path, &ast);
        table
    }

    // ── Basic symbol tracking ───────────────────────────────────────

    #[test]
    fn tracks_template_symbol() {
        let table = parse_and_index(
            r#"
            template Adder(n) {
                signal input a;
                signal input b;
                signal output c;
                c <== a + b;
            }
            "#,
            "main.circom",
        );

        let scope = table.file_scope("main.circom").unwrap();
        let sym = table.lookup(scope, "Adder").unwrap();
        assert_eq!(sym.name, "Adder");
        match &sym.kind {
            SymbolKind::Template(t) => {
                assert_eq!(t.params, vec!["n"]);
                assert!(!t.is_custom);
                assert!(!t.is_parallel);
            }
            _ => panic!("expected template symbol"),
        }
    }

    #[test]
    fn tracks_function_symbol() {
        let table = parse_and_index(
            r#"
            function add(a, b) {
                return a + b;
            }
            "#,
            "main.circom",
        );

        let scope = table.file_scope("main.circom").unwrap();
        let sym = table.lookup(scope, "add").unwrap();
        assert_eq!(sym.name, "add");
        match &sym.kind {
            SymbolKind::Function(f) => {
                assert_eq!(f.params, vec!["a", "b"]);
            }
            _ => panic!("expected function symbol"),
        }
    }

    #[test]
    fn tracks_bus_symbol() {
        let table = parse_and_index(
            r#"
            pragma circom 2.2.0;
            bus Point() {
                signal input x;
                signal input y;
            }
            "#,
            "main.circom",
        );

        let scope = table.file_scope("main.circom").unwrap();
        let sym = table.lookup(scope, "Point").unwrap();
        assert_eq!(sym.name, "Point");
        match &sym.kind {
            SymbolKind::Bus(b) => {
                assert!(b.params.is_empty());
            }
            _ => panic!("expected bus symbol"),
        }
    }

    #[test]
    fn tracks_signals_in_template() {
        let table = parse_and_index(
            r#"
            template T() {
                signal input a;
                signal output b;
                signal c;
            }
            "#,
            "main.circom",
        );

        let scope = table.file_scope("main.circom").unwrap();
        let tmpl = table.lookup(scope, "T").unwrap();
        let body_scope = match &tmpl.kind {
            SymbolKind::Template(t) => t.body_scope,
            _ => panic!("expected template"),
        };

        let a = table.lookup(body_scope, "a").unwrap();
        match &a.kind {
            SymbolKind::Signal(s) => assert_eq!(s.kind, SignalKind::Input),
            _ => panic!("expected signal"),
        }

        let b = table.lookup(body_scope, "b").unwrap();
        match &b.kind {
            SymbolKind::Signal(s) => assert_eq!(s.kind, SignalKind::Output),
            _ => panic!("expected signal"),
        }

        let c = table.lookup(body_scope, "c").unwrap();
        match &c.kind {
            SymbolKind::Signal(s) => assert_eq!(s.kind, SignalKind::Intermediate),
            _ => panic!("expected signal"),
        }
    }

    #[test]
    fn tracks_variables_and_components() {
        let table = parse_and_index(
            r#"
            template Circuit() {
                var x = 5;
                component hasher = Poseidon(2);
            }
            "#,
            "main.circom",
        );

        let scope = table.file_scope("main.circom").unwrap();
        let tmpl = table.lookup(scope, "Circuit").unwrap();
        let body_scope = match &tmpl.kind {
            SymbolKind::Template(t) => t.body_scope,
            _ => panic!("expected template"),
        };

        let x = table.lookup(body_scope, "x").unwrap();
        assert!(matches!(x.kind, SymbolKind::Variable));

        let h = table.lookup(body_scope, "hasher").unwrap();
        match &h.kind {
            SymbolKind::Component(c) => {
                assert_eq!(c.template_name.as_deref(), Some("Poseidon"));
            }
            _ => panic!("expected component"),
        }
    }

    #[test]
    fn tracks_parameters() {
        let table = parse_and_index(
            r#"
            template T(a, b) {
                signal output c;
            }
            "#,
            "main.circom",
        );

        let scope = table.file_scope("main.circom").unwrap();
        let tmpl = table.lookup(scope, "T").unwrap();
        let body_scope = match &tmpl.kind {
            SymbolKind::Template(t) => t.body_scope,
            _ => panic!("expected template"),
        };

        let a = table.lookup(body_scope, "a").unwrap();
        assert!(matches!(a.kind, SymbolKind::Parameter));

        let b = table.lookup(body_scope, "b").unwrap();
        assert!(matches!(b.kind, SymbolKind::Parameter));
    }

    // ── Scope resolution ────────────────────────────────────────────

    #[test]
    fn scope_resolution_inner_shadows_outer() {
        let table = parse_and_index(
            r#"
            template T() {
                var x = 1;
                for (var i = 0; i < 10; i++) {
                    var x = 2;
                }
            }
            "#,
            "main.circom",
        );

        // The outer x is in template scope, the inner x is in for-block scope.
        // Both should exist without error (shadowing is allowed).
        let scope = table.file_scope("main.circom").unwrap();
        let tmpl = table.lookup(scope, "T").unwrap();
        let body_scope = match &tmpl.kind {
            SymbolKind::Template(t) => t.body_scope,
            _ => panic!("expected template"),
        };

        // The template scope has x.
        let x = table.scopes.lookup_local(body_scope, "x");
        assert!(x.is_some());

        // There should be a block scope child with its own x.
        let children = &table.scopes.get(body_scope).children;
        assert!(!children.is_empty());
        let block_scope = children[0];
        let inner_x = table.scopes.lookup_local(block_scope, "x");
        assert!(inner_x.is_some());

        // The inner x's symbol ID should be different from the outer x's.
        let outer_ids = x.unwrap();
        let inner_ids = inner_x.unwrap();
        assert_ne!(outer_ids[0], inner_ids[0]);
    }

    #[test]
    fn block_scope_variable_not_visible_outside() {
        let table = parse_and_index(
            r#"
            template T() {
                for (var i = 0; i < 10; i++) {
                    var inner = 1;
                }
            }
            "#,
            "main.circom",
        );

        let scope = table.file_scope("main.circom").unwrap();
        let tmpl = table.lookup(scope, "T").unwrap();
        let body_scope = match &tmpl.kind {
            SymbolKind::Template(t) => t.body_scope,
            _ => panic!("expected template"),
        };

        // "inner" should NOT be visible in the template scope.
        assert!(table.scopes.lookup_local(body_scope, "inner").is_none());

        // But "i" declared in the for-init should be in the block scope.
        let children = &table.scopes.get(body_scope).children;
        let block_scope = children[0];
        assert!(table.scopes.lookup_local(block_scope, "i").is_some());
        assert!(table.scopes.lookup_local(block_scope, "inner").is_some());
    }

    // ── Cross-file resolution via includes ──────────────────────────

    #[test]
    fn cross_file_resolution_via_includes() {
        let mut table = SymbolTable::new();

        // Index the library file.
        let (lib_ast, _) = parser::parse(
            r#"
            template Poseidon(nInputs) {
                signal input inputs[nInputs];
                signal output out;
            }
            "#,
        );
        table.index_file("poseidon.circom", &lib_ast);

        // Index the main file that includes the library.
        let (main_ast, _) = parser::parse(
            r#"
            include "poseidon.circom";
            template Main() {
                signal input a;
                component h = Poseidon(1);
            }
            "#,
        );
        table.index_file("main.circom", &main_ast);

        // Lookup Poseidon from main.circom's file scope with includes.
        let main_scope = table.file_scope("main.circom").unwrap();
        let sym = table
            .lookup_with_includes(main_scope, "Poseidon", "main.circom")
            .unwrap();
        assert_eq!(sym.name, "Poseidon");
        assert!(matches!(&sym.kind, SymbolKind::Template(_)));
    }

    // ── Qualified name resolution ───────────────────────────────────

    #[test]
    fn qualified_name_resolution_component_signal() {
        let mut table = SymbolTable::new();

        let (ast, _) = parser::parse(
            r#"
            template Inner() {
                signal input x;
                signal output y;
                y <== x;
            }
            template Outer() {
                component c = Inner();
            }
            "#,
        );
        table.index_file("main.circom", &ast);

        let scope = table.file_scope("main.circom").unwrap();
        let outer = table.lookup(scope, "Outer").unwrap();
        let body_scope = match &outer.kind {
            SymbolKind::Template(t) => t.body_scope,
            _ => panic!("expected template"),
        };

        // Resolve c.x
        let resolved = table
            .resolve_qualified(body_scope, &["c", "x"], "main.circom")
            .unwrap();
        assert_eq!(resolved.name, "x");
        match &resolved.kind {
            SymbolKind::Signal(s) => assert_eq!(s.kind, SignalKind::Input),
            _ => panic!("expected signal"),
        }

        // Resolve c.y
        let resolved = table
            .resolve_qualified(body_scope, &["c", "y"], "main.circom")
            .unwrap();
        assert_eq!(resolved.name, "y");
    }

    #[test]
    fn qualified_name_resolution_bus_field() {
        let mut table = SymbolTable::new();

        let (ast, _) = parser::parse(
            r#"
            pragma circom 2.2.0;
            bus Point() {
                signal input x;
                signal input y;
            }
            "#,
        );
        table.index_file("main.circom", &ast);

        let scope = table.file_scope("main.circom").unwrap();

        // Resolve Point's fields directly (bus itself as first segment).
        let resolved = table
            .resolve_qualified(scope, &["Point", "x"], "main.circom")
            .unwrap();
        assert_eq!(resolved.name, "x");
    }

    // ── Duplicate symbol detection ──────────────────────────────────

    #[test]
    fn detects_duplicate_template() {
        let table = parse_and_index(
            r#"
            template Foo() { signal input x; }
            template Foo() { signal input y; }
            "#,
            "main.circom",
        );

        let dups: Vec<_> = table
            .diagnostics()
            .iter()
            .filter(|d| d.kind == DiagnosticKind::DuplicateSymbol)
            .collect();
        assert_eq!(dups.len(), 1);
        assert!(dups[0].message.contains("Foo"));
    }

    #[test]
    fn detects_duplicate_variable_in_same_scope() {
        let table = parse_and_index(
            r#"
            template T() {
                var x = 1;
                var x = 2;
            }
            "#,
            "main.circom",
        );

        let dups: Vec<_> = table
            .diagnostics()
            .iter()
            .filter(|d| d.kind == DiagnosticKind::DuplicateSymbol)
            .collect();
        assert_eq!(dups.len(), 1);
        assert!(dups[0].message.contains("x"));
    }

    // ── Undeclared symbol detection ─────────────────────────────────

    #[test]
    fn detects_undeclared_symbol() {
        let (ast, _) = parser::parse(
            r#"
            template T() {
                signal output c;
                c <== undefined_var;
            }
            "#,
        );
        let mut table = SymbolTable::new();
        table.index_file("main.circom", &ast);
        table.check_undeclared("main.circom", &ast);

        let undecl: Vec<_> = table
            .diagnostics()
            .iter()
            .filter(|d| d.kind == DiagnosticKind::UndeclaredSymbol)
            .collect();
        assert_eq!(undecl.len(), 1);
        assert!(undecl[0].message.contains("undefined_var"));
    }

    #[test]
    fn no_false_positive_for_block_scoped_symbols() {
        let (ast, _) = parser::parse(
            r#"
            template T(n) {
                for (var i = 0; i < n; i++) {
                    var x = i;
                }
                if (n > 0) {
                    var y = n;
                }
                while (n > 0) {
                    var z = n;
                }
            }
            "#,
        );
        let mut table = SymbolTable::new();
        table.index_file("main.circom", &ast);
        table.check_undeclared("main.circom", &ast);

        let undecl: Vec<_> = table
            .diagnostics()
            .iter()
            .filter(|d| d.kind == DiagnosticKind::UndeclaredSymbol)
            .collect();
        assert!(
            undecl.is_empty(),
            "unexpected undeclared diagnostics: {undecl:?}"
        );
    }

    #[test]
    fn no_false_positive_for_declared_symbols() {
        let (ast, _) = parser::parse(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <== a;
            }
            "#,
        );
        let mut table = SymbolTable::new();
        table.index_file("main.circom", &ast);
        table.check_undeclared("main.circom", &ast);

        let undecl: Vec<_> = table
            .diagnostics()
            .iter()
            .filter(|d| d.kind == DiagnosticKind::UndeclaredSymbol)
            .collect();
        assert!(
            undecl.is_empty(),
            "unexpected undeclared diagnostics: {undecl:?}"
        );
    }

    // ── Incremental update ──────────────────────────────────────────

    #[test]
    fn incremental_update_replaces_file_symbols() {
        let mut table = SymbolTable::new();

        // Index version 1.
        let (ast1, _) = parser::parse(
            r#"
            template Foo() { signal input x; }
            "#,
        );
        table.index_file("main.circom", &ast1);

        let scope = table.file_scope("main.circom").unwrap();
        assert!(table.lookup(scope, "Foo").is_some());

        // Index version 2 (renamed template).
        let (ast2, _) = parser::parse(
            r#"
            template Bar() { signal input y; }
            "#,
        );
        table.index_file("main.circom", &ast2);

        let scope = table.file_scope("main.circom").unwrap();
        assert!(table.lookup(scope, "Bar").is_some());
        // Old symbol "Foo" should no longer be reachable.
        assert!(table.lookup(scope, "Foo").is_none());
    }

    #[test]
    fn incremental_update_other_file_unaffected() {
        let mut table = SymbolTable::new();

        let (ast_a, _) = parser::parse(r#"template A() { signal input x; }"#);
        table.index_file("a.circom", &ast_a);

        let (ast_b, _) = parser::parse(r#"template B() { signal input y; }"#);
        table.index_file("b.circom", &ast_b);

        // Re-index file a.
        let (ast_a2, _) = parser::parse(r#"template A2() { signal input z; }"#);
        table.index_file("a.circom", &ast_a2);

        // File b's symbols should be unaffected.
        let scope_b = table.file_scope("b.circom").unwrap();
        let b = table.lookup(scope_b, "B").unwrap();
        assert_eq!(b.name, "B");

        // File a should have A2, not A.
        let scope_a = table.file_scope("a.circom").unwrap();
        assert!(table.lookup(scope_a, "A2").is_some());
        assert!(table.lookup(scope_a, "A").is_none());
    }

    #[test]
    fn remove_file_preserves_other_file_diagnostics() {
        let mut table = SymbolTable::new();

        // Index file a with a duplicate symbol.
        let (ast_a, _) = parser::parse(
            r#"
            template Foo() { signal input x; }
            template Foo() { signal input y; }
            "#,
        );
        table.index_file("a.circom", &ast_a);

        // Index file b with a duplicate symbol.
        let (ast_b, _) = parser::parse(
            r#"
            template Bar() { signal input x; }
            template Bar() { signal input y; }
            "#,
        );
        table.index_file("b.circom", &ast_b);

        assert_eq!(table.diagnostics().len(), 2);

        // Remove file a — file b's diagnostic should remain.
        table.remove_file("a.circom");

        assert_eq!(table.diagnostics().len(), 1);
        assert!(table.diagnostics()[0].message.contains("Bar"));
        assert_eq!(table.diagnostics()[0].file, "b.circom");
    }

    // ── Bus fields in bus scope ─────────────────────────────────────

    #[test]
    fn bus_fields_in_scope() {
        let table = parse_and_index(
            r#"
            pragma circom 2.2.0;
            bus Point() {
                signal input x;
                signal input y;
            }
            "#,
            "main.circom",
        );

        let scope = table.file_scope("main.circom").unwrap();
        let point = table.lookup(scope, "Point").unwrap();
        let body_scope = match &point.kind {
            SymbolKind::Bus(b) => b.body_scope,
            _ => panic!("expected bus"),
        };

        assert!(table.scopes.lookup_local(body_scope, "x").is_some());
        assert!(table.scopes.lookup_local(body_scope, "y").is_some());
    }

    // ── Signal tags ─────────────────────────────────────────────────

    #[test]
    fn signal_tags_tracked() {
        let table = parse_and_index(
            r#"
            template T() {
                signal input {binary} x;
            }
            "#,
            "main.circom",
        );

        let scope = table.file_scope("main.circom").unwrap();
        let tmpl = table.lookup(scope, "T").unwrap();
        let body_scope = match &tmpl.kind {
            SymbolKind::Template(t) => t.body_scope,
            _ => panic!("expected template"),
        };

        let x = table.lookup(body_scope, "x").unwrap();
        match &x.kind {
            SymbolKind::Signal(s) => {
                assert_eq!(s.tags, vec!["binary"]);
            }
            _ => panic!("expected signal"),
        }
    }
}
