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

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

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

    /// Look up a simple name from a given scope, also searching included
    /// files transitively (BFS over the include graph).
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

        // BFS over the include graph to find the symbol in transitive
        // includes. Track visited files to handle cycles.
        let mut visited = HashSet::new();
        visited.insert(file_path.to_string());
        let mut queue = VecDeque::new();

        if let Some(includes) = self.includes.get(file_path) {
            for inc_path in includes {
                if visited.insert(inc_path.clone()) {
                    queue.push_back(inc_path.clone());
                }
            }
        }

        while let Some(current) = queue.pop_front() {
            if let Some(entry) = self.file_scopes.get(&current) {
                if let Some(ids) = self.scopes.lookup_local(entry.root_scope, name) {
                    return Some(&self.symbols[ids[0].0 as usize]);
                }
            }
            // Enqueue transitive includes.
            if let Some(includes) = self.includes.get(&current) {
                for inc_path in includes {
                    if visited.insert(inc_path.clone()) {
                        queue.push_back(inc_path.clone());
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
                // A bus-typed signal (`signal input Point() p`) lets the
                // caller drill into the bus's fields via dot notation:
                // `p.x` resolves through the bus's body scope.
                SymbolKind::Signal(sig) => {
                    let bus_name = sig.bus_type.as_ref()?;
                    let bus = self.lookup_with_includes(scope, bus_name, file_path)?;
                    match &bus.kind {
                        SymbolKind::Bus(b) => b.body_scope,
                        _ => return None,
                    }
                }
                _ => return None,
            };
            let ids = self.scopes.lookup_local(body_scope, part)?;
            current = &self.symbols[ids[0].0 as usize];
        }

        Some(current)
    }

    /// Resolve an include path relative to the including file's directory.
    /// Also searches library directories if configured.
    pub fn resolve_include_path(
        &self,
        include_path: &str,
        from_file: &str,
        lib_dirs: &[String],
    ) -> Option<String> {
        let from_dir = Path::new(from_file).parent().unwrap_or(Path::new(""));

        // Try relative to the including file's directory.
        let candidate = from_dir.join(include_path);
        if candidate.exists() {
            if let Ok(canonical) = candidate.canonicalize() {
                return Some(canonical.to_string_lossy().into_owned());
            }
        }

        // Try each library directory.
        for dir in lib_dirs {
            let candidate = Path::new(dir).join(include_path);
            if candidate.exists() {
                if let Ok(canonical) = candidate.canonicalize() {
                    return Some(canonical.to_string_lossy().into_owned());
                }
            }
        }

        None
    }

    /// Get the full transitive include closure for a file (BFS).
    pub fn transitive_includes(&self, file_path: &str) -> Vec<&str> {
        let mut visited = HashSet::new();
        visited.insert(file_path);
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        if let Some(includes) = self.includes.get(file_path) {
            for inc in includes {
                if visited.insert(inc.as_str()) {
                    queue.push_back(inc.as_str());
                }
            }
        }

        while let Some(current) = queue.pop_front() {
            result.push(current);
            if let Some(includes) = self.includes.get(current) {
                for inc in includes {
                    if visited.insert(inc.as_str()) {
                        queue.push_back(inc.as_str());
                    }
                }
            }
        }

        result
    }

    /// Detect circular includes. Returns the cycle path if found.
    pub fn detect_circular_includes(&self) -> Option<Vec<String>> {
        // DFS with path tracking on each file.
        for start in self.includes.keys() {
            let mut visited = HashSet::new();
            let mut path = Vec::new();
            if let Some(cycle) = self.dfs_find_cycle(start, &mut visited, &mut path) {
                return Some(cycle);
            }
        }
        None
    }

    /// DFS helper for cycle detection.
    fn dfs_find_cycle(
        &self,
        node: &str,
        visiting: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        if !visiting.insert(node.to_string()) {
            // Found a cycle — extract it from the path.
            if let Some(pos) = path.iter().position(|p| p == node) {
                let mut cycle: Vec<String> = path[pos..].to_vec();
                cycle.push(node.to_string());
                return Some(cycle);
            }
            return None;
        }

        path.push(node.to_string());

        if let Some(includes) = self.includes.get(node) {
            for inc in includes {
                if let Some(cycle) = self.dfs_find_cycle(inc, visiting, path) {
                    return Some(cycle);
                }
            }
        }

        path.pop();
        visiting.remove(node);
        None
    }

    /// Get files that include the given file (reverse lookup).
    pub fn included_by(&self, file_path: &str) -> Vec<&str> {
        self.includes
            .iter()
            .filter_map(|(from, includes)| {
                if includes.iter().any(|inc| inc == file_path) {
                    Some(from.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get topological ordering of all files (for incremental
    /// re-analysis). Returns files in dependency order: a file
    /// appears after all files it includes. Returns `None` if there
    /// is a cycle.
    pub fn topological_order(&self) -> Option<Vec<&str>> {
        // Collect all known files.
        let mut all_files: HashSet<&str> = HashSet::new();
        for (file, includes) in &self.includes {
            all_files.insert(file.as_str());
            for inc in includes {
                all_files.insert(inc.as_str());
            }
        }
        for file in self.file_scopes.keys() {
            all_files.insert(file.as_str());
        }

        // Kahn's algorithm. Edge: A includes B means B is a dependency
        // of A. We want B before A, so in-degree counts how many files
        // a file depends on (i.e. how many files it includes).
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        for &file in &all_files {
            in_degree.insert(file, 0);
        }
        for (includer, includes) in &self.includes {
            // Each include is a dependency edge includer -> included.
            // In-degree of the includer increases per dependency.
            *in_degree.entry(includer.as_str()).or_insert(0) += includes.len();
        }

        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&file, _)| file)
            .collect();

        let mut result = Vec::new();
        while let Some(file) = queue.pop_front() {
            result.push(file);
            // For each file that includes `file`, decrement in-degree.
            for (includer, includes) in &self.includes {
                if includes.iter().any(|inc| inc.as_str() == file) {
                    let deg = in_degree.get_mut(includer.as_str()).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(includer.as_str());
                    }
                }
            }
        }

        if result.len() == all_files.len() {
            Some(result)
        } else {
            None // cycle detected
        }
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

    /// Run type checks on a file's AST and append diagnostics.
    pub fn check_types(&mut self, file_path: &str, ast: &File) {
        let diags = crate::type_checker::check_types(self, file_path, ast);
        self.diagnostics.extend(diags);
    }

    /// Run constraint checks on a file's AST and append diagnostics.
    pub fn check_constraints(&mut self, file_path: &str, ast: &File) {
        let diags = crate::constraint_checker::check_constraints(self, file_path, ast);
        self.diagnostics.extend(diags);
    }

    /// Run underconstrained-signal analysis on a file's AST and append
    /// diagnostics.
    pub fn check_underconstrained(&mut self, file_path: &str, ast: &File) {
        let diags = crate::underconstrained::analyze(self, file_path, ast);
        self.diagnostics.extend(diags);
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

    fn check_stmt_control_flow(&mut self, stmt: &Statement) -> bool {
        match &stmt.kind {
            StatementKind::For(f) => {
                self.enter_child_scope();
                self.check_statement(&f.init);
                self.check_expr(&f.cond);
                self.check_statement(&f.step);
                self.check_block(&f.body);
                self.leave_scope();
                true
            }
            StatementKind::While(w) => {
                self.enter_child_scope();
                self.check_expr(&w.cond);
                self.check_block(&w.body);
                self.leave_scope();
                true
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
                true
            }
            StatementKind::Block(b) => {
                self.enter_child_scope();
                self.check_block(b);
                self.leave_scope();
                true
            }
            _ => false,
        }
    }

    fn check_stmt_decl(&mut self, stmt: &Statement) -> bool {
        match &stmt.kind {
            StatementKind::VarDecl(v) => {
                for entry in &v.names {
                    if let Some(init) = &entry.init {
                        self.check_expr(init);
                    }
                }
                true
            }
            StatementKind::SignalDecl(s) => {
                for entry in &s.names {
                    if let Some((_, init)) = &entry.init {
                        self.check_expr(init);
                    }
                }
                true
            }
            StatementKind::ComponentDecl(c) => {
                for entry in &c.names {
                    if let Some(init) = &entry.init {
                        self.check_expr(init);
                    }
                }
                true
            }
            StatementKind::BusDecl(b) => {
                if let Some((_, init)) = &b.init {
                    self.check_expr(init);
                }
                true
            }
            _ => false,
        }
    }

    fn check_statement(&mut self, stmt: &Statement) {
        if self.check_stmt_control_flow(stmt) || self.check_stmt_decl(stmt) {
            return;
        }
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
            StatementKind::Return(r) => self.check_expr(&r.value),
            StatementKind::Assert(a) => self.check_expr(&a.expr),
            StatementKind::Log(l) => {
                for arg in &l.args {
                    if let LogArg::Expr(e) = arg {
                        self.check_expr(e);
                    }
                }
            }
            StatementKind::Expression(e)
            | StatementKind::Increment(e)
            | StatementKind::Decrement(e) => self.check_expr(e),
            StatementKind::TupleAssign(t) => {
                for e in t.targets.iter().flatten() {
                    self.check_expr(e);
                }
                self.check_expr(&t.rhs);
            }
            _ => {}
        }
    }

    fn report_undeclared_ident(&mut self, name: &str, span: Span) {
        if self
            .table
            .lookup_with_includes(self.current_scope, name, &self.file)
            .is_none()
        {
            self.new_diagnostics.push(SymbolDiagnostic {
                span,
                message: format!("undeclared symbol '{name}'"),
                kind: DiagnosticKind::UndeclaredSymbol,
                file: self.file.clone(),
            });
        }
    }

    fn check_anon_comp(&mut self, ac: &AnonymousComp) {
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

    fn check_expr(&mut self, expr: &Expression) {
        match expr.kind.as_ref() {
            ExpressionKind::Ident(name) => self.report_undeclared_ident(name, expr.span),
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
            ExpressionKind::AnonymousComp(ac) => self.check_anon_comp(ac),
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

    // ── Transitive include resolution ──────────────────────────────

    #[test]
    fn transitive_include_resolution() {
        let mut table = SymbolTable::new();

        // C defines a template.
        let (ast_c, _) = parser::parse(r#"template Leaf() { signal input x; }"#);
        table.index_file("c.circom", &ast_c);

        // B includes C.
        let (ast_b, _) = parser::parse(
            r#"
            include "c.circom";
            template Middle() { signal input y; }
            "#,
        );
        table.index_file("b.circom", &ast_b);

        // A includes B (but not C directly).
        let (ast_a, _) = parser::parse(
            r#"
            include "b.circom";
            template Top() { signal input z; }
            "#,
        );
        table.index_file("a.circom", &ast_a);

        let scope_a = table.file_scope("a.circom").unwrap();

        // A can see B's symbols (direct include).
        let middle = table
            .lookup_with_includes(scope_a, "Middle", "a.circom")
            .unwrap();
        assert_eq!(middle.name, "Middle");

        // A can see C's symbols (transitive include via B).
        let leaf = table
            .lookup_with_includes(scope_a, "Leaf", "a.circom")
            .unwrap();
        assert_eq!(leaf.name, "Leaf");
    }

    #[test]
    fn circular_include_detection() {
        let mut table = SymbolTable::new();

        // A includes B.
        let (ast_a, _) = parser::parse(
            r#"
            include "b.circom";
            template A() { signal input x; }
            "#,
        );
        table.index_file("a.circom", &ast_a);

        // B includes A.
        let (ast_b, _) = parser::parse(
            r#"
            include "a.circom";
            template B() { signal input y; }
            "#,
        );
        table.index_file("b.circom", &ast_b);

        let cycle = table.detect_circular_includes();
        assert!(cycle.is_some(), "expected circular include");
        let cycle = cycle.unwrap();
        // Cycle should contain both files.
        assert!(cycle.contains(&"a.circom".to_string()));
        assert!(cycle.contains(&"b.circom".to_string()));
    }

    #[test]
    fn no_cycle_in_acyclic_graph() {
        let mut table = SymbolTable::new();

        let (ast_c, _) = parser::parse(r#"template C() { signal input x; }"#);
        table.index_file("c.circom", &ast_c);

        let (ast_b, _) = parser::parse(
            r#"
            include "c.circom";
            template B() { signal input y; }
            "#,
        );
        table.index_file("b.circom", &ast_b);

        let (ast_a, _) = parser::parse(
            r#"
            include "b.circom";
            template A() { signal input z; }
            "#,
        );
        table.index_file("a.circom", &ast_a);

        assert!(table.detect_circular_includes().is_none());
    }

    #[test]
    fn diamond_dependency() {
        let mut table = SymbolTable::new();

        // D is at the bottom.
        let (ast_d, _) = parser::parse(r#"template D() { signal input w; }"#);
        table.index_file("d.circom", &ast_d);

        // B and C both include D.
        let (ast_b, _) = parser::parse(
            r#"
            include "d.circom";
            template B() { signal input x; }
            "#,
        );
        table.index_file("b.circom", &ast_b);

        let (ast_c, _) = parser::parse(
            r#"
            include "d.circom";
            template C() { signal input y; }
            "#,
        );
        table.index_file("c.circom", &ast_c);

        // A includes both B and C.
        let (ast_a, _) = parser::parse(
            r#"
            include "b.circom";
            include "c.circom";
            template A() { signal input z; }
            "#,
        );
        table.index_file("a.circom", &ast_a);

        let scope_a = table.file_scope("a.circom").unwrap();

        // A can see D's symbol through both B and C.
        let d = table
            .lookup_with_includes(scope_a, "D", "a.circom")
            .unwrap();
        assert_eq!(d.name, "D");

        // No circular dependency in a diamond.
        assert!(table.detect_circular_includes().is_none());
    }

    #[test]
    fn transitive_includes_closure() {
        let mut table = SymbolTable::new();

        let (ast_c, _) = parser::parse(r#"template C() { signal input x; }"#);
        table.index_file("c.circom", &ast_c);

        let (ast_b, _) = parser::parse(
            r#"
            include "c.circom";
            template B() { signal input y; }
            "#,
        );
        table.index_file("b.circom", &ast_b);

        let (ast_a, _) = parser::parse(
            r#"
            include "b.circom";
            template A() { signal input z; }
            "#,
        );
        table.index_file("a.circom", &ast_a);

        let closure = table.transitive_includes("a.circom");
        assert!(closure.contains(&"b.circom"));
        assert!(closure.contains(&"c.circom"));
        assert_eq!(closure.len(), 2);
    }

    #[test]
    fn included_by_reverse_lookup() {
        let mut table = SymbolTable::new();

        let (ast_lib, _) = parser::parse(r#"template Lib() { signal input x; }"#);
        table.index_file("lib.circom", &ast_lib);

        let (ast_a, _) = parser::parse(
            r#"
            include "lib.circom";
            template A() { signal input y; }
            "#,
        );
        table.index_file("a.circom", &ast_a);

        let (ast_b, _) = parser::parse(
            r#"
            include "lib.circom";
            template B() { signal input z; }
            "#,
        );
        table.index_file("b.circom", &ast_b);

        let mut dependents = table.included_by("lib.circom");
        dependents.sort();
        assert_eq!(dependents, vec!["a.circom", "b.circom"]);
    }

    #[test]
    fn topological_order_acyclic() {
        let mut table = SymbolTable::new();

        let (ast_c, _) = parser::parse(r#"template C() { signal input x; }"#);
        table.index_file("c.circom", &ast_c);

        let (ast_b, _) = parser::parse(
            r#"
            include "c.circom";
            template B() { signal input y; }
            "#,
        );
        table.index_file("b.circom", &ast_b);

        let (ast_a, _) = parser::parse(
            r#"
            include "b.circom";
            template A() { signal input z; }
            "#,
        );
        table.index_file("a.circom", &ast_a);

        let order = table.topological_order().unwrap();
        // c should come before b, and b before a.
        let pos_c = order.iter().position(|&f| f == "c.circom").unwrap();
        let pos_b = order.iter().position(|&f| f == "b.circom").unwrap();
        let pos_a = order.iter().position(|&f| f == "a.circom").unwrap();
        assert!(pos_c < pos_b);
        assert!(pos_b < pos_a);
    }

    #[test]
    fn topological_order_returns_none_on_cycle() {
        let mut table = SymbolTable::new();

        let (ast_a, _) = parser::parse(
            r#"
            include "b.circom";
            template A() { signal input x; }
            "#,
        );
        table.index_file("a.circom", &ast_a);

        let (ast_b, _) = parser::parse(
            r#"
            include "a.circom";
            template B() { signal input y; }
            "#,
        );
        table.index_file("b.circom", &ast_b);

        assert!(table.topological_order().is_none());
    }

    #[test]
    fn transitive_lookup_does_not_loop_on_cycle() {
        let mut table = SymbolTable::new();

        // A includes B, B includes A — circular.
        let (ast_a, _) = parser::parse(
            r#"
            include "b.circom";
            template A() { signal input x; }
            "#,
        );
        table.index_file("a.circom", &ast_a);

        let (ast_b, _) = parser::parse(
            r#"
            include "a.circom";
            template B() { signal input y; }
            "#,
        );
        table.index_file("b.circom", &ast_b);

        let scope_a = table.file_scope("a.circom").unwrap();

        // Should still resolve B (direct include).
        let b = table
            .lookup_with_includes(scope_a, "B", "a.circom")
            .unwrap();
        assert_eq!(b.name, "B");

        // Should not hang looking for a nonexistent symbol.
        assert!(table
            .lookup_with_includes(scope_a, "Nonexistent", "a.circom")
            .is_none());
    }
}
