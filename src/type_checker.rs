//! Type checking for Circom semantic analysis.
//!
//! Validates:
//! - Signal direction (cannot assign to input signals inside a template)
//! - Assignment operator correctness (`=` for variables, `<==`/`<--` for signals)
//! - Template parameter count on component instantiation
//! - Signals cannot appear in function bodies
//! - Signal-tag propagation (Circom 2.1+): assigning a signal with fewer tags
//!   into a target that declares a tag produces a `TagLoss` warning.
//! - Template-instantiation analysis (#60): unknown component field accesses
//!   (`c.not_a_real_signal`) and unused component outputs.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::span::Span;
use crate::symbol::*;
use crate::symbol_table::SymbolTable;

/// Run type checks on a file's AST using the populated symbol table.
///
/// Returns diagnostics for any type errors found.
pub fn check_types(table: &SymbolTable, file_path: &str, ast: &File) -> Vec<SymbolDiagnostic> {
    let file_scope = match table.file_scope(file_path) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut checker = TypeChecker {
        table,
        file: file_path.to_string(),
        current_scope: file_scope,
        diagnostics: Vec::new(),
        context: CheckContext::File,
    };
    checker.check_file(ast);
    checker.diagnostics
}

/// Tracks whether we're inside a template or function body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckContext {
    File,
    Template,
    Function,
}

struct TypeChecker<'a> {
    table: &'a SymbolTable,
    file: String,
    current_scope: ScopeId,
    diagnostics: Vec<SymbolDiagnostic>,
    context: CheckContext,
}

impl<'a> TypeChecker<'a> {
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
        if let Some(sym) = self.table.lookup(self.current_scope, &node.name.name) {
            if let SymbolKind::Template(ref t) = sym.kind {
                let outer_scope = self.current_scope;
                let outer_context = self.context;
                self.current_scope = t.body_scope;
                self.context = CheckContext::Template;
                self.check_block(&node.body);
                self.check_component_instantiations(&node.body);
                self.current_scope = outer_scope;
                self.context = outer_context;
            }
        }
    }

    /// Validate component instantiations within a template body (#60):
    /// - Any `c.field` access must name a real signal on the component's
    ///   template (otherwise the user has a typo).
    /// - Any declared output signal on an instantiated component that is
    ///   never read is flagged as an unused output (warning).
    fn template_signal_info(&self, tmpl_body: ScopeId) -> TemplateSignalInfo {
        let tmpl_scope = self.table.scopes.get(tmpl_body);
        let mut info = TemplateSignalInfo::default();
        for sid in tmpl_scope.all_symbols() {
            let s = self.table.get_symbol(sid);
            if let SymbolKind::Signal(sig) = &s.kind {
                info.known.insert(s.name.clone());
                match sig.kind {
                    SignalKind::Output => info.outputs.push((s.name.clone(), s.span)),
                    SignalKind::Input => info.inputs.push((s.name.clone(), s.span)),
                    SignalKind::Intermediate => {}
                }
            }
        }
        info
    }

    fn warn_unknown_component_accesses(
        &mut self,
        cname: &str,
        template_name: &str,
        known: &HashSet<String>,
        access: &ComponentAccess,
    ) {
        for (field, span) in access
            .reads
            .get(cname)
            .into_iter()
            .flatten()
            .chain(access.writes.get(cname).into_iter().flatten())
        {
            if !known.contains(field) {
                self.diagnostics.push(SymbolDiagnostic {
                    span: *span,
                    message: format!(
                        "template '{template_name}' has no signal '{field}' (component '{cname}')"
                    ),
                    kind: DiagnosticKind::UnknownComponentSignal,
                    file: self.file.clone(),
                });
            }
        }
    }

    fn warn_unused_outputs(
        &mut self,
        cname: &str,
        info: &ComponentInfo,
        outputs: &[(String, Span)],
        access: &ComponentAccess,
    ) {
        let reads_for_c: HashSet<&str> = access
            .reads
            .get(cname)
            .into_iter()
            .flatten()
            .map(|(n, _)| n.as_str())
            .collect();
        for (name, _) in outputs {
            if !reads_for_c.contains(name.as_str()) {
                self.diagnostics.push(SymbolDiagnostic {
                    span: info.decl_span,
                    message: format!("output '{name}' of component '{cname}' is never read"),
                    kind: DiagnosticKind::UnusedComponentOutput,
                    file: self.file.clone(),
                });
            }
        }
    }

    fn warn_missing_inputs(
        &mut self,
        cname: &str,
        info: &ComponentInfo,
        inputs: &[(String, Span)],
        access: &ComponentAccess,
    ) {
        let writes_for_c: HashSet<&str> = access
            .writes
            .get(cname)
            .into_iter()
            .flatten()
            .map(|(n, _)| n.as_str())
            .collect();
        for (name, _) in inputs {
            if !writes_for_c.contains(name.as_str()) {
                self.diagnostics.push(SymbolDiagnostic {
                    span: info.decl_span,
                    message: format!("input '{name}' of component '{cname}' is never assigned"),
                    kind: DiagnosticKind::MissingComponentInput,
                    file: self.file.clone(),
                });
            }
        }
    }

    fn check_single_component(
        &mut self,
        cname: &str,
        info: &ComponentInfo,
        access: &ComponentAccess,
    ) {
        let Some(template_name) = info.template_name.as_ref() else {
            return;
        };
        let tmpl_sym =
            match self
                .table
                .lookup_with_includes(self.current_scope, template_name, &self.file)
            {
                Some(s) => s,
                None => return,
            };
        let tmpl_body = match &tmpl_sym.kind {
            SymbolKind::Template(t) => t.body_scope,
            _ => return,
        };

        let sig_info = self.template_signal_info(tmpl_body);
        self.warn_unknown_component_accesses(cname, template_name, &sig_info.known, access);
        self.warn_unused_outputs(cname, info, &sig_info.outputs, access);
        self.warn_missing_inputs(cname, info, &sig_info.inputs, access);
    }

    fn check_component_instantiations(&mut self, body: &Block) {
        let mut components: HashMap<String, ComponentInfo> = HashMap::new();
        self.collect_components(body, &mut components);
        if components.is_empty() {
            return;
        }

        let mut access = ComponentAccess {
            reads: HashMap::new(),
            writes: HashMap::new(),
        };
        self.collect_component_accesses(body, &components, &mut access);

        for (cname, info) in &components {
            self.check_single_component(cname, info, &access);
        }
    }

    fn collect_components(&self, block: &Block, out: &mut HashMap<String, ComponentInfo>) {
        for stmt in &block.stmts {
            match &stmt.kind {
                StatementKind::ComponentDecl(c) => {
                    for entry in &c.names {
                        let template_name = entry
                            .init
                            .as_ref()
                            .and_then(extract_template_name_from_expr);
                        out.insert(
                            entry.name.name.clone(),
                            ComponentInfo {
                                template_name,
                                decl_span: entry.name.span,
                            },
                        );
                    }
                }
                StatementKind::For(f) => self.collect_components(&f.body, out),
                StatementKind::While(w) => self.collect_components(&w.body, out),
                StatementKind::IfElse(ie) => {
                    self.collect_components(&ie.then_body, out);
                    if let Some(eb) = &ie.else_body {
                        self.collect_components(eb, out);
                    }
                }
                StatementKind::Block(b) => self.collect_components(b, out),
                _ => {}
            }
        }
    }

    fn collect_accesses_from_assignment(
        &self,
        a: &AssignStmt,
        components: &HashMap<String, ComponentInfo>,
        out: &mut ComponentAccess,
    ) {
        self.collect_accesses_in_expr(&a.rhs, components, out, /*is_write*/ false);
        match a.op {
            AssignOp::SafeLeft | AssignOp::UnsafeLeft => {
                self.record_if_component_access(&a.lhs, components, out, true);
                self.collect_accesses_in_expr(&a.lhs, components, out, false);
            }
            AssignOp::SafeRight | AssignOp::UnsafeRight => {
                self.record_if_component_access(&a.rhs, components, out, true);
            }
            AssignOp::Eq => {
                self.collect_accesses_in_expr(&a.lhs, components, out, false);
            }
        }
    }

    fn collect_component_accesses(
        &self,
        block: &Block,
        components: &HashMap<String, ComponentInfo>,
        out: &mut ComponentAccess,
    ) {
        for stmt in &block.stmts {
            match &stmt.kind {
                StatementKind::Assignment(a) => {
                    self.collect_accesses_from_assignment(a, components, out);
                }
                StatementKind::ConstraintEq(c) => {
                    self.collect_accesses_in_expr(&c.lhs, components, out, false);
                    self.collect_accesses_in_expr(&c.rhs, components, out, false);
                }
                StatementKind::For(f) => {
                    self.collect_accesses_in_stmt(&f.init, components, out);
                    self.collect_accesses_in_expr(&f.cond, components, out, false);
                    self.collect_accesses_in_stmt(&f.step, components, out);
                    self.collect_component_accesses(&f.body, components, out);
                }
                StatementKind::While(w) => {
                    self.collect_accesses_in_expr(&w.cond, components, out, false);
                    self.collect_component_accesses(&w.body, components, out);
                }
                StatementKind::IfElse(ie) => {
                    self.collect_accesses_in_expr(&ie.cond, components, out, false);
                    self.collect_component_accesses(&ie.then_body, components, out);
                    if let Some(eb) = &ie.else_body {
                        self.collect_component_accesses(eb, components, out);
                    }
                }
                StatementKind::Block(b) => self.collect_component_accesses(b, components, out),
                StatementKind::TupleAssign(t) => {
                    self.collect_accesses_in_expr(&t.rhs, components, out, false);
                    for target in t.targets.iter().flatten() {
                        self.record_if_component_access(target, components, out, true);
                    }
                }
                StatementKind::Expression(e)
                | StatementKind::Increment(e)
                | StatementKind::Decrement(e) => {
                    self.collect_accesses_in_expr(e, components, out, false);
                }
                _ => {}
            }
        }
    }

    fn collect_accesses_in_stmt(
        &self,
        stmt: &Statement,
        components: &HashMap<String, ComponentInfo>,
        out: &mut ComponentAccess,
    ) {
        if let StatementKind::Assignment(a) = &stmt.kind {
            self.collect_accesses_in_expr(&a.lhs, components, out, false);
            self.collect_accesses_in_expr(&a.rhs, components, out, false);
        }
    }

    fn collect_member_access(
        &self,
        base: &Expression,
        field: &Identifier,
        components: &HashMap<String, ComponentInfo>,
        out: &mut ComponentAccess,
        is_write: bool,
    ) {
        if let Some(name) = base.extract_base_name() {
            if components.contains_key(&name) {
                let map = if is_write {
                    &mut out.writes
                } else {
                    &mut out.reads
                };
                map.entry(name)
                    .or_default()
                    .push((field.name.clone(), field.span));
                return;
            }
        }
        self.collect_accesses_in_expr(base, components, out, is_write);
    }

    fn collect_anon_comp_accesses(
        &self,
        ac: &AnonymousComp,
        components: &HashMap<String, ComponentInfo>,
        out: &mut ComponentAccess,
    ) {
        for a in &ac.template_args {
            self.collect_accesses_in_expr(a, components, out, false);
        }
        for inp in &ac.inputs {
            match inp {
                AnonCompInput::Positional(e) | AnonCompInput::Named(_, e) => {
                    self.collect_accesses_in_expr(e, components, out, false);
                }
            }
        }
    }

    fn collect_accesses_in_expr(
        &self,
        expr: &Expression,
        components: &HashMap<String, ComponentInfo>,
        out: &mut ComponentAccess,
        is_write: bool,
    ) {
        match expr.kind.as_ref() {
            ExpressionKind::Member(base, field) => {
                self.collect_member_access(base, field, components, out, is_write);
            }
            ExpressionKind::Index(base, idx) => {
                self.collect_accesses_in_expr(base, components, out, is_write);
                self.collect_accesses_in_expr(idx, components, out, false);
            }
            ExpressionKind::Unary(_, e) => self.collect_accesses_in_expr(e, components, out, false),
            ExpressionKind::Binary(l, _, r) => {
                self.collect_accesses_in_expr(l, components, out, false);
                self.collect_accesses_in_expr(r, components, out, false);
            }
            ExpressionKind::Ternary(c, t, f) => {
                self.collect_accesses_in_expr(c, components, out, false);
                self.collect_accesses_in_expr(t, components, out, false);
                self.collect_accesses_in_expr(f, components, out, false);
            }
            ExpressionKind::Call(callee, args) => {
                self.collect_accesses_in_expr(callee, components, out, false);
                for a in args {
                    self.collect_accesses_in_expr(a, components, out, false);
                }
            }
            ExpressionKind::AnonymousComp(ac) => {
                self.collect_anon_comp_accesses(ac, components, out);
            }
            ExpressionKind::ArrayLit(elems) => {
                for e in elems {
                    self.collect_accesses_in_expr(e, components, out, false);
                }
            }
            ExpressionKind::Paren(e) | ExpressionKind::Parallel(e) => {
                self.collect_accesses_in_expr(e, components, out, false);
            }
            _ => {}
        }
    }

    fn record_if_component_access(
        &self,
        expr: &Expression,
        components: &HashMap<String, ComponentInfo>,
        out: &mut ComponentAccess,
        is_write: bool,
    ) {
        if let ExpressionKind::Member(base, field) = expr.kind.as_ref() {
            if let Some(name) = base.extract_base_name() {
                if components.contains_key(&name) {
                    let map = if is_write {
                        &mut out.writes
                    } else {
                        &mut out.reads
                    };
                    map.entry(name)
                        .or_default()
                        .push((field.name.clone(), field.span));
                }
            }
        } else if let ExpressionKind::Index(base, _) = expr.kind.as_ref() {
            self.record_if_component_access(base, components, out, is_write);
        }
    }

    fn check_function(&mut self, node: &FunctionDef) {
        if let Some(sym) = self.table.lookup(self.current_scope, &node.name.name) {
            if let SymbolKind::Function(ref f) = sym.kind {
                let outer_scope = self.current_scope;
                let outer_context = self.context;
                self.current_scope = f.body_scope;
                self.context = CheckContext::Function;
                self.check_block(&node.body);
                self.current_scope = outer_scope;
                self.context = outer_context;
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
                self.check_assignment(a, stmt.span);
            }
            StatementKind::ConstraintEq(_) if self.context == CheckContext::Function => {
                self.diagnostics.push(SymbolDiagnostic {
                    span: stmt.span,
                    message: "constraint equality '===' cannot be used in a function".to_string(),
                    kind: DiagnosticKind::SignalInFunction,
                    file: self.file.clone(),
                });
            }
            StatementKind::ConstraintEq(_) => {}
            StatementKind::SignalDecl(s) if self.context == CheckContext::Function => {
                self.diagnostics.push(SymbolDiagnostic {
                    span: s.span,
                    message: "signal declarations are not allowed in functions".to_string(),
                    kind: DiagnosticKind::SignalInFunction,
                    file: self.file.clone(),
                });
            }
            StatementKind::ComponentDecl(c) => {
                for entry in &c.names {
                    if let Some(init) = &entry.init {
                        self.check_component_init(init);
                    }
                }
            }
            StatementKind::For(f) => {
                self.check_statement(&f.init);
                self.check_block(&f.body);
                self.check_statement(&f.step);
            }
            StatementKind::While(w) => {
                self.check_block(&w.body);
            }
            StatementKind::IfElse(ie) => {
                self.check_block(&ie.then_body);
                if let Some(eb) = &ie.else_body {
                    self.check_block(eb);
                }
            }
            StatementKind::Block(b) => {
                self.check_block(b);
            }
            StatementKind::TupleAssign(t) => {
                self.check_tuple_assign(t, stmt.span);
            }
            _ => {}
        }
    }

    fn check_assignment(&mut self, assign: &AssignStmt, span: Span) {
        match assign.op {
            AssignOp::SafeLeft | AssignOp::UnsafeLeft => {
                // <== or <-- : LHS should be a signal (output or intermediate)
                self.check_signal_assign_target(&assign.lhs, assign.op);
                self.check_tag_propagation(&assign.lhs, &assign.rhs);
                self.check_bus_type_match(&assign.lhs, &assign.rhs);
            }
            AssignOp::SafeRight | AssignOp::UnsafeRight => {
                // ==> or --> : RHS is the signal being assigned
                self.check_signal_assign_target(&assign.rhs, assign.op);
                self.check_tag_propagation(&assign.rhs, &assign.lhs);
                self.check_bus_type_match(&assign.rhs, &assign.lhs);
            }
            AssignOp::Eq => {
                // = : should be used for variables, not signals
                self.check_var_assign_target(&assign.lhs, span);
            }
        }
        // Validate any anonymous-component instantiations contained within.
        self.check_anon_comps_in_expr(&assign.lhs);
        self.check_anon_comps_in_expr(&assign.rhs);
    }

    /// When a bus-typed signal appears as the target of `<==` / `<--`, the
    /// source must carry the same bus type. We resolve both sides through
    /// any indexing / member access and compare their declared bus type
    /// names. If neither side resolves to a bus, this check is a no-op.
    fn check_bus_type_match(&mut self, lhs_expr: &Expression, rhs_expr: &Expression) {
        let Some(lhs_bus) = self.resolve_bus_type(lhs_expr) else {
            return;
        };
        let rhs_bus = self.resolve_bus_type(rhs_expr);
        match rhs_bus {
            Some(rb) if rb == lhs_bus => {}
            Some(rb) => {
                self.diagnostics.push(SymbolDiagnostic {
                    span: rhs_expr.span,
                    message: format!("bus type mismatch: expected '{lhs_bus}', found '{rb}'"),
                    kind: DiagnosticKind::BusTypeMismatch,
                    file: self.file.clone(),
                });
            }
            None => {
                // RHS is not itself a bus-typed signal. Only flag when the
                // RHS is a bare reference we could resolve to a non-bus
                // signal — complex expressions (literals, arithmetic) are
                // skipped because Circom's bus initialization semantics
                // allow bus construction via assignment in those cases.
                if let Some(rhs_name) = rhs_expr.extract_base_name() {
                    if self.is_non_bus_signal(&rhs_name) {
                        self.diagnostics.push(SymbolDiagnostic {
                            span: rhs_expr.span,
                            message: format!(
                                "bus type mismatch: expected '{lhs_bus}', found non-bus signal '{rhs_name}'"
                            ),
                            kind: DiagnosticKind::BusTypeMismatch,
                            file: self.file.clone(),
                        });
                    }
                }
            }
        }
    }

    /// Follow an expression through `Index` / `Member` nodes and return the
    /// bus-type name of the underlying signal, if any. Handles:
    /// - `x` where `x` is a bus-typed signal
    /// - `x[i]` / `x[i][j]` (arrays preserve element type)
    /// - `c.b` where `c` is a component and `b` is a bus-typed field of
    ///   its template
    /// - `p.inner` where `p` is a bus-typed signal and `inner` is a nested
    ///   bus field
    fn resolve_bus_type(&self, expr: &Expression) -> Option<String> {
        match expr.kind.as_ref() {
            ExpressionKind::Ident(name) => self.bus_type_of_name(name),
            ExpressionKind::Index(base, _) => self.resolve_bus_type(base),
            ExpressionKind::Paren(inner) => self.resolve_bus_type(inner),
            ExpressionKind::Member(base, field) => self.resolve_member_bus_type(base, field),
            _ => None,
        }
    }

    fn resolve_member_bus_type(&self, base: &Expression, field: &Identifier) -> Option<String> {
        let parts = collect_member_path(base, field)?;
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        let sym = self
            .table
            .resolve_qualified(self.current_scope, &refs, &self.file)?;
        match &sym.kind {
            SymbolKind::Signal(s) => s.bus_type.clone(),
            _ => None,
        }
    }

    fn bus_type_of_name(&self, name: &str) -> Option<String> {
        let sym = self
            .table
            .lookup_with_includes(self.current_scope, name, &self.file)?;
        match &sym.kind {
            SymbolKind::Signal(s) => s.bus_type.clone(),
            _ => None,
        }
    }

    fn is_non_bus_signal(&self, name: &str) -> bool {
        match self
            .table
            .lookup_with_includes(self.current_scope, name, &self.file)
        {
            Some(sym) => matches!(
                &sym.kind,
                SymbolKind::Signal(s) if s.bus_type.is_none()
            ),
            None => false,
        }
    }

    /// Walk an expression and validate every `AnonymousComp` found: parameter
    /// counts must match the referenced template, and named-input identifiers
    /// must be real inputs on it.
    fn check_anon_comps_in_expr(&mut self, expr: &Expression) {
        match expr.kind.as_ref() {
            ExpressionKind::AnonymousComp(ac) => {
                self.check_anonymous_comp(ac, expr.span);
                for a in &ac.template_args {
                    self.check_anon_comps_in_expr(a);
                }
                for inp in &ac.inputs {
                    match inp {
                        AnonCompInput::Positional(e) | AnonCompInput::Named(_, e) => {
                            self.check_anon_comps_in_expr(e);
                        }
                    }
                }
            }
            ExpressionKind::Unary(_, e) => self.check_anon_comps_in_expr(e),
            ExpressionKind::Binary(l, _, r) => {
                self.check_anon_comps_in_expr(l);
                self.check_anon_comps_in_expr(r);
            }
            ExpressionKind::Ternary(c, t, f) => {
                self.check_anon_comps_in_expr(c);
                self.check_anon_comps_in_expr(t);
                self.check_anon_comps_in_expr(f);
            }
            ExpressionKind::Index(b, i) => {
                self.check_anon_comps_in_expr(b);
                self.check_anon_comps_in_expr(i);
            }
            ExpressionKind::Member(b, _) => self.check_anon_comps_in_expr(b),
            ExpressionKind::Call(callee, args) => {
                self.check_anon_comps_in_expr(callee);
                for a in args {
                    self.check_anon_comps_in_expr(a);
                }
            }
            ExpressionKind::ArrayLit(xs) => {
                for x in xs {
                    self.check_anon_comps_in_expr(x);
                }
            }
            ExpressionKind::Paren(e) | ExpressionKind::Parallel(e) => {
                self.check_anon_comps_in_expr(e);
            }
            _ => {}
        }
    }

    fn check_anonymous_comp(&mut self, ac: &AnonymousComp, span: Span) {
        let name = match ac.template.kind.as_ref() {
            ExpressionKind::Ident(n) => n.clone(),
            _ => return,
        };
        let Some(sym) = self
            .table
            .lookup_with_includes(self.current_scope, &name, &self.file)
        else {
            return;
        };
        let SymbolKind::Template(tmpl) = &sym.kind else {
            return;
        };

        // 1. Parameter count.
        if ac.template_args.len() != tmpl.params.len() {
            self.diagnostics.push(SymbolDiagnostic {
                span,
                message: format!(
                    "template '{}' expects {} parameter(s), but {} provided",
                    name,
                    tmpl.params.len(),
                    ac.template_args.len()
                ),
                kind: DiagnosticKind::ParameterCountMismatch,
                file: self.file.clone(),
            });
        }

        // 2. Named-input validation: each `name <== ...` must correspond to
        //    a real input signal on the template.
        let tmpl_scope = self.table.scopes.get(tmpl.body_scope);
        let mut input_names: HashSet<String> = HashSet::new();
        for sid in tmpl_scope.all_symbols() {
            let s = self.table.get_symbol(sid);
            if let SymbolKind::Signal(sig) = &s.kind {
                if sig.kind == SignalKind::Input {
                    input_names.insert(s.name.clone());
                }
            }
        }
        for inp in &ac.inputs {
            if let AnonCompInput::Named(ident, _) = inp {
                if !input_names.contains(&ident.name) {
                    self.diagnostics.push(SymbolDiagnostic {
                        span: ident.span,
                        message: format!("template '{name}' has no input signal '{}'", ident.name),
                        kind: DiagnosticKind::UnknownComponentSignal,
                        file: self.file.clone(),
                    });
                }
            }
        }
    }

    /// Verify that assigning `rhs_expr` to `lhs_expr` does not silently drop
    /// any signal tag that the target declares. In Circom 2.1+, tags are
    /// intended to carry compile-time invariants (e.g. `binary`,
    /// `maxvalue`) — if the source signal lacks a tag the target requires,
    /// we warn.
    ///
    /// The rule is intentionally conservative: we only warn when we can
    /// resolve both sides to concrete signal symbols and the target has at
    /// least one tag that the source is missing. Expressions that mix
    /// signals or involve constants/variables produce no warning because
    /// the compiler treats the result as untagged and the user has
    /// explicitly opted out of propagation.
    fn check_tag_propagation(&mut self, lhs_expr: &Expression, rhs_expr: &Expression) {
        let lhs_name = match lhs_expr.extract_base_name() {
            Some(n) => n,
            None => return,
        };
        let lhs_tags: HashSet<String> = match self.tags_of(&lhs_name) {
            Some(t) if !t.is_empty() => t,
            _ => return,
        };

        // The RHS is a bare signal reference (or indexed/member access to
        // one). If it's a more complex expression, we skip — the user is
        // explicitly computing a value.
        let rhs_name = match rhs_expr.extract_base_name() {
            Some(n) => n,
            None => return,
        };
        // Only warn when RHS is itself a signal; otherwise the compiler
        // does not carry tags and warning would be noisy.
        if !self.is_signal(&rhs_name) {
            return;
        }
        let rhs_tags = self.tags_of(&rhs_name).unwrap_or_default();

        let missing: Vec<String> = lhs_tags
            .iter()
            .filter(|t| !rhs_tags.contains(*t))
            .cloned()
            .collect();
        if !missing.is_empty() {
            self.diagnostics.push(SymbolDiagnostic {
                span: rhs_expr.span,
                message: format!(
                    "signal '{rhs_name}' assigned to '{lhs_name}' which requires tag(s) {{{}}}; tag info is lost",
                    missing.join(", ")
                ),
                kind: DiagnosticKind::TagLoss,
                file: self.file.clone(),
            });
        }
    }

    fn tags_of(&self, name: &str) -> Option<HashSet<String>> {
        let sym = self
            .table
            .lookup_with_includes(self.current_scope, name, &self.file)?;
        match &sym.kind {
            SymbolKind::Signal(s) => Some(s.tags.iter().cloned().collect()),
            _ => None,
        }
    }

    fn is_signal(&self, name: &str) -> bool {
        self.table
            .lookup_with_includes(self.current_scope, name, &self.file)
            .map(|s| matches!(s.kind, SymbolKind::Signal(_)))
            .unwrap_or(false)
    }

    fn check_tuple_assign(&mut self, assign: &TupleAssignStmt, _span: Span) {
        match assign.op {
            AssignOp::SafeLeft | AssignOp::UnsafeLeft => {
                for target in assign.targets.iter().flatten() {
                    self.check_signal_assign_target(target, assign.op);
                }
            }
            _ => {}
        }
    }

    /// Check that the target of a signal assignment (<== or <--) is valid:
    /// - Must be a signal (not a variable)
    /// - Cannot be an input signal (inside a template)
    fn check_signal_assign_target(&mut self, expr: &Expression, op: AssignOp) {
        let name = match expr.extract_base_name() {
            Some(n) => n,
            None => return,
        };

        if let Some(sym) = self
            .table
            .lookup_with_includes(self.current_scope, &name, &self.file)
        {
            match &sym.kind {
                SymbolKind::Signal(sig)
                    if sig.kind == SignalKind::Input && self.context == CheckContext::Template =>
                {
                    let op_str = match op {
                        AssignOp::SafeLeft => "<==",
                        AssignOp::UnsafeLeft => "<--",
                        AssignOp::SafeRight => "==>",
                        AssignOp::UnsafeRight => "-->",
                        _ => "signal assign",
                    };
                    self.diagnostics.push(SymbolDiagnostic {
                        span: expr.span,
                        message: format!("cannot assign to input signal '{name}' with '{op_str}'"),
                        kind: DiagnosticKind::AssignToInput,
                        file: self.file.clone(),
                    });
                }
                SymbolKind::Variable | SymbolKind::Parameter => {
                    let op_str = match op {
                        AssignOp::SafeLeft => "<==",
                        AssignOp::UnsafeLeft => "<--",
                        AssignOp::SafeRight => "==>",
                        AssignOp::UnsafeRight => "-->",
                        _ => "signal assign",
                    };
                    self.diagnostics.push(SymbolDiagnostic {
                        span: expr.span,
                        message: format!("signal operator '{op_str}' used on variable '{name}'"),
                        kind: DiagnosticKind::SignalAssignToVar,
                        file: self.file.clone(),
                    });
                }
                _ => {}
            }
        }
    }

    /// Check that `=` is not used on a signal.
    fn check_var_assign_target(&mut self, expr: &Expression, span: Span) {
        let name = match expr.extract_base_name() {
            Some(n) => n,
            None => return,
        };

        if let Some(sym) = self
            .table
            .lookup_with_includes(self.current_scope, &name, &self.file)
        {
            if let SymbolKind::Signal(_) = &sym.kind {
                self.diagnostics.push(SymbolDiagnostic {
                    span,
                    message: format!(
                        "cannot use '=' to assign to signal '{name}'; use '<==' or '<--'"
                    ),
                    kind: DiagnosticKind::VarAssignToSignal,
                    file: self.file.clone(),
                });
            }
        }
    }

    /// Check that a component initialization call matches the template parameter count.
    fn check_component_init(&mut self, expr: &Expression) {
        if let ExpressionKind::Call(callee, args) = expr.kind.as_ref() {
            if let ExpressionKind::Ident(name) = callee.kind.as_ref() {
                if let Some(sym) =
                    self.table
                        .lookup_with_includes(self.current_scope, name, &self.file)
                {
                    if let SymbolKind::Template(tmpl) = &sym.kind {
                        if args.len() != tmpl.params.len() {
                            self.diagnostics.push(SymbolDiagnostic {
                                span: expr.span,
                                message: format!(
                                    "template '{}' expects {} parameter(s), but {} provided",
                                    name,
                                    tmpl.params.len(),
                                    args.len()
                                ),
                                kind: DiagnosticKind::ParameterCountMismatch,
                                file: self.file.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Info cached per component within a template body.
struct ComponentInfo {
    template_name: Option<String>,
    decl_span: Span,
}

/// Summary of the signal-shape of a template body: every declared signal's
/// name plus separate lists of output/input signal names and their spans.
#[derive(Default)]
struct TemplateSignalInfo {
    known: HashSet<String>,
    outputs: Vec<(String, Span)>,
    inputs: Vec<(String, Span)>,
}

/// Map of `component name -> [(field, span)]`. Reads and writes are tracked
/// separately so we can distinguish unused outputs from unused inputs.
#[derive(Default)]
struct ComponentAccess {
    reads: HashMap<String, Vec<(String, Span)>>,
    writes: HashMap<String, Vec<(String, Span)>>,
}

/// Collect `[base, ..., field]` name segments for a `Member(...)` chain.
/// Returns `None` if the chain contains anything other than idents, index
/// ops (skipped), or member accesses — e.g. calls or arithmetic.
fn collect_member_path(base: &Expression, field: &Identifier) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    if !collect_path_segments(base, &mut parts) {
        return None;
    }
    parts.push(field.name.clone());
    Some(parts)
}

fn collect_path_segments(expr: &Expression, out: &mut Vec<String>) -> bool {
    match expr.kind.as_ref() {
        ExpressionKind::Ident(name) => {
            out.push(name.clone());
            true
        }
        ExpressionKind::Index(base, _) | ExpressionKind::Paren(base) => {
            collect_path_segments(base, out)
        }
        ExpressionKind::Member(base, field) => {
            if !collect_path_segments(base, out) {
                return false;
            }
            out.push(field.name.clone());
            true
        }
        _ => false,
    }
}

fn extract_template_name_from_expr(expr: &Expression) -> Option<String> {
    match expr.kind.as_ref() {
        ExpressionKind::Call(callee, _) => match callee.kind.as_ref() {
            ExpressionKind::Ident(name) => Some(name.clone()),
            _ => None,
        },
        ExpressionKind::Parallel(inner) => extract_template_name_from_expr(inner),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    fn parse_and_check(src: &str) -> Vec<SymbolDiagnostic> {
        let (ast, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut table = SymbolTable::new();
        table.index_file("main.circom", &ast);
        check_types(&table, "main.circom", &ast)
    }

    fn diags_of_kind(diags: &[SymbolDiagnostic], kind: DiagnosticKind) -> Vec<&SymbolDiagnostic> {
        diags.iter().filter(|d| d.kind == kind).collect()
    }

    // ── Signal direction ───────────────────────────────────────────

    #[test]
    fn detects_assign_to_input_signal() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                a <== 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::AssignToInput);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("'a'"));
    }

    #[test]
    fn allows_assign_to_output_signal() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal output b;
                b <== a;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::AssignToInput);
        assert!(errors.is_empty());
    }

    #[test]
    fn allows_assign_to_intermediate_signal() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                signal s;
                signal output b;
                s <== a;
                b <== s;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::AssignToInput);
        assert!(errors.is_empty());
    }

    #[test]
    fn detects_assign_to_input_with_unsafe_op() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input a;
                a <-- 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::AssignToInput);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("<--"));
    }

    // ── Assignment operator validation ─────────────────────────────

    #[test]
    fn detects_var_assign_to_signal() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal output b;
                b = 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::VarAssignToSignal);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("'b'"));
    }

    #[test]
    fn detects_signal_assign_to_var() {
        let diags = parse_and_check(
            r#"
            template T() {
                var x;
                x <== 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::SignalAssignToVar);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("'x'"));
    }

    #[test]
    fn allows_var_assign_to_var() {
        let diags = parse_and_check(
            r#"
            template T() {
                var x;
                x = 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::VarAssignToSignal);
        assert!(errors.is_empty());
        let errors = diags_of_kind(&diags, DiagnosticKind::SignalAssignToVar);
        assert!(errors.is_empty());
    }

    // ── Template parameter count ───────────────────────────────────

    #[test]
    fn detects_parameter_count_mismatch() {
        let diags = parse_and_check(
            r#"
            template Adder(n) {
                signal input a;
                signal output b;
                b <== a;
            }
            template Main() {
                component c = Adder(1, 2);
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::ParameterCountMismatch);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("expects 1"));
        assert!(errors[0].message.contains("2 provided"));
    }

    #[test]
    fn allows_correct_parameter_count() {
        let diags = parse_and_check(
            r#"
            template Adder(n) {
                signal input a;
                signal output b;
                b <== a;
            }
            template Main() {
                component c = Adder(4);
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::ParameterCountMismatch);
        assert!(errors.is_empty());
    }

    #[test]
    fn detects_zero_args_for_parameterized_template() {
        let diags = parse_and_check(
            r#"
            template Adder(n) {
                signal input a;
                signal output b;
                b <== a;
            }
            template Main() {
                component c = Adder();
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::ParameterCountMismatch);
        assert_eq!(errors.len(), 1);
    }

    // ── Signals in functions ───────────────────────────────────────

    #[test]
    fn detects_signal_decl_in_function() {
        let diags = parse_and_check(
            r#"
            function foo() {
                signal input x;
                return 0;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::SignalInFunction);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("signal"));
    }

    #[test]
    fn detects_constraint_eq_in_function() {
        let diags = parse_and_check(
            r#"
            function foo(a, b) {
                a === b;
                return 0;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::SignalInFunction);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("==="));
    }

    #[test]
    fn allows_signal_decl_in_template() {
        let diags = parse_and_check(
            r#"
            template T() {
                signal input x;
                signal output y;
                y <== x;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::SignalInFunction);
        assert!(errors.is_empty());
    }

    // ── Indexed signal access ──────────────────────────────────────

    #[test]
    fn detects_assign_to_input_array_element() {
        let diags = parse_and_check(
            r#"
            template T(n) {
                signal input a[n];
                signal output b;
                a[0] <== 1;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::AssignToInput);
        assert_eq!(errors.len(), 1);
    }

    // ── Tag propagation (#45) ─────────────────────────────────────

    #[test]
    fn warns_when_tag_is_lost() {
        let diags = parse_and_check(
            r#"
            pragma circom 2.1.0;
            template T() {
                signal input x;
                signal output {binary} y;
                y <== x;
            }
            "#,
        );
        let warnings = diags_of_kind(&diags, DiagnosticKind::TagLoss);
        assert_eq!(warnings.len(), 1, "got: {diags:?}");
        assert!(warnings[0].message.contains("binary"));
        assert!(warnings[0].message.contains("'x'"));
    }

    #[test]
    fn no_warning_when_tags_are_preserved() {
        let diags = parse_and_check(
            r#"
            pragma circom 2.1.0;
            template T() {
                signal input {binary} x;
                signal output {binary} y;
                y <== x;
            }
            "#,
        );
        let warnings = diags_of_kind(&diags, DiagnosticKind::TagLoss);
        assert!(warnings.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn no_tag_warning_when_rhs_is_expression() {
        // RHS is a literal / expression, so tag propagation rules don't apply.
        let diags = parse_and_check(
            r#"
            pragma circom 2.1.0;
            template T() {
                signal input x;
                signal output {binary} y;
                y <== x * (1 - x);
            }
            "#,
        );
        let warnings = diags_of_kind(&diags, DiagnosticKind::TagLoss);
        assert!(warnings.is_empty(), "got: {diags:?}");
    }

    // ── Template instantiation (#60) ─────────────────────────────

    #[test]
    fn detects_unknown_component_field() {
        let diags = parse_and_check(
            r#"
            template Inner() {
                signal input a;
                signal output b;
                b <== a;
            }
            template Outer() {
                signal input a;
                signal output b;
                component c = Inner();
                c.a <== a;
                b <== c.bogus;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::UnknownComponentSignal);
        assert_eq!(errors.len(), 1, "got: {diags:?}");
        assert!(errors[0].message.contains("'bogus'"));
    }

    #[test]
    fn warns_on_unused_component_output() {
        let diags = parse_and_check(
            r#"
            template Inner() {
                signal input a;
                signal output b;
                signal output c;
                b <== a;
                c <== a + 1;
            }
            template Outer() {
                signal input a;
                signal output out;
                component inner = Inner();
                inner.a <== a;
                out <== inner.b;
            }
            "#,
        );
        let warnings = diags_of_kind(&diags, DiagnosticKind::UnusedComponentOutput);
        assert!(
            warnings.iter().any(|w| w.message.contains("'c'")),
            "expected unused-output warning for 'c'; got: {diags:?}"
        );
    }

    #[test]
    fn warns_on_missing_component_input() {
        let diags = parse_and_check(
            r#"
            template Inner() {
                signal input a;
                signal input b;
                signal output c;
                c <== a + b;
            }
            template Outer() {
                signal input a;
                signal output out;
                component inner = Inner();
                inner.a <== a;
                out <== inner.c;
            }
            "#,
        );
        let warnings = diags_of_kind(&diags, DiagnosticKind::MissingComponentInput);
        assert!(
            warnings.iter().any(|w| w.message.contains("'b'")),
            "expected missing-input warning for 'b'; got: {diags:?}"
        );
    }

    #[test]
    fn no_component_warnings_when_all_wired() {
        let diags = parse_and_check(
            r#"
            template Inner() {
                signal input a;
                signal output b;
                b <== a;
            }
            template Outer() {
                signal input a;
                signal output out;
                component inner = Inner();
                inner.a <== a;
                out <== inner.b;
            }
            "#,
        );
        let any = diags_of_kind(&diags, DiagnosticKind::UnknownComponentSignal).len()
            + diags_of_kind(&diags, DiagnosticKind::UnusedComponentOutput).len()
            + diags_of_kind(&diags, DiagnosticKind::MissingComponentInput).len();
        assert_eq!(any, 0, "unexpected component warnings: {diags:?}");
    }

    #[test]
    fn anonymous_component_param_count_mismatch() {
        let diags = parse_and_check(
            r#"
            template Multiplier(n) {
                signal input a;
                signal input b;
                signal output c;
                c <== a * b;
            }
            template T() {
                signal input x;
                signal output y;
                y <== Multiplier()(x, x);
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::ParameterCountMismatch);
        assert_eq!(errors.len(), 1, "got: {diags:?}");
        assert!(errors[0].message.contains("Multiplier"));
    }

    #[test]
    fn anonymous_component_named_input_unknown() {
        let diags = parse_and_check(
            r#"
            template Multiplier() {
                signal input a;
                signal input b;
                signal output c;
                c <== a * b;
            }
            template T() {
                signal input x;
                signal output y;
                y <== Multiplier()(a <== x, bogus <== x);
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::UnknownComponentSignal);
        assert_eq!(errors.len(), 1, "got: {diags:?}");
        assert!(errors[0].message.contains("bogus"));
    }

    // ── Bus type matching (#46) ───────────────────────────────────

    #[test]
    fn bus_type_mismatch_flagged_on_safe_assign() {
        let diags = parse_and_check(
            r#"
            pragma circom 2.2.0;
            bus A() { signal x; }
            bus B() { signal x; }
            template T() {
                signal input A() a;
                signal output B() b;
                b <== a;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::BusTypeMismatch);
        assert_eq!(errors.len(), 1, "got: {diags:?}");
        assert!(errors[0].message.contains("'B'"));
        assert!(errors[0].message.contains("'A'"));
    }

    #[test]
    fn bus_type_match_no_diagnostic() {
        let diags = parse_and_check(
            r#"
            pragma circom 2.2.0;
            bus Point() { signal x; signal y; }
            template T() {
                signal input Point() a;
                signal output Point() b;
                b <== a;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::BusTypeMismatch);
        assert!(errors.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn bus_type_mismatch_with_plain_signal_rhs() {
        let diags = parse_and_check(
            r#"
            pragma circom 2.2.0;
            bus Point() { signal x; }
            template T() {
                signal input plain;
                signal output Point() p;
                p <== plain;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::BusTypeMismatch);
        assert_eq!(errors.len(), 1, "got: {diags:?}");
        assert!(errors[0].message.contains("non-bus"));
    }

    #[test]
    fn bus_type_mismatch_via_unsafe_assign() {
        let diags = parse_and_check(
            r#"
            pragma circom 2.2.0;
            bus A() { signal x; }
            bus B() { signal x; }
            template T() {
                signal input A() a;
                signal output B() b;
                b <-- a;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::BusTypeMismatch);
        assert_eq!(errors.len(), 1, "got: {diags:?}");
    }

    #[test]
    fn bus_type_mismatch_through_indexing() {
        let diags = parse_and_check(
            r#"
            pragma circom 2.2.0;
            bus A() { signal x; }
            bus B() { signal x; }
            template T(n) {
                signal input A() a[n];
                signal output B() b[n];
                b[0] <== a[0];
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::BusTypeMismatch);
        assert_eq!(errors.len(), 1, "got: {diags:?}");
    }

    #[test]
    fn bus_type_match_via_component_member() {
        let diags = parse_and_check(
            r#"
            pragma circom 2.2.0;
            bus Point() { signal x; signal y; }
            template Producer() {
                signal output Point() p;
            }
            template T() {
                signal output Point() r;
                component c = Producer();
                r <== c.p;
            }
            "#,
        );
        let errors = diags_of_kind(&diags, DiagnosticKind::BusTypeMismatch);
        assert!(errors.is_empty(), "got: {diags:?}");
    }
}
